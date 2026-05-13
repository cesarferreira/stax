use anyhow::Result;
use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum TmuxCommand {
    /// Print a compact tmux-formatted status string for use in status-right
    Status,
    /// Open an interactive stack popup via tmux display-popup
    Popup,
}

#[allow(dead_code)]
pub fn run(cmd: TmuxCommand) -> Result<()> {
    match cmd {
        TmuxCommand::Status => run_status(),
        TmuxCommand::Popup => run_popup(),
    }
}

#[allow(dead_code)]
pub fn format_status_line(
    branch: &str,
    pos: usize,
    total: usize,
    pr_number: Option<u64>,
    pr_is_draft: bool,
    pr_state: Option<&str>,
    ci_state: Option<&str>,
) -> String {
    let branch_display = if branch.len() > 20 {
        format!("{}…", &branch[..19])
    } else {
        branch.to_string()
    };

    let pr_str = match pr_number {
        None => "#[fg=colour240]⊘#[fg=default]".to_string(),
        Some(n) if pr_is_draft => format!("#[fg=colour240]#{} draft#[fg=default]", n),
        Some(n) if pr_state.map(|s| s.eq_ignore_ascii_case("merged")).unwrap_or(false) => {
            format!("#[fg=magenta]#{} merged#[fg=default]", n)
        }
        Some(n) => format!("#[fg=magenta]#{}#[fg=default]", n),
    };

    let ci_str = match ci_state {
        Some("success") => "#[fg=green]● passing#[fg=default]",
        Some("failure") => "#[fg=red]✗ failing#[fg=default]",
        Some("pending") => "#[fg=yellow]⟳ running#[fg=default]",
        _ => "#[fg=colour240]– no CI#[fg=default]",
    };

    format!(" {} [{}/{}] {}  {}", branch_display, pos, total, pr_str, ci_str)
}

fn run_status() -> Result<()> {
    todo!()
}

fn run_popup() -> Result<()> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_no_pr_no_ci() {
        let result = format_status_line("feat/foo", 1, 3, None, false, None, None);
        assert!(result.contains("feat/foo"), "branch name missing: {result}");
        assert!(result.contains("[1/3]"), "position missing: {result}");
        assert!(result.contains('⊘'), "no-PR symbol missing: {result}");
        assert!(result.contains("– no CI"), "no-CI text missing: {result}");
    }

    #[test]
    fn test_status_with_open_pr_and_passing_ci() {
        let result = format_status_line("feat/foo", 2, 4, Some(42), false, Some("OPEN"), Some("success"));
        assert!(result.contains("[2/4]"), "position missing: {result}");
        assert!(result.contains("#42"), "PR number missing: {result}");
        assert!(result.contains("● passing"), "CI passing text missing: {result}");
    }

    #[test]
    fn test_status_draft_pr() {
        let result = format_status_line("feat/foo", 1, 1, Some(7), true, Some("OPEN"), None);
        assert!(result.contains("#7 draft"), "draft PR missing: {result}");
    }

    #[test]
    fn test_status_merged_pr() {
        let result = format_status_line("feat/foo", 1, 1, Some(99), false, Some("MERGED"), None);
        assert!(result.contains("#99 merged"), "merged PR missing: {result}");
    }

    #[test]
    fn test_status_failing_ci() {
        let result = format_status_line("feat/foo", 1, 1, None, false, None, Some("failure"));
        assert!(result.contains("✗ failing"), "failing CI missing: {result}");
    }

    #[test]
    fn test_status_running_ci() {
        let result = format_status_line("feat/foo", 1, 1, None, false, None, Some("pending"));
        assert!(result.contains("⟳ running"), "running CI missing: {result}");
    }

    #[test]
    fn test_branch_name_truncated_at_20_chars() {
        let long = "feat/this-is-a-very-long-branch-name";
        let result = format_status_line(long, 1, 1, None, false, None, None);
        // &long[..19] = "feat/this-is-a-very" → appended with "…"
        assert!(result.contains("feat/this-is-a-very…"), "truncation wrong: {result}");
        assert!(!result.contains("feat/this-is-a-very-long"), "should be truncated: {result}");
    }
}
