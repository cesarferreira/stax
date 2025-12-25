use crate::config::Config;
use crate::engine::Stack;
use crate::git::GitRepo;
use crate::github::GitHubClient;
use crate::remote::{self, Provider, RemoteInfo};
use anyhow::Result;
use colored::{Color, Colorize};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::process::Command;

// Colors for different depths
const DEPTH_COLORS: &[Color] = &[
    Color::Yellow,
    Color::Green,
    Color::Magenta,
    Color::Cyan,
    Color::Blue,
    Color::BrightRed,
    Color::BrightYellow,
    Color::BrightGreen,
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
) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let config = Config::load()?;
    let workdir = repo.workdir()?;
    let has_tracked = stack.branches.len() > 1;

    let remote_info = RemoteInfo::from_repo(&repo, &config).ok();
    let remote_branches = remote::get_remote_branches(workdir, config.remote_name())
        .unwrap_or_default()
        .into_iter()
        .collect::<HashSet<_>>();

    // By default show all branches (like fp ls). Use --stack to filter to a specific stack.
    let allowed_branches = if let Some(ref filter) = stack_filter {
        if !stack.branches.contains_key(filter) {
            anyhow::bail!("Branch '{}' is not tracked in the stack.", filter);
        }
        Some(stack.current_stack(filter).into_iter().collect::<HashSet<_>>())
    } else {
        None // Show all branches by default
    };
    let _ = all; // --all flag kept for backwards compatibility but is now the default

    // Get trunk children and build display list with proper tree structure
    let trunk_info = stack.branches.get(&stack.trunk);
    let trunk_children: Vec<String> = trunk_info
        .map(|b| b.children.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|b| allowed_branches.as_ref().map_or(true, |a| a.contains(b)))
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
            i,
            &mut display_branches,
            &mut max_column,
            allowed_branches.as_ref(),
        );
    }

    let tree_target_width = (max_column + 1) * 2;
    let mut ordered_branches: Vec<String> =
        display_branches.iter().map(|b| b.name.clone()).collect();
    ordered_branches.push(stack.trunk.clone());

    let ci_states =
        fetch_ci_states(&repo, remote_info.as_ref(), &stack, &ordered_branches);

    let mut branch_statuses: Vec<BranchStatusJson> = Vec::new();
    let mut branch_status_map: HashMap<String, BranchStatusJson> = HashMap::new();

    for name in &ordered_branches {
        let info = stack.branches.get(name);
        let parent = info.and_then(|b| b.parent.clone());
        let (ahead, behind) = parent
            .as_deref()
            .and_then(|p| get_commits_ahead_behind(workdir, p, name))
            .unwrap_or((0, 0));
        let (lines_added, lines_deleted) = parent
            .as_deref()
            .and_then(|p| get_line_diff_stats(workdir, p, name))
            .unwrap_or((0, 0));

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
        let has_remote = remote_branches.contains(branch);
        let color = DEPTH_COLORS[db.column % DEPTH_COLORS.len()];

        // Check if we need a corner connector - this happens when the PREVIOUS branch was at a higher column
        // The corner shows that a side branch joins back to this level
        let prev_branch_col = if i > 0 { Some(display_branches[i - 1].column) } else { None };
        let needs_corner = prev_branch_col.map_or(false, |pc| pc > db.column);

        // Build tree graphics - pad to consistent width based on max_column
        let mut tree = String::new();
        let mut visual_width = 0;
        // Draw columns 0 to db.column
        for col in 0..=db.column {
            if col == db.column {
                // This is our column - draw circle
                let circle = if is_current { "◉" } else { "○" };
                tree.push_str(&format!("{}", circle.color(color)));
                visual_width += 1;

                // Check if we need corner connector (side branch ending)
                if needs_corner {
                    tree.push_str(&format!("{}", "─┘".color(color)));
                    visual_width += 2;
                }
            } else {
                // Columns to our left - always draw vertical lines for active columns
                let line_color = DEPTH_COLORS[col % DEPTH_COLORS.len()];
                tree.push_str(&format!("{} ", "│".color(line_color)));
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

        if has_remote {
            info_str.push_str(&format!("{} ", "☁".bright_blue()));
        }

        if is_current {
            info_str.push_str(&format!("{}", branch.bold()));
        } else {
            info_str.push_str(branch);
        }

        if let Some(entry) = branch_status_map.get(branch) {
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

            // Show restack icon
            if entry.needs_restack {
                info_str.push_str(&format!(" {}", "⟳".bright_yellow()));
            }

            if let Some(pr_number) = entry.pr_number {
                let pr_label = remote_info
                    .as_ref()
                    .map(|r| r.provider.pr_label())
                    .unwrap_or("PR");
                let mut pr_text = format!(" {} #{}", pr_label, pr_number);
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

            if let Some(ref ci) = entry.ci_state {
                info_str.push_str(&format!("{}", format!(" CI:{}", ci).bright_cyan()));
            }
        }

        println!("{}{}", tree, info_str);
    }

    // Render trunk with corner connector (fp-style: ○─┘ for 1 col, ○─┴─┘ for 2, ○─┴─┴─┘ for 3, etc.)
    let is_trunk_current = stack.trunk == current;
    let trunk_color = DEPTH_COLORS[0];

    let mut trunk_tree = String::new();
    let mut trunk_visual_width = 0;

    let trunk_circle = if is_trunk_current { "◉" } else { "○" };
    trunk_tree.push_str(&format!("{}", trunk_circle.color(trunk_color)));
    trunk_visual_width += 1;

    // Show connectors for all columns: ─┴ for middle columns, ─┘ for the last
    if max_column >= 1 {
        for col in 1..=max_column {
            if col < max_column {
                trunk_tree.push_str(&format!("{}", "─┴".color(trunk_color)));
            } else {
                trunk_tree.push_str(&format!("{}", "─┘".color(trunk_color)));
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
    if remote_branches.contains(&stack.trunk) {
        trunk_info.push_str(&format!("{} ", "☁".bright_blue()));
    }
    if is_trunk_current {
        trunk_info.push_str(&format!("{}", stack.trunk.bold()));
    } else {
        trunk_info.push_str(&stack.trunk);
    }

    println!("{}{}", trunk_tree, trunk_info);

    if !has_tracked && !quiet {
        println!("{}", "No tracked branches yet (showing trunk only).".dimmed());
        println!(
            "Use {} to start tracking branches.",
            "stax branch track".cyan()
        );
    }

    // Show restack hint
    let needs_restack = stack.needs_restack();
    if !needs_restack.is_empty() && !quiet {
        println!();
        println!(
            "{}",
            format!("⟳ {} branch(es) need restacking", needs_restack.len()).bright_yellow()
        );
        println!("Run {} to rebase.", "stax rs --restack".bright_cyan());
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
            .filter(|c| allowed.map_or(true, |set| set.contains(*c)))
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

fn get_commits_ahead_behind(
    workdir: &std::path::Path,
    parent: &str,
    branch: &str,
) -> Option<(usize, usize)> {
    let ahead_output = Command::new("git")
        .args(["rev-list", "--count", &format!("{}..{}", parent, branch)])
        .current_dir(workdir)
        .output()
        .ok()?;

    let ahead = if ahead_output.status.success() {
        String::from_utf8_lossy(&ahead_output.stdout)
            .trim()
            .parse()
            .ok()?
    } else {
        0
    };

    let behind_output = Command::new("git")
        .args(["rev-list", "--count", &format!("{}..{}", branch, parent)])
        .current_dir(workdir)
        .output()
        .ok()?;

    let behind = if behind_output.status.success() {
        String::from_utf8_lossy(&behind_output.stdout)
            .trim()
            .parse()
            .ok()?
    } else {
        0
    };

    Some((ahead, behind))
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

    if remote.provider != Provider::GitHub {
        return HashMap::new();
    }

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
