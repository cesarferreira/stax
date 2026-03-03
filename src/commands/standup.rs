use crate::commands::generate;
use crate::config::Config;
use crate::engine::Stack;
use crate::git::GitRepo;
use crate::github::{GitHubClient, PrActivity, ReviewActivity};
use crate::progress::LiveTimer;
use crate::remote::RemoteInfo;
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use colored::Colorize;
use regex::Regex;
use serde::Serialize;
use std::process::Command;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    jit: Option<JitSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    jit_error: Option<String>,
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

#[derive(Clone, Serialize)]
struct JitTicket {
    key: String,
    summary: String,
    status: String,
    prs: Vec<String>,
}

#[derive(Clone, Serialize)]
struct JitSummary {
    sprint: Option<String>,
    total_tickets: usize,
    tickets_with_prs: Vec<JitTicket>,
    tickets_with_prs_not_started: Vec<JitTicket>,
    next_up: Vec<JitTicket>,
}

/// All collected standup activity in one place.
struct StandupData {
    hours: i64,
    current_branch: String,
    trunk: String,
    merged_prs: Vec<PrActivity>,
    opened_prs: Vec<PrActivity>,
    reviews_received: Vec<ReviewActivity>,
    reviews_given: Vec<ReviewActivity>,
    recent_pushes: Vec<PushActivity>,
    needs_attention: NeedsAttention,
}

fn collect_standup_data(all: bool, hours: i64) -> Result<StandupData> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let config = Config::load()?;
    let remote_info = RemoteInfo::from_repo(&repo, &config).ok();

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

    let (merged_prs, opened_prs, reviews_received, reviews_given) =
        fetch_github_activity(&remote_info, hours);

    let recent_pushes = get_recent_pushes(&repo, &branches_to_show, hours);
    let needs_attention =
        build_needs_attention(&repo, &stack, &branches_to_show, &reviews_received);

    Ok(StandupData {
        hours,
        current_branch: current,
        trunk: stack.trunk,
        merged_prs,
        opened_prs,
        reviews_received,
        reviews_given,
        recent_pushes,
        needs_attention,
    })
}

pub fn run(
    json: bool,
    all: bool,
    hours: i64,
    summary: bool,
    jit: bool,
    agent_flag: Option<String>,
    plain_text: bool,
) -> Result<()> {
    if plain_text && !summary {
        bail!("--plain-text only applies when used with --summary");
    }

    let data = collect_standup_data(all, hours)?;
    let (jit_summary, jit_error) = if jit {
        match collect_jit_summary(30) {
            Ok(summary) => (Some(summary), None),
            Err(err) => (None, Some(err.to_string())),
        }
    } else {
        (None, None)
    };

    if summary {
        // --summary --json → {"summary": "..."}
        if json {
            let raw = generate_summary(&data, jit_summary.as_ref(), agent_flag.as_deref(), true)?;
            let mut out = serde_json::json!({ "summary": raw.trim() });
            if jit {
                out["jit"] = serde_json::to_value(&jit_summary)?;
                out["jit_error"] = serde_json::to_value(&jit_error)?;
            }
            println!("{}", serde_json::to_string_pretty(&out)?);
            return Ok(());
        }
        // --summary --plain-text → raw text, no spinner, no colors
        if plain_text {
            let raw = generate_summary(&data, jit_summary.as_ref(), agent_flag.as_deref(), true)?;
            println!("{}", raw.trim());
            return Ok(());
        }
        if let Some(err) = &jit_error {
            println!(
                "{} {}",
                "Warning:".yellow().bold(),
                format!("could not load jit context ({})", err).dimmed()
            );
            println!();
        }
        // --summary alone → spinner + card with colors
        let raw = generate_summary(&data, jit_summary.as_ref(), agent_flag.as_deref(), false)?;
        print_summary_card(raw.trim());
        return Ok(());
    }

    let StandupData {
        hours,
        current_branch,
        trunk,
        merged_prs,
        opened_prs,
        reviews_received,
        reviews_given,
        recent_pushes,
        needs_attention,
    } = data;

    if json {
        let output = StandupJson {
            period_hours: hours,
            current_branch: current_branch.clone(),
            trunk: trunk.clone(),
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
            jit: jit_summary,
            jit_error: if jit { jit_error } else { None },
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

    println!("{}", format!("Standup Summary ({})", period).bold());
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
    let pushes_with_activity: Vec<_> = recent_pushes
        .iter()
        .filter(|p| p.commit_count > 0)
        .collect();
    if !pushes_with_activity.is_empty() {
        println!("{}", "Pushed".yellow().bold());
        for push in &pushes_with_activity {
            let commit_word = if push.commit_count == 1 {
                "commit"
            } else {
                "commits"
            };
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
            println!(
                "   {} PR on {} has requested changes",
                "•".red(),
                branch.cyan()
            );
        }

        for branch in &needs_attention.ci_failing {
            println!("   {} CI failing on {}", "•".red(), branch.cyan());
        }

        for branch in &needs_attention.branches_needing_restack {
            println!("   {} {} needs restack", "•".yellow(), branch.cyan());
        }
        println!();
    }

    if let Some(jit_data) = &jit_summary {
        print_jit_section(jit_data);
    } else if let Some(err) = &jit_error {
        println!("{}", "Jira (jit)".magenta().bold());
        println!(
            "   {} {}",
            "•".yellow(),
            format!("Unable to load context: {}", err).dimmed()
        );
        println!();
    }

    let has_jit_signal = jit_summary
        .as_ref()
        .map(|j| {
            !j.tickets_with_prs.is_empty()
                || !j.tickets_with_prs_not_started.is_empty()
                || !j.next_up.is_empty()
        })
        .unwrap_or(false);

    // Empty state
    if merged_prs.is_empty()
        && opened_prs.is_empty()
        && reviews_received.is_empty()
        && reviews_given.is_empty()
        && pushes_with_activity.is_empty()
        && !has_attention
        && !has_jit_signal
    {
        println!("{}", "No activity in the last {} hours.".dimmed());
        println!();
    }

    Ok(())
}

fn build_standup_prompt(data: &StandupData, jit: Option<&JitSummary>) -> String {
    let period = if data.hours == 24 {
        "last 24 hours".to_string()
    } else {
        format!("last {} hours", data.hours)
    };

    let mut prompt = String::new();
    prompt.push_str(
        "You are writing a standup update that will be spoken out loud in a team meeting. \
        Write 2-3 short, natural sentences in first person. \
        Focus on WHAT was worked on, not git mechanics. \
        Do NOT mention PR numbers, branch names, commit counts, or Jira ticket IDs — those are noise. \
        Do NOT list items — write flowing sentences like a human would say them. \
        If there are branches needing restack or cleanup, just say something vague like \
        \"I also have some branch cleanup to do\" — don't enumerate them. \
        Past tense for finished work, present/future for what's next.\n\n",
    );
    prompt.push_str(&format!("Activity from the {}:\n\n", period));

    if !data.merged_prs.is_empty() {
        prompt.push_str("Shipped/merged:\n");
        for pr in &data.merged_prs {
            prompt.push_str(&format!("- {}\n", pr.title));
        }
        prompt.push('\n');
    }

    if !data.opened_prs.is_empty() {
        prompt.push_str("Opened for review:\n");
        for pr in &data.opened_prs {
            prompt.push_str(&format!("- {}\n", pr.title));
        }
        prompt.push('\n');
    }

    // Deduplicate reviews by PR number so the same PR doesn't appear multiple times
    let mut seen_review_prs = std::collections::HashSet::new();
    let unique_reviews_given: Vec<_> = data
        .reviews_given
        .iter()
        .filter(|r| seen_review_prs.insert(r.pr_number))
        .collect();

    if !unique_reviews_given.is_empty() {
        let count = unique_reviews_given.len();
        prompt.push_str(&format!("Reviewed {} PR(s) from teammates.\n\n", count));
    }

    let has_attention = !data.needs_attention.branches_needing_restack.is_empty()
        || !data.needs_attention.ci_failing.is_empty()
        || !data.needs_attention.prs_with_requested_changes.is_empty();

    if !data.needs_attention.prs_with_requested_changes.is_empty() {
        prompt.push_str("Blockers: has PRs with requested changes that need addressing.\n\n");
    } else if has_attention {
        prompt.push_str("Has some branch/stack cleanup to do today.\n\n");
    }

    if let Some(jit) = jit {
        if let Some(sprint) = &jit.sprint {
            prompt.push_str(&format!("Jira sprint context: {}.\n", sprint));
        }
        if !jit.tickets_with_prs.is_empty() {
            prompt.push_str("Jira tickets that already have PRs in flight:\n");
            for ticket in &jit.tickets_with_prs {
                prompt.push_str(&format!("- {} [{}]\n", ticket.summary, ticket.status));
            }
            prompt.push('\n');
        }
        if !jit.tickets_with_prs_not_started.is_empty() {
            prompt.push_str(
                "Jira tickets with linked PRs but likely no coding started yet (do not frame as active work):\n",
            );
            for ticket in &jit.tickets_with_prs_not_started {
                prompt.push_str(&format!("- {} [{}]\n", ticket.summary, ticket.status));
            }
            prompt.push('\n');
        }
        if !jit.next_up.is_empty() {
            prompt.push_str("Likely next Jira backlog items to pick up:\n");
            for ticket in &jit.next_up {
                prompt.push_str(&format!("- {} [{}]\n", ticket.summary, ticket.status));
            }
            prompt.push('\n');
        }
    }

    prompt.push_str("Write only the standup update — no preamble, no labels, just the sentences.");
    prompt
}

fn generate_summary(
    data: &StandupData,
    jit: Option<&JitSummary>,
    agent_flag: Option<&str>,
    quiet: bool,
) -> Result<String> {
    let config = Config::load()?;

    let agent = if let Some(a) = agent_flag {
        a.to_string()
    } else {
        config
            .ai
            .agent
            .as_deref()
            .filter(|a| !a.is_empty())
            .context(
                "No AI agent configured. Add [ai] agent = \"claude\" (or \"codex\" / \"gemini\" / \"opencode\") \
                 to ~/.config/stax/config.toml, or pass --agent <name>",
            )?
            .to_string()
    };

    let model = config.ai.model.clone();
    let prompt = build_standup_prompt(data, jit);

    if quiet {
        let raw = generate::invoke_ai_agent(&agent, model.as_deref(), &prompt)?;
        return Ok(raw);
    }

    let timer = LiveTimer::new(&format!("Generating standup summary with {}", agent));
    let result = generate::invoke_ai_agent(&agent, model.as_deref(), &prompt);
    timer.finish_timed();
    result
}

/// Word-wrap plain text to `width` columns. Returns one string per line.
fn word_wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        let mut line_len: usize = 0;
        for word in paragraph.split_whitespace() {
            let word_len = word.chars().count();
            if line_len == 0 {
                line.push_str(word);
                line_len = word_len;
            } else if line_len + 1 + word_len <= width {
                line.push(' ');
                line.push_str(word);
                line_len += 1 + word_len;
            } else {
                lines.push(line);
                line = word.to_string();
                line_len = word_len;
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines
}

/// Render the AI summary inside a padded card, word-wrapped to fit the terminal.
fn print_summary_card(text: &str) {
    let term_width = console::Term::stdout().size().1 as usize;
    // Content width: terminal minus indent (2) + borders (2) + inner padding (4)
    let content_width = term_width.saturating_sub(10).clamp(40, 76);
    let inner_width = content_width + 4; // 2 spaces padding each side

    let lines = word_wrap(text, content_width);

    println!();
    println!(
        "  {}{}{}",
        "╭".dimmed(),
        "─".repeat(inner_width).dimmed(),
        "╮".dimmed()
    );
    println!(
        "  {}{}{}",
        "│".dimmed(),
        " ".repeat(inner_width),
        "│".dimmed()
    );
    for line in &lines {
        let visible_len = line.chars().count();
        let pad_right = content_width.saturating_sub(visible_len);
        let colored = colorize_summary(line);
        println!(
            "  {}  {}{}  {}",
            "│".dimmed(),
            colored,
            " ".repeat(pad_right),
            "│".dimmed()
        );
    }
    println!(
        "  {}{}{}",
        "│".dimmed(),
        " ".repeat(inner_width),
        "│".dimmed()
    );
    println!(
        "  {}{}{}",
        "╰".dimmed(),
        "─".repeat(inner_width).dimmed(),
        "╯".dimmed()
    );
    println!();
}

/// Apply terminal colors to key phrases in the AI-generated standup text.
fn colorize_summary(text: &str) -> String {
    // Patterns and their colorizers, applied in order (word-boundary aware where needed).
    // Each entry: (regex pattern, replacement closure).
    // We process word by word isn't practical with `colored`, so we use regex replace
    // with ANSI escape codes directly via the `colored` crate on captured groups.

    let rules: &[(&str, &dyn Fn(&str) -> String)] = &[
        // Shipped / completed work → green
        (
            r"(?i)\b(shipped|merged|landed|released|finished|completed|closed|deployed)\b",
            &|m: &str| m.green().bold().to_string(),
        ),
        // Active / in-progress work → cyan
        (
            r"(?i)\b(opened|submitted|created|started|pushed|added|wrote|built)\b",
            &|m: &str| m.cyan().to_string(),
        ),
        // Reviewing / collaboration → blue
        (
            r"(?i)\b(reviewed|approved|commented|feedback)\b",
            &|m: &str| m.blue().to_string(),
        ),
        // Blockers / upcoming tasks → yellow
        (
            r"(?i)\b(today|next|cleanup|restack|rebas\w*|unblock\w*|fix\w*|need\w*|working on|going to|plan\w*|also have)\b",
            &|m: &str| m.yellow().to_string(),
        ),
        // "some branch cleanup" type phrases → yellow (multi-word)
        (r"(?i)(some\s+\w+\s+cleanup|some cleanup)", &|m: &str| {
            m.yellow().to_string()
        }),
    ];

    let mut result = text.to_string();
    for (pattern, colorize) in rules {
        let re = Regex::new(pattern).expect("invalid regex");
        // regex::Regex::replace_all with a closure that applies color
        result = re
            .replace_all(&result, |caps: &regex::Captures| colorize(&caps[0]))
            .to_string();
    }
    result
}

fn collect_jit_summary(limit: usize) -> Result<JitSummary> {
    let output = Command::new("jit")
        .args([
            "--my-tickets",
            "--include-prs",
            "--limit",
            &limit.to_string(),
        ])
        .env("NO_COLOR", "1")
        .output()
        .context("failed to execute `jit`")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            bail!("`jit` exited with status {}", output.status);
        }
        bail!("`jit` failed: {}", stderr);
    }

    let stdout = String::from_utf8(output.stdout).context("`jit` returned non-UTF8 output")?;
    parse_jit_summary(&stdout)
}

fn parse_jit_summary(raw: &str) -> Result<JitSummary> {
    #[derive(Clone, Copy)]
    struct HeaderIndex {
        key: usize,
        summary: usize,
        status: usize,
        prs: Option<usize>,
    }

    let sprint = raw
        .lines()
        .find_map(|line| {
            line.strip_prefix("Current Sprint:")
                .map(|s| s.trim().to_string())
        })
        .filter(|s| !s.is_empty());

    let ansi_re = Regex::new(r"\x1b\[[0-9;]*m").expect("valid ANSI regex");
    let key_re = Regex::new(r"^[A-Z][A-Z0-9]+-\d+$").expect("valid Jira key regex");
    let pr_re = Regex::new(r"#\d+").expect("valid PR regex");

    let mut header: Option<HeaderIndex> = None;
    let mut tickets: Vec<JitTicket> = Vec::new();

    for line in raw.lines() {
        if !line.contains('│') {
            continue;
        }

        let cleaned = ansi_re.replace_all(line, "");
        let cols: Vec<String> = cleaned
            .split('│')
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(ToString::to_string)
            .collect();

        if cols.is_empty() {
            continue;
        }

        if header.is_none() {
            let mut key_idx = None;
            let mut summary_idx = None;
            let mut status_idx = None;
            let mut prs_idx = None;
            for (idx, col) in cols.iter().enumerate() {
                let label = col.to_ascii_lowercase();
                match label.as_str() {
                    "key" => key_idx = Some(idx),
                    "summary" => summary_idx = Some(idx),
                    "status" => status_idx = Some(idx),
                    "prs" | "prs." | "pull requests" => prs_idx = Some(idx),
                    _ => {}
                }
            }
            if let (Some(key), Some(summary), Some(status)) = (key_idx, summary_idx, status_idx) {
                header = Some(HeaderIndex {
                    key,
                    summary,
                    status,
                    prs: prs_idx,
                });
            }
            continue;
        }

        let Some(h) = header else {
            continue;
        };
        if cols.len() <= h.key || cols.len() <= h.summary || cols.len() <= h.status {
            continue;
        }

        let key = cols[h.key].trim();
        if !key_re.is_match(key) {
            continue;
        }

        let prs = h
            .prs
            .and_then(|idx| cols.get(idx))
            .map(|cell| {
                pr_re
                    .find_iter(cell)
                    .map(|m| m.as_str().to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        tickets.push(JitTicket {
            key: key.to_string(),
            summary: cols[h.summary].trim().to_string(),
            status: cols[h.status].trim().to_string(),
            prs,
        });
    }

    if tickets.is_empty() {
        bail!("no tickets found in `jit` output");
    }

    let mut tickets_with_prs_not_started: Vec<JitTicket> = tickets
        .iter()
        .filter(|t| !t.prs.is_empty())
        .filter(|t| is_not_started_status(&t.status))
        .cloned()
        .collect();
    tickets_with_prs_not_started.sort_by_key(|t| jit_next_up_rank(&t.status));
    tickets_with_prs_not_started.truncate(5);

    let mut tickets_with_prs: Vec<JitTicket> = tickets
        .iter()
        .filter(|t| !t.prs.is_empty())
        .filter(|t| !is_not_started_status(&t.status))
        .cloned()
        .collect();
    tickets_with_prs.sort_by_key(|t| jit_inflight_rank(&t.status));
    tickets_with_prs.truncate(5);

    let mut next_up: Vec<JitTicket> = tickets
        .iter()
        .filter(|t| t.prs.is_empty())
        .filter(|t| !is_done_status(&t.status))
        .filter(|t| is_not_started_status(&t.status))
        .cloned()
        .collect();

    if next_up.is_empty() {
        next_up = tickets
            .iter()
            .filter(|t| t.prs.is_empty())
            .filter(|t| !is_done_status(&t.status))
            .cloned()
            .collect();
    }

    next_up.sort_by_key(|t| jit_next_up_rank(&t.status));
    next_up.truncate(5);

    Ok(JitSummary {
        sprint,
        total_tickets: tickets.len(),
        tickets_with_prs,
        tickets_with_prs_not_started,
        next_up,
    })
}

fn is_done_status(status: &str) -> bool {
    let s = status.to_ascii_lowercase();
    s.contains("done")
        || s.contains("closed")
        || s.contains("resolved")
        || s.contains("complete")
        || s.contains("cancelled")
        || s.contains("canceled")
}

fn is_not_started_status(status: &str) -> bool {
    let s = status.to_ascii_lowercase();
    s.contains("selected")
        || s.contains("backlog")
        || s == "todo"
        || s == "to do"
        || s.contains("ready")
        || s.contains("triage")
        || s.contains("open")
}

fn jit_inflight_rank(status: &str) -> usize {
    let s = status.to_ascii_lowercase();
    if s.contains("in progress") {
        0
    } else if s.contains("review") {
        1
    } else if s.contains("selected") || s.contains("ready") {
        2
    } else if is_done_status(status) {
        3
    } else {
        4
    }
}

fn jit_next_up_rank(status: &str) -> usize {
    let s = status.to_ascii_lowercase();
    if s.contains("selected") || s.contains("ready") {
        0
    } else if s == "todo" || s == "to do" {
        1
    } else if s.contains("backlog") {
        2
    } else if s.contains("triage") || s.contains("open") {
        3
    } else if s.contains("in progress") {
        4
    } else {
        5
    }
}

fn print_jit_section(jit: &JitSummary) {
    println!("{}", "Jira (jit)".magenta().bold());
    if let Some(sprint) = &jit.sprint {
        println!("   {} Sprint: {}", "•".magenta(), sprint.cyan());
    }

    if !jit.tickets_with_prs.is_empty() {
        println!("   {} Tickets with PRs:", "•".magenta());
        for ticket in &jit.tickets_with_prs {
            let prs = ticket.prs.join(", ");
            println!(
                "     {} {}: {} ({}, {})",
                "•".magenta(),
                ticket.key.cyan(),
                ticket.summary,
                ticket.status.dimmed(),
                prs.cyan()
            );
        }
    }

    if !jit.tickets_with_prs_not_started.is_empty() {
        println!("   {} Linked PRs but likely not started:", "•".yellow());
        for ticket in &jit.tickets_with_prs_not_started {
            let prs = ticket.prs.join(", ");
            println!(
                "     {} {}: {} ({}, {})",
                "•".yellow(),
                ticket.key.cyan(),
                ticket.summary,
                ticket.status.dimmed(),
                prs.cyan()
            );
        }
    }

    if !jit.next_up.is_empty() {
        println!("   {} Next up from backlog:", "•".yellow());
        for ticket in &jit.next_up {
            println!(
                "     {} {}: {} ({})",
                "•".yellow(),
                ticket.key.cyan(),
                ticket.summary,
                ticket.status.dimmed()
            );
        }
    }

    if jit.tickets_with_prs.is_empty()
        && jit.tickets_with_prs_not_started.is_empty()
        && jit.next_up.is_empty()
    {
        println!(
            "   {} No in-flight or next-up Jira tickets found",
            "•".dimmed()
        );
    }

    println!();
}

fn fetch_github_activity(
    remote_info: &Option<RemoteInfo>,
    hours: i64,
) -> (
    Vec<PrActivity>,
    Vec<PrActivity>,
    Vec<ReviewActivity>,
    Vec<ReviewActivity>,
) {
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
                stack.branches.get(*b).and_then(|info| info.pr_number) == Some(r.pr_number)
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
            jit: None,
            jit_error: None,
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

    #[test]
    fn test_parse_jit_summary_extracts_smart_buckets() {
        let raw = r#"
Current Sprint: OBX Sprint

┌────────┬───────────────────────────────┬──────────────────────────┬──────────┐
│ Key    │ Summary                       │ Status                   │ PRs      │
├────────┼───────────────────────────────┼──────────────────────────┼──────────┤
│ APP-1  │ Improve startup time          │ In progress              │ #12345   │
├────────┼───────────────────────────────┼──────────────────────────┼──────────┤
│ APP-2  │ Handle map re-centering       │ Selected for development │ -        │
├────────┼───────────────────────────────┼──────────────────────────┼──────────┤
│ APP-3  │ Add missing route telemetry   │ Backlog                  │ -        │
├────────┼───────────────────────────────┼──────────────────────────┼──────────┤
│ APP-4  │ Update docs                   │ Done                     │ #54321   │
├────────┼───────────────────────────────┼──────────────────────────┼──────────┤
│ APP-5  │ Spike integration approach    │ Backlog                  │ #77777   │
└────────┴───────────────────────────────┴──────────────────────────┴──────────┘
"#;

        let parsed = parse_jit_summary(raw).unwrap();

        assert_eq!(parsed.sprint.as_deref(), Some("OBX Sprint"));
        assert_eq!(parsed.total_tickets, 5);
        assert_eq!(parsed.tickets_with_prs.len(), 2);
        assert_eq!(parsed.tickets_with_prs[0].key, "APP-1");
        assert_eq!(parsed.tickets_with_prs_not_started.len(), 1);
        assert_eq!(parsed.tickets_with_prs_not_started[0].key, "APP-5");
        assert_eq!(parsed.next_up.len(), 2);
        assert_eq!(parsed.next_up[0].key, "APP-2");
    }

    #[test]
    fn test_parse_jit_summary_errors_when_no_tickets_present() {
        let raw = "Current Sprint: Sprint\n\nNo tickets found in the current sprint.";
        assert!(parse_jit_summary(raw).is_err());
    }
}
