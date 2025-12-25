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

// Colors for different depths (matching status.rs)
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
struct CommitJson {
    short_hash: String,
    message: String,
}

#[derive(Serialize, Clone)]
struct BranchLogJson {
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
    has_remote: bool,
    age: Option<String>,
    commits: Vec<CommitJson>,
}

#[derive(Serialize)]
struct LogJson {
    trunk: String,
    current: String,
    branches: Vec<BranchLogJson>,
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
    let workdir = repo.workdir()?;
    let config = Config::load()?;
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
    let mut next_column = 0;
    let mut sorted_trunk_children = trunk_children;
    sorted_trunk_children.sort_by(|a, b| {
        let size_a = count_chain_size(&stack, a, allowed_branches.as_ref());
        let size_b = count_chain_size(&stack, b, allowed_branches.as_ref());
        size_b.cmp(&size_a).then_with(|| a.cmp(b))
    });

    for root in &sorted_trunk_children {
        collect_display_branches_with_nesting(
            &stack,
            root,
            next_column,
            &mut display_branches,
            &mut max_column,
            allowed_branches.as_ref(),
        );
        next_column = max_column + 1;
    }

    let tree_target_width = (max_column + 1) * 2;

    let mut ordered_branches: Vec<String> =
        display_branches.iter().map(|b| b.name.clone()).collect();
    ordered_branches.push(stack.trunk.clone());

    let ci_states =
        fetch_ci_states(&repo, remote_info.as_ref(), &stack, &ordered_branches);

    let mut branch_logs: Vec<BranchLogJson> = Vec::new();
    let mut branch_log_map: HashMap<String, BranchLogJson> = HashMap::new();

    for name in &ordered_branches {
        let info = stack.branches.get(name);
        let parent = info.and_then(|b| b.parent.clone());
        let (ahead, behind) = parent
            .as_deref()
            .and_then(|p| get_commits_ahead_behind(workdir, p, name))
            .unwrap_or((0, 0));

        let pr_state = info
            .and_then(|b| b.pr_state.clone())
            .and_then(|s| if s.trim().is_empty() { None } else { Some(s) });

        let pr_number = info.and_then(|b| b.pr_number);
        let pr_url = pr_number.and_then(|n| remote_info.as_ref().map(|r| r.pr_url(n)));
        let ci_state = ci_states.get(name).cloned();

        let commits = repo
            .branch_commits(name, parent.as_deref())
            .unwrap_or_default()
            .into_iter()
            .map(|c| CommitJson {
                short_hash: c.short_hash,
                message: c.message,
            })
            .collect::<Vec<_>>();

        let age = repo.branch_age(name).ok();

        let entry = BranchLogJson {
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
            has_remote: remote_branches.contains(name),
            age,
            commits,
        };

        branch_log_map.insert(name.clone(), entry.clone());
        branch_logs.push(entry);
    }

    if json {
        let output = LogJson {
            trunk: stack.trunk.clone(),
            current: current.clone(),
            branches: branch_logs,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    if compact {
        for entry in &branch_logs {
            let parent = entry.parent.clone().unwrap_or_default();
            let pr_state = entry.pr_state.clone().unwrap_or_default();
            let pr_number = entry
                .pr_number
                .map(|n| n.to_string())
                .unwrap_or_default();
            let ci_state = entry.ci_state.clone().unwrap_or_default();
            let age = entry.age.clone().unwrap_or_default();
            let last_commit = entry
                .commits
                .first()
                .map(|c| format!("{} {}", c.short_hash, c.message))
                .unwrap_or_default();
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                entry.name,
                parent,
                entry.ahead,
                entry.behind,
                pr_number,
                pr_state,
                ci_state,
                age,
                last_commit
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

        // Check if we need a corner connector
        let prev_branch_col = if i > 0 { Some(display_branches[i - 1].column) } else { None };
        let needs_corner = prev_branch_col.map_or(false, |pc| pc > db.column);

        // Build tree graphics
        let mut tree = String::new();
        let mut visual_width = 0;

        for col in 0..=db.column {
            if col == db.column {
                let circle = if is_current { "◉" } else { "○" };
                tree.push_str(&format!("{}", circle.color(color)));
                visual_width += 1;

                if needs_corner {
                    tree.push_str(&format!("{}", "─┘".color(color)));
                    visual_width += 2;
                }
            } else {
                let line_color = DEPTH_COLORS[col % DEPTH_COLORS.len()];
                tree.push_str(&format!("{} ", "│".color(line_color)));
                visual_width += 2;
            }
        }

        // Pad to consistent width
        while visual_width < tree_target_width {
            tree.push(' ');
            visual_width += 1;
        }

        // Build info part
        let mut info_str = String::new();
        info_str.push(' ');

        // Remote indicator
        if has_remote {
            info_str.push_str(&format!("{} ", "☁".bright_blue()));
        }

        // Branch name
        if is_current {
            info_str.push_str(&format!("{}", branch.bold()));
        } else {
            info_str.push_str(branch);
        }

        if let Some(entry) = branch_log_map.get(branch) {
            if entry.ahead > 0 || entry.behind > 0 {
                let mut commit_info = String::new();
                if entry.ahead > 0 {
                    commit_info.push_str(&format!(" +{}", entry.ahead));
                }
                if entry.behind > 0 {
                    commit_info.push_str(&format!(" -{}", entry.behind));
                }
                if is_current {
                    info_str.push_str(&format!("{}", commit_info.bright_green()));
                } else {
                    info_str.push_str(&format!("{}", commit_info.dimmed()));
                }
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

            if entry.needs_restack {
                info_str.push_str(&format!("{}", " ↻ needs restack".bright_yellow()));
            }
        }

        println!("{}{}", tree, info_str);

        // Show commits for this branch
        if let Some(entry) = branch_log_map.get(branch) {
            let detail_prefix =
                build_detail_prefix(&display_branches, i, tree_target_width, max_column);

            if let Some(ref age) = entry.age {
                println!("{}   {}", detail_prefix, age.dimmed());
            }

            for commit in entry.commits.iter().take(3) {
                println!(
                    "{}   {} {}",
                    detail_prefix,
                    commit.short_hash.bright_yellow(),
                    commit.message.white()
                );
            }
        }
    }

    // Render trunk with corner connector
    let is_trunk_current = stack.trunk == current;
    let trunk_color = DEPTH_COLORS[0];

    let mut trunk_tree = String::new();
    let mut trunk_visual_width = 0;

    let trunk_circle = if is_trunk_current { "◉" } else { "○" };
    trunk_tree.push_str(&format!("{}", trunk_circle.color(trunk_color)));
    trunk_visual_width += 1;

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

    while trunk_visual_width < tree_target_width {
        trunk_tree.push(' ');
        trunk_visual_width += 1;
    }

    let mut trunk_info_str = String::new();
    trunk_info_str.push(' ');
    if remote_branches.contains(&stack.trunk) {
        trunk_info_str.push_str(&format!("{} ", "☁".bright_blue()));
    }
    if is_trunk_current {
        trunk_info_str.push_str(&format!("{}", stack.trunk.bold()));
    } else {
        trunk_info_str.push_str(&stack.trunk);
    }

    println!("{}{}", trunk_tree, trunk_info_str);

    // Trunk details
    let trunk_detail_prefix = " ".repeat(tree_target_width);
    if let Some(entry) = branch_log_map.get(&stack.trunk) {
        if let Some(ref age) = entry.age {
            println!("{}   {}", trunk_detail_prefix, age.dimmed());
        }
        for commit in entry.commits.iter().take(3) {
            println!(
                "{}   {} {}",
                trunk_detail_prefix,
                commit.short_hash.bright_yellow(),
                commit.message.white()
            );
        }
    }

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
            format!("↻ {} branch(es) need restacking", needs_restack.len()).bright_yellow()
        );
        println!("Run {} to rebase.", "stax rs --restack".bright_cyan());
    }

    Ok(())
}

fn build_detail_prefix(
    display_branches: &[DisplayBranch],
    current_idx: usize,
    tree_target_width: usize,
    _max_column: usize,
) -> String {
    let current_col = display_branches[current_idx].column;
    let mut prefix = String::new();
    let mut visual_width = 0;

    for col in 0..=current_col {
        let line_color = DEPTH_COLORS[col % DEPTH_COLORS.len()];
        prefix.push_str(&format!("{} ", "│".color(line_color)));
        visual_width += 2;
    }

    while visual_width < tree_target_width {
        prefix.push(' ');
        visual_width += 1;
    }

    prefix
}

/// Collect branches with proper nesting for branches that have multiple children
fn collect_display_branches_with_nesting(
    stack: &Stack,
    branch: &str,
    column: usize,
    result: &mut Vec<DisplayBranch>,
    max_column: &mut usize,
    allowed: Option<&HashSet<String>>,
) {
    if allowed.map_or(false, |set| !set.contains(branch)) {
        return;
    }

    *max_column = (*max_column).max(column);

    if let Some(info) = stack.branches.get(branch) {
        let children = &info.children;

        if children.len() > 1 {
            let mut children_with_sizes: Vec<(&String, usize)> = children
                .iter()
                .map(|c| (c, count_chain_size(stack, c, allowed)))
                .collect();

            children_with_sizes.sort_by(|a, b| {
                b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0))
            });

            let main_child = children_with_sizes[0].0;
            let side_children: Vec<&String> = children_with_sizes[1..].iter().map(|(c, _)| *c).collect();

            collect_display_branches_with_nesting(
                stack,
                main_child,
                column,
                result,
                max_column,
                allowed,
            );

            for side in &side_children {
                collect_display_branches_with_nesting(
                    stack,
                    side,
                    column + 1,
                    result,
                    max_column,
                    allowed,
                );
            }
        } else if children.len() == 1 {
            collect_display_branches_with_nesting(
                stack,
                &children[0],
                column,
                result,
                max_column,
                allowed,
            );
        }
    }

    result.push(DisplayBranch {
        name: branch.to_string(),
        column,
    });
}

fn get_commits_ahead_behind(workdir: &std::path::Path, parent: &str, branch: &str) -> Option<(usize, usize)> {
    // Commits ahead: parent..branch
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

    // Commits behind: branch..parent
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

fn count_chain_size(stack: &Stack, root: &str, allowed: Option<&HashSet<String>>) -> usize {
    if allowed.map_or(false, |set| !set.contains(root)) {
        return 0;
    }

    let mut count = 1;
    if let Some(info) = stack.branches.get(root) {
        for child in &info.children {
            count += count_chain_size(stack, child, allowed);
        }
    }
    count
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
