use crate::commands::checkout;
use crate::config::Config;
use crate::engine::{BranchMetadata, PrInfo, Stack};
use crate::forge::ForgeClient;
use crate::git::GitRepo;
use crate::remote::RemoteInfo;
use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct GetOptions {
    pub branch: Option<String>,
    pub parent: Option<String>,
    pub no_checkout: bool,
    pub force: bool,
    pub downstack: bool,
    pub remote_upstack: bool,
    pub no_restack: bool,
    pub unfrozen: bool,
}

#[derive(Debug, Clone)]
struct GetTarget {
    branch: String,
    parent: String,
    required_remote: bool,
    pr_info: Option<PrInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BranchSyncOutcome {
    Synced,
    Skipped,
    SkippedWorktree,
}

pub fn run(options: GetOptions) -> Result<()> {
    let repo = GitRepo::open()?;
    let workdir = repo.workdir()?.to_path_buf();
    let config = Config::load()?;
    let remote = config.remote_name().to_string();
    let trunk = repo.trunk_branch()?;

    if options.unfrozen {
        println!(
            "{} {} is accepted for Graphite compatibility; Stax does not freeze branches.",
            "Note:".yellow().bold(),
            "--unfrozen".cyan()
        );
    }

    let Some(requested) = options.branch.as_deref() else {
        return crate::commands::sync::run(
            !options.no_restack,
            false,
            false,
            true,
            false,
            options.force,
            false,
            false,
            false,
            false,
            false,
            &[],
        );
    };

    let target = resolve_requested_target(&repo, &config, requested, options.parent.as_deref())?;
    if target.branch == trunk {
        anyhow::bail!(
            "'{}' is the trunk branch and cannot be tracked. Use {} to checkout trunk.",
            target.branch,
            "stax trunk".cyan()
        );
    }
    let requested_branch = target.branch.clone();

    let stack = Stack::load(&repo)?;
    let targets = collect_targets(&repo, &workdir, &config, &stack, target, &options, &trunk)?;
    let mut skipped = Vec::new();

    for target in &targets {
        let outcome = sync_branch(&repo, &workdir, &remote, target, options.force)?;
        if outcome != BranchSyncOutcome::Synced {
            skipped.push(target.branch.clone());
        }
    }

    if !options.no_checkout && !skipped.iter().any(|branch| branch == &requested_branch) {
        checkout::run(
            Some(requested_branch.clone()),
            None,
            false,
            false,
            None,
            false,
        )?;
    } else if options.no_checkout {
        println!("{}", "Skipped checkout (--no-checkout).".dimmed());
    }

    if !options.no_restack {
        let worktree_skipped: Vec<String> = targets
            .iter()
            .filter_map(|target| {
                repo.branch_worktree(&target.branch)
                    .ok()
                    .flatten()
                    .filter(|worktree| !same_path(&worktree.path, &workdir))
                    .map(|_| target.branch.clone())
            })
            .collect();
        if worktree_skipped.is_empty() && !options.no_checkout {
            crate::commands::restack::run(
                false,
                false,
                false,
                false,
                true,
                false,
                false,
                crate::commands::restack::SubmitAfterRestack::No,
            )?;
        } else if !worktree_skipped.is_empty() {
            println!(
                "{} restack because {} checked out in another worktree.",
                "Skipped".yellow().bold(),
                format_branch_list(&worktree_skipped).cyan()
            );
        }
    }

    Ok(())
}

fn sync_branch(
    repo: &GitRepo,
    workdir: &Path,
    remote: &str,
    target: &GetTarget,
    force: bool,
) -> Result<BranchSyncOutcome> {
    let branch = &target.branch;
    let parent_branch = &target.parent;

    if repo.branch_commit(parent_branch).is_err() {
        anyhow::bail!("Parent branch '{}' does not exist locally.", parent_branch);
    }

    if let Some(worktree) = repo.branch_worktree(branch)? {
        if !same_path(&worktree.path, workdir) {
            println!(
                "{} {} because it is checked out in another worktree: {}",
                "Skipped".yellow().bold(),
                branch.cyan(),
                worktree.path.display()
            );
            return Ok(BranchSyncOutcome::SkippedWorktree);
        }
    }

    println!(
        "{} {} from {}...",
        "Fetching".blue().bold(),
        branch.cyan(),
        remote.cyan()
    );
    if !fetch_remote_branch(workdir, remote, branch, target.required_remote)? {
        println!(
            "{} {} because no remote branch exists on {}.",
            "Skipped".yellow().bold(),
            branch.cyan(),
            remote.cyan()
        );
        return Ok(BranchSyncOutcome::Skipped);
    }

    let remote_ref = format!("{}/{}", remote, branch);
    let remote_sha = rev_parse(workdir, &remote_ref)?;
    let local_exists = local_branch_exists(workdir, branch)?;

    if local_exists {
        let local_sha = rev_parse(workdir, branch)?;
        if local_sha != remote_sha {
            if force {
                force_update_local_branch(workdir, branch, &remote_ref)?;
                println!(
                    "{} {} to {}.",
                    "Reset".yellow().bold(),
                    branch.cyan(),
                    remote_ref.cyan()
                );
            } else if is_ancestor(workdir, branch, &remote_ref)? {
                fast_forward_local_branch(workdir, branch, &remote_ref)?;
                println!(
                    "{} {} to {}.",
                    "Fast-forwarded".green().bold(),
                    branch.cyan(),
                    remote_ref.cyan()
                );
            } else if is_ancestor(workdir, &remote_ref, branch)? {
                println!(
                    "{} {} already contains {}.",
                    "Kept".green().bold(),
                    branch.cyan(),
                    remote_ref.cyan()
                );
            } else {
                rebase_local_branch(workdir, branch, &remote_ref)?;
                println!(
                    "{} {} onto {}.",
                    "Rebased".green().bold(),
                    branch.cyan(),
                    remote_ref.cyan()
                );
            }
        }
        set_upstream(workdir, branch, &remote_ref)?;
    } else {
        create_tracking_branch(workdir, branch, &remote_ref)?;
        println!(
            "{} {} tracking {}.",
            "Created".green().bold(),
            branch.cyan(),
            remote_ref.cyan()
        );
    }

    let repo = GitRepo::open()?;
    write_tracking_metadata(&repo, branch, parent_branch, remote, target.pr_info.clone())?;

    println!(
        "{} {} with parent {}.",
        "Tracked".green().bold(),
        branch.cyan(),
        parent_branch.cyan()
    );

    Ok(BranchSyncOutcome::Synced)
}

fn resolve_requested_target(
    repo: &GitRepo,
    config: &Config,
    requested: &str,
    explicit_parent: Option<&str>,
) -> Result<GetTarget> {
    if let Ok(number) = requested.parse::<u64>() {
        let remote_info = RemoteInfo::from_repo(repo, config)?;
        let runtime = tokio::runtime::Runtime::new().context("Failed to create async runtime")?;
        let _enter = runtime.enter();
        let client = ForgeClient::new(&remote_info)?;
        let pr = runtime
            .block_on(client.get_pr_with_head(number))
            .with_context(|| format!("Failed to load PR #{}", number))?;
        let parent = explicit_parent.unwrap_or(&pr.info.base).to_string();
        return Ok(GetTarget {
            branch: normalize_remote_branch(&pr.head, config.remote_name())?,
            parent,
            required_remote: true,
            pr_info: Some(PrInfo {
                number: pr.info.number,
                state: pr.info.state,
                is_draft: Some(pr.info.is_draft),
            }),
        });
    }

    Ok(GetTarget {
        branch: normalize_remote_branch(requested, config.remote_name())?,
        parent: explicit_parent
            .map(ToString::to_string)
            .unwrap_or_else(|| repo.trunk_branch().unwrap_or_else(|_| "main".to_string())),
        required_remote: true,
        pr_info: None,
    })
}

fn collect_targets(
    repo: &GitRepo,
    workdir: &Path,
    config: &Config,
    stack: &Stack,
    target: GetTarget,
    options: &GetOptions,
    trunk: &str,
) -> Result<Vec<GetTarget>> {
    let mut targets = Vec::new();
    let mut seen = HashSet::new();
    let local_target_exists = local_branch_exists(workdir, &target.branch)?;

    if local_target_exists && stack.branches.contains_key(&target.branch) {
        let mut ancestors = stack.ancestors(&target.branch);
        ancestors.reverse();
        for branch in ancestors {
            if branch != trunk {
                add_existing_stack_target(repo, &mut targets, &mut seen, &branch, true)?;
            }
        }
    } else if stack.branches.contains_key(&target.parent) {
        let mut ancestors = stack.ancestors(&target.parent);
        ancestors.reverse();
        for branch in ancestors {
            if branch != trunk {
                add_existing_stack_target(repo, &mut targets, &mut seen, &branch, false)?;
            }
        }
        if target.parent != trunk {
            add_existing_stack_target(repo, &mut targets, &mut seen, &target.parent, false)?;
        }
    }

    add_target(&mut targets, &mut seen, target);

    if local_target_exists && !options.downstack {
        for branch in stack.descendants(&targets.last().expect("target added").branch) {
            add_existing_stack_target(repo, &mut targets, &mut seen, &branch, false)?;
        }
    }

    if options.remote_upstack {
        add_remote_upstack_targets(repo, config, &mut targets, &mut seen)?;
    }

    Ok(targets)
}

fn add_existing_stack_target(
    repo: &GitRepo,
    targets: &mut Vec<GetTarget>,
    seen: &mut HashSet<String>,
    branch: &str,
    required_remote: bool,
) -> Result<()> {
    let Some(meta) = BranchMetadata::read(repo.inner(), branch)? else {
        return Ok(());
    };
    add_target(
        targets,
        seen,
        GetTarget {
            branch: branch.to_string(),
            parent: meta.parent_branch_name,
            required_remote,
            pr_info: meta.pr_info,
        },
    );
    Ok(())
}

fn add_remote_upstack_targets(
    repo: &GitRepo,
    config: &Config,
    targets: &mut Vec<GetTarget>,
    seen: &mut HashSet<String>,
) -> Result<()> {
    let remote_info = RemoteInfo::from_repo(repo, config)?;
    let runtime = tokio::runtime::Runtime::new().context("Failed to create async runtime")?;
    let _enter = runtime.enter();
    let client = ForgeClient::new(&remote_info)?;
    let prs = runtime
        .block_on(client.list_open_prs_by_head())
        .context("Failed to list open PRs for --remote-upstack")?;
    let prs_by_base = prs_by_base(prs);
    let mut index = 0;

    while index < targets.len() {
        let base = targets[index].branch.clone();
        index += 1;

        let Some(children) = prs_by_base.get(&base) else {
            continue;
        };

        for pr in children {
            let branch = normalize_remote_branch(&pr.head, config.remote_name())?;
            if seen.contains(&branch) {
                continue;
            }
            add_target(
                targets,
                seen,
                GetTarget {
                    branch,
                    parent: base.clone(),
                    required_remote: true,
                    pr_info: Some(PrInfo {
                        number: pr.info.number,
                        state: pr.info.state.clone(),
                        is_draft: Some(pr.info.is_draft),
                    }),
                },
            );
        }
    }

    Ok(())
}

fn prs_by_base(
    prs: HashMap<String, crate::github::pr::PrInfoWithHead>,
) -> HashMap<String, Vec<crate::github::pr::PrInfoWithHead>> {
    let mut by_base: HashMap<String, Vec<crate::github::pr::PrInfoWithHead>> = HashMap::new();
    for pr in prs.into_values() {
        by_base.entry(pr.info.base.clone()).or_default().push(pr);
    }
    for children in by_base.values_mut() {
        children.sort_by(|a, b| a.head.cmp(&b.head));
    }
    by_base
}

fn add_target(targets: &mut Vec<GetTarget>, seen: &mut HashSet<String>, target: GetTarget) {
    if seen.insert(target.branch.clone()) {
        targets.push(target);
    }
}

fn normalize_remote_branch(branch: &str, remote: &str) -> Result<String> {
    let trimmed = branch.trim();
    if trimmed.is_empty() {
        anyhow::bail!("Branch name cannot be empty.");
    }

    if let Some(local) = trimmed.strip_prefix(&format!("{remote}/")) {
        if local.is_empty() {
            anyhow::bail!("Branch name cannot be empty.");
        }
        return Ok(local.to_string());
    }

    if trimmed.starts_with("refs/") {
        anyhow::bail!("Pass a branch name, not a full ref: '{}'.", branch);
    }

    Ok(trimmed.to_string())
}

fn write_tracking_metadata(
    repo: &GitRepo,
    branch: &str,
    parent_branch: &str,
    remote: &str,
    pr_info: Option<PrInfo>,
) -> Result<()> {
    if let Some(existing) = BranchMetadata::read(repo.inner(), branch)? {
        if existing.parent_branch_name != parent_branch {
            anyhow::bail!(
                "Branch '{}' is already tracked with parent '{}'.\n\
                 Use {} to change its parent.",
                branch,
                existing.parent_branch_name,
                "stax branch reparent".cyan()
            );
        }
        if let Some(pr_info) = pr_info {
            let updated = BranchMetadata {
                pr_info: Some(pr_info),
                ..existing
            };
            updated.write(repo.inner(), branch)?;
        }
        return Ok(());
    }

    let parent_rev = repo
        .merge_base(parent_branch, branch)
        .or_else(|_| repo.branch_commit(parent_branch))?;
    let meta = BranchMetadata {
        source_remote: Some(remote.to_string()),
        pr_info,
        ..BranchMetadata::new(parent_branch, &parent_rev)
    };
    meta.write(repo.inner(), branch)?;
    Ok(())
}

fn fetch_remote_branch(workdir: &Path, remote: &str, branch: &str, required: bool) -> Result<bool> {
    let refspec = format!("refs/heads/{branch}:refs/remotes/{remote}/{branch}");
    let output = Command::new("git")
        .args(["fetch", "--no-tags", remote, &refspec])
        .current_dir(workdir)
        .output()
        .with_context(|| format!("Failed to run git fetch {}", remote))?;

    if output.status.success() {
        return Ok(true);
    }

    if !required {
        return Ok(false);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!(
        "Remote branch '{}' was not found on '{}'.\n\ngit stderr:\n{}",
        branch,
        remote,
        stderr.trim()
    );
}

fn same_path(left: &Path, right: &Path) -> bool {
    normalize_path(left) == normalize_path(right)
}

fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn format_branch_list(branches: &[String]) -> String {
    match branches {
        [] => "no branches".to_string(),
        [branch] => branch.clone(),
        _ => format!("{} branches", branches.len()),
    }
}

fn create_tracking_branch(workdir: &Path, branch: &str, remote_ref: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["branch", "--track", branch, remote_ref])
        .current_dir(workdir)
        .output()
        .with_context(|| format!("Failed to create local branch '{}'", branch))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!(
        "Failed to create local tracking branch '{}': {}",
        branch,
        stderr.trim()
    );
}

fn force_update_local_branch(workdir: &Path, branch: &str, remote_ref: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["branch", "--force", branch, remote_ref])
        .current_dir(workdir)
        .output()
        .with_context(|| format!("Failed to reset local branch '{}'", branch))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!(
        "Failed to reset local branch '{}': {}",
        branch,
        stderr.trim()
    );
}

fn fast_forward_local_branch(workdir: &Path, branch: &str, remote_ref: &str) -> Result<()> {
    let output = if current_branch(workdir).as_deref() == Some(branch) {
        Command::new("git")
            .args(["merge", "--ff-only", remote_ref])
            .current_dir(workdir)
            .output()
    } else {
        Command::new("git")
            .args(["branch", "--force", branch, remote_ref])
            .current_dir(workdir)
            .output()
    }
    .with_context(|| format!("Failed to fast-forward local branch '{}'", branch))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!(
        "Failed to fast-forward local branch '{}': {}",
        branch,
        stderr.trim()
    );
}

fn current_branch(workdir: &Path) -> Option<String> {
    Command::new("git")
        .args(["symbolic-ref", "--quiet", "--short", "HEAD"])
        .current_dir(workdir)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn rebase_local_branch(workdir: &Path, branch: &str, remote_ref: &str) -> Result<()> {
    let original_branch = current_branch(workdir);
    let output = Command::new("git")
        .args(["rebase", remote_ref, branch])
        .current_dir(workdir)
        .output()
        .with_context(|| format!("Failed to rebase local branch '{}'", branch))?;

    if output.status.success() {
        if original_branch
            .as_deref()
            .is_some_and(|original| original != branch)
        {
            if let Some(original) = original_branch {
                checkout_branch(workdir, &original)?;
            }
        }
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!(
        "Failed to rebase local branch '{}' onto '{}'.\n\
         Resolve conflicts, then run {}.\n\n\
         git stderr:\n{}",
        branch,
        remote_ref,
        "stax continue".cyan(),
        stderr.trim()
    );
}

fn checkout_branch(workdir: &Path, branch: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["checkout", branch])
        .current_dir(workdir)
        .output()
        .with_context(|| format!("Failed to restore branch '{}'", branch))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!("Failed to restore branch '{}': {}", branch, stderr.trim());
}

fn set_upstream(workdir: &Path, branch: &str, remote_ref: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["branch", "--set-upstream-to", remote_ref, branch])
        .current_dir(workdir)
        .output()
        .with_context(|| format!("Failed to set upstream for '{}'", branch))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!("Failed to set upstream for '{}': {}", branch, stderr.trim());
}

fn is_ancestor(workdir: &Path, ancestor: &str, descendant: &str) -> Result<bool> {
    let output = Command::new("git")
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .current_dir(workdir)
        .output()
        .context("Failed to inspect branch ancestry")?;

    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to inspect branch ancestry: {}", stderr.trim());
        }
    }
}

fn local_branch_exists(workdir: &Path, branch: &str) -> Result<bool> {
    let output = Command::new("git")
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .current_dir(workdir)
        .output()
        .context("Failed to inspect local branches")?;
    Ok(output.status.success())
}

fn rev_parse(workdir: &Path, rev: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", rev])
        .current_dir(workdir)
        .output()
        .with_context(|| format!("Failed to resolve '{}'", rev))?;

    if output.status.success() {
        return Ok(String::from_utf8(output.stdout)?.trim().to_string());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!("Failed to resolve '{}': {}", rev, stderr.trim());
}
