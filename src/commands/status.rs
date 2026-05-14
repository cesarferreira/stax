use crate::cache::CiCache;
use crate::commands::stack_palette;
use crate::config::Config;
use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::remote::{self, RemoteInfo};
use anyhow::Result;
use colored::Colorize;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;
use std::thread;

const LINKED_WORKTREE_GLYPH: &str = "↳";

/// Represents a branch in the display with its column position
struct DisplayBranch {
    name: String,
    column: usize,
}

fn column_color(column: usize) -> colored::Color {
    stack_palette::lane_color(column)
}

fn restack_label() -> String {
    format!("{}", "(needs restack)".white().bold())
}

fn behind_label(behind: usize) -> String {
    format!("{}", format!("{}↓", behind).red())
}

fn ahead_label(ahead: usize) -> String {
    format!("{}", format!("{}↑", ahead).green())
}

fn divergence_labels(ahead: usize, behind: usize) -> String {
    let mut labels = String::new();
    if ahead > 0 {
        labels.push_str(&format!(" {}", ahead_label(ahead)));
    }
    if behind > 0 {
        labels.push_str(&format!(" {}", behind_label(behind)));
    }
    labels
}

fn missing_parent_label(parent: &str) -> String {
    format!(
        "{}",
        format!("(missing parent: {})", parent).yellow().bold()
    )
}

#[derive(Serialize, Clone)]
struct BranchStatusJson {
    name: String,
    parent: Option<String>,
    is_current: bool,
    is_trunk: bool,
    linked_worktree: Option<String>,
    needs_restack: bool,
    missing_parent: Option<String>,
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
    current_only: bool,
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

    // By default show all branches. Use --current to show only current stack.
    let allowed_branches = if let Some(ref filter) = stack_filter {
        if !stack.branches.contains_key(filter) {
            anyhow::bail!("Branch '{}' is not tracked in the stack.", filter);
        }
        Some(
            stack
                .current_stack(filter)
                .into_iter()
                .collect::<HashSet<_>>(),
        )
    } else if current_only {
        // Show only current stack
        Some(
            stack
                .current_stack(&current)
                .into_iter()
                .collect::<HashSet<_>>(),
        )
    } else {
        None // Default: show all branches
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
            i, // column
            &mut display_branches,
            &mut max_column,
            allowed_branches.as_ref(),
        );
    }

    let tree_target_width = (max_column + 1) * 2;
    let mut ordered_branches: Vec<String> =
        display_branches.iter().map(|b| b.name.clone()).collect();
    ordered_branches.push(stack.trunk.clone());
    let remote_branches = remote::get_existing_remote_branches_from_repo(
        repo.inner(),
        config.remote_name(),
        &ordered_branches,
    );
    let missing_parent_by_branch = collect_missing_parent_branches(&repo, &stack);

    // Load CI cache (refresh happens in `stax ci`)
    let cache = CiCache::load(git_dir);

    // Build CI states from cache
    let ci_states: HashMap<String, String> = ordered_branches
        .iter()
        .filter_map(|b| cache.get_ci_state(b).map(|s| (b.clone(), s)))
        .collect();

    let mut branch_statuses: Vec<BranchStatusJson> = Vec::new();
    let mut branch_status_map: HashMap<String, BranchStatusJson> = HashMap::new();
    let linked_worktrees_by_branch: HashMap<String, String> =
        repo.linked_worktree_names_by_branch()?;
    let ahead_behind_pairs = ordered_branches
        .iter()
        .map(|name| {
            let base = if name == &stack.trunk {
                format!("{}/{}", config.remote_name(), name)
            } else {
                stack
                    .branches
                    .get(name)
                    .and_then(|b| b.parent.clone())
                    .unwrap_or_else(|| stack.trunk.clone())
            };
            (base, name.clone())
        })
        .collect::<Vec<_>>();
    let ahead_behind = repo.commits_ahead_behind_many(&ahead_behind_pairs);
    let line_diff_pairs = ordered_branches
        .iter()
        .map(|name| {
            stack
                .branches
                .get(name)
                .and_then(|b| b.parent.clone())
                .map(|parent| (parent, name.clone()))
        })
        .collect::<Vec<_>>();
    let line_diff_stats = if json {
        get_line_diff_stats_many(workdir, &line_diff_pairs)
    } else {
        vec![(0, 0); ordered_branches.len()]
    };

    for (idx, name) in ordered_branches.iter().enumerate() {
        let info = stack.branches.get(name);
        let parent = info.and_then(|b| b.parent.clone());
        let (ahead, behind) = ahead_behind
            .get(idx)
            .and_then(|result| result.as_ref().ok().copied())
            .unwrap_or((0, 0));
        let (lines_added, lines_deleted) = line_diff_stats.get(idx).copied().unwrap_or((0, 0));

        let pr_state = info.and_then(|b| b.pr_state.clone()).and_then(|s| {
            if s.trim().is_empty() {
                None
            } else {
                Some(s)
            }
        });

        let pr_number = info.and_then(|b| b.pr_number);
        let pr_url = pr_number.and_then(|n| remote_info.as_ref().map(|r| r.pr_url(n)));
        let ci_state = ci_states.get(name).cloned();
        let missing_parent = missing_parent_by_branch.get(name).cloned();

        let entry = BranchStatusJson {
            name: name.clone(),
            parent: parent.clone(),
            is_current: name == &current,
            is_trunk: name == &stack.trunk,
            linked_worktree: linked_worktrees_by_branch.get(name).cloned(),
            needs_restack: info.map(|b| b.needs_restack).unwrap_or(false),
            missing_parent,
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
            let pr_number = entry.pr_number.map(|n| n.to_string()).unwrap_or_default();
            let ci_state = entry.ci_state.clone().unwrap_or_default();
            let stack_status = if let Some(parent) = &entry.missing_parent {
                format!("missing-parent:{}", parent)
            } else if entry.needs_restack {
                "restack".to_string()
            } else {
                String::new()
            };
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                entry.name,
                parent,
                entry.ahead,
                entry.behind,
                pr_number,
                pr_state,
                ci_state,
                stack_status
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
        let has_remote =
            remote_branches.contains(branch) || entry.and_then(|e| e.pr_number).is_some();

        // Check if we need a corner connector - this happens when the PREVIOUS branch was at a higher column
        // The corner shows that a side branch joins back to this level
        let prev_branch_col = if i > 0 {
            Some(display_branches[i - 1].column)
        } else {
            None
        };
        let needs_corner = prev_branch_col.is_some_and(|pc| pc > db.column);

        // Build tree graphics - pad to consistent width based on max_column
        let mut tree = String::new();
        let mut visual_width = 0;
        // Draw columns 0 to db.column
        for col in 0..=db.column {
            let col_color = column_color(col);
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
            info_str.push_str(&format!("{} ", "☁️".bright_blue()));
        } else {
            info_str.push_str("   "); // Space for alignment when no remote (emoji is 2 cells wide)
        }

        if entry.and_then(|e| e.linked_worktree.as_ref()).is_some() {
            info_str.push_str(&format!("{} ", LINKED_WORKTREE_GLYPH.bright_cyan()));
        }

        // Color branch names to match their column in the graph
        let branch_color = column_color(db.column);
        if is_current {
            info_str.push_str(&format!("{}", branch.color(branch_color).bold()));
        } else {
            info_str.push_str(&format!("{}", branch.color(branch_color)));
        }

        if let Some(entry) = entry {
            info_str.push_str(&divergence_labels(entry.ahead, entry.behind));
            if let Some(parent) = &entry.missing_parent {
                info_str.push_str(&format!(" {}", missing_parent_label(parent)));
            } else if entry.needs_restack {
                info_str.push_str(&format!(" {}", restack_label()));
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
    let trunk_child_max_col = if sorted_trunk_children.is_empty() {
        0
    } else {
        sorted_trunk_children.len() - 1
    };

    let mut trunk_tree = String::new();
    let mut trunk_visual_width = 0;

    let trunk_circle = if is_trunk_current { "◉" } else { "○" };
    let trunk_color = column_color(0);
    trunk_tree.push_str(&format!("{}", trunk_circle.color(trunk_color)));
    trunk_visual_width += 1;

    // Show connectors only for trunk children columns: ─┴ for middle, ─┘ for last
    if trunk_child_max_col >= 1 {
        for col in 1..=trunk_child_max_col {
            let col_color = column_color(col);
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
        trunk_info.push_str(&format!("{} ", "☁️".bright_blue()));
    } else {
        trunk_info.push_str("   "); // Space for alignment when no remote (emoji is 2 cells wide)
    }

    if branch_status_map
        .get(&stack.trunk)
        .and_then(|entry| entry.linked_worktree.as_ref())
        .is_some()
    {
        trunk_info.push_str(&format!("{} ", LINKED_WORKTREE_GLYPH.bright_cyan()));
    }
    // Color trunk name to match column 0
    if is_trunk_current {
        trunk_info.push_str(&format!("{}", stack.trunk.color(trunk_color).bold()));
    } else {
        trunk_info.push_str(&format!("{}", stack.trunk.color(trunk_color)));
    }

    // Show commits ahead/behind for trunk (compared to origin)
    if let Some(entry) = branch_status_map.get(&stack.trunk) {
        trunk_info.push_str(&divergence_labels(entry.ahead, entry.behind));
    }

    println!("{}{}", trunk_tree, trunk_info);

    if !has_tracked && !quiet {
        println!(
            "{}",
            "No tracked branches yet (showing trunk only).".dimmed()
        );
        println!(
            "Use {} to start tracking branches.",
            "stax branch track".cyan()
        );
    }

    // Show stack health hints
    let needs_restack = stack.needs_restack();
    let restack_only: Vec<String> = needs_restack
        .into_iter()
        .filter(|branch| !missing_parent_by_branch.contains_key(branch))
        .collect();
    let config = Config::load().unwrap_or_default();
    let mut printed_stack_hint = false;
    if !quiet && config.ui.tips && !missing_parent_by_branch.is_empty() {
        println!();
        let count = missing_parent_by_branch.len();
        println!(
            "{} Run {} to repair.",
            format!(
                "! {} {} {} missing parent metadata.",
                count,
                if count == 1 { "branch" } else { "branches" },
                if count == 1 { "has" } else { "have" }
            )
            .bright_yellow(),
            "stax fix --yes".bright_cyan()
        );
        printed_stack_hint = true;
    }
    if !quiet && config.ui.tips && !restack_only.is_empty() {
        if !printed_stack_hint {
            println!();
        }
        println!(
            "{} Run {} to rebase.",
            format!(
                "⇅ {} {} need restacking.",
                restack_only.len(),
                if restack_only.len() == 1 {
                    "branch"
                } else {
                    "branches"
                }
            )
            .bright_yellow(),
            "stax rs --restack".bright_cyan()
        );
        printed_stack_hint = true;
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
            if !printed_stack_hint {
                println!(); // Add newline if we didn't already print restack hint
            }
            let mut stats = vec![format!(
                "{} {}",
                total_branches,
                if total_branches == 1 {
                    "branch"
                } else {
                    "branches"
                }
            )];
            if branches_with_remote > 0 {
                stats.push(format!("{} pushed", branches_with_remote));
            }
            if open_prs > 0 {
                stats.push(format!(
                    "{} open {}",
                    open_prs,
                    if open_prs == 1 { "PR" } else { "PRs" }
                ));
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
    #[derive(Clone)]
    struct Frame {
        branch: String,
        column: usize,
        expanded: bool,
    }

    let mut stack_frames = vec![Frame {
        branch: branch.to_string(),
        column: base_column,
        expanded: false,
    }];
    let mut visiting = HashSet::new();
    let mut emitted = HashSet::new();

    while let Some(frame) = stack_frames.pop() {
        if allowed.is_some_and(|set| !set.contains(&frame.branch)) {
            continue;
        }

        if frame.expanded {
            visiting.remove(&frame.branch);
            if emitted.insert(frame.branch.clone()) {
                result.push(DisplayBranch {
                    name: frame.branch,
                    column: frame.column,
                });
            }
            continue;
        }

        if emitted.contains(&frame.branch) || !visiting.insert(frame.branch.clone()) {
            continue;
        }

        *max_column = (*max_column).max(frame.column);
        stack_frames.push(Frame {
            branch: frame.branch.clone(),
            column: frame.column,
            expanded: true,
        });

        if let Some(info) = stack.branches.get(&frame.branch) {
            let mut children: Vec<&String> = info
                .children
                .iter()
                .filter(|child| allowed.is_none_or(|set| set.contains(*child)))
                .collect();

            children.sort();

            for (i, child) in children.into_iter().enumerate().rev() {
                if emitted.contains(child) || visiting.contains(child) {
                    continue;
                }

                stack_frames.push(Frame {
                    branch: child.clone(),
                    column: frame.column + i,
                    expanded: false,
                });
            }
        }
    }
}

fn collect_missing_parent_branches(repo: &GitRepo, stack: &Stack) -> HashMap<String, String> {
    let mut missing = HashMap::new();

    for name in stack.branches.keys().filter(|name| *name != &stack.trunk) {
        let Ok(Some(meta)) = BranchMetadata::read(repo.inner(), name) else {
            continue;
        };
        let parent = meta.parent_branch_name.trim();
        if parent.is_empty() || parent == stack.trunk {
            continue;
        }
        if repo.branch_commit(parent).is_err() {
            missing.insert(name.clone(), parent.to_string());
        }
    }

    missing
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::stack::StackBranch;

    fn branch(parent: Option<&str>, children: Vec<String>) -> StackBranch {
        StackBranch {
            name: String::new(),
            parent: parent.map(str::to_string),
            parent_revision: None,
            children,
            needs_restack: false,
            pr_number: None,
            pr_state: None,
            pr_is_draft: None,
        }
    }

    #[test]
    fn collect_display_branches_handles_deep_chains_without_recursion() {
        let depth = 500;
        let mut branches = HashMap::new();
        let trunk = "main".to_string();
        branches.insert(
            trunk.clone(),
            StackBranch {
                name: trunk.clone(),
                parent: None,
                parent_revision: None,
                children: vec!["branch-0".to_string()],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );

        for i in 0..depth {
            let name = format!("branch-{i}");
            let child = (i + 1 < depth).then(|| format!("branch-{}", i + 1));
            branches.insert(
                name.clone(),
                StackBranch {
                    name,
                    parent: Some(if i == 0 {
                        trunk.clone()
                    } else {
                        format!("branch-{}", i - 1)
                    }),
                    parent_revision: None,
                    children: child.into_iter().collect(),
                    needs_restack: false,
                    pr_number: None,
                    pr_state: None,
                    pr_is_draft: None,
                },
            );
        }

        let stack = Stack { branches, trunk };
        let mut result = Vec::new();
        let mut max_column = 0;
        collect_display_branches_with_nesting(
            &stack,
            "branch-0",
            0,
            &mut result,
            &mut max_column,
            None,
        );

        assert_eq!(result.len(), depth);
        assert_eq!(result.first().map(|b| b.name.as_str()), Some("branch-499"));
        assert_eq!(result.last().map(|b| b.name.as_str()), Some("branch-0"));
        assert_eq!(max_column, 0);
    }

    #[test]
    fn collect_display_branches_skips_cycles() {
        let mut branches = HashMap::new();
        branches.insert(
            "main".to_string(),
            StackBranch {
                name: "main".to_string(),
                parent: None,
                parent_revision: None,
                children: vec!["a".to_string()],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );
        branches.insert("a".to_string(), branch(Some("main"), vec!["b".to_string()]));
        branches.insert("b".to_string(), branch(Some("a"), vec!["a".to_string()]));

        let stack = Stack {
            branches,
            trunk: "main".to_string(),
        };
        let mut result = Vec::new();
        let mut max_column = 0;
        collect_display_branches_with_nesting(&stack, "a", 0, &mut result, &mut max_column, None);

        let names: Vec<&str> = result.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, vec!["b", "a"]);
        assert_eq!(max_column, 0);
    }

    #[test]
    fn status_lane_palette_is_shared_vivid_rgb() {
        use crate::commands::stack_palette::{lane_color, lane_rgb};

        assert_eq!(lane_rgb(0), (56, 189, 248));
        assert_eq!(lane_rgb(4), (251, 146, 60));
        assert_eq!(lane_rgb(8), lane_rgb(0));
        assert_eq!(column_color(0), lane_color(0));
        assert_eq!(
            column_color(5),
            colored::Color::TrueColor {
                r: 248,
                g: 113,
                b: 113
            }
        );
    }

    #[test]
    fn status_restack_label_is_bold_white() {
        colored::control::set_override(true);

        let label = restack_label();

        colored::control::unset_override();
        assert_eq!(label, "\u{1b}[1;37m(needs restack)\u{1b}[0m");
    }
}

/// Get line additions and deletions between parent and branch
fn get_line_diff_stats_many(
    workdir: &Path,
    branch_pairs: &[Option<(String, String)>],
) -> Vec<(usize, usize)> {
    thread::scope(|scope| {
        let handles = branch_pairs
            .iter()
            .map(|pair| {
                pair.as_ref().map(|(parent, branch)| {
                    scope.spawn(move || {
                        get_line_diff_stats(workdir, parent, branch).unwrap_or((0, 0))
                    })
                })
            })
            .collect::<Vec<_>>();

        handles
            .into_iter()
            .map(|handle| match handle {
                Some(handle) => handle.join().unwrap_or((0, 0)),
                None => (0, 0),
            })
            .collect()
    })
}

fn get_line_diff_stats(workdir: &Path, parent: &str, branch: &str) -> Option<(usize, usize)> {
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
