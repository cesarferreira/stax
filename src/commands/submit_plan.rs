use crate::commands::submit::{SubmitOptions, SubmitScope};
use crate::config::{
    Config, NativeStackMode, SingleStackMode, StackLinksMode, StackLinksWhenNative,
};
use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::remote::{self, RemoteInfo};
use anyhow::{Context, Result};
use serde::Serialize;

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
    let remote_branches = remote::get_remote_branches(workdir, &remote.name)?;
    let mut branch_plans = Vec::with_capacity(branches.len());

    for branch in branches {
        let meta = BranchMetadata::read(repo.inner(), &branch)?
            .with_context(|| format!("No metadata for branch {branch}"))?;
        let stack_branch = stack
            .branches
            .get(&branch)
            .with_context(|| format!("Branch {branch} is not in the loaded stack"))?;
        let is_imported = meta.source_remote.is_some();
        let is_empty =
            repo.branch_commit(&branch).ok() == repo.branch_commit(&meta.parent_branch_name).ok();
        let needs_push = !is_imported
            && super::submit::ref_needs_push(
                workdir,
                &remote.name,
                &branch,
                &format!("refs/heads/{branch}"),
            );
        let push = if is_imported {
            "skip_imported"
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
            publish_source: if stack_branch.needs_restack {
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

    let pr_count = branch_plans
        .iter()
        .filter(|branch| branch.pull_request != "skip")
        .count();
    let stack_links_enabled = !options.no_pr
        && config.submit.stack_links != StackLinksMode::Off
        && (config.submit.single_stack == SingleStackMode::On || pr_count > 1)
        && !(config.submit.native_stack != NativeStackMode::Off
            && config.submit.stack_links_when_native == StackLinksWhenNative::Off);
    let native_mode = options
        .native_stack_override
        .unwrap_or(config.submit.native_stack);

    let plan = SubmitPlan {
        schema_version: 1,
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
            action: if stack_links_enabled {
                "update"
            } else {
                "skip"
            }
            .into(),
            reason: format!(
                "mode={}, single_stack={}",
                stack_links_mode(config.submit.stack_links),
                single_stack_mode(config.submit.single_stack)
            ),
        },
        native_stack: PlannedOperation {
            action: match native_mode {
                NativeStackMode::Off => "skip",
                NativeStackMode::Link => "link",
                NativeStackMode::Auto => "auto",
            }
            .into(),
            reason: "configured submit.native_stack mode".into(),
        },
    };

    if options.json {
        println!("{}", serde_json::to_string_pretty(&plan)?);
    } else {
        print_human_plan(&plan);
    }
    Ok(())
}

fn print_human_plan(plan: &SubmitPlan) {
    println!("Submit plan ({}, read-only)", plan.scope);
    println!("  fetch: {}", plan.fetch.action);
    for branch in &plan.branches {
        println!(
            "  {} <- {}: push={}, pr={}, metadata={}",
            branch.branch, branch.parent, branch.push, branch.pull_request, branch.metadata
        );
        if branch.needs_restack {
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
