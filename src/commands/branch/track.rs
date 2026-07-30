use crate::application::{
    ParentSource, RepoFacts, TrackCandidate, branches_needing_upstream, newly_created_branches,
    plan_fetches, resolve_parent, topological_order,
};
use crate::config::Config;
use crate::engine::{BranchMetadata, PrInfo};
use crate::forge::ForgeClient;
use crate::git::GitRepo;
use crate::progress::LiveTimer;
use crate::remote::{self, RemoteInfo};
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{FuzzySelect, theme::ColorfulTheme};
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

pub fn run(parent: Option<String>, all_prs: bool) -> Result<()> {
    if all_prs {
        return run_track_all_prs();
    }
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let config = Config::load()?;
    let trunk = repo.trunk_branch()?;

    // Can't track trunk
    if current == trunk {
        println!(
            "{} is the trunk branch and cannot be tracked.",
            current.yellow()
        );
        return Ok(());
    }

    // Check if already tracked
    if let Some(existing) = BranchMetadata::read(repo.inner(), &current)? {
        println!(
            "Branch '{}' is already tracked with parent '{}'.",
            current.yellow(),
            existing.parent_branch_name.blue()
        );
        println!("Use {} to update.", "stax branch reparent".cyan());
        return Ok(());
    }

    // Determine parent
    let parent_branch = match parent {
        Some(p) => {
            // Validate the branch exists
            if repo.branch_commit(&p).is_err() {
                anyhow::bail!("Branch '{}' does not exist", p);
            }
            p
        }
        None => {
            // Build list of potential parents
            let mut branches = repo.list_branches()?;
            branches.retain(|b| b != &current);
            branches.sort();

            // Put trunk first as the recommended default
            if let Some(pos) = branches.iter().position(|b| b == &trunk) {
                branches.remove(pos);
                branches.insert(0, trunk.clone());
            }

            if branches.is_empty() {
                anyhow::bail!("No branches available to be parent");
            }

            // Build display with recommendation hint
            let items: Vec<String> = branches
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    if i == 0 {
                        format!("{} (recommended)", b)
                    } else {
                        b.clone()
                    }
                })
                .collect();

            let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
                .with_prompt(format!("Select parent branch for '{}'", current))
                .items(&items)
                .default(0)
                .interact()?;

            branches[selection].clone()
        }
    };

    // Use the divergence point (merge-base) rather than the parent's current tip.
    // This matches freephite's `trackBranch` which stores `getMergeBase(branch, parent)`.
    // Storing the tip would make the stored revision a non-ancestor of `current`,
    // which causes `git rebase --onto` to scope the replay incorrectly.
    let parent_rev = repo
        .merge_base(&parent_branch, &current)
        .or_else(|_| repo.branch_commit(&parent_branch))?;

    // Create metadata
    let meta = BranchMetadata::new(&parent_branch, &parent_rev);
    meta.write(repo.inner(), &current)?;

    if let Ok(remote_branches) = remote::get_remote_branches(repo.workdir()?, config.remote_name())
        && !remote_branches.contains(&parent_branch)
    {
        println!(
            "{}",
            format!(
                "Warning: parent '{}' is not on remote '{}'.",
                parent_branch,
                config.remote_name()
            )
            .yellow()
        );
    }

    println!(
        "✓ Tracking '{}' with parent '{}'",
        current.green(),
        parent_branch.blue()
    );

    Ok(())
}

/// Track all open PRs authored by the current user
fn run_track_all_prs() -> Result<()> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let trunk = repo.trunk_branch()?;
    let workdir = repo.workdir()?;
    let remote_name = config.remote_name();

    // Get remote info for forge API
    let remote_info = RemoteInfo::from_repo(&repo, &config)?;

    let rt = tokio::runtime::Runtime::new().context("Failed to create async runtime")?;
    let _enter = rt.enter();
    let client = ForgeClient::new(&remote_info)?;

    // Get current user
    let username = rt
        .block_on(async { client.get_current_user().await })
        .context("Failed to get current forge user")?;

    // Fetch all open PRs
    let open_prs = rt
        .block_on(async { client.get_user_open_prs(&username).await })
        .context("Failed to fetch open PRs")?;

    if open_prs.is_empty() {
        println!(
            "No open PRs found for user '{}' in {}/{}.",
            username.cyan(),
            remote_info.owner().dimmed(),
            remote_info.repo.dimmed()
        );
        println!(
            "{}",
            "Tip: This only finds PRs in the current repository.".dimmed()
        );
        return Ok(());
    }

    println!(
        "Found {} open PR(s) by {}:\n",
        open_prs.len().to_string().cyan(),
        username.cyan()
    );

    // Phase 0 — partition into already-tracked vs. untracked candidates.
    let mut untracked = Vec::new();
    let mut skipped_count = 0usize;
    for pr in open_prs {
        if BranchMetadata::read(repo.inner(), &pr.head_branch)?.is_some() {
            println!(
                "  {} {} (already tracked)",
                "▸".dimmed(),
                pr.head_branch.dimmed()
            );
            skipped_count += 1;
            continue;
        }
        untracked.push(pr);
    }

    let candidates: Vec<TrackCandidate> = untracked
        .iter()
        .map(|pr| TrackCandidate {
            number: pr.number,
            head: pr.head_branch.clone(),
            base: pr.base_branch.clone(),
        })
        .collect();

    // Phase 1 — fetch everything up front, in one batch where possible.
    let local_before = local_branch_names(workdir)?;
    let plan = plan_fetches(&candidates, &trunk, &local_before);
    let to_fetch: Vec<String> = plan
        .required
        .iter()
        .chain(plan.optional.iter())
        .cloned()
        .collect();

    if !to_fetch.is_empty() {
        let timer = LiveTimer::maybe_new(
            true,
            &format!(
                "Fetching {} branch(es) from {}...",
                to_fetch.len(),
                remote_name
            ),
        );
        let batch = fetch_branches_batch(workdir, remote_name, &to_fetch);
        drop(timer);
        if let Err(e) = batch {
            println!(
                "  {} batch fetch incomplete, retrying individually: {}",
                "!".yellow(),
                e
            );
        }
    }

    let mut local_after = local_branch_names(workdir)?;
    let mut failed_heads: HashSet<String> = HashSet::new();
    for b in &to_fetch {
        if local_after.contains(b) {
            continue;
        }
        print!("  {} Fetching {}...", "↓".blue(), b.cyan());
        std::io::Write::flush(&mut std::io::stdout()).ok();
        match fetch_branch_from_remote(workdir, remote_name, b) {
            Ok(()) => {
                println!(" {}", "done".green());
                local_after.insert(b.clone());
            }
            Err(e) => {
                println!(" {}", "failed".red());
                eprintln!("    Error: {}", e);
                if plan.required.contains(b) {
                    failed_heads.insert(b.clone());
                }
            }
        }
    }

    let newly_fetched = newly_created_branches(&to_fetch, &local_before, &local_after);
    let fetched_count = newly_fetched.len();

    // A refspec fetch (`<b>:<b>`) creates the local branch but configures no
    // upstream, so a later plain `git pull` on it fails with "There is no
    // tracking information for the current branch". Set it here, but only for
    // branches this run created — never rewrite an upstream the user chose.
    let already_configured = repo.branches_with_configured_upstream().unwrap_or_default();
    let mut upstream_set = 0usize;
    let mut upstream_failed: Vec<String> = Vec::new();
    for b in branches_needing_upstream(&newly_fetched, &already_configured) {
        match repo.set_branch_upstream(&b, remote_name) {
            Ok(()) => upstream_set += 1,
            Err(_) => upstream_failed.push(b),
        }
    }
    if !upstream_failed.is_empty() {
        let shown = upstream_failed
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let more = if upstream_failed.len() > 5 {
            format!(" and {} more", upstream_failed.len() - 5)
        } else {
            String::new()
        };
        println!(
            "{}",
            format!(
                "  ! Could not set upstream for {} branch(es): {}{}. Run `git branch --set-upstream-to={}/<branch> <branch>` if `git pull` fails there.",
                upstream_failed.len(),
                shown,
                more,
                remote_name
            )
            .yellow()
        );
    }

    let remote_branches: HashSet<String> = remote::get_remote_branches(workdir, remote_name)
        .unwrap_or_default()
        .into_iter()
        .collect();

    // Phase 2 — resolve parents and write metadata in dependency order, so a
    // stacked PR's base is always tracked before its dependent is resolved.
    let mut tracked_count = 0usize;
    for i in topological_order(&candidates) {
        let pr = &untracked[i];
        if failed_heads.contains(&pr.head_branch) {
            continue;
        }

        let facts = RepoFacts {
            trunk: &trunk,
            remote: remote_name,
            local_branches: &local_after,
            remote_branches: &remote_branches,
        };
        let decision = resolve_parent(&candidates[i], &facts);

        match &decision.source {
            ParentSource::TrunkFallback { unresolved_base } => println!(
                "{}",
                format!(
                    "  ! PR #{}: base '{}' was not found locally or on '{}' — parenting '{}' onto trunk '{}'. This branch will appear as a direct child of trunk, not stacked.",
                    pr.number, unresolved_base, remote_name, pr.head_branch, trunk
                )
                .yellow()
            ),
            ParentSource::RemoteOnly => println!(
                "{}",
                format!(
                    "  ! PR #{}: parent '{}' exists only as '{}/{}'; fetch it locally before restacking.",
                    pr.number, decision.parent, remote_name, decision.parent
                )
                .yellow()
            ),
            _ => {}
        }

        // Use the divergence point (merge-base) rather than the parent's current tip.
        // Matches freephite's `trackBranch`: store `getMergeBase(branch, parent)` so
        // that `git rebase --onto` scopes the replay to only the branch's own commits.
        let parent_rev = match repo
            .merge_base_refs(&decision.parent_rev_ref, &pr.head_branch)
            .or_else(|_| repo.rev_parse(&decision.parent_rev_ref))
        {
            Ok(rev) => rev,
            Err(_) => {
                eprintln!(
                    "  {} Could not get parent revision for '{}'",
                    "✗".red(),
                    pr.head_branch
                );
                continue;
            }
        };

        // Create metadata with PR info
        let meta = BranchMetadata {
            parent_branch_name: decision.parent.clone(),
            parent_branch_revision: parent_rev,
            source_remote: None,
            frozen: false,
            pr_info: Some(PrInfo {
                number: pr.number,
                state: pr.state.to_uppercase(),
                is_draft: Some(pr.is_draft),
            }),
        };

        meta.write(repo.inner(), &pr.head_branch)?;
        local_after.insert(pr.head_branch.clone());

        let draft_indicator = if pr.is_draft { " (draft)" } else { "" };
        println!(
            "  {} Tracked '{}' (PR #{}{}) with parent '{}'",
            "✓".green(),
            pr.head_branch.green(),
            pr.number.to_string().yellow(),
            draft_indicator.dimmed(),
            decision.parent.blue()
        );
        tracked_count += 1;
    }

    let upstream_note = if upstream_set > 0 {
        format!(
            " Set upstream to '{}' on {} newly fetched branch(es).",
            remote_name, upstream_set
        )
    } else {
        String::new()
    };
    println!();
    println!(
        "Tracked {} branch(es), fetched {}, skipped {} (already tracked).{}",
        tracked_count.to_string().green(),
        fetched_count.to_string().blue(),
        skipped_count.to_string().dimmed(),
        upstream_note.dimmed()
    );

    Ok(())
}

/// Fetch several branches from `remote` in one invocation, creating local
/// branches of the same name. Errors carry git's stderr so callers can report it.
fn fetch_branches_batch(workdir: &Path, remote: &str, branches: &[String]) -> Result<()> {
    if branches.is_empty() {
        return Ok(());
    }
    let mut cmd = Command::new("git");
    cmd.args(["fetch", "--no-tags", remote]);
    for b in branches {
        cmd.arg(format!("{}:{}", b, b));
    }
    let output = cmd
        .current_dir(workdir)
        .output()
        .context("Failed to run git fetch")?;
    if !output.status.success() {
        anyhow::bail!(
            "git fetch {} failed ({}): {}",
            remote,
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// Local branch names read straight from git, so refs created by a subprocess
/// `git fetch` are always visible (bypasses any libgit2 ref caching).
fn local_branch_names(workdir: &Path) -> Result<HashSet<String>> {
    let output = Command::new("git")
        .args(["for-each-ref", "--format=%(refname:short)", "refs/heads"])
        .current_dir(workdir)
        .output()
        .context("Failed to list local branches")?;
    if !output.status.success() {
        anyhow::bail!(
            "git for-each-ref failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// Fetch a single branch from remote and create local tracking branch
fn fetch_branch_from_remote(workdir: &Path, remote: &str, branch: &str) -> Result<()> {
    let output = Command::new("git")
        .args([
            "fetch",
            "--no-tags",
            remote,
            &format!("{}:{}", branch, branch),
        ])
        .current_dir(workdir)
        .output()
        .context("Failed to run git fetch")?;

    if !output.status.success() {
        anyhow::bail!(
            "Failed to fetch branch '{}' from remote '{}' ({}): {}",
            branch,
            remote,
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(())
}
