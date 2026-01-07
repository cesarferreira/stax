use crate::cache::CiCache;
use crate::config::Config;
use crate::engine::Stack;
use crate::git::GitRepo;
use crate::github::GitHubClient;
use crate::remote::{self, RemoteInfo};
use anyhow::Result;
use colored::Colorize;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Serialize, Clone)]
struct BranchSummary {
    name: String,
    commits_ahead: usize,
    pr_number: Option<u64>,
    pr_state: Option<String>,
    pr_is_draft: Option<bool>,
    ci_state: Option<String>,
    needs_restack: bool,
    age: Option<String>,
    is_current: bool,
}

#[derive(Serialize)]
struct StandupJson {
    current_branch: String,
    trunk: String,
    total_branches: usize,
    branches_needing_restack: Vec<String>,
    open_prs: Vec<BranchSummary>,
    in_progress: Vec<BranchSummary>,
    ci_failing: Vec<String>,
    ci_pending: Vec<String>,
}

pub fn run(json: bool, all: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let config = Config::load()?;
    let workdir = repo.workdir()?;
    let git_dir = repo.git_dir()?;

    let remote_info = RemoteInfo::from_repo(&repo, &config).ok();
    let remote_branches = remote::get_remote_branches(workdir, config.remote_name())
        .unwrap_or_default()
        .into_iter()
        .collect::<HashSet<_>>();

    // Get branches to show (current stack or all)
    let branches_to_show: Vec<String> = if all {
        stack
            .branches
            .keys()
            .filter(|b| *b != &stack.trunk)
            .cloned()
            .collect()
    } else {
        stack
            .current_stack(&current)
            .into_iter()
            .filter(|b| b != &stack.trunk)
            .collect()
    };

    // Load CI cache
    let cache = CiCache::load(git_dir);

    // Fetch fresh CI states if needed
    let ci_states = if cache.is_stale() {
        fetch_ci_states(&repo, remote_info.as_ref(), &stack, &branches_to_show)
    } else {
        branches_to_show
            .iter()
            .filter_map(|b| cache.get_ci_state(b).map(|s| (b.clone(), s)))
            .collect()
    };

    // Build branch summaries
    let mut summaries: Vec<BranchSummary> = Vec::new();
    for name in &branches_to_show {
        let info = stack.branches.get(name);
        let parent = info.and_then(|b| b.parent.clone());

        let (ahead, _behind) = parent
            .as_deref()
            .and_then(|p| repo.commits_ahead_behind(p, name).ok())
            .unwrap_or((0, 0));

        let pr_state = info
            .and_then(|b| b.pr_state.clone())
            .and_then(|s| if s.trim().is_empty() { None } else { Some(s) });

        let summary = BranchSummary {
            name: name.clone(),
            commits_ahead: ahead,
            pr_number: info.and_then(|b| b.pr_number),
            pr_state,
            pr_is_draft: info.and_then(|b| b.pr_is_draft),
            ci_state: ci_states.get(name).cloned(),
            needs_restack: info.map(|b| b.needs_restack).unwrap_or(false),
            age: repo.branch_age(name).ok(),
            is_current: name == &current,
        };
        summaries.push(summary);
    }

    // Categorize branches
    let needs_restack: Vec<String> = summaries
        .iter()
        .filter(|s| s.needs_restack)
        .map(|s| s.name.clone())
        .collect();

    let open_prs: Vec<BranchSummary> = summaries
        .iter()
        .filter(|s| {
            s.pr_number.is_some()
                && s.pr_state
                    .as_ref()
                    .map(|st| st.to_lowercase() == "open")
                    .unwrap_or(false)
        })
        .cloned()
        .collect();

    let in_progress: Vec<BranchSummary> = summaries
        .iter()
        .filter(|s| s.pr_number.is_none() && s.commits_ahead > 0)
        .cloned()
        .collect();

    let ci_failing: Vec<String> = summaries
        .iter()
        .filter(|s| {
            s.ci_state
                .as_ref()
                .map(|st| st.to_lowercase() == "failure" || st.to_lowercase() == "error")
                .unwrap_or(false)
        })
        .map(|s| s.name.clone())
        .collect();

    let ci_pending: Vec<String> = summaries
        .iter()
        .filter(|s| {
            s.ci_state
                .as_ref()
                .map(|st| st.to_lowercase() == "pending")
                .unwrap_or(false)
        })
        .map(|s| s.name.clone())
        .collect();

    if json {
        let output = StandupJson {
            current_branch: current.clone(),
            trunk: stack.trunk.clone(),
            total_branches: branches_to_show.len(),
            branches_needing_restack: needs_restack,
            open_prs,
            in_progress,
            ci_failing,
            ci_pending,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    // Human-readable output
    println!("{}", "📋 Standup Summary".bold());
    println!();

    // Current position
    println!(
        "{}  on {} (trunk: {})",
        "📍".dimmed(),
        current.cyan().bold(),
        stack.trunk.dimmed()
    );

    let scope = if all { "all stacks" } else { "current stack" };
    println!(
        "{}  {} branches in {}",
        "📊".dimmed(),
        branches_to_show.len(),
        scope
    );
    println!();

    // Needs attention section
    let has_attention_items =
        !needs_restack.is_empty() || !ci_failing.is_empty() || !ci_pending.is_empty();

    if has_attention_items {
        println!("{}", "⚠️  Needs Attention".yellow().bold());

        if !needs_restack.is_empty() {
            println!(
                "   {} {} need restacking:",
                "⟳".bright_yellow(),
                needs_restack.len()
            );
            for branch in &needs_restack {
                let marker = if branch == &current { " ◉" } else { "  " };
                println!("     {}{}", marker, branch.bright_yellow());
            }
        }

        if !ci_failing.is_empty() {
            println!(
                "   {} {} with failing CI:",
                "✗".red(),
                ci_failing.len()
            );
            for branch in &ci_failing {
                let marker = if branch == &current { " ◉" } else { "  " };
                println!("     {}{}", marker, branch.red());
            }
        }

        if !ci_pending.is_empty() {
            println!(
                "   {} {} with pending CI:",
                "⏳".bright_yellow(),
                ci_pending.len()
            );
            for branch in &ci_pending {
                let marker = if branch == &current { " ◉" } else { "  " };
                println!("     {}{}", marker, branch.bright_yellow());
            }
        }
        println!();
    }

    // Open PRs section
    if !open_prs.is_empty() {
        println!("{}", "🔄 Open PRs".green().bold());
        for pr in &open_prs {
            let marker = if pr.is_current { "◉" } else { "○" };
            let draft_indicator = if pr.pr_is_draft.unwrap_or(false) {
                " (draft)".dimmed().to_string()
            } else {
                String::new()
            };
            let ci_indicator = match pr.ci_state.as_deref() {
                Some("success") => " ✓".green().to_string(),
                Some("failure") | Some("error") => " ✗".red().to_string(),
                Some("pending") => " ⏳".yellow().to_string(),
                _ => String::new(),
            };
            let pr_num = pr
                .pr_number
                .map(|n| format!(" PR #{}", n).bright_magenta().to_string())
                .unwrap_or_default();

            println!(
                "   {} {}{}{}{}",
                marker.cyan(),
                pr.name.cyan(),
                pr_num,
                draft_indicator,
                ci_indicator
            );

            if let Some(age) = &pr.age {
                println!("     {}", age.dimmed());
            }
        }
        println!();
    }

    // In progress (no PR yet)
    if !in_progress.is_empty() {
        println!("{}", "🚧 In Progress (no PR)".blue().bold());
        for branch in &in_progress {
            let marker = if branch.is_current { "◉" } else { "○" };
            let commits = if branch.commits_ahead == 1 {
                "1 commit".to_string()
            } else {
                format!("{} commits", branch.commits_ahead)
            };

            println!(
                "   {} {} ({})",
                marker.blue(),
                branch.name.blue(),
                commits.dimmed()
            );

            if let Some(age) = &branch.age {
                println!("     {}", age.dimmed());
            }
        }
        println!();
    }

    // Quick actions
    if has_attention_items || !in_progress.is_empty() {
        println!("{}", "💡 Suggested Actions".dimmed());
        if !needs_restack.is_empty() {
            println!("   {} to rebase branches", "stax rs --restack".cyan());
        }
        if !in_progress.is_empty() {
            println!("   {} to push and create PRs", "stax submit".cyan());
        }
        if !ci_failing.is_empty() {
            println!("   Check CI failures and push fixes");
        }
    } else if open_prs.is_empty() && in_progress.is_empty() {
        println!("{}", "✨ All caught up! No active work in progress.".green());
    }

    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_summary(
        name: &str,
        commits_ahead: usize,
        pr_number: Option<u64>,
        pr_state: Option<&str>,
        ci_state: Option<&str>,
        needs_restack: bool,
        is_current: bool,
    ) -> BranchSummary {
        BranchSummary {
            name: name.to_string(),
            commits_ahead,
            pr_number,
            pr_state: pr_state.map(|s| s.to_string()),
            pr_is_draft: Some(false),
            ci_state: ci_state.map(|s| s.to_string()),
            needs_restack,
            age: Some("1 hour ago".to_string()),
            is_current,
        }
    }

    #[test]
    fn test_branch_summary_serialization() {
        let summary = make_summary("feature-1", 3, Some(123), Some("open"), Some("success"), false, true);
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"name\":\"feature-1\""));
        assert!(json.contains("\"commits_ahead\":3"));
        assert!(json.contains("\"pr_number\":123"));
        assert!(json.contains("\"pr_state\":\"open\""));
        assert!(json.contains("\"ci_state\":\"success\""));
        assert!(json.contains("\"needs_restack\":false"));
        assert!(json.contains("\"is_current\":true"));
    }

    #[test]
    fn test_standup_json_serialization() {
        let summary = make_summary("feature-1", 2, Some(42), Some("open"), Some("pending"), false, true);
        let standup = StandupJson {
            current_branch: "feature-1".to_string(),
            trunk: "main".to_string(),
            total_branches: 3,
            branches_needing_restack: vec!["feature-2".to_string()],
            open_prs: vec![summary],
            in_progress: vec![],
            ci_failing: vec![],
            ci_pending: vec!["feature-1".to_string()],
        };
        let json = serde_json::to_string_pretty(&standup).unwrap();
        assert!(json.contains("\"current_branch\": \"feature-1\""));
        assert!(json.contains("\"trunk\": \"main\""));
        assert!(json.contains("\"total_branches\": 3"));
        assert!(json.contains("\"branches_needing_restack\""));
        assert!(json.contains("feature-2"));
    }

    #[test]
    fn test_filter_needs_restack() {
        let summaries = vec![
            make_summary("branch-1", 1, None, None, None, true, false),
            make_summary("branch-2", 2, None, None, None, false, false),
            make_summary("branch-3", 3, None, None, None, true, true),
        ];

        let needs_restack: Vec<String> = summaries
            .iter()
            .filter(|s| s.needs_restack)
            .map(|s| s.name.clone())
            .collect();

        assert_eq!(needs_restack.len(), 2);
        assert!(needs_restack.contains(&"branch-1".to_string()));
        assert!(needs_restack.contains(&"branch-3".to_string()));
    }

    #[test]
    fn test_filter_open_prs() {
        let summaries = vec![
            make_summary("branch-1", 1, Some(1), Some("open"), None, false, false),
            make_summary("branch-2", 2, Some(2), Some("closed"), None, false, false),
            make_summary("branch-3", 3, Some(3), Some("merged"), None, false, false),
            make_summary("branch-4", 4, None, None, None, false, false),
            make_summary("branch-5", 5, Some(5), Some("OPEN"), None, false, true), // uppercase
        ];

        let open_prs: Vec<BranchSummary> = summaries
            .iter()
            .filter(|s| {
                s.pr_number.is_some()
                    && s.pr_state
                        .as_ref()
                        .map(|st| st.to_lowercase() == "open")
                        .unwrap_or(false)
            })
            .cloned()
            .collect();

        assert_eq!(open_prs.len(), 2);
        assert_eq!(open_prs[0].name, "branch-1");
        assert_eq!(open_prs[1].name, "branch-5");
    }

    #[test]
    fn test_filter_in_progress() {
        let summaries = vec![
            make_summary("branch-1", 3, None, None, None, false, false), // in progress
            make_summary("branch-2", 0, None, None, None, false, false), // no commits
            make_summary("branch-3", 2, Some(1), Some("open"), None, false, false), // has PR
            make_summary("branch-4", 5, None, None, None, false, true), // in progress, current
        ];

        let in_progress: Vec<BranchSummary> = summaries
            .iter()
            .filter(|s| s.pr_number.is_none() && s.commits_ahead > 0)
            .cloned()
            .collect();

        assert_eq!(in_progress.len(), 2);
        assert_eq!(in_progress[0].name, "branch-1");
        assert_eq!(in_progress[1].name, "branch-4");
    }

    #[test]
    fn test_filter_ci_failing() {
        let summaries = vec![
            make_summary("branch-1", 1, Some(1), Some("open"), Some("failure"), false, false),
            make_summary("branch-2", 2, Some(2), Some("open"), Some("success"), false, false),
            make_summary("branch-3", 3, Some(3), Some("open"), Some("error"), false, false),
            make_summary("branch-4", 4, Some(4), Some("open"), Some("pending"), false, false),
            make_summary("branch-5", 5, Some(5), Some("open"), None, false, false),
        ];

        let ci_failing: Vec<String> = summaries
            .iter()
            .filter(|s| {
                s.ci_state
                    .as_ref()
                    .map(|st| st.to_lowercase() == "failure" || st.to_lowercase() == "error")
                    .unwrap_or(false)
            })
            .map(|s| s.name.clone())
            .collect();

        assert_eq!(ci_failing.len(), 2);
        assert!(ci_failing.contains(&"branch-1".to_string()));
        assert!(ci_failing.contains(&"branch-3".to_string()));
    }

    #[test]
    fn test_filter_ci_pending() {
        let summaries = vec![
            make_summary("branch-1", 1, Some(1), Some("open"), Some("pending"), false, false),
            make_summary("branch-2", 2, Some(2), Some("open"), Some("success"), false, false),
            make_summary("branch-3", 3, Some(3), Some("open"), Some("PENDING"), false, false), // uppercase
        ];

        let ci_pending: Vec<String> = summaries
            .iter()
            .filter(|s| {
                s.ci_state
                    .as_ref()
                    .map(|st| st.to_lowercase() == "pending")
                    .unwrap_or(false)
            })
            .map(|s| s.name.clone())
            .collect();

        assert_eq!(ci_pending.len(), 2);
        assert!(ci_pending.contains(&"branch-1".to_string()));
        assert!(ci_pending.contains(&"branch-3".to_string()));
    }

    #[test]
    fn test_branch_summary_clone() {
        let summary = make_summary("test", 5, Some(100), Some("open"), Some("success"), true, true);
        let cloned = summary.clone();
        assert_eq!(cloned.name, "test");
        assert_eq!(cloned.commits_ahead, 5);
        assert_eq!(cloned.pr_number, Some(100));
        assert_eq!(cloned.pr_state, Some("open".to_string()));
        assert_eq!(cloned.ci_state, Some("success".to_string()));
        assert!(cloned.needs_restack);
        assert!(cloned.is_current);
    }

    #[test]
    fn test_empty_summaries() {
        let summaries: Vec<BranchSummary> = vec![];

        let needs_restack: Vec<String> = summaries
            .iter()
            .filter(|s| s.needs_restack)
            .map(|s| s.name.clone())
            .collect();

        let open_prs: Vec<BranchSummary> = summaries
            .iter()
            .filter(|s| {
                s.pr_number.is_some()
                    && s.pr_state
                        .as_ref()
                        .map(|st| st.to_lowercase() == "open")
                        .unwrap_or(false)
            })
            .cloned()
            .collect();

        assert!(needs_restack.is_empty());
        assert!(open_prs.is_empty());
    }

    #[test]
    fn test_commit_count_formatting() {
        // Test single commit
        let branch = make_summary("test", 1, None, None, None, false, false);
        let commits = if branch.commits_ahead == 1 {
            "1 commit".to_string()
        } else {
            format!("{} commits", branch.commits_ahead)
        };
        assert_eq!(commits, "1 commit");

        // Test multiple commits
        let branch = make_summary("test", 5, None, None, None, false, false);
        let commits = if branch.commits_ahead == 1 {
            "1 commit".to_string()
        } else {
            format!("{} commits", branch.commits_ahead)
        };
        assert_eq!(commits, "5 commits");
    }

    #[test]
    fn test_draft_pr_handling() {
        let mut summary = make_summary("feature", 2, Some(42), Some("open"), None, false, false);
        summary.pr_is_draft = Some(true);

        assert!(summary.pr_is_draft.unwrap_or(false));

        let draft_indicator = if summary.pr_is_draft.unwrap_or(false) {
            " (draft)"
        } else {
            ""
        };
        assert_eq!(draft_indicator, " (draft)");
    }

    #[test]
    fn test_ci_indicator_matching() {
        let test_cases = vec![
            (Some("success"), "✓"),
            (Some("failure"), "✗"),
            (Some("error"), "✗"),
            (Some("pending"), "⏳"),
            (None, ""),
        ];

        for (ci_state, expected_icon) in test_cases {
            let indicator = match ci_state {
                Some("success") => "✓",
                Some("failure") | Some("error") => "✗",
                Some("pending") => "⏳",
                _ => "",
            };
            assert_eq!(indicator, expected_icon);
        }
    }
}
