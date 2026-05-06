use crate::commands::stack_palette;
use crate::commands::worktree::{go, shared::emit_shell_message};
use crate::config::Config;
use crate::engine::Stack;
use crate::git::repo::WorktreeInfo;
use crate::git::{checkout_branch_in, refs, GitRepo};
use crate::progress::LiveTimer;
use crate::remote;
use anyhow::Result;
use colored::Colorize;
use console::{colors_enabled_stderr, measure_text_width, truncate_str, Color, Style};
use crossterm::terminal;
use dialoguer::{
    theme::{ColorfulTheme, Theme},
    FuzzySelect,
};
use fuzzy_matcher::skim::SkimMatcherV2;
use std::collections::HashSet;
use std::fmt::{self, Display};
use std::path::Path;

const LINKED_WORKTREE_GLYPH: &str = "↳";
const BRIGHT_BLUE: CheckoutColor = CheckoutColor::new(Color::Blue, true);
const BRIGHT_CYAN: CheckoutColor = CheckoutColor::new(Color::Cyan, true);
const CHECKOUT_PICKER_ITEM_SEPARATOR: char = '\u{1f}';
const ACTIVE_ROW_BACKGROUND: &str = "\u{1b}[48;5;236m";
const CHECKOUT_BRANCH_STYLE: &str = "\u{1b}[1;96m";
const ANSI_RESET: &str = "\u{1b}[0m";

#[derive(Clone, Copy)]
struct CheckoutColor {
    color: Color,
    bright: bool,
}

impl CheckoutColor {
    const fn new(color: Color, bright: bool) -> Self {
        Self { color, bright }
    }
}

/// Represents a branch in the display with its column position
struct DisplayBranch {
    name: String,
    column: usize,
}

struct CheckoutRow {
    branch: String,
    display: String,
    active_display: String,
}

#[derive(Default)]
struct CheckoutPickerTheme {
    inner: ColorfulTheme,
}

impl Theme for CheckoutPickerTheme {
    fn format_fuzzy_select_prompt_item(
        &self,
        f: &mut dyn fmt::Write,
        text: &str,
        active: bool,
        _highlight_matches: bool,
        _matcher: &SkimMatcherV2,
        _search_term: &str,
    ) -> fmt::Result {
        if active {
            write!(f, "{}", checkout_picker_item_display(text, true))
        } else {
            write!(f, "  {}", checkout_picker_item_display(text, false))
        }
    }

    fn format_fuzzy_select_prompt(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        search_term: &str,
        bytes_pos: usize,
    ) -> fmt::Result {
        Theme::format_fuzzy_select_prompt(&self.inner, f, prompt, search_term, bytes_pos)
    }
}

fn checkout_style(spec: CheckoutColor) -> Style {
    let style = Style::new().for_stderr().fg(spec.color);
    if spec.bright {
        style.bright()
    } else {
        style
    }
}

fn checkout_lane_color(column: usize) -> CheckoutColor {
    CheckoutColor::new(stack_palette::lane_console_color(column), false)
}

fn restack_label() -> String {
    render_stderr("(needs restack)", Style::new().for_stderr().white().bold())
}

fn behind_label(behind: usize) -> String {
    render_stderr(format!("{}↓", behind), Style::new().for_stderr().red())
}

fn ahead_label(ahead: usize) -> String {
    render_stderr(format!("{}↑", ahead), Style::new().for_stderr().green())
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

fn render_stderr<T: Display>(value: T, style: Style) -> String {
    format!("{}", style.apply_to(value))
}

fn checkout_completion_message(branch: &str) -> String {
    format!(
        "{} {}.",
        "Checked out".green().bold(),
        branch.bright_cyan().bold()
    )
}

fn already_on_message(branch: &str) -> String {
    format!(
        "{} {}.",
        "Already on".yellow().bold(),
        branch.bright_cyan().bold()
    )
}

fn checkout_completion_shell_message(branch: &str) -> String {
    format!("Checked out {CHECKOUT_BRANCH_STYLE}{branch}{ANSI_RESET}.")
}

fn already_on_shell_message(branch: &str) -> String {
    format!("Already on {CHECKOUT_BRANCH_STYLE}{branch}{ANSI_RESET}.")
}

fn active_checkout_row(text: &str, min_width: usize) -> String {
    let mut row = text.to_string();
    let visible_width = measure_text_width(text);
    if visible_width < min_width {
        row.push_str(&" ".repeat(min_width - visible_width));
    }

    if !colors_enabled_stderr() {
        return row;
    }

    let mut highlighted = String::with_capacity(row.len() + ACTIVE_ROW_BACKGROUND.len() * 4);
    highlighted.push_str(ACTIVE_ROW_BACKGROUND);

    let mut remaining = row.as_str();
    while let Some(reset_index) = remaining.find(ANSI_RESET) {
        let end = reset_index + ANSI_RESET.len();
        highlighted.push_str(&remaining[..end]);
        highlighted.push_str(ACTIVE_ROW_BACKGROUND);
        remaining = &remaining[end..];
    }
    highlighted.push_str(remaining);
    highlighted.push_str(ANSI_RESET);
    highlighted
}

fn checkout_picker_item(branch: &str, display: &str, active_display: &str) -> String {
    format!(
        "{branch}{CHECKOUT_PICKER_ITEM_SEPARATOR}{display}{CHECKOUT_PICKER_ITEM_SEPARATOR}{active_display}"
    )
}

fn checkout_picker_item_display(item: &str, active: bool) -> &str {
    let mut parts = item.splitn(3, CHECKOUT_PICKER_ITEM_SEPARATOR);
    let _search_text = parts.next();
    let inactive_display = parts.next();
    let active_display = parts.next();

    match (inactive_display, active_display) {
        (Some(_inactive), Some(active_item)) if active => active_item,
        (Some(inactive), Some(_)) => inactive,
        _ => item,
    }
}

pub fn run(
    branch: Option<String>,
    pr: Option<u64>,
    trunk: bool,
    parent: bool,
    child: Option<usize>,
    shell_output: bool,
) -> Result<()> {
    let repo = GitRepo::open()?;
    let workdir = repo.workdir()?.to_path_buf();
    let current = repo.current_branch()?;

    // Handle explicit --pr flag
    if let Some(pr_num) = pr {
        return checkout_by_pr(&repo, pr_num, shell_output);
    }

    // Parse "#123" from branch string
    if let Some(ref branch_str) = branch {
        if let Some(pr_num) = parse_pr_number(branch_str)? {
            return checkout_by_pr(&repo, pr_num, shell_output);
        }
    }

    if branch.is_some() && (trunk || parent || child.is_some()) {
        anyhow::bail!("Cannot combine explicit branch with --trunk/--parent/--child");
    }

    let target = if trunk || parent || child.is_some() {
        let stack = Stack::load(&repo)?;
        if trunk {
            stack.trunk.clone()
        } else if parent {
            let parent_branch = stack
                .branches
                .get(&current)
                .and_then(|b| b.parent.clone())
                .ok_or_else(|| anyhow::anyhow!("Branch '{}' has no tracked parent.", current))?;
            parent_branch
        } else {
            let children: Vec<String> = stack
                .branches
                .get(&current)
                .map(|b| b.children.clone())
                .unwrap_or_default();

            if children.is_empty() {
                anyhow::bail!("Branch '{}' has no tracked children.", current);
            }

            let idx = child.unwrap_or(1);
            if idx == 0 || idx > children.len() {
                anyhow::bail!("Child index {} out of range (1-{})", idx, children.len());
            }
            children[idx - 1].clone()
        }
    } else {
        match branch {
            Some(b) => b,
            None => {
                let stack = Stack::load(&repo)?;
                let _workdir = repo.workdir()?;

                if stack.branches.is_empty() {
                    println!("No branches found.");
                    return Ok(());
                }

                // Get trunk children (each starts a chain)
                let trunk_info = stack.branches.get(&stack.trunk);
                let trunk_children: Vec<String> =
                    trunk_info.map(|b| b.children.clone()).unwrap_or_default();

                if trunk_children.is_empty() {
                    println!("No branches found.");
                    return Ok(());
                }

                let rows = build_checkout_rows(&stack, &repo, &current)?;

                if rows.is_empty() {
                    println!("No branches found.");
                    return Ok(());
                }

                let items: Vec<String> = rows
                    .iter()
                    .map(|r| checkout_picker_item(&r.branch, &r.display, &r.active_display))
                    .collect();
                let branch_names: Vec<String> = rows.iter().map(|r| r.branch.clone()).collect();

                let theme = CheckoutPickerTheme::default();

                let term = console::Term::stderr();
                if term.is_term() {
                    let _ = term.clear_screen();
                    let _ = term.move_cursor_to(0, 0);
                }

                let default_index = branch_names
                    .iter()
                    .position(|name| name == &current)
                    .unwrap_or(0);

                let selection = FuzzySelect::with_theme(&theme)
                    .with_prompt("Checkout a branch (autocomplete or arrow keys)")
                    .items(&items)
                    .default(default_index)
                    .highlight_matches(false) // Disabled - conflicts with ANSI colors
                    .report(false)
                    .interact()?;

                branch_names[selection].clone()
            }
        }
    };

    drop(repo);

    if let Some(worktree) = route_checkout_to_worktree(&workdir, &target, shell_output)? {
        if shell_output {
            emit_shell_message(&format!(
                "Routed checkout to worktree '{}' for branch '{}'",
                worktree.name, target
            ));
        } else {
            println!(
                "{}",
                format!(
                    "Branch '{}' is already checked out in worktree '{}' - routing there instead.",
                    target, worktree.name
                )
                .yellow()
            );
        }
        go::run_go_on_worktree(
            &worktree,
            false,
            shell_output,
            None,
            None,
            None,
            false,
            None,
            Vec::new(),
            false,
            Vec::new(),
        )?;
    } else if target == current {
        if shell_output {
            emit_shell_message(&already_on_shell_message(&target));
        } else {
            println!("{}", already_on_message(&target));
        }
    } else {
        if let Err(e) = refs::write_prev_branch_at(&workdir, &current) {
            eprintln!("Warning: failed to save previous branch: {}", e);
        }
        let timer = LiveTimer::maybe_new(true, &format!("Checking out {}...", target));
        checkout_branch_in(&workdir, &target)?;
        LiveTimer::maybe_finish_ok(timer, "done");
        if shell_output {
            emit_shell_message(&checkout_completion_shell_message(&target));
        } else {
            println!("{}", checkout_completion_message(&target));
        }
    }

    Ok(())
}

fn route_checkout_to_worktree(
    workdir: &Path,
    target: &str,
    shell_output: bool,
) -> Result<Option<WorktreeInfo>> {
    let Some(worktree) = GitRepo::branch_worktree_in(workdir, target)? else {
        return Ok(None);
    };

    let current_path = std::fs::canonicalize(workdir).unwrap_or_else(|_| workdir.to_path_buf());
    let target_path =
        std::fs::canonicalize(&worktree.path).unwrap_or_else(|_| worktree.path.clone());
    if current_path == target_path {
        return Ok(None);
    }

    if !shell_output {
        println!();
    }

    Ok(Some(worktree))
}

fn build_checkout_rows(stack: &Stack, repo: &GitRepo, current: &str) -> Result<Vec<CheckoutRow>> {
    let config = Config::load()?;
    let linked_worktrees_by_branch: HashSet<String> = repo
        .list_worktrees()?
        .into_iter()
        .filter(|worktree| !worktree.is_main && !worktree.is_prunable)
        .filter_map(|worktree| worktree.branch)
        .collect();
    let show_worktree_column = !linked_worktrees_by_branch.is_empty();

    let trunk_info = stack.branches.get(&stack.trunk);
    let trunk_children: Vec<String> = trunk_info
        .map(|b| b.children.clone())
        .unwrap_or_default()
        .into_iter()
        .collect();

    if trunk_children.is_empty() {
        return Ok(Vec::new());
    }

    let mut display_branches: Vec<DisplayBranch> = Vec::new();
    let mut max_column = 0;
    let mut sorted_trunk_children = trunk_children;
    sorted_trunk_children.sort();

    for (i, root) in sorted_trunk_children.iter().enumerate() {
        collect_display_branches_with_nesting(
            stack,
            root,
            i,
            &mut display_branches,
            &mut max_column,
        )?;
    }

    // Build ordered list of all branches we care about (displayed + trunk)
    let mut ordered_branches: Vec<String> =
        display_branches.iter().map(|db| db.name.clone()).collect();
    ordered_branches.push(stack.trunk.clone());

    // Only check remote existence for the branches we're displaying (fast)
    let remote_branches = remote::get_existing_remote_branches_from_repo(
        repo.inner(),
        config.remote_name(),
        &ordered_branches,
    );

    // Batch all ahead/behind queries in parallel threads
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

    let tree_target_width = (max_column + 1) * 2;
    let picker_row_width = terminal_width().saturating_sub(1);
    let item_width = picker_row_width.saturating_sub(2);
    let mut rows = Vec::new();

    for (i, db) in display_branches.iter().enumerate() {
        let branch = &db.name;
        let is_current = branch == current;
        let entry = stack.branches.get(branch);
        let (ahead, behind) = ahead_behind
            .get(i)
            .and_then(|r| r.as_ref().ok().copied())
            .unwrap_or((0, 0));
        let needs_restack = entry.map(|b| b.needs_restack).unwrap_or(false);
        let has_pr = entry.and_then(|b| b.pr_number).is_some();
        let has_remote = remote_branches.contains(branch) || has_pr;
        let has_linked_worktree = linked_worktrees_by_branch.contains(branch);

        let prev_branch_col = if i > 0 {
            Some(display_branches[i - 1].column)
        } else {
            None
        };
        let needs_corner = prev_branch_col.is_some_and(|pc| pc > db.column);

        let mut tree = String::new();
        let mut visual_width = 0;
        for col in 0..=db.column {
            let col_color = checkout_lane_color(col);
            if col == db.column {
                let circle = if is_current { "◉" } else { "○" };
                tree.push_str(&render_stderr(circle, checkout_style(col_color)));
                visual_width += 1;
                if needs_corner {
                    tree.push_str(&render_stderr("─┘", checkout_style(col_color)));
                    visual_width += 2;
                }
            } else {
                tree.push_str(&render_stderr("│", checkout_style(col_color)));
                tree.push(' ');
                visual_width += 2;
            }
        }

        while visual_width < tree_target_width {
            tree.push(' ');
            visual_width += 1;
        }

        let mut info_str =
            render_presence_markers(has_remote, show_worktree_column, has_linked_worktree);

        let branch_color = checkout_lane_color(db.column);
        if is_current {
            info_str.push_str(&render_stderr(branch, checkout_style(branch_color).bold()));
        } else {
            info_str.push_str(&render_stderr(branch, checkout_style(branch_color)));
        }

        info_str.push_str(&divergence_labels(ahead, behind));
        if needs_restack {
            info_str.push_str(&format!(" {}", restack_label()));
        }

        let display = truncate_display(&format!("{}{}", tree, info_str), item_width);
        let active_display = active_checkout_row(
            &format!(
                "{} {}",
                render_stderr("›", Style::new().for_stderr().cyan().bold()),
                display
            ),
            picker_row_width,
        );
        rows.push(CheckoutRow {
            branch: branch.clone(),
            display,
            active_display,
        });
    }

    // Render trunk row (matches status.rs)
    let is_trunk_current = stack.trunk == current;
    let trunk_child_max_col = if sorted_trunk_children.is_empty() {
        0
    } else {
        sorted_trunk_children.len() - 1
    };

    let mut trunk_tree = String::new();
    let mut trunk_visual_width = 0;
    let trunk_circle = if is_trunk_current { "◉" } else { "○" };
    let trunk_color = checkout_lane_color(0);
    trunk_tree.push_str(&render_stderr(trunk_circle, checkout_style(trunk_color)));
    trunk_visual_width += 1;

    if trunk_child_max_col >= 1 {
        for col in 1..=trunk_child_max_col {
            let col_color = checkout_lane_color(col);
            if col < trunk_child_max_col {
                trunk_tree.push_str(&render_stderr("─┴", checkout_style(col_color)));
            } else {
                trunk_tree.push_str(&render_stderr("─┘", checkout_style(col_color)));
            }
            trunk_visual_width += 2;
        }
    }

    while trunk_visual_width < tree_target_width {
        trunk_tree.push(' ');
        trunk_visual_width += 1;
    }

    let mut trunk_info = render_presence_markers(
        remote_branches.contains(&*stack.trunk),
        show_worktree_column,
        linked_worktrees_by_branch.contains(&stack.trunk),
    );
    if is_trunk_current {
        trunk_info.push_str(&render_stderr(
            &stack.trunk,
            checkout_style(trunk_color).bold(),
        ));
    } else {
        trunk_info.push_str(&render_stderr(&stack.trunk, checkout_style(trunk_color)));
    }

    // Trunk is the last entry in ahead_behind (appended after display_branches)
    let trunk_idx = display_branches.len();
    let (ahead, behind) = ahead_behind
        .get(trunk_idx)
        .and_then(|r| r.as_ref().ok().copied())
        .unwrap_or((0, 0));
    trunk_info.push_str(&divergence_labels(ahead, behind));

    let trunk_display = truncate_display(&format!("{}{}", trunk_tree, trunk_info), item_width);
    let active_trunk_display = active_checkout_row(
        &format!(
            "{} {}",
            render_stderr("›", Style::new().for_stderr().cyan().bold()),
            trunk_display
        ),
        picker_row_width,
    );
    rows.push(CheckoutRow {
        branch: stack.trunk.clone(),
        display: trunk_display,
        active_display: active_trunk_display,
    });

    Ok(rows)
}

fn render_presence_markers(
    has_remote: bool,
    show_worktree_column: bool,
    has_linked_worktree: bool,
) -> String {
    let mut info_str = String::new();
    info_str.push(' ');
    if has_remote {
        info_str.push_str(&render_stderr("☁️", checkout_style(BRIGHT_BLUE)));
        info_str.push(' ');
    } else {
        info_str.push_str("   ");
    }

    if show_worktree_column {
        if has_linked_worktree {
            info_str.push_str(&render_stderr(
                LINKED_WORKTREE_GLYPH,
                checkout_style(BRIGHT_CYAN),
            ));
            info_str.push(' ');
        } else {
            info_str.push_str("  ");
        }
    }

    info_str
}

fn truncate_display(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    truncate_str(text, max_width, "...").into_owned()
}

fn terminal_width() -> usize {
    terminal::size()
        .map(|(cols, _)| cols as usize)
        .unwrap_or(120)
        .max(20)
}

/// Collect branches with proper nesting for branches that have multiple children
/// fp-style: children sorted alphabetically, each child gets column + index
fn collect_display_branches_with_nesting(
    stack: &Stack,
    branch: &str,
    base_column: usize,
    result: &mut Vec<DisplayBranch>,
    max_column: &mut usize,
) -> Result<()> {
    let mut active = HashSet::new();
    let mut seen = HashSet::new();
    collect_recursive(
        stack,
        branch,
        base_column,
        result,
        max_column,
        &mut active,
        &mut seen,
    )
}

fn collect_recursive(
    stack: &Stack,
    branch: &str,
    column: usize,
    result: &mut Vec<DisplayBranch>,
    max_column: &mut usize,
    active: &mut HashSet<String>,
    seen: &mut HashSet<String>,
) -> Result<()> {
    if active.contains(branch) {
        anyhow::bail!("Cycle detected in stack metadata at branch '{}'", branch);
    }
    if seen.contains(branch) {
        return Ok(());
    }

    active.insert(branch.to_string());
    seen.insert(branch.to_string());
    *max_column = (*max_column).max(column);

    if let Some(info) = stack.branches.get(branch) {
        let mut children: Vec<&String> = info.children.iter().collect();

        if !children.is_empty() {
            children.sort();
            for (i, child) in children.iter().enumerate() {
                collect_recursive(stack, child, column + i, result, max_column, active, seen)?;
            }
        }
    }

    active.remove(branch);
    result.push(DisplayBranch {
        name: branch.to_string(),
        column,
    });
    Ok(())
}

/// Parse PR number from string (supports "#123" format or plain number)
fn parse_pr_number(input: &str) -> Result<Option<u64>> {
    if let Some(num_str) = input.strip_prefix('#') {
        let num = num_str
            .parse::<u64>()
            .map_err(|_| anyhow::anyhow!("Invalid PR number after #: {}", num_str))?;
        return Ok(Some(num));
    }
    Ok(None)
}

/// Checkout branch by PR number
fn checkout_by_pr(repo: &GitRepo, pr_num: u64, shell_output: bool) -> Result<()> {
    let config = Config::load()?;
    let remote_info = crate::remote::RemoteInfo::from_repo(repo, &config)?;

    // Get PR info including head branch
    let rt = tokio::runtime::Runtime::new()?;
    let pr_info = rt.block_on(async {
        let client = crate::forge::ForgeClient::new(&remote_info)?;
        client.get_pr_with_head(pr_num).await
    })?;

    let target_branch = pr_info.head;

    // Check if branch exists locally
    let local_branches = repo.list_branches()?;
    if !local_branches.contains(&target_branch) {
        anyhow::bail!(
            "PR #{} points to branch '{}' which doesn't exist locally.\n\
             Hint: Try 'stax branch track --all-prs' to track all your open PRs",
            pr_num,
            target_branch
        );
    }

    // Checkout the branch
    let workdir = repo.workdir()?;
    let timer = LiveTimer::maybe_new(
        true,
        &format!("Checking out {}...", target_branch),
    );
    checkout_branch_in(workdir, &target_branch)?;
    LiveTimer::maybe_finish_ok(timer, "done");

    if shell_output {
        let message = format!("Checked out PR #{}: {}", pr_num, target_branch);
        emit_shell_message(&message);
    } else {
        println!("Checked out PR #{}: {}", pr_num, target_branch.cyan());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::stack::StackBranch;
    use regex::Regex;
    use std::collections::HashMap;
    use std::sync::Mutex;

    static STDERR_COLOR_LOCK: Mutex<()> = Mutex::new(());

    fn with_stderr_colors_enabled<T>(f: impl FnOnce() -> T) -> T {
        let _guard = STDERR_COLOR_LOCK
            .lock()
            .expect("stderr color lock poisoned");
        let previous = console::colors_enabled_stderr();
        console::set_colors_enabled_stderr(true);
        let result = f();
        console::set_colors_enabled_stderr(previous);
        result
    }

    fn test_stack() -> Stack {
        // main (trunk)
        // ├── auth
        // │   └── auth-api
        // │       └── auth-ui
        // └── hotfix
        let mut branches: HashMap<String, StackBranch> = HashMap::new();

        branches.insert(
            "auth".to_string(),
            StackBranch {
                name: "auth".to_string(),
                parent: Some("main".to_string()),
                parent_revision: None,
                children: vec!["auth-api".to_string()],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );

        branches.insert(
            "auth-api".to_string(),
            StackBranch {
                name: "auth-api".to_string(),
                parent: Some("auth".to_string()),
                parent_revision: None,
                children: vec!["auth-ui".to_string()],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );

        branches.insert(
            "auth-ui".to_string(),
            StackBranch {
                name: "auth-ui".to_string(),
                parent: Some("auth-api".to_string()),
                parent_revision: None,
                children: vec![],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );

        branches.insert(
            "hotfix".to_string(),
            StackBranch {
                name: "hotfix".to_string(),
                parent: Some("main".to_string()),
                parent_revision: None,
                children: vec![],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );

        branches.insert(
            "main".to_string(),
            StackBranch {
                name: "main".to_string(),
                parent: None,
                parent_revision: None,
                children: vec!["auth".to_string(), "hotfix".to_string()],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );

        Stack {
            branches,
            trunk: "main".to_string(),
        }
    }

    #[test]
    fn test_display_branch_order() {
        let stack = test_stack();
        let mut display_branches = Vec::new();
        let mut max_column = 0;
        let mut roots = vec!["auth".to_string(), "hotfix".to_string()];
        roots.sort();
        for (i, root) in roots.iter().enumerate() {
            collect_display_branches_with_nesting(
                &stack,
                root,
                i,
                &mut display_branches,
                &mut max_column,
            )
            .unwrap();
        }
        let names: Vec<_> = display_branches.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, vec!["auth-ui", "auth-api", "auth", "hotfix"]);
    }

    #[test]
    fn test_truncate_display_caps_width() {
        let text = "• very-very-long-branch-name  stack  +12/-3  #123 ⟳";
        let truncated = truncate_display(text, 16);
        assert!(console::measure_text_width(&truncated) <= 16);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn test_collect_display_branches_detects_cycle() {
        let mut stack = test_stack();
        stack.branches.get_mut("auth").unwrap().children = vec!["auth-api".to_string()];
        stack.branches.get_mut("auth-api").unwrap().children = vec!["auth".to_string()];

        let mut display_branches = Vec::new();
        let mut max_column = 0;
        let err = collect_display_branches_with_nesting(
            &stack,
            "auth",
            0,
            &mut display_branches,
            &mut max_column,
        )
        .unwrap_err();
        assert!(err.to_string().contains("Cycle detected in stack metadata"));
    }

    fn strip_ansi(s: &str) -> String {
        Regex::new(r"\x1b\[[0-9;]*m")
            .expect("valid ANSI regex")
            .replace_all(s, "")
            .into_owned()
    }

    #[test]
    fn test_checkout_style_uses_stderr_color_channel() {
        let styled = with_stderr_colors_enabled(|| {
            render_stderr("branch", checkout_style(checkout_lane_color(0)).bold())
        });
        assert!(styled.contains("\x1b["));
    }

    #[test]
    fn test_render_presence_markers_aligns_worktree_column() {
        assert_eq!(
            strip_ansi(&render_presence_markers(true, true, true)),
            " ☁️ ↳ "
        );
        assert_eq!(
            strip_ansi(&render_presence_markers(false, true, false)),
            "      "
        );
    }

    #[test]
    fn checkout_lane_palette_uses_same_vivid_rgb_as_status() {
        use crate::commands::stack_palette::{lane_console_color, lane_rgb};

        assert_eq!(lane_rgb(0), (56, 189, 248));
        assert_eq!(lane_rgb(5), (248, 113, 113));
        assert_eq!(lane_rgb(8), lane_rgb(0));

        for column in 0..8 {
            let color = checkout_lane_color(column);
            assert_eq!(color.color, lane_console_color(column));
            assert!(!color.bright);
        }
    }

    #[test]
    fn checkout_restack_label_is_bold_white() {
        let label = with_stderr_colors_enabled(restack_label);
        assert_eq!(label, "\u{1b}[37m\u{1b}[1m(needs restack)\u{1b}[0m");
    }

    #[test]
    fn checkout_status_labels_use_stderr_colors() {
        let (behind, ahead) = with_stderr_colors_enabled(|| (behind_label(4), ahead_label(6)));
        assert_eq!(behind, "\u{1b}[31m4↓\u{1b}[0m");
        assert_eq!(ahead, "\u{1b}[32m6↑\u{1b}[0m");
    }

    #[test]
    fn checkout_divergence_labels_match_compact_ahead_then_behind_order() {
        let labels = with_stderr_colors_enabled(|| divergence_labels(3, 1));
        assert_eq!(strip_ansi(&labels), " 3↑ 1↓");
        assert!(labels.find("3↑").unwrap() < labels.find("1↓").unwrap());
        assert!(labels.contains("\u{1b}[32m3↑\u{1b}[0m"));
        assert!(labels.contains("\u{1b}[31m1↓\u{1b}[0m"));
    }

    #[test]
    fn checkout_completion_message_colors_status_and_branch() {
        colored::control::set_override(true);
        let message = checkout_completion_message("feature/payments-api");
        colored::control::unset_override();

        assert_eq!(strip_ansi(&message), "Checked out feature/payments-api.");
        assert!(
            message.contains("\u{1b}[32m") || message.contains("\u{1b}[1;32m"),
            "Expected success text to include green styling, got: {message:?}"
        );
        assert!(
            message.contains("\u{1b}[96m") || message.contains("\u{1b}[1;96m"),
            "Expected branch text to include bright cyan styling, got: {message:?}"
        );
    }

    #[test]
    fn checkout_picker_item_uses_active_display_for_selected_row() {
        let item = checkout_picker_item("feature/auth", "inactive row", "active row");

        assert_eq!(checkout_picker_item_display(&item, false), "inactive row");
        assert_eq!(checkout_picker_item_display(&item, true), "active row");
    }

    #[test]
    fn active_checkout_row_uses_background_instead_of_guide_line() {
        let active = with_stderr_colors_enabled(|| active_checkout_row("○   feature/auth", 20));

        assert_eq!(strip_ansi(&active), "○   feature/auth    ");
        assert!(
            active.contains("\u{1b}[48;5;236m"),
            "Expected active row background styling, got: {active:?}"
        );
        assert!(
            !strip_ansi(&active).contains("───"),
            "Active row should not draw a guide line: {active:?}"
        );
    }

    #[test]
    fn active_checkout_row_pads_colored_text_by_visible_width() {
        let active = with_stderr_colors_enabled(|| {
            active_checkout_row(
                &render_stderr("○", checkout_style(checkout_lane_color(0))),
                4,
            )
        });

        assert_eq!(strip_ansi(&active), "○   ");
    }

    #[test]
    fn active_picker_row_keeps_prefix_inside_row_width() {
        let theme = CheckoutPickerTheme::default();
        let active_display =
            with_stderr_colors_enabled(|| active_checkout_row("› ○ feature/auth", 16));
        let item = checkout_picker_item("feature/auth", "○ feature/auth", &active_display);
        let mut rendered = String::new();

        with_stderr_colors_enabled(|| {
            Theme::format_fuzzy_select_prompt_item(
                &theme,
                &mut rendered,
                &item,
                true,
                false,
                &SkimMatcherV2::default(),
                "",
            )
        })
        .expect("format active picker item");

        assert_eq!(strip_ansi(&rendered).chars().count(), 16);
        assert!(strip_ansi(&rendered).starts_with('›'));
    }
}
