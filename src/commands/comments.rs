use crate::config::Config;
use crate::engine::Stack;
use crate::git::GitRepo;
use crate::github::pr::PrComment;
use crate::github::GitHubClient;
use crate::remote::RemoteInfo;
use anyhow::Result;
use colored::Colorize;

/// Show comments on the current branch's PR
pub fn run() -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let config = Config::load()?;

    // Check if branch is tracked
    let branch_info = stack.branches.get(&current);
    if branch_info.is_none() {
        anyhow::bail!(
            "Branch '{}' is not tracked. Use {} to track it first.",
            current,
            "stax branch track".cyan()
        );
    }

    // Check if branch has a PR
    let pr_number = branch_info.and_then(|b| b.pr_number);
    if pr_number.is_none() {
        anyhow::bail!(
            "No PR found for branch '{}'. Use {} to create one.",
            current,
            "stax submit".cyan()
        );
    }

    let pr_number = pr_number.unwrap();
    let remote_info = RemoteInfo::from_repo(&repo, &config)?;

    // Create GitHub client and fetch comments
    let rt = tokio::runtime::Runtime::new()?;
    let client = GitHubClient::new(
        &remote_info.namespace,
        &remote_info.repo,
        remote_info.api_base_url.clone(),
    )?;

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
        print_comment(comment);
        println!();
    }

    Ok(())
}

fn print_comment(comment: &PrComment) {
    let timestamp = comment.created_at().format("%Y-%m-%d %H:%M").to_string();

    match comment {
        PrComment::Issue(c) => {
            println!(
                "{} {} {}",
                "●".blue(),
                c.user.cyan(),
                timestamp.dimmed()
            );
            print_body(&c.body);
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

            print_body(&c.body);
        }
    }
}

fn print_body(body: &str) {
    // Indent and print body, handling multi-line
    for line in body.lines() {
        println!("  {}", line);
    }
}
