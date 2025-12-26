use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::Result;
use skim::prelude::*;
use std::borrow::Cow;

struct DisplayBranch {
    name: String,
    column: usize,
}

// Custom SkimItem that handles ANSI colors properly
struct BranchItem {
    display: String,      // with ANSI codes for display
    search_text: String,  // plain text for fuzzy matching
    branch_name: String,  // actual branch name for checkout
}

impl SkimItem for BranchItem {
    fn text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.search_text)
    }

    fn display<'a>(&'a self, _context: DisplayContext<'a>) -> AnsiString<'a> {
        AnsiString::parse(&self.display)
    }

    fn output(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.branch_name)
    }
}

// ANSI color codes
const YELLOW: &str = "\x1b[33m";
const GREEN: &str = "\x1b[32m";
const CYAN: &str = "\x1b[36m";
const RESET: &str = "\x1b[0m";

pub fn run(
    branch: Option<String>,
    trunk: bool,
    parent: bool,
    child: Option<usize>,
) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;

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
                let trunk_children: Vec<String> = trunk_info
                    .map(|b| b.children.clone())
                    .unwrap_or_default();

                if trunk_children.is_empty() {
                    println!("No branches found.");
                    return Ok(());
                }

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
                    );
                }

                // Build display items - simpler format for fuzzy finder
                // Show indentation based on depth, no complex tree graphics
                let mut items: Vec<BranchItem> = Vec::new();

                for db in display_branches.iter() {
                    let is_current = db.name == current;

                    // Build simple indented display
                    let indent = "  ".repeat(db.column);
                    let (color, circle) = if is_current {
                        (GREEN, "◉")
                    } else {
                        (YELLOW, "○")
                    };

                    let mut display = format!("{}{}{}{} {}", indent, color, circle, RESET, db.name);

                    if let Some(info) = stack.branches.get(&db.name) {
                        if let Some(parent) = info.parent.as_deref() {
                            if let Ok((ahead, behind)) =
                                repo.commits_ahead_behind(parent, &db.name)
                            {
                                if ahead > 0 {
                                    display.push_str(&format!(" {}{}↑{}", GREEN, ahead, RESET));
                                }
                                if behind > 0 {
                                    display.push_str(&format!(" {}{}↓{}", YELLOW, behind, RESET));
                                }
                            }
                        }

                        if info.needs_restack {
                            display.push_str(&format!(" {}⟳{}", YELLOW, RESET));
                        }
                        if let Some(pr) = info.pr_number {
                            let mut pr_text = format!(" {}PR #{}", CYAN, pr);
                            if let Some(ref state) = info.pr_state {
                                pr_text.push_str(&format!(" {}", state.to_lowercase()));
                            }
                            pr_text.push_str(RESET);
                            display.push_str(&pr_text);
                        }
                    }

                    let plain_display = console::strip_ansi_codes(&display).to_string();

                    items.push(BranchItem {
                        display,
                        search_text: plain_display,
                        branch_name: db.name.clone(),
                    });
                }

                // Add trunk at the end
                let is_trunk_current = stack.trunk == current;
                let (trunk_color, trunk_circle) = if is_trunk_current {
                    (GREEN, "◉")
                } else {
                    (YELLOW, "○")
                };

                let trunk_display = format!("{}{}{} {}", trunk_color, trunk_circle, RESET, stack.trunk);
                let trunk_plain = console::strip_ansi_codes(&trunk_display).to_string();
                items.push(BranchItem {
                    display: trunk_display,
                    search_text: trunk_plain,
                    branch_name: stack.trunk.clone(),
                });

                // Reverse so trunk is at bottom (matching stax ls)
                items.reverse();

                if items.is_empty() {
                    println!("No branches found.");
                    return Ok(());
                }

                // Use skim for fuzzy selection with custom colored items
                let options = SkimOptionsBuilder::default()
                    .height("~40%".to_string())  // ~ means minimum height, grows to fit
                    .multi(false)
                    .reverse(true)  // Show list bottom-up so trunk is at bottom
                    .prompt("Checkout branch > ".to_string())
                    .build()
                    .unwrap();

                // Convert items to Arc<dyn SkimItem> for skim
                let skim_items: Vec<Arc<dyn SkimItem>> = items
                    .into_iter()
                    .map(|item| Arc::new(item) as Arc<dyn SkimItem>)
                    .collect();

                let (tx, rx): (SkimItemSender, SkimItemReceiver) = unbounded();
                for item in skim_items {
                    let _ = tx.send(item);
                }
                drop(tx);

                let selected = Skim::run_with(&options, Some(rx))
                    .filter(|out| !out.is_abort)
                    .map(|out| out.selected_items)
                    .unwrap_or_default();

                if selected.is_empty() {
                    return Ok(());
                }

                // output() returns the branch name directly
                selected[0].output().to_string()
            }
        }
    };

    if target == repo.current_branch()? {
        println!("Already on '{}'", target);
    } else {
        repo.checkout(&target)?;
        println!("Switched to branch '{}'", target);
    }

    Ok(())
}

/// fp-style: children sorted alphabetically, each child gets column + index
fn collect_display_branches_with_nesting(
    stack: &Stack,
    branch: &str,
    base_column: usize,
    result: &mut Vec<DisplayBranch>,
    max_column: &mut usize,
) {
    collect_recursive(stack, branch, base_column, result, max_column);
}

fn collect_recursive(
    stack: &Stack,
    branch: &str,
    column: usize,
    result: &mut Vec<DisplayBranch>,
    max_column: &mut usize,
) {
    *max_column = (*max_column).max(column);

    if let Some(info) = stack.branches.get(branch) {
        let mut children: Vec<&String> = info.children.iter().collect();

        if !children.is_empty() {
            // Sort children alphabetically (like fp)
            children.sort();

            // Each child gets column + index: first child at same column, second at +1, etc.
            for (i, child) in children.iter().enumerate() {
                collect_recursive(stack, child, column + i, result, max_column);
            }
        }
    }

    result.push(DisplayBranch {
        name: branch.to_string(),
        column,
    });
}
