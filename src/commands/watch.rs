use crate::cache::CiCache;
use crate::commands::ci::{fetch_ci_statuses, update_ci_cache, BranchCiStatus};
use crate::config::Config;
use crate::engine::Stack;
use crate::forge::ForgeClient;
use crate::git::GitRepo;
use crate::remote::{self, RemoteInfo};
use anyhow::Result;
use chrono::Local;
use colored::Colorize;
use std::collections::HashSet;
use std::io::Write as _;
use std::time::Duration;

const DEFAULT_INTERVAL_ACTIVE: u64 = 15;
const DEFAULT_INTERVAL_IDLE: u64 = 60;
const DEFAULT_INTERVAL_QUIET: u64 = 120;

pub fn run(current_only: bool, interval: Option<u64>) -> Result<()> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let remote_info = RemoteInfo::from_repo(&repo, &config)?;
    let rt = tokio::runtime::Runtime::new()?;
    let _enter = rt.enter();
    let client = ForgeClient::new(&remote_info)?;

    println!(
        "{}",
        "Watching stack... (Ctrl+C to stop)".cyan().bold()
    );

    let mut iteration = 0usize;

    loop {
        // Reload stack each iteration to pick up local branch changes
        let stack = Stack::load(&repo)?;
        let current = repo.current_branch()?;
        let git_dir = repo.git_dir()?;
        let workdir = repo.workdir()?;

        let branches_to_watch: Vec<String> = if current_only {
            stack
                .current_stack(&current)
                .into_iter()
                .filter(|b| b != &stack.trunk)
                .collect()
        } else {
            let mut branches: Vec<String> = stack
                .branches
                .keys()
                .filter(|b| *b != &stack.trunk)
                .cloned()
                .collect();
            branches.sort();
            branches
        };

        // Fetch live CI statuses and refresh the cache
        let ci_statuses: Vec<BranchCiStatus> = if !branches_to_watch.is_empty() {
            match fetch_ci_statuses(&repo, &rt, &client, &stack, &branches_to_watch) {
                Ok(s) => {
                    update_ci_cache(&repo, &stack, &s);
                    s
                }
                Err(_) => {
                    // Fall back to cached data on network errors
                    load_ci_from_cache(git_dir, &branches_to_watch)
                }
            }
        } else {
            vec![]
        };

        // Collect remote branch set for the ☁ indicator
        let remote_branches: HashSet<String> =
            remote::get_remote_branches(workdir, config.remote_name())
                .unwrap_or_default()
                .into_iter()
                .collect();

        iteration += 1;

        // Clear terminal (skip on first iteration so the initial message is visible briefly)
        if iteration > 1 {
            print!("\x1B[2J\x1B[H");
            let _ = std::io::stdout().flush();
        }

        let now = Local::now().format("%H:%M:%S");
        println!(
            "{}{}",
            "⟳  Watching stack  (Ctrl+C to stop)".cyan().bold(),
            format!("          {}", now).dimmed(),
        );
        println!();

        if branches_to_watch.is_empty() {
            println!("{}", "No tracked branches.".dimmed());
        } else {
            render_watch_table(&stack, &current, &branches_to_watch, &ci_statuses, &remote_branches);
        }

        // Decide next interval
        let next_interval = interval.unwrap_or_else(|| {
            adaptive_interval(&ci_statuses, &stack, &branches_to_watch)
        });

        println!();
        println!(
            "{}",
            format!(
                "Refreshing in {}s… (iteration #{})",
                next_interval, iteration
            )
            .dimmed()
        );

        std::thread::sleep(Duration::from_secs(next_interval));
    }
}

fn render_watch_table(
    stack: &Stack,
    current: &str,
    branches: &[String],
    ci_statuses: &[BranchCiStatus],
    remote_branches: &HashSet<String>,
) {
    let ci_by_branch: std::collections::HashMap<&str, &BranchCiStatus> = ci_statuses
        .iter()
        .map(|s| (s.branch.as_str(), s))
        .collect();

    let name_width = branches
        .iter()
        .map(|b| b.len())
        .max()
        .unwrap_or(10)
        .max(10);

    for branch in branches {
        let info = stack.branches.get(branch);
        let is_current = branch == current;

        let marker = if is_current {
            "◉".cyan().bold().to_string()
        } else {
            "○".dimmed().to_string()
        };

        let cloud = if remote_branches.contains(branch)
            || info.and_then(|b| b.pr_number).is_some()
        {
            "☁".bright_blue().to_string()
        } else {
            " ".to_string()
        };

        let branch_display = if is_current {
            format!("{:<width$}", branch, width = name_width)
                .bold()
                .to_string()
        } else {
            format!("{:<width$}", branch, width = name_width)
        };

        // Stack status
        let stack_tag = if info.map(|b| b.needs_restack).unwrap_or(false) {
            "⟳ restack".bright_yellow().to_string()
        } else {
            "         ".to_string()
        };

        // CI status
        let ci_tag = match ci_by_branch.get(branch.as_str()) {
            None => "  –          ".dimmed().to_string(),
            Some(s) if s.check_runs.is_empty() => "  –          ".dimmed().to_string(),
            Some(s) => match s.overall_status.as_deref() {
                Some("success") => "  ✓ passed   ".green().to_string(),
                Some("failure") => "  ✗ failed   ".red().bold().to_string(),
                Some("pending") => "  ● running… ".yellow().to_string(),
                _ => "  ○ unknown  ".dimmed().to_string(),
            },
        };

        // PR info
        let pr_tag = match info.and_then(|b| b.pr_number) {
            Some(n) => {
                let state = info
                    .and_then(|b| b.pr_state.as_deref())
                    .unwrap_or("open");
                let draft = ci_by_branch
                    .get(branch.as_str())
                    .and_then(|s| s.pr_is_draft)
                    .or_else(|| info.and_then(|b| b.pr_is_draft))
                    .unwrap_or(false);
                if draft {
                    format!("  PR #{} draft", n).dimmed().to_string()
                } else if state.to_lowercase() == "merged" {
                    format!("  PR #{} merged", n).bright_magenta().to_string()
                } else {
                    format!("  PR #{}", n).bright_magenta().to_string()
                }
            }
            None => String::new(),
        };

        println!(
            "  {}  {} {}  {}  {}{}",
            marker, cloud, branch_display, stack_tag, ci_tag, pr_tag
        );
    }

    // Always render trunk at the bottom
    let trunk = &stack.trunk;
    let is_trunk_current = trunk == current;
    let trunk_marker = if is_trunk_current {
        "◉".cyan().bold().to_string()
    } else {
        "○".dimmed().to_string()
    };
    let trunk_cloud = if remote_branches.contains(trunk) {
        "☁".bright_blue().to_string()
    } else {
        " ".to_string()
    };
    println!(
        "  {}  {} {}",
        trunk_marker,
        trunk_cloud,
        trunk.dimmed()
    );
}

fn load_ci_from_cache(git_dir: &std::path::Path, branches: &[String]) -> Vec<BranchCiStatus> {
    let cache = CiCache::load(git_dir);
    branches
        .iter()
        .filter_map(|b| {
            cache.get_ci_state(b).map(|_| {
                let pr_is_draft = cache
                    .branches
                    .get(b.as_str())
                    .and_then(|e| e.pr_state.as_deref())
                    .map(|s| s.eq_ignore_ascii_case("draft"));
                BranchCiStatus {
                    branch: b.clone(),
                    sha: String::new(),
                    sha_short: String::new(),
                    overall_status: cache.get_ci_state(b),
                    check_runs: vec![],
                    pr_number: None,
                    pr_is_draft,
                }
            })
        })
        .collect()
}

fn adaptive_interval(
    ci_statuses: &[BranchCiStatus],
    stack: &Stack,
    branches: &[String],
) -> u64 {
    // Any CI actively running → poll fast
    let any_running = ci_statuses.iter().any(|s| {
        s.check_runs.iter().any(|c| {
            matches!(
                c.status.as_str(),
                "in_progress" | "queued" | "waiting" | "requested" | "pending"
            )
        })
    });
    if any_running {
        return DEFAULT_INTERVAL_ACTIVE;
    }

    // Any open PRs → poll at medium rate (PR state can change)
    let any_open_prs = branches.iter().any(|b| {
        stack
            .branches
            .get(b)
            .and_then(|br| br.pr_state.as_deref())
            .map(|s| s.to_lowercase() == "open")
            .unwrap_or(false)
    });
    if any_open_prs {
        return DEFAULT_INTERVAL_IDLE;
    }

    // Nothing active — back off
    DEFAULT_INTERVAL_QUIET
}
