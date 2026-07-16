use crate::cache::CiCache;
use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::Result;
use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum TmuxCommand {
    /// Print a compact tmux-formatted status string for use in status-right
    Status,
    /// Open an interactive stack popup via tmux display-popup
    Popup,
}

pub fn run(cmd: TmuxCommand) -> Result<()> {
    match cmd {
        TmuxCommand::Status => run_status(),
        TmuxCommand::Popup => run_popup(),
    }
}

fn truncate_branch_name(branch: &str) -> String {
    if branch.chars().count() > 50 {
        format!("{}…", branch.chars().take(49).collect::<String>())
    } else {
        branch.to_string()
    }
}

pub fn format_status_line(
    branch: &str,
    pos: usize,
    total: usize,
    pr_number: Option<u64>,
    pr_is_draft: bool,
    pr_state: Option<&str>,
    ci_state: Option<&str>,
) -> String {
    let branch_display = truncate_branch_name(branch);

    let pr_str = match pr_number {
        None => "#[fg=colour240]⊘#[fg=default]".to_string(),
        Some(n) if pr_is_draft => format!("#[fg=yellow]#{} draft#[fg=default]", n),
        Some(n)
            if pr_state
                .map(|s| s.eq_ignore_ascii_case("merged"))
                .unwrap_or(false) =>
        {
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

    format!(
        " #[fg=colour250]#[fg=default] {} [{}/{}] {}  {}",
        branch_display, pos, total, pr_str, ci_str
    )
}

pub fn format_branch_status_line(branch: &str) -> String {
    format!(
        " #[fg=colour250]\u{f418}#[fg=default] {}",
        truncate_branch_name(branch)
    )
}

fn run_status() -> Result<()> {
    // Status bar context: fail silently so tmux shows an empty segment rather than an error string
    let repo = match GitRepo::open() {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };
    let stack = match Stack::load(&repo) {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };
    let current = match repo.current_branch() {
        Ok(b) => b,
        Err(_) => return Ok(()),
    };

    if current == stack.trunk {
        print!("{}", format_branch_status_line(&current));
        return Ok(());
    }

    let stack_branches = stack.current_stack(&current);
    let non_trunk: Vec<&String> = stack_branches
        .iter()
        .filter(|b| *b != &stack.trunk)
        .collect();
    let pos = non_trunk
        .iter()
        .position(|b| *b == &current)
        .map(|i| i + 1)
        .unwrap_or(1);
    let total = non_trunk.len().max(1);

    let info = stack.branches.get(&current);
    let pr_number = info.and_then(|b| b.pr_number);
    let pr_state = info.and_then(|b| b.pr_state.as_deref());

    let cache_dir = repo.common_git_dir()?;
    let cache = CiCache::load(&cache_dir);
    let ci_state_owned = repo
        .branch_commit(&current)
        .ok()
        .and_then(|revision| cache.get_ci_state_for_revision(&current, &revision));
    let ci_state = ci_state_owned.as_deref();

    // Prefer cache pr_state over metadata: cache is refreshed on every CI fetch,
    // metadata only updates on `stax submit`. If cache says OPEN the PR is no longer draft.
    let cached_pr_state = cache
        .branches
        .get(&current)
        .and_then(|e| e.pr_state.as_deref());
    let pr_is_draft = match cached_pr_state {
        Some(s) if s.eq_ignore_ascii_case("draft") => true,
        Some(s) if s.eq_ignore_ascii_case("open") => false,
        _ => info.and_then(|b| b.pr_is_draft).unwrap_or(false),
    };

    let output = format_status_line(
        &current,
        pos,
        total,
        pr_number,
        pr_is_draft,
        pr_state,
        ci_state,
    );
    print!("{}", output);

    // Spawn a background `stax ci` refresh when the cache is older than 90 seconds so the
    // status bar stays current without the user having to run stax ci manually.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if now.saturating_sub(cache.last_refresh) > 90
        && let Ok(exe) = std::env::current_exe()
    {
        let _ = std::process::Command::new(exe)
            .arg("ci")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }

    Ok(())
}

fn run_popup() -> Result<()> {
    if std::env::var("TMUX").is_err() {
        anyhow::bail!("Not inside a tmux session. Run this command from within tmux.");
    }
    std::process::Command::new("tmux")
        .args([
            "display-popup",
            "-E",
            "-w",
            "80%",
            "-h",
            "80%",
            "sh",
            "-c",
            "stax watch --current",
        ])
        .status()?;
    Ok(())
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
        let result = format_status_line(
            "feat/foo",
            2,
            4,
            Some(42),
            false,
            Some("OPEN"),
            Some("success"),
        );
        assert!(result.contains("[2/4]"), "position missing: {result}");
        assert!(result.contains("#42"), "PR number missing: {result}");
        assert!(
            result.contains("● passing"),
            "CI passing text missing: {result}"
        );
    }

    #[test]
    fn test_status_draft_pr() {
        let result = format_status_line("feat/foo", 1, 1, Some(7), true, Some("OPEN"), None);
        assert!(result.contains("#7 draft"), "draft PR missing: {result}");
        assert!(
            result.contains("#[fg=yellow]#7 draft"),
            "draft PR should be readable on tmux gray backgrounds: {result}"
        );
    }

    #[test]
    fn test_status_branch_has_nerd_font_icon() {
        let result = format_status_line("feat/foo", 1, 1, None, false, None, None);
        assert!(
            result.contains("#[fg=default] feat/foo"),
            "branch icon missing: {result}"
        );
    }

    #[test]
    fn test_trunk_status_shows_branch_without_stack_metadata() {
        let result = format_branch_status_line("main");
        assert_eq!(result, " #[fg=colour250]#[fg=default] main");
        assert!(
            !result.contains("[1/"),
            "trunk status should not show stack position: {result}"
        );
        assert!(
            !result.contains("CI"),
            "trunk status should not show CI metadata: {result}"
        );
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
    fn test_branch_name_truncated_at_50_chars() {
        let long = "cesar/codex/OBX-2734-remove-worm-feature-and-related-cleanup";
        let result = format_status_line(long, 1, 1, None, false, None, None);
        assert!(
            result.contains("cesar/codex/OBX-2734-remove-worm-feature-and-rela…"),
            "truncation wrong: {result}"
        );
        assert!(
            !result.contains("cesar/codex/OBX-2734-remove-worm-feature-and-related-cleanup"),
            "should be truncated: {result}"
        );
    }

    #[test]
    fn test_branch_name_unicode_does_not_panic() {
        // A branch name where byte 49 falls inside a multi-byte character.
        // Before the fix, &branch[..49] would panic on such input.
        let long = "feat/é".repeat(12); // each "é" is 2 bytes; > 50 chars
        let result = format_status_line(&long, 1, 1, None, false, None, None);
        assert!(result.contains('…'), "should be truncated: {result}");
    }

    #[test]
    fn test_trunk_long_name_truncated() {
        let long = "release/".to_string() + &"a".repeat(50);
        let result = format_branch_status_line(&long);
        assert!(
            result.contains('…'),
            "trunk long name should be truncated: {result}"
        );
        assert!(
            !result.contains(&long),
            "full branch name should not appear: {result}"
        );
    }
}
