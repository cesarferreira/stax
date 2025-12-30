use crate::cache::CiCache;
use crate::config::Config;
use crate::engine::Stack;
use crate::git::GitRepo;
use crate::github::GitHubClient;
use crate::remote::{self, RemoteInfo};
use anyhow::Result;
use colored::{Color, Colorize};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::process::Command;

// Colors for different columns (fp-style: each column has its own color)
// Avoiding yellow since it's used for "needs restack" indicator
const COLUMN_COLORS: &[Color] = &[
    Color::Cyan,
    Color::Green,
    Color::Magenta,
    Color::Blue,
    Color::BrightCyan,
    Color::BrightGreen,
    Color::BrightMagenta,
    Color::BrightBlue,
];

/// Represents a branch in the display with its column position
struct DisplayBranch {
    name: String,
    column: usize,
}

#[derive(Serialize, Clone)]
struct BranchStatusJson {
    name: String,
    parent: Option<String>,
    is_current: bool,
    is_trunk: bool,
    needs_restack: bool,
    pr_number: Option<u64>,
    pr_state: Option<String>,
    pr_is_draft: Option<bool>,
    pr_url: Option<String>,
    ci_state: Option<String>,
    ahead: usize,
    behind: usize,
    lines_added: usize,
    lines_deleted: usize,
    has_remote: bool,
}

#[derive(Serialize)]
struct StatusJson {
    trunk: String,
    current: String,
    branches: Vec<BranchStatusJson>,
}

pub fn run(
    json: bool,
    stack_filter: Option<String>,
    all: bool,
    compact: bool,
    quiet: bool,
    verbose: bool,
) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let config = Config::load()?;
    let workdir = repo.workdir()?;
    let has_tracked = stack.branches.len() > 1;
    let git_dir = repo.git_dir()?;

    let remote_info = RemoteInfo::from_repo(&repo, &config).ok();
    let remote_branches = remote::get_remote_branches(workdir, config.remote_name())
        .unwrap_or_default()
        .into_iter()
        .collect::<HashSet<_>>();

    // By default show only the current stack (like fp ls). Use --all to show all branches.
    let allowed_branches = if all {
        None // Show all branches
    } else if let Some(ref filter) = stack_filter {
        if !stack.branches.contains_key(filter) {
            anyhow::bail!("Branch '{}' is not tracked in the stack.", filter);
        }
        Some(stack.current_stack(filter).into_iter().collect::<HashSet<_>>())
    } else {
        // Default: show only current stack
        Some(stack.current_stack(&current).into_iter().collect::<HashSet<_>>())
    };

    // Get trunk children and build display list with proper tree structure
    let trunk_info = stack.branches.get(&stack.trunk);
    let trunk_children: Vec<String> = trunk_info
        .map(|b| b.children.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|b| allowed_branches.as_ref().is_none_or(|a| a.contains(b)))
        .collect();

    // Build display list: each trunk child gets its own column, stacked left to right
    let mut display_branches: Vec<DisplayBranch> = Vec::new();
    let mut max_column = 0;
    let mut sorted_trunk_children = trunk_children;
    // Sort trunk children alphabetically (like fp)
    sorted_trunk_children.sort();

    // Each trunk child gets column = index (first at 0, second at 1, etc.)
    for (i, root) in sorted_trunk_children.iter().enumerate() {
        collect_display_branches_with_nesting(
            &stack,
            root,
            i,      // column
            &mut display_branches,
            &mut max_column,
            allowed_branches.as_ref(),
        );
    }

    let tree_target_width = (max_column + 1) * 2;
    let mut ordered_branches: Vec<String> =
        display_branches.iter().map(|b| b.name.clone()).collect();
    ordered_branches.push(stack.trunk.clone());

    // Load CI cache and refresh if stale (TTL expired)
    let mut cache = CiCache::load(git_dir);
    if cache.is_stale() {
        let fresh_states = fetch_ci_states(&repo, remote_info.as_ref(), &stack, &ordered_branches);
        for (branch, state) in fresh_states {
            cache.update(&branch, Some(state), None);
        }
        cache.mark_refreshed();
        cache.cleanup(&ordered_branches);
        let _ = cache.save(git_dir);
    }

    // Build CI states from cache
    let ci_states: HashMap<String, String> = ordered_branches
        .iter()
        .filter_map(|b| cache.get_ci_state(b).map(|s| (b.clone(), s)))
        .collect();

    let mut branch_statuses: Vec<BranchStatusJson> = Vec::new();
    let mut branch_status_map: HashMap<String, BranchStatusJson> = HashMap::new();

    for name in &ordered_branches {
        let info = stack.branches.get(name);
        let parent = info.and_then(|b| b.parent.clone());
        let is_trunk = name == &stack.trunk;

        // For trunk, compare against remote tracking branch (e.g., origin/main)
        // For other branches, compare against parent (using libgit2, no subprocess)
        let (ahead, behind) = if is_trunk {
            let remote_ref = format!("{}/{}", config.remote_name(), name);
            repo.commits_ahead_behind(&remote_ref, name).unwrap_or((0, 0))
        } else {
            parent
                .as_deref()
                .and_then(|p| repo.commits_ahead_behind(p, name).ok())
                .unwrap_or((0, 0))
        };
        // Only compute line stats for JSON output (expensive subprocess per branch)
        let (lines_added, lines_deleted) = if json {
            parent
                .as_deref()
                .and_then(|p| get_line_diff_stats(workdir, p, name))
                .unwrap_or((0, 0))
        } else {
            (0, 0)
        };

        let pr_state = info
            .and_then(|b| b.pr_state.clone())
            .and_then(|s| if s.trim().is_empty() { None } else { Some(s) });

        let pr_number = info.and_then(|b| b.pr_number);
        let pr_url = pr_number.and_then(|n| remote_info.as_ref().map(|r| r.pr_url(n)));
        let ci_state = ci_states.get(name).cloned();

        let entry = BranchStatusJson {
            name: name.clone(),
            parent: parent.clone(),
            is_current: name == &current,
            is_trunk: name == &stack.trunk,
            needs_restack: info.map(|b| b.needs_restack).unwrap_or(false),
            pr_number,
            pr_state,
            pr_is_draft: info.and_then(|b| b.pr_is_draft),
            pr_url,
            ci_state,
            ahead,
            behind,
            lines_added,
            lines_deleted,
            has_remote: remote_branches.contains(name),
        };

        branch_status_map.insert(name.clone(), entry.clone());
        branch_statuses.push(entry);
    }

    if json {
        let output = StatusJson {
            trunk: stack.trunk.clone(),
            current: current.clone(),
            branches: branch_statuses,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    if compact {
        for entry in &branch_statuses {
            let parent = entry.parent.clone().unwrap_or_default();
            let pr_state = entry.pr_state.clone().unwrap_or_default();
            let pr_number = entry
                .pr_number
                .map(|n| n.to_string())
                .unwrap_or_default();
            let ci_state = entry.ci_state.clone().unwrap_or_default();
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                entry.name,
                parent,
                entry.ahead,
                entry.behind,
                pr_number,
                pr_state,
                ci_state,
                if entry.needs_restack { "restack" } else { "" }
            );
        }
        return Ok(());
    }

    // Render each branch
    for (i, db) in display_branches.iter().enumerate() {
        let branch = &db.name;
        let is_current = branch == &current;
        let entry = branch_status_map.get(branch);
        // Show cloud if branch exists on remote OR has a PR (PR implies it was pushed)
        let has_remote = remote_branches.contains(branch) || entry.and_then(|e| e.pr_number).is_some();

        // Check if we need a corner connector - this happens when the PREVIOUS branch was at a higher column
        // The corner shows that a side branch joins back to this level
        let prev_branch_col = if i > 0 { Some(display_branches[i - 1].column) } else { None };
        let needs_corner = prev_branch_col.is_some_and(|pc| pc > db.column);

        // Build tree graphics - pad to consistent width based on max_column
        let mut tree = String::new();
        let mut visual_width = 0;
        // Draw columns 0 to db.column
        for col in 0..=db.column {
            let col_color = COLUMN_COLORS[col % COLUMN_COLORS.len()];
            if col == db.column {
                // This is our column - draw circle
                let circle = if is_current { "◉" } else { "○" };
                tree.push_str(&format!("{}", circle.color(col_color)));
                visual_width += 1;

                // Check if we need corner connector (side branch ending)
                if needs_corner {
                    tree.push_str(&format!("{}", "─┘".color(col_color)));
                    visual_width += 2;
                }
            } else {
                // Columns to our left - always draw vertical lines for active columns
                tree.push_str(&format!("{} ", "│".color(col_color)));
                visual_width += 2;
            }
        }

        // Pad to consistent width so branch names align
        while visual_width < tree_target_width {
            tree.push(' ');
            visual_width += 1;
        }

        // Build info part
        let mut info_str = String::new();
        info_str.push(' '); // Space after tree

        // Show cloud icon or space for alignment
        if has_remote {
            info_str.push_str(&format!("{} ", "☁".bright_blue()));
        } else {
            info_str.push_str("  "); // Space for alignment when no remote
        }

        // Color branch names to match their column in the graph
        let branch_color = COLUMN_COLORS[db.column % COLUMN_COLORS.len()];
        if is_current {
            info_str.push_str(&format!("{}", branch.color(branch_color).bold()));
        } else {
            info_str.push_str(&format!("{}", branch.color(branch_color)));
        }

        if let Some(entry) = entry {
            // Show commits ahead/behind with arrows
            if entry.ahead > 0 || entry.behind > 0 {
                let mut commits_str = String::new();
                if entry.ahead > 0 {
                    commits_str.push_str(&format!(" {}", format!("{}↑", entry.ahead).green()));
                }
                if entry.behind > 0 {
                    commits_str.push_str(&format!(" {}", format!("{}↓", entry.behind).red()));
                }
                info_str.push_str(&commits_str);
            }

            // Show restack indicator
            if entry.needs_restack {
                if verbose {
                    info_str.push_str(&format!(" {}", "⟳".bright_yellow()));
                } else {
                    info_str.push_str(&format!(" {}", "(needs restack)".bright_yellow()));
                }
            }

            // Only show PR info in verbose mode (ll command)
            if verbose {
                if let Some(pr_number) = entry.pr_number {
                    let mut pr_text = format!(" PR #{}", pr_number);
                    if let Some(ref state) = entry.pr_state {
                        pr_text.push_str(&format!(" {}", state.to_lowercase()));
                    }
                    if entry.pr_is_draft.unwrap_or(false) {
                        pr_text.push_str(" draft");
                    }
                    if let Some(ref url) = entry.pr_url {
                        pr_text.push_str(&format!(" {}", url));
                    }
                    info_str.push_str(&format!("{}", pr_text.bright_magenta()));
                }
            }

            // Only show CI state in verbose mode (ll command)
            if verbose {
                if let Some(ref ci) = entry.ci_state {
                    info_str.push_str(&format!("{}", format!(" CI:{}", ci).bright_cyan()));
                }
            }
        }

        println!("{}{}", tree, info_str);
    }

    // Render trunk with corner connector (fp-style: ○─┘ for 1 col, ○─┴─┘ for 2, etc.)
    // Only connect columns used by direct trunk children, not nested columns
    let is_trunk_current = stack.trunk == current;
    let trunk_child_max_col = if sorted_trunk_children.is_empty() { 0 } else { sorted_trunk_children.len() - 1 };

    let mut trunk_tree = String::new();
    let mut trunk_visual_width = 0;

    let trunk_circle = if is_trunk_current { "◉" } else { "○" };
    let trunk_color = COLUMN_COLORS[0];
    trunk_tree.push_str(&format!("{}", trunk_circle.color(trunk_color)));
    trunk_visual_width += 1;

    // Show connectors only for trunk children columns: ─┴ for middle, ─┘ for last
    if trunk_child_max_col >= 1 {
        for col in 1..=trunk_child_max_col {
            let col_color = COLUMN_COLORS[col % COLUMN_COLORS.len()];
            if col < trunk_child_max_col {
                trunk_tree.push_str(&format!("{}", "─┴".color(col_color)));
            } else {
                trunk_tree.push_str(&format!("{}", "─┘".color(col_color)));
            }
            trunk_visual_width += 2;
        }
    }

    // Pad to match branch name alignment
    while trunk_visual_width < tree_target_width {
        trunk_tree.push(' ');
        trunk_visual_width += 1;
    }

    let mut trunk_info = String::new();
    trunk_info.push(' '); // Space after tree (same as branches)
    // Show cloud icon or space for alignment
    if remote_branches.contains(&stack.trunk) {
        trunk_info.push_str(&format!("{} ", "☁".bright_blue()));
    } else {
        trunk_info.push_str("  "); // Space for alignment when no remote
    }
    // Color trunk name to match column 0
    if is_trunk_current {
        trunk_info.push_str(&format!("{}", stack.trunk.color(trunk_color).bold()));
    } else {
        trunk_info.push_str(&format!("{}", stack.trunk.color(trunk_color)));
    }

    // Show commits ahead/behind for trunk (compared to origin)
    if let Some(entry) = branch_status_map.get(&stack.trunk) {
        if entry.ahead > 0 || entry.behind > 0 {
            if entry.ahead > 0 {
                trunk_info.push_str(&format!(" {}", format!("{}↑", entry.ahead).green()));
            }
            if entry.behind > 0 {
                trunk_info.push_str(&format!(" {}", format!("{}↓", entry.behind).red()));
            }
        }
    }

    println!("{}{}", trunk_tree, trunk_info);

    if !has_tracked && !quiet {
        println!("{}", "No tracked branches yet (showing trunk only).".dimmed());
        println!(
            "Use {} to start tracking branches.",
            "stax branch track".cyan()
        );
    }

    // Show restack hint (show in both ls and ll when tips enabled)
    let needs_restack = stack.needs_restack();
    let config = Config::load().unwrap_or_default();
    if !needs_restack.is_empty() && !quiet && config.ui.tips {
        println!();
        println!(
            "{}",
            format!("⟳ {} {} need restacking", needs_restack.len(), if needs_restack.len() == 1 { "branch" } else { "branches" }).bright_yellow()
        );
        println!("Run {} to rebase.", "stax rs --restack".bright_cyan());
    }

    // Show additional stats only in verbose mode (ll command)
    if verbose && !quiet && config.ui.tips {
        let total_branches = stack.branches.len().saturating_sub(1); // Exclude trunk
        let open_prs: usize = branch_statuses
            .iter()
            .filter(|b| {
                b.pr_number.is_some()
                    && b.pr_state
                        .as_ref()
                        .map(|s| s.to_lowercase() == "open")
                        .unwrap_or(false)
            })
            .count();
        let branches_with_remote: usize = branch_statuses
            .iter()
            .filter(|b| b.has_remote && !b.is_trunk)
            .count();

        // Only show summary if there are branches to show
        if total_branches > 0 {
            if needs_restack.is_empty() {
                println!(); // Add newline if we didn't already print restack hint
            }
            let mut stats = vec![format!("{} {}", total_branches, if total_branches == 1 { "branch" } else { "branches" })];
            if branches_with_remote > 0 {
                stats.push(format!("{} pushed", branches_with_remote));
            }
            if open_prs > 0 {
                stats.push(format!("{} open {}", open_prs, if open_prs == 1 { "PR" } else { "PRs" }));
            }
            println!("{}", stats.join(" · ").dimmed());
        }
    }

    Ok(())
}

/// Collect branches with proper nesting for branches that have multiple children
/// fp-style: children sorted alphabetically, each child gets column + index
fn collect_display_branches_with_nesting(
    stack: &Stack,
    branch: &str,
    base_column: usize,
    result: &mut Vec<DisplayBranch>,
    max_column: &mut usize,
    allowed: Option<&HashSet<String>>,
) {
    collect_recursive(stack, branch, base_column, result, max_column, allowed);
}

fn collect_recursive(
    stack: &Stack,
    branch: &str,
    column: usize,
    result: &mut Vec<DisplayBranch>,
    max_column: &mut usize,
    allowed: Option<&HashSet<String>>,
) {
    if allowed.is_some_and(|set| !set.contains(branch)) {
        return;
    }

    *max_column = (*max_column).max(column);

    if let Some(info) = stack.branches.get(branch) {
        let mut children: Vec<&String> = info
            .children
            .iter()
            .filter(|c| allowed.is_none_or(|set| set.contains(*c)))
            .collect();

        if !children.is_empty() {
            // Sort children alphabetically (like fp)
            children.sort();

            // Each child gets column + index: first child at same column, second at +1, etc.
            for (i, child) in children.iter().enumerate() {
                collect_recursive(
                    stack,
                    child,
                    column + i,
                    result,
                    max_column,
                    allowed,
                );
            }
        }
    }

    // Add current branch after all children are processed (post-order)
    result.push(DisplayBranch {
        name: branch.to_string(),
        column,
    });
}

/// Get line additions and deletions between parent and branch
fn get_line_diff_stats(
    workdir: &std::path::Path,
    parent: &str,
    branch: &str,
) -> Option<(usize, usize)> {
    let output = Command::new("git")
        .args(["diff", "--numstat", &format!("{}...{}", parent, branch)])
        .current_dir(workdir)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut additions = 0usize;
    let mut deletions = 0usize;

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 2 {
            // Binary files show "-" instead of numbers
            if let Ok(add) = parts[0].parse::<usize>() {
                additions += add;
            }
            if let Ok(del) = parts[1].parse::<usize>() {
                deletions += del;
            }
        }
    }

    Some((additions, deletions))
}

fn fetch_ci_states(
    repo: &GitRepo,
    remote_info: Option<&RemoteInfo>,
    stack: &Stack,
    branches: &[String],
) -> HashMap<String, String> {
    let Some(remote) = remote_info else {
        return HashMap::new();
    };

    if Config::github_token().is_none() {
        return HashMap::new();
    }

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return HashMap::new(),
    };

    let client = match rt.block_on(async {
        GitHubClient::new(remote.owner(), &remote.repo, remote.api_base_url.clone())
    }) {
        Ok(client) => client,
        Err(_) => return HashMap::new(),
    };

    let mut results = HashMap::new();
    for branch in branches {
        let has_pr = stack
            .branches
            .get(branch)
            .and_then(|b| b.pr_number)
            .is_some();

        if !has_pr {
            continue;
        }

        let sha = match repo.branch_commit(branch) {
            Ok(sha) => sha,
            Err(_) => continue,
        };

        let state = rt
            .block_on(async { client.combined_status_state(&sha).await })
            .ok()
            .flatten();

        if let Some(state) = state {
            results.insert(branch.clone(), state);
        }
    }

    results
}
