use crate::config::Config;
use crate::engine::Stack;
use crate::git::GitRepo;
use crate::github::pr::PrComment;
use crate::github::GitHubClient;
use crate::remote::RemoteInfo;
use anyhow::Result;
use colored::Colorize;
use termimad::MadSkin;

/// Show comments on the current branch's PR
pub fn run(plain: bool) -> Result<()> {
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
    // Must create client inside block_on - Octocrab requires runtime context
    let rt = tokio::runtime::Runtime::new()?;
    let client = rt.block_on(async {
        GitHubClient::new(
            &remote_info.namespace,
            &remote_info.repo,
            remote_info.api_base_url.clone(),
        )
    })?;

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

fn print_comment(comment: &PrComment, plain: bool) {
    let timestamp = comment.created_at().format("%Y-%m-%d %H:%M").to_string();

    match comment {
        PrComment::Issue(c) => {
            println!(
                "{} {} {}",
                "●".blue(),
                c.user.cyan(),
                timestamp.dimmed()
            );
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
