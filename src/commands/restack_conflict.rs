use crate::git::GitRepo;
use colored::Colorize;

pub(crate) struct RestackConflictContext<'a> {
    pub branch: &'a str,
    pub parent_branch: &'a str,
    pub completed_branches: &'a [String],
    pub remaining_branches: usize,
    pub continue_commands: &'a [&'a str],
}

pub(crate) fn print_restack_conflict(repo: &GitRepo, context: &RestackConflictContext<'_>) {
    let conflicted_files = repo.conflicted_files().unwrap_or_default();

    println!();
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
    use super::{render_restack_conflict_details, RestackConflictContext};

    #[test]
    fn renders_progress_completed_branches_and_conflicted_files() {
        let completed = vec!["feature-a".to_string(), "feature-b".to_string()];
        let conflicted_files = vec!["src/lib.rs".to_string(), "README.md".to_string()];
        let context = RestackConflictContext {
            branch: "feature-c",
            parent_branch: "feature-b",
            completed_branches: &completed,
            remaining_branches: 1,
            continue_commands: &["stax continue"],
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
        let context = RestackConflictContext {
            branch: "feature-a",
            parent_branch: "main",
            completed_branches: &completed,
            remaining_branches: 0,
            continue_commands: &["stax continue"],
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
}
