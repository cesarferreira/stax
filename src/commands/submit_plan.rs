use crate::commands::submit::{SubmitOptions, SubmitScope};
use crate::config::{
    Config, NativeStackMode, SingleStackMode, StackLinksMode, StackLinksWhenNative,
};
use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::remote::{self, ForgeType, RemoteInfo};
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Serialize)]
struct SubmitPlan {
    schema_version: u8,
    read_only: bool,
    scope: &'static str,
    current_branch: String,
    trunk: String,
    remote: String,
    fetch: PlannedOperation,
    branches: Vec<BranchPlan>,
    stack_links: PlannedOperation,
    native_stack: PlannedOperation,
}

#[derive(Debug, Serialize)]
struct PlannedOperation {
    action: String,
    reason: String,
}

#[derive(Debug, Serialize)]
struct BranchPlan {
    branch: String,
    parent: String,
    needs_restack: bool,
    publish_source: &'static str,
    push: &'static str,
    pull_request: &'static str,
    pr_number: Option<u64>,
    desired_base: Option<String>,
    metadata: &'static str,
}

pub(crate) fn run(scope: SubmitScope, options: &SubmitOptions) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let config = Config::load()?;
    let remote = RemoteInfo::from_repo(&repo, &config)?;
    let workdir = repo.workdir()?;

    if matches!(scope, SubmitScope::Branch) && current == stack.trunk {
        anyhow::bail!(
            "Cannot plan submission of trunk '{}' as a branch",
            stack.trunk
        );
    }

    let branches = super::submit::resolve_branches_for_scope(&stack, &current, scope);
    let live_remote_heads = if options.no_fetch {
        None
    } else {
        Some(remote::ls_remote_head_oids(workdir, &remote.name)?)
    };
    let remote_branches = match &live_remote_heads {
        Some(heads) => heads.keys().cloned().collect::<HashSet<_>>(),
        None => remote::get_remote_branches(workdir, &remote.name)?
            .into_iter()
            .collect(),
    };
    let mut branch_plans = Vec::with_capacity(branches.len());
    let mut temporary_publish_branches = HashSet::new();

    for branch in branches {
        let meta = BranchMetadata::read(repo.inner(), &branch)?
            .with_context(|| format!("No metadata for branch {branch}"))?;
        let stack_branch = stack
            .branches
            .get(&branch)
            .with_context(|| format!("Branch {branch} is not in the loaded stack"))?;
        let is_imported = meta.source_remote.is_some();
        let needs_temporary_publish = !is_imported
            && (stack_branch.needs_restack
                || temporary_publish_branches.contains(&meta.parent_branch_name));
        if needs_temporary_publish {
            temporary_publish_branches.insert(branch.clone());
        }
        let is_empty =
            repo.branch_commit(&branch).ok() == repo.branch_commit(&meta.parent_branch_name).ok();
        let needs_push = !is_imported
            && needs_push(
                &repo,
                workdir,
                &remote.name,
                &branch,
                live_remote_heads.as_ref(),
            );
        let push = if is_imported {
            "skip_imported"
        } else if needs_temporary_publish && remote_branches.contains(&branch) {
            "evaluate_after_temporary_restack"
        } else if !needs_push {
            "none"
        } else if remote_branches.contains(&branch) {
            "update"
        } else {
            "create"
        };
        let pr_number = meta
            .pr_info
            .as_ref()
            .filter(|pr| pr.number > 0)
            .map(|pr| pr.number);
        let pull_request = if options.no_pr || is_imported || is_empty {
            "skip"
        } else if pr_number.is_some() {
            "inspect_and_update"
        } else {
            "create"
        };
        let metadata = match pull_request {
            "create" => "record_pr",
            "inspect_and_update" => "refresh_pr",
            _ => "none",
        };

        branch_plans.push(BranchPlan {
            branch,
            parent: meta.parent_branch_name.clone(),
            needs_restack: stack_branch.needs_restack,
            publish_source: if needs_temporary_publish {
                "temporary_restack"
            } else {
                "local_branch"
            },
            push,
            pull_request,
            pr_number,
            desired_base: (pull_request == "inspect_and_update").then_some(meta.parent_branch_name),
            metadata,
        });
    }

    let planned_pr_branches = branch_plans
        .iter()
        .filter(|branch| branch.pull_request != "skip")
        .map(|branch| branch.branch.as_str())
        .collect::<HashSet<_>>();
    let context_branches = stack
        .current_stack(&current)
        .into_iter()
        .filter(|branch| branch != &stack.trunk)
        .collect::<Vec<_>>();
    let known_pr_branches = context_branches
        .iter()
        .filter(|branch| {
            planned_pr_branches.contains(branch.as_str())
                || stack
                    .branches
                    .get(branch.as_str())
                    .and_then(|info| info.pr_number)
                    .is_some()
        })
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let pr_count = known_pr_branches.len();
    let has_unresolved_prs = pr_count < context_branches.len();
    let native_mode = options
        .native_stack_override
        .unwrap_or(config.submit.native_stack);
    let known_prs_have_fork = known_pr_branches.iter().any(|branch| {
        stack.branches.get(*branch).is_some_and(|info| {
            info.children
                .iter()
                .filter(|child| known_pr_branches.contains(child.as_str()))
                .count()
                > 1
        })
    });
    let native_stack_action = if options.no_pr
        || native_mode == NativeStackMode::Off
        || remote.forge != ForgeType::GitHub
        || context_branches.len() < 2
        || known_prs_have_fork
        || (!has_unresolved_prs && pr_count < 2)
    {
        "skip"
    } else if has_unresolved_prs {
        "evaluate_after_pr_discovery"
    } else {
        "attempt"
    };
    let stack_links_action = if options.no_pr || config.submit.stack_links == StackLinksMode::Off {
        "skip"
    } else if config.submit.single_stack == SingleStackMode::Off
        && pr_count <= 1
        && has_unresolved_prs
    {
        "evaluate_after_pr_discovery"
    } else if config.submit.single_stack == SingleStackMode::Off && pr_count <= 1 {
        "skip"
    } else if native_stack_action != "skip"
        && config.submit.stack_links_when_native == StackLinksWhenNative::Off
    {
        "update_unless_native_link_succeeds"
    } else {
        "update"
    };

    let plan = SubmitPlan {
        schema_version: 2,
        read_only: true,
        scope: scope.label(),
        current_branch: current,
        trunk: stack.trunk,
        remote: remote.name,
        fetch: if options.no_fetch {
            PlannedOperation {
                action: "skip".into(),
                reason: "--no-fetch uses cached remote-tracking refs".into(),
            }
        } else {
            PlannedOperation {
                action: "fetch".into(),
                reason: "submit refreshes trunk and selected branch refs".into(),
            }
        },
        branches: branch_plans,
        stack_links: PlannedOperation {
            action: stack_links_action.into(),
            reason: format!(
                "mode={}, single_stack={}, native_stack={}",
                stack_links_mode(config.submit.stack_links),
                single_stack_mode(config.submit.single_stack),
                native_stack_mode(native_mode)
            ),
        },
        native_stack: PlannedOperation {
            action: native_stack_action.into(),
            reason: format!(
                "configured mode={}; native linking also depends on PR discovery and gh-stack runtime support",
                native_stack_mode(native_mode)
            ),
        },
    };

    if options.json {
        println!("{}", serde_json::to_string_pretty(&plan)?);
    } else {
        print_human_plan(&plan);
    }
    Ok(())
}

fn needs_push(
    repo: &GitRepo,
    workdir: &std::path::Path,
    remote: &str,
    branch: &str,
    live_remote_heads: Option<&HashMap<String, String>>,
) -> bool {
    if let Some(heads) = live_remote_heads {
        return repo
            .branch_commit(branch)
            .ok()
            .is_none_or(|local| heads.get(branch) != Some(&local));
    }

    super::submit::ref_needs_push(workdir, remote, branch, &format!("refs/heads/{branch}"))
}

fn print_human_plan(plan: &SubmitPlan) {
    println!("Submit plan ({}, read-only)", plan.scope);
    println!("  fetch: {}", plan.fetch.action);
    for branch in &plan.branches {
        println!(
            "  {} <- {}: push={}, pr={}, metadata={}",
            branch.branch, branch.parent, branch.push, branch.pull_request, branch.metadata
        );
        if branch.publish_source == "temporary_restack" {
            println!("    prepare temporary restack before push");
        }
        if let Some(base) = &branch.desired_base {
            println!("    verify/retarget PR base to {base}");
        }
    }
    println!("  stack links: {}", plan.stack_links.action);
    println!("  native stack: {}", plan.native_stack.action);
}

fn stack_links_mode(mode: StackLinksMode) -> &'static str {
    match mode {
        StackLinksMode::Comment => "comment",
        StackLinksMode::Body => "body",
        StackLinksMode::Both => "both",
        StackLinksMode::Off => "off",
    }
}

fn single_stack_mode(mode: SingleStackMode) -> &'static str {
    match mode {
        SingleStackMode::On => "on",
        SingleStackMode::Off => "off",
    }
}

fn native_stack_mode(mode: NativeStackMode) -> &'static str {
    match mode {
        NativeStackMode::Off => "off",
        NativeStackMode::Auto => "auto",
        NativeStackMode::Link => "link",
    }
}
