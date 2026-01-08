use crate::config::Config;
use crate::engine::Stack;
use crate::git::GitRepo;
use crate::github::{GitHubClient, PrActivity, ReviewActivity};
use crate::remote::RemoteInfo;
use anyhow::Result;
use chrono::{DateTime, Utc};
use colored::Colorize;
use serde::Serialize;

/// JSON output structure for standup
#[derive(Serialize)]
struct StandupJson {
    period_hours: i64,
    current_branch: String,
    trunk: String,
    merged_prs: Vec<PrActivityJson>,
    opened_prs: Vec<PrActivityJson>,
    reviews_received: Vec<ReviewActivityJson>,
    reviews_given: Vec<ReviewActivityJson>,
    recent_pushes: Vec<PushActivity>,
    needs_attention: NeedsAttention,
}

#[derive(Serialize)]
struct PrActivityJson {
    number: u64,
    title: String,
    timestamp: String,
    age: String,
}

#[derive(Serialize)]
struct ReviewActivityJson {
    pr_number: u64,
    pr_title: String,
    reviewer: String,
    state: String,
    timestamp: String,
    age: String,
}

#[derive(Serialize)]
struct PushActivity {
    branch: String,
    commit_count: usize,
    age: String,
}

#[derive(Serialize)]
struct NeedsAttention {
    branches_needing_restack: Vec<String>,
    ci_failing: Vec<String>,
    prs_with_requested_changes: Vec<String>,
}

pub fn run(json: bool, all: bool, hours: i64) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let config = Config::load()?;
    let remote_info = RemoteInfo::from_repo(&repo, &config).ok();

    // Get branches to check for activity
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

    // Fetch GitHub activity
    let (merged_prs, opened_prs, reviews_received, reviews_given) =
        fetch_github_activity(&remote_info, hours);

    // Get recent push activity from git
    let recent_pushes = get_recent_pushes(&repo, &branches_to_show, hours);

    // Build needs attention section
    let needs_attention = build_needs_attention(&repo, &stack, &branches_to_show, &reviews_received);

    if json {
        let output = StandupJson {
            period_hours: hours,
            current_branch: current.clone(),
            trunk: stack.trunk.clone(),
            merged_prs: merged_prs
                .iter()
                .map(|pr| PrActivityJson {
                    number: pr.number,
                    title: pr.title.clone(),
                    timestamp: pr.timestamp.to_rfc3339(),
                    age: format_age(pr.timestamp),
                })
                .collect(),
            opened_prs: opened_prs
                .iter()
                .map(|pr| PrActivityJson {
                    number: pr.number,
                    title: pr.title.clone(),
                    timestamp: pr.timestamp.to_rfc3339(),
                    age: format_age(pr.timestamp),
                })
                .collect(),
            reviews_received: reviews_received
                .iter()
                .map(|r| ReviewActivityJson {
                    pr_number: r.pr_number,
                    pr_title: r.pr_title.clone(),
                    reviewer: r.reviewer.clone(),
                    state: r.state.clone(),
                    timestamp: r.timestamp.to_rfc3339(),
                    age: format_age(r.timestamp),
                })
                .collect(),
            reviews_given: reviews_given
                .iter()
                .map(|r| ReviewActivityJson {
                    pr_number: r.pr_number,
                    pr_title: r.pr_title.clone(),
                    reviewer: r.reviewer.clone(),
                    state: r.state.clone(),
                    timestamp: r.timestamp.to_rfc3339(),
                    age: format_age(r.timestamp),
                })
                .collect(),
            recent_pushes,
            needs_attention,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    // Human-readable output
    let period = if hours == 24 {
        "last 24 hours".to_string()
    } else {
        format!("last {} hours", hours)
    };

    println!(
        "{}",
        format!("Standup Summary ({})", period).bold()
    );
    println!("{}", "─".repeat(40).dimmed());
    println!();

    // Merged PRs
    if !merged_prs.is_empty() {
        println!("{}", "Merged".green().bold());
        for pr in &merged_prs {
            println!(
                "   {} PR #{}: {} ({})",
                "•".green(),
                pr.number.to_string().bright_magenta(),
                pr.title,
                format_age(pr.timestamp).dimmed()
            );
        }
        println!();
    }

    // Opened PRs
    if !opened_prs.is_empty() {
        println!("{}", "Opened".cyan().bold());
        for pr in &opened_prs {
            println!(
                "   {} PR #{}: {} ({})",
                "•".cyan(),
                pr.number.to_string().bright_magenta(),
                pr.title,
                format_age(pr.timestamp).dimmed()
            );
        }
        println!();
    }

    // Reviews
    if !reviews_received.is_empty() || !reviews_given.is_empty() {
        println!("{}", "Reviews".blue().bold());

        for review in &reviews_received {
            let state_str = format_review_state(&review.state);
            println!(
                "   {} {} on PR #{} from @{} ({})",
                "•".blue(),
                state_str,
                review.pr_number.to_string().bright_magenta(),
                review.reviewer.cyan(),
                format_age(review.timestamp).dimmed()
            );
        }

        for review in &reviews_given {
            let state_str = format_review_state(&review.state);
            println!(
                "   {} You {} PR #{} ({})",
                "•".blue(),
                state_str.to_lowercase(),
                review.pr_number.to_string().bright_magenta(),
                format_age(review.timestamp).dimmed()
            );
        }
        println!();
    }

    // Recent pushes
    let pushes_with_activity: Vec<_> = recent_pushes.iter().filter(|p| p.commit_count > 0).collect();
    if !pushes_with_activity.is_empty() {
        println!("{}", "Pushed".yellow().bold());
        for push in &pushes_with_activity {
            let commit_word = if push.commit_count == 1 { "commit" } else { "commits" };
            println!(
                "   {} {} {} to {} ({})",
                "•".yellow(),
                push.commit_count,
                commit_word,
                push.branch.cyan(),
                push.age.dimmed()
            );
        }
        println!();
    }

    // Needs attention
    let has_attention = !needs_attention.branches_needing_restack.is_empty()
        || !needs_attention.ci_failing.is_empty()
        || !needs_attention.prs_with_requested_changes.is_empty();

    if has_attention {
        println!("{}", "Needs Attention".red().bold());

        for branch in &needs_attention.prs_with_requested_changes {
            println!("   {} PR on {} has requested changes", "•".red(), branch.cyan());
        }

        for branch in &needs_attention.ci_failing {
            println!("   {} CI failing on {}", "•".red(), branch.cyan());
        }

        for branch in &needs_attention.branches_needing_restack {
            println!("   {} {} needs restack", "•".yellow(), branch.cyan());
        }
        println!();
    }

    // Empty state
    if merged_prs.is_empty()
        && opened_prs.is_empty()
        && reviews_received.is_empty()
        && reviews_given.is_empty()
        && pushes_with_activity.is_empty()
        && !has_attention
    {
        println!(
            "{}",
            "No activity in the last {} hours.".dimmed()
        );
        println!();
    }

    Ok(())
}

fn fetch_github_activity(
    remote_info: &Option<RemoteInfo>,
    hours: i64,
) -> (Vec<PrActivity>, Vec<PrActivity>, Vec<ReviewActivity>, Vec<ReviewActivity>) {
    let Some(remote) = remote_info else {
        return (vec![], vec![], vec![], vec![]);
    };

    if Config::github_token().is_none() {
        return (vec![], vec![], vec![], vec![]);
    }

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return (vec![], vec![], vec![], vec![]),
    };

    let client = match rt.block_on(async {
        GitHubClient::new(remote.owner(), &remote.repo, remote.api_base_url.clone())
    }) {
        Ok(client) => client,
        Err(_) => return (vec![], vec![], vec![], vec![]),
    };

    // Get current user
    let username = rt
        .block_on(async { client.get_current_user().await })
        .unwrap_or_default();

    if username.is_empty() {
        return (vec![], vec![], vec![], vec![]);
    }

    // Fetch all activity - using search API filtered by user (fast)
    let merged_prs = rt
        .block_on(async { client.get_recent_merged_prs(hours, &username).await })
        .unwrap_or_default();

    let opened_prs = rt
        .block_on(async { client.get_recent_opened_prs(hours, &username).await })
        .unwrap_or_default();

    let reviews_received = rt
        .block_on(async { client.get_reviews_received(hours, &username).await })
        .unwrap_or_default();

    let reviews_given = rt
        .block_on(async { client.get_reviews_given(hours, &username).await })
        .unwrap_or_default();

    (merged_prs, opened_prs, reviews_received, reviews_given)
}

fn get_recent_pushes(repo: &GitRepo, branches: &[String], hours: i64) -> Vec<PushActivity> {
    branches
        .iter()
        .filter_map(|branch| {
            repo.recent_branch_activity(branch, hours)
                .ok()
                .flatten()
                .map(|(count, age)| PushActivity {
                    branch: branch.clone(),
                    commit_count: count,
                    age,
                })
        })
        .collect()
}

fn build_needs_attention(
    _repo: &GitRepo,
    stack: &Stack,
    branches: &[String],
    reviews_received: &[ReviewActivity],
) -> NeedsAttention {
    let branches_needing_restack: Vec<String> = branches
        .iter()
        .filter(|b| {
            stack
                .branches
                .get(*b)
                .map(|info| info.needs_restack)
                .unwrap_or(false)
        })
        .cloned()
        .collect();

    // Find PRs with "CHANGES_REQUESTED" reviews
    let prs_with_requested_changes: Vec<String> = reviews_received
        .iter()
        .filter(|r| r.state == "CHANGES_REQUESTED")
        .filter_map(|r| {
            // Find the branch for this PR
            branches.iter().find(|b| {
                stack
                    .branches
                    .get(*b)
                    .and_then(|info| info.pr_number)
                    == Some(r.pr_number)
            })
        })
        .cloned()
        .collect();

    // Note: CI failing would require fetching CI status which adds latency
    // For now, we skip it to keep standup fast
    let ci_failing: Vec<String> = vec![];

    NeedsAttention {
        branches_needing_restack,
        ci_failing,
        prs_with_requested_changes,
    }
}

fn format_age(timestamp: DateTime<Utc>) -> String {
    let now = Utc::now();
    let diff = now.signed_duration_since(timestamp);

    let minutes = diff.num_minutes();
    let hours = diff.num_hours();

    if minutes < 1 {
        "just now".to_string()
    } else if minutes < 60 {
        format!("{}m ago", minutes)
    } else if hours < 24 {
        format!("{}h ago", hours)
    } else {
        let days = hours / 24;
        format!("{}d ago", days)
    }
}

fn format_review_state(state: &str) -> String {
    match state {
        "APPROVED" => "Approved".green().to_string(),
        "CHANGES_REQUESTED" => "Changes requested".red().to_string(),
        "COMMENTED" => "Commented".blue().to_string(),
        "DISMISSED" => "Dismissed".dimmed().to_string(),
        _ => state.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_format_age_just_now() {
        let now = Utc::now();
        assert_eq!(format_age(now), "just now");
    }

    #[test]
    fn test_format_age_minutes() {
        let timestamp = Utc::now() - Duration::minutes(30);
        assert_eq!(format_age(timestamp), "30m ago");
    }

    #[test]
    fn test_format_age_hours() {
        let timestamp = Utc::now() - Duration::hours(5);
        assert_eq!(format_age(timestamp), "5h ago");
    }

    #[test]
    fn test_format_age_days() {
        let timestamp = Utc::now() - Duration::hours(48);
        assert_eq!(format_age(timestamp), "2d ago");
    }

    #[test]
    fn test_format_review_state_approved() {
        let result = format_review_state("APPROVED");
        assert!(result.contains("Approved"));
    }

    #[test]
    fn test_format_review_state_changes_requested() {
        let result = format_review_state("CHANGES_REQUESTED");
        assert!(result.contains("Changes requested"));
    }

    #[test]
    fn test_format_review_state_commented() {
        let result = format_review_state("COMMENTED");
        assert!(result.contains("Commented"));
    }

    #[test]
    fn test_format_review_state_unknown() {
        let result = format_review_state("UNKNOWN_STATE");
        assert_eq!(result, "UNKNOWN_STATE");
    }

    #[test]
    fn test_standup_json_serialization() {
        let output = StandupJson {
            period_hours: 24,
            current_branch: "feature-1".to_string(),
            trunk: "main".to_string(),
            merged_prs: vec![PrActivityJson {
                number: 42,
                title: "Add feature".to_string(),
                timestamp: "2024-01-01T00:00:00Z".to_string(),
                age: "2h ago".to_string(),
            }],
            opened_prs: vec![],
            reviews_received: vec![],
            reviews_given: vec![],
            recent_pushes: vec![PushActivity {
                branch: "feature-1".to_string(),
                commit_count: 3,
                age: "1h ago".to_string(),
            }],
            needs_attention: NeedsAttention {
                branches_needing_restack: vec!["feature-2".to_string()],
                ci_failing: vec![],
                prs_with_requested_changes: vec![],
            },
        };

        let json = serde_json::to_string_pretty(&output).unwrap();
        assert!(json.contains("\"period_hours\": 24"));
        assert!(json.contains("\"current_branch\": \"feature-1\""));
        assert!(json.contains("\"number\": 42"));
        assert!(json.contains("\"commit_count\": 3"));
        assert!(json.contains("feature-2"));
    }

    #[test]
    fn test_push_activity_serialization() {
        let push = PushActivity {
            branch: "my-branch".to_string(),
            commit_count: 5,
            age: "30m ago".to_string(),
        };

        let json = serde_json::to_string(&push).unwrap();
        assert!(json.contains("\"branch\":\"my-branch\""));
        assert!(json.contains("\"commit_count\":5"));
        assert!(json.contains("\"age\":\"30m ago\""));
    }

    #[test]
    fn test_needs_attention_empty() {
        let needs = NeedsAttention {
            branches_needing_restack: vec![],
            ci_failing: vec![],
            prs_with_requested_changes: vec![],
        };

        let has_attention = !needs.branches_needing_restack.is_empty()
            || !needs.ci_failing.is_empty()
            || !needs.prs_with_requested_changes.is_empty();

        assert!(!has_attention);
    }

    #[test]
    fn test_needs_attention_with_items() {
        let needs = NeedsAttention {
            branches_needing_restack: vec!["branch-1".to_string()],
            ci_failing: vec![],
            prs_with_requested_changes: vec!["branch-2".to_string()],
        };

        let has_attention = !needs.branches_needing_restack.is_empty()
            || !needs.ci_failing.is_empty()
            || !needs.prs_with_requested_changes.is_empty();

        assert!(has_attention);
    }
}
