use crate::git::GitRepo;
use colored::Colorize;

pub(crate) struct RestackConflictContext<'a> {
    pub branch: &'a str,
    pub parent_branch: &'a str,
    pub completed_branches: &'a [String],
    pub remaining_branches: usize,
    pub continue_commands: &'a [&'a str],
    /// Ordered list of branches in the current stack (from trunk to leaf) for
    /// the conflict position indicator.
    pub stack_branches: &'a [String],
}

pub(crate) fn print_restack_conflict(repo: &GitRepo, context: &RestackConflictContext<'_>) {
    let conflicted_files = repo.conflicted_files().unwrap_or_default();

    println!();
    if !context.stack_branches.is_empty() {
        println!("{}", "Stack position:".yellow());
        for line in render_stack_position(context.stack_branches, context.branch) {
            println!("{}", line);
        }
        println!();
    }
    println!("{}", "Restack stopped on conflict:".yellow());
    for line in render_restack_conflict_details(context, &conflicted_files) {
        println!("{}", line);
    }
    println!();
    println!("{}", "Resolve conflicts and run:".yellow());
    for command in context.continue_commands {
        println!("  {}", command.cyan());
    }
}

/// Render a mini stack diagram highlighting the conflicting branch.
///
/// Branches are displayed top-to-bottom (leaf first, trunk last) with
/// `◉` for the conflicting branch and `○` for the rest.
fn render_stack_position(stack_branches: &[String], conflict_branch: &str) -> Vec<String> {
    // Display from leaf to trunk (reverse of the stored order).
    stack_branches
        .iter()
        .rev()
        .map(|name| {
            if name == conflict_branch {
                format!(
                    "  {} {}    {}",
                    "◉".red().bold(),
                    name.red().bold(),
                    "← conflict".red()
                )
            } else {
                format!("  {} {}", "○".dimmed(), name.dimmed())
            }
        })
        .collect()
}

fn render_restack_conflict_details(
    context: &RestackConflictContext<'_>,
    conflicted_files: &[String],
) -> Vec<String> {
    let completed_count = context.completed_branches.len();

    let mut lines = vec![
        format!("  Stopped at: {}", context.branch),
        format!("  Parent: {}", context.parent_branch),
        format!(
            "  Progress: {} rebased before conflict, {} remaining in stack",
            branch_count_label(completed_count),
            branch_count_label(context.remaining_branches)
        ),
    ];

    if !context.completed_branches.is_empty() {
        lines.push(format!(
            "  Completed: {}",
            context.completed_branches.join(", ")
        ));
    }

    if conflicted_files.is_empty() {
        lines.push("  Conflicted files: (not detected; run `git status`)".to_string());
    } else {
        lines.push("  Conflicted files:".to_string());
        for path in conflicted_files {
            lines.push(format!("    {}", path));
        }
    }

    lines
}

fn branch_count_label(count: usize) -> String {
    format!(
        "{} {}",
        count,
        if count == 1 { "branch" } else { "branches" }
    )
}

#[cfg(test)]
mod tests {
    use super::{render_restack_conflict_details, render_stack_position, RestackConflictContext};

    #[test]
    fn renders_progress_completed_branches_and_conflicted_files() {
        let completed = vec!["feature-a".to_string(), "feature-b".to_string()];
        let stack_branches = vec![
            "main".to_string(),
            "feature-a".to_string(),
            "feature-b".to_string(),
            "feature-c".to_string(),
        ];
        let conflicted_files = vec!["src/lib.rs".to_string(), "README.md".to_string()];
        let context = RestackConflictContext {
            branch: "feature-c",
            parent_branch: "feature-b",
            completed_branches: &completed,
            remaining_branches: 1,
            continue_commands: &["stax continue"],
            stack_branches: &stack_branches,
        };

        let lines = render_restack_conflict_details(&context, &conflicted_files);

        assert_eq!(lines[0], "  Stopped at: feature-c");
        assert_eq!(lines[1], "  Parent: feature-b");
        assert_eq!(
            lines[2],
            "  Progress: 2 branches rebased before conflict, 1 branch remaining in stack"
        );
        assert_eq!(lines[3], "  Completed: feature-a, feature-b");
        assert_eq!(lines[4], "  Conflicted files:");
        assert_eq!(lines[5], "    src/lib.rs");
        assert_eq!(lines[6], "    README.md");
    }

    #[test]
    fn renders_git_status_hint_when_conflicted_files_are_unknown() {
        let completed = Vec::new();
        let stack_branches = vec!["main".to_string(), "feature-a".to_string()];
        let context = RestackConflictContext {
            branch: "feature-a",
            parent_branch: "main",
            completed_branches: &completed,
            remaining_branches: 0,
            continue_commands: &["stax continue"],
            stack_branches: &stack_branches,
        };

        let lines = render_restack_conflict_details(&context, &[]);

        assert_eq!(lines[0], "  Stopped at: feature-a");
        assert_eq!(lines[1], "  Parent: main");
        assert_eq!(
            lines[2],
            "  Progress: 0 branches rebased before conflict, 0 branches remaining in stack"
        );
        assert_eq!(
            lines[3],
            "  Conflicted files: (not detected; run `git status`)"
        );
    }

    #[test]
    fn renders_stack_position_with_conflict_highlighted() {
        let stack = vec![
            "main".to_string(),
            "feature-a".to_string(),
            "feature-b".to_string(),
            "feature-c".to_string(),
        ];

        let lines = render_stack_position(&stack, "feature-b");

        // Displayed leaf-first (reverse order). Check that 4 lines are
        // produced and the conflict branch is in the expected position.
        assert_eq!(lines.len(), 4);
        // The conflict line (feature-b) should contain the marker.
        assert!(lines[1].contains("feature-b"));
        assert!(lines[1].contains("conflict"));
        // Non-conflict lines should still mention their branch names.
        assert!(lines[0].contains("feature-c"));
        assert!(lines[2].contains("feature-a"));
        assert!(lines[3].contains("main"));
    }

    #[test]
    fn renders_stack_position_single_branch() {
        let stack = vec!["main".to_string(), "solo".to_string()];

        let lines = render_stack_position(&stack, "solo");

        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("solo"));
        assert!(lines[0].contains("conflict"));
        assert!(lines[1].contains("main"));
    }
}
