use crate::config::Config;
use crate::engine::Stack;
use crate::forge::ForgeClient;
use crate::git::GitRepo;
use crate::github::pr::PrComment;
use crate::remote::RemoteInfo;
use anyhow::Result;
use colored::Colorize;
use futures_util::{StreamExt, stream};
use serde::Serialize;
use termimad::MadSkin;

/// Show comments on the current branch's PR
pub fn run(plain: bool, stack_scope: bool, all: bool, json: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let config = Config::load()?;

    if stack_scope || all || json {
        return run_inbox(
            &repo,
            &stack,
            &config,
            &current,
            stack_scope,
            all,
            plain,
            json,
        );
    }

    // Check if branch is tracked
    let branch_info = stack.branches.get(&current);
    if branch_info.is_none() {
        anyhow::bail!(
            "Branch '{}' is not tracked. Use {} to track it first.",
            current,
            "stax branch track".cyan()
        );
    }

    // Resolve PR number (local metadata or forge fallback)
    let pr_number = super::resolve_pr::resolve_pr_number(&repo, &stack, &current, &config)?;
    if pr_number.is_none() {
        anyhow::bail!(
            "No PR found for branch '{}'. Use {} to create one.",
            current,
            "stax submit".cyan()
        );
    }

    let pr_number = pr_number.unwrap();
    let remote_info = RemoteInfo::from_repo(&repo, &config)?;

    let rt = tokio::runtime::Runtime::new()?;
    let _enter = rt.enter();
    let client = ForgeClient::new(&remote_info)?;

    let comments = rt.block_on(async { client.list_all_comments(pr_number).await })?;

    if comments.is_empty() {
        println!("No comments on PR #{}", pr_number);
        return Ok(());
    }

    println!(
        "Comments on PR #{} ({} total)\n",
        pr_number.to_string().cyan(),
        comments.len()
    );

    for comment in &comments {
        print_comment(comment, plain);
        println!();
    }

    Ok(())
}

#[derive(Clone)]
struct InboxTarget {
    index: usize,
    branch: String,
    pr_number: u64,
}

#[derive(Serialize)]
struct ReviewInbox {
    schema_version: u8,
    scope: &'static str,
    total_comments: usize,
    skipped_without_pr: Vec<String>,
    pull_requests: Vec<InboxPullRequest>,
}

#[derive(Serialize)]
struct InboxPullRequest {
    branch: String,
    pr_number: u64,
    comments: Vec<InboxComment>,
}

#[derive(Serialize)]
struct InboxComment {
    kind: &'static str,
    author: String,
    created_at: String,
    body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u32>,
}

#[allow(clippy::too_many_arguments)]
fn run_inbox(
    repo: &GitRepo,
    stack: &Stack,
    config: &Config,
    current: &str,
    stack_scope: bool,
    all: bool,
    plain: bool,
    json: bool,
) -> Result<()> {
    let branches = if all {
        let mut branches = stack
            .branches
            .keys()
            .filter(|branch| *branch != &stack.trunk)
            .cloned()
            .collect::<Vec<_>>();
        branches.sort();
        branches
    } else if stack_scope {
        stack
            .current_stack(current)
            .into_iter()
            .filter(|branch| branch != &stack.trunk)
            .collect()
    } else {
        vec![current.to_string()]
    };

    let mut skipped_without_pr = Vec::new();
    let targets = branches
        .into_iter()
        .enumerate()
        .filter_map(|(index, branch)| {
            match stack.branches.get(&branch).and_then(|info| info.pr_number) {
                Some(pr_number) => Some(InboxTarget {
                    index,
                    branch,
                    pr_number,
                }),
                None => {
                    skipped_without_pr.push(branch);
                    None
                }
            }
        })
        .collect::<Vec<_>>();

    let remote = RemoteInfo::from_repo(repo, config)?;
    let rt = tokio::runtime::Runtime::new()?;
    let _enter = rt.enter();
    let client = ForgeClient::new(&remote)?;
    let mut fetched = rt.block_on(async {
        let mut pending = stream::iter(targets.into_iter().map(|target| {
            let client = client.clone();
            async move {
                let comments = client.list_all_comments(target.pr_number).await?;
                Ok::<_, anyhow::Error>((target, comments))
            }
        }))
        .buffer_unordered(crate::parallel::IO_CONCURRENCY_LIMIT);
        let mut fetched = Vec::new();
        while let Some(result) = pending.next().await {
            fetched.push(result?);
        }
        Ok::<_, anyhow::Error>(fetched)
    })?;
    fetched.sort_by_key(|(target, _)| target.index);

    let pull_requests = fetched
        .into_iter()
        .map(|(target, comments)| InboxPullRequest {
            branch: target.branch,
            pr_number: target.pr_number,
            comments: comments
                .into_iter()
                .filter(|comment| !is_stax_generated(comment))
                .map(InboxComment::from)
                .collect(),
        })
        .collect::<Vec<_>>();
    let total_comments = pull_requests.iter().map(|pr| pr.comments.len()).sum();
    let inbox = ReviewInbox {
        schema_version: 1,
        scope: if all {
            "all"
        } else if stack_scope {
            "stack"
        } else {
            "branch"
        },
        total_comments,
        skipped_without_pr,
        pull_requests,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&inbox)?);
    } else {
        print_inbox(&inbox, plain);
    }
    Ok(())
}

fn is_stax_generated(comment: &PrComment) -> bool {
    comment.body().contains(crate::forge::STACK_COMMENT_MARKER)
}

impl From<PrComment> for InboxComment {
    fn from(comment: PrComment) -> Self {
        match comment {
            PrComment::Issue(comment) => Self {
                kind: "conversation",
                author: comment.user,
                created_at: comment.created_at.to_rfc3339(),
                body: strip_html_comments(&comment.body).trim().to_string(),
                path: None,
                line: None,
            },
            PrComment::Review(comment) => Self {
                kind: "review",
                author: comment.user,
                created_at: comment.created_at.to_rfc3339(),
                body: strip_html_comments(&comment.body).trim().to_string(),
                path: Some(comment.path),
                line: comment.line,
            },
        }
    }
}

fn print_inbox(inbox: &ReviewInbox, plain: bool) {
    println!(
        "Review inbox: {} comments across {} PRs\n",
        inbox.total_comments,
        inbox.pull_requests.len()
    );
    for pr in &inbox.pull_requests {
        println!("{}  PR #{}", pr.branch.cyan().bold(), pr.pr_number);
        if pr.comments.is_empty() {
            println!("  {}", "No comments".dimmed());
        }
        for comment in &pr.comments {
            println!(
                "  {} {} {}",
                if comment.kind == "review" {
                    "◆"
                } else {
                    "●"
                },
                comment.author.cyan(),
                comment.created_at.dimmed()
            );
            print_body(&comment.body, plain);
        }
        println!();
    }
    if !inbox.skipped_without_pr.is_empty() {
        println!(
            "Skipped without PR: {}",
            inbox.skipped_without_pr.join(", ").dimmed()
        );
    }
}

fn print_comment(comment: &PrComment, plain: bool) {
    let timestamp = comment.created_at().format("%Y-%m-%d %H:%M").to_string();

    match comment {
        PrComment::Issue(c) => {
            println!("{} {} {}", "●".blue(), c.user.cyan(), timestamp.dimmed());
            print_body(&c.body, plain);
        }
        PrComment::Review(c) => {
            let location = if let Some(line) = c.line {
                if let Some(start) = c.start_line {
                    if start != line {
                        format!("{}:{}-{}", c.path, start, line)
                    } else {
                        format!("{}:{}", c.path, line)
                    }
                } else {
                    format!("{}:{}", c.path, line)
                }
            } else {
                c.path.clone()
            };

            println!(
                "{} {} {} {}",
                "◆".yellow(),
                c.user.cyan(),
                location.yellow(),
                timestamp.dimmed()
            );

            // Show diff hunk context if available
            if let Some(ref hunk) = c.diff_hunk {
                // Show last few lines of diff hunk for context
                let lines: Vec<&str> = hunk.lines().collect();
                let start = if lines.len() > 3 { lines.len() - 3 } else { 0 };
                for line in &lines[start..] {
                    let colored_line = if line.starts_with('+') {
                        line.green().to_string()
                    } else if line.starts_with('-') {
                        line.red().to_string()
                    } else {
                        line.dimmed().to_string()
                    };
                    println!("  {}", colored_line);
                }
            }

            print_body(&c.body, plain);
        }
    }
}

fn print_body(body: &str, plain: bool) {
    // Strip HTML comments
    let cleaned = strip_html_comments(body);
    let cleaned = cleaned.trim();

    if cleaned.is_empty() {
        return;
    }

    if plain {
        // Plain mode: just indent and print
        for line in cleaned.lines() {
            println!("  {}", line);
        }
    } else {
        // Render markdown for terminal
        let skin = MadSkin::default();
        let rendered = skin.term_text(cleaned);
        for line in rendered.to_string().lines() {
            println!("  {}", line);
        }
    }
}

/// Strip HTML comments from text (e.g., <!-- comment -->)
fn strip_html_comments(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '<' {
            // Check for <!--
            let mut lookahead = String::from("<");
            let mut is_comment = false;

            for _ in 0..3 {
                if let Some(&next) = chars.peek() {
                    lookahead.push(next);
                    chars.next();
                }
            }

            if lookahead == "<!--" {
                is_comment = true;
                // Skip until -->
                let mut found_end = false;
                while let Some(c) = chars.next() {
                    if c == '-' {
                        if let Some(&'-') = chars.peek() {
                            chars.next();
                            if let Some(&'>') = chars.peek() {
                                chars.next();
                                found_end = true;
                                break;
                            }
                        }
                    }
                }
                if !found_end {
                    // Malformed comment, just skip
                }
            }

            if !is_comment {
                result.push_str(&lookahead);
            }
        } else {
            result.push(c);
        }
    }

    result
}
