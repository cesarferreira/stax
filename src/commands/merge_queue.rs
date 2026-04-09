//! Enqueue a stack into the forge's merge queue — no polling, no local git.
//!
//! Retargets all stack PRs to trunk, then enqueues them via the forge's
//! merge queue API (GitHub GraphQL `enqueuePullRequest`, GitLab merge
//! trains REST API).  The forge handles CI and merging.

use crate::config::Config;
use crate::engine::Stack;
use crate::forge::ForgeClient;
use crate::git::GitRepo;
use crate::progress::LiveTimer;
use crate::remote::{ForgeType, RemoteInfo};
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm};

#[derive(Debug)]
struct QueueBranchInfo {
    branch: String,
    pr_number: u64,
}

pub fn run(all: bool, yes: bool, quiet: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let config = Config::load()?;

    if current == stack.trunk {
        if !quiet {
            println!(
                "{}",
                "You are on trunk. Checkout a branch in a stack to merge.".yellow()
            );
        }
        return Ok(());
    }

    if !stack.branches.contains_key(&current) {
        if !quiet {
            println!(
                "{}",
                format!(
                    "Branch '{}' is not tracked. Run 'stax branch track' first.",
                    current
                )
                .yellow()
            );
        }
        return Ok(());
    }

    let remote_info = RemoteInfo::from_repo(&repo, &config)
        .context("Failed to read git remote configuration")?;
    if remote_info.forge == ForgeType::Gitea {
        anyhow::bail!(
            "`stax merge --queue` is not supported for Gitea/Forgejo — \
             Gitea does not have a merge queue feature.\n\
             Tip: use `stax merge` or `stax merge --when-ready` instead."
        );
    }

    let client = ForgeClient::new(&remote_info).context(
        "Failed to connect to the configured forge. Check your token and remote configuration.",
    )?;
    let forge_name = remote_info.forge.to_string();

    let rt = tokio::runtime::Runtime::new()?;
    let _enter = rt.enter();

    let (to_queue, trunk) = calculate_queue_scope(&stack, &current, all);

    let fetch_timer = LiveTimer::maybe_new(!quiet, "Fetching PR info...");

    let open_prs = rt
        .block_on(async { client.list_open_prs_by_head().await })
        .ok();

    let mut branches: Vec<QueueBranchInfo> = Vec::new();
    for branch_name in &to_queue {
        let pr_number = stack
            .branches
            .get(branch_name)
            .and_then(|b| b.pr_number)
            .or_else(|| {
                open_prs
                    .as_ref()
                    .and_then(|prs| prs.get(branch_name))
                    .map(|pr| pr.info.number)
            });

        match pr_number {
            Some(num) => branches.push(QueueBranchInfo {
                branch: branch_name.clone(),
                pr_number: num,
            }),
            None => {
                LiveTimer::maybe_finish_err(fetch_timer, "missing PR");
                anyhow::bail!(
                    "Branch '{}' has no PR. Run 'stax submit' first to create PRs.",
                    branch_name
                );
            }
        }
    }

    LiveTimer::maybe_finish_ok(fetch_timer, "done");

    if branches.is_empty() {
        if !quiet {
            println!("{}", "No branches to enqueue.".yellow());
        }
        return Ok(());
    }

    if !quiet {
        println!();
        print_header("Merge Queue");
        println!();
        let pr_word = if branches.len() == 1 { "PR" } else { "PRs" };
        println!(
            "Will retarget and enqueue {} {} into {}'s merge queue:",
            branches.len().to_string().bold(),
            pr_word,
            trunk.cyan()
        );
        println!();
        for (idx, branch) in branches.iter().enumerate() {
            println!(
                "  {}. {} (#{})",
                (idx + 1).to_string().bold(),
                branch.branch.bold(),
                branch.pr_number,
            );
        }
        println!();
        println!(
            "{}",
            format!(
                "{} will run CI on the combined changes and merge automatically.",
                forge_name
            )
            .dimmed()
        );
    }

    if !yes {
        let confirm = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Proceed with merge --queue?")
            .default(false)
            .interact()?;

        if !confirm {
            println!("{}", "Aborted.".dimmed());
            return Ok(());
        }
    }

    if !quiet {
        println!();
        print_header("Enqueuing");
    }

    let total = branches.len();
    let mut enqueued: Vec<(String, u64, Option<u32>)> = Vec::new();
    let mut failed: Option<(String, u64, String)> = None;

    for (idx, branch) in branches.iter().enumerate() {
        if !quiet {
            println!(
                "\n[{}/{}] {} (#{})",
                (idx + 1).to_string().cyan(),
                total,
                branch.branch.bold(),
                branch.pr_number
            );
        }

        match rt.block_on(async { client.is_pr_merged(branch.pr_number).await }) {
            Ok(true) => {
                if !quiet {
                    println!("      {} Already merged", "✓".green());
                }
                continue;
            }
            Ok(false) => {}
            Err(e) => {
                failed = Some((
                    branch.branch.clone(),
                    branch.pr_number,
                    format!("Failed to check merge status: {}", e),
                ));
                break;
            }
        }

        let retarget_timer = LiveTimer::maybe_new(
            !quiet,
            &format!("Retargeting #{} to {}...", branch.pr_number, trunk),
        );

        match rt.block_on(async { client.update_pr_base(branch.pr_number, &trunk).await }) {
            Ok(()) => LiveTimer::maybe_finish_ok(retarget_timer, "done"),
            Err(e) => {
                LiveTimer::maybe_finish_err(retarget_timer, "failed");
                failed = Some((
                    branch.branch.clone(),
                    branch.pr_number,
                    format!("Failed to retarget PR: {}", e),
                ));
                break;
            }
        }

        let enqueue_timer =
            LiveTimer::maybe_new(!quiet, &format!("Enqueuing #{}...", branch.pr_number));

        match rt.block_on(async { client.enqueue_pr(branch.pr_number).await }) {
            Ok(result) => {
                let position = result.merge_queue_entry.and_then(|e| e.position);
                let msg = match position {
                    Some(pos) => format!("queued at position {}", pos),
                    None => "queued".to_string(),
                };
                LiveTimer::maybe_finish_ok(enqueue_timer, &msg);
                enqueued.push((branch.branch.clone(), branch.pr_number, position));
            }
            Err(e) => {
                LiveTimer::maybe_finish_err(enqueue_timer, "failed");
                failed = Some((
                    branch.branch.clone(),
                    branch.pr_number,
                    format!("Failed to enqueue: {}", e),
                ));
                break;
            }
        }
    }

    println!();

    if let Some((branch, pr, reason)) = &failed {
        print_header_error("Merge Queue Failed");
        println!();
        println!("Progress:");
        for (queued_branch, queued_pr, _) in &enqueued {
            println!(
                "  {} #{} {} → enqueued",
                "✓".green(),
                queued_pr,
                queued_branch
            );
        }
        println!("  {} #{} {} → {}", "✗".red(), pr, branch, reason);
        println!();
        println!(
            "{}",
            "Already enqueued PRs remain in the merge queue.".dimmed()
        );
        println!(
            "{}",
            "Fix the issue and run 'stax merge --queue' to continue.".dimmed()
        );
    } else if enqueued.is_empty() {
        if !quiet {
            println!("{}", "All PRs were already merged.".dimmed());
        }
    } else {
        print_header_success("Stack Enqueued");
        println!();
        println!(
            "Enqueued {} {} into {}'s merge queue:",
            enqueued.len(),
            if enqueued.len() == 1 { "PR" } else { "PRs" },
            trunk.cyan()
        );
        for (branch, pr, position) in &enqueued {
            let pos_str = match position {
                Some(pos) => format!(" (position {})", pos),
                None => String::new(),
            };
            println!("  {} #{} {}{}", "✓".green(), pr, branch, pos_str.dimmed());
        }

        println!();
        println!(
            "{}",
            format!(
                "{} will run CI and merge automatically. Run `stax rs` to sync once merged.",
                forge_name
            )
            .dimmed()
        );
    }

    Ok(())
}

fn calculate_queue_scope(stack: &Stack, current: &str, all: bool) -> (Vec<String>, String) {
    let mut to_queue = stack.ancestors(current);
    to_queue.reverse();
    to_queue.retain(|b| b != &stack.trunk);
    to_queue.push(current.to_string());

    if all {
        to_queue.extend(stack.descendants(current));
    }

    (to_queue, stack.trunk.clone())
}

// --- Display helpers (same as merge_remote.rs) ---

fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_escape = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
            continue;
        }
        if in_escape {
            if c == 'm' {
                in_escape = false;
            }
            continue;
        }
        result.push(c);
    }
    result
}

fn display_width(s: &str) -> usize {
    let stripped = strip_ansi(s);
    stripped
        .chars()
        .map(|c| match c {
            '\x00'..='\x1f' | '\x7f' => 0,
            '\x20'..='\x7e' => 1,
            '─' | '│' | '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' | '╭' | '╮'
            | '╯' | '╰' | '║' | '═' => 1,
            '←' | '→' | '↑' | '↓' => 1,
            '✓' | '✗' | '✔' | '✘' => 1,
            _ => 2,
        })
        .sum()
}

fn print_header(title: &str) {
    let width: usize = 56;
    let title_width = display_width(title);
    let padding = width.saturating_sub(title_width) / 2;
    println!("╭{}╮", "─".repeat(width));
    println!(
        "│{}{}{}│",
        " ".repeat(padding),
        title.bold(),
        " ".repeat(width.saturating_sub(padding + title_width))
    );
    println!("╰{}╯", "─".repeat(width));
}

fn print_header_success(title: &str) {
    let width: usize = 56;
    let full_title = format!("✓ {}", title);
    let title_width = display_width(&full_title);
    let padding = width.saturating_sub(title_width) / 2;
    println!("╭{}╮", "─".repeat(width));
    println!(
        "│{}{}{}│",
        " ".repeat(padding),
        full_title.green().bold(),
        " ".repeat(width.saturating_sub(padding + title_width))
    );
    println!("╰{}╯", "─".repeat(width));
}

fn print_header_error(title: &str) {
    let width: usize = 56;
    let full_title = format!("✗ {}", title);
    let title_width = display_width(&full_title);
    let padding = width.saturating_sub(title_width) / 2;
    println!("╭{}╮", "─".repeat(width));
    println!(
        "│{}{}{}│",
        " ".repeat(padding),
        full_title.red().bold(),
        " ".repeat(width.saturating_sub(padding + title_width))
    );
    println!("╰{}╯", "─".repeat(width));
}
