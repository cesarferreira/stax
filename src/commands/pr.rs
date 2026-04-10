use crate::commands::github_list::{
    format_relative_time, print_table, split_flexible_width, terminal_width, CellTone, TableCell,
    TableColumn, TruncationMode,
};
use crate::commands::open::open_url_in_browser;
use crate::config::Config;
use crate::engine::Stack;
use crate::forge::{ForgeClient, RepoPrListItem};
use crate::git::GitRepo;
use crate::remote::RemoteInfo;
use anyhow::Result;
use colored::Colorize;

const TITLE_MIN_WIDTH: usize = 24;
const BRANCH_MIN_WIDTH: usize = 18;
const BRANCH_MAX_WIDTH: usize = 36;

#[allow(dead_code)]
pub fn run() -> Result<()> {
    run_open()
}

/// Open the PR for the current branch in the default browser.
pub fn run_open() -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let config = Config::load()?;

    let branch_info = stack.branches.get(&current);
    if branch_info.is_none() {
        anyhow::bail!(
            "Branch '{}' is not tracked. Use {} to track it first.",
            current,
            "stax branch track".cyan()
        );
    }

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
    let pr_url = remote_info.pr_url(pr_number);

    println!("Opening {} in browser...", pr_url.cyan());
    open_url_in_browser(&pr_url);
    Ok(())
}

/// List open pull requests for the current repository.
pub fn run_list(limit: u8, json: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let remote_info = RemoteInfo::from_repo(&repo, &config)?;
    let repo_label = format!("{}/{}", remote_info.namespace, remote_info.repo);

    let rt = tokio::runtime::Runtime::new()?;
    let prs = rt.block_on(async {
        let client = ForgeClient::new(&remote_info)?;
        client.list_open_pull_requests(limit).await
    })?;

    if json {
        println!("{}", serde_json::to_string_pretty(&prs)?);
        return Ok(());
    }

    print_pr_table(&repo_label, &prs);
    Ok(())
}

fn print_pr_table(repo_label: &str, prs: &[RepoPrListItem]) {
    let branch_strings: Vec<String> = prs.iter().map(|pr| pr.head_branch.clone()).collect();
    let created_strings: Vec<String> = prs
        .iter()
        .map(|pr| format_relative_time(pr.created_at))
        .collect();
    let state_strings: Vec<String> = prs
        .iter()
        .map(|pr| {
            if pr.is_draft {
                "draft".to_string()
            } else {
                pr.state.to_lowercase()
            }
        })
        .collect();

    let id_width = prs
        .iter()
        .map(|pr| format!("#{}", pr.number).len())
        .max()
        .unwrap_or(2)
        .max("ID".len());
    let state_width = state_strings
        .iter()
        .map(|value| value.len())
        .max()
        .unwrap_or("STATE".len())
        .max("STATE".len());
    let created_width = created_strings
        .iter()
        .map(|value| value.len())
        .max()
        .unwrap_or("CREATED".len())
        .max("CREATED".len());
    let branch_pref = branch_strings
        .iter()
        .map(|value| value.len())
        .max()
        .unwrap_or("BRANCH".len())
        .clamp(BRANCH_MIN_WIDTH, BRANCH_MAX_WIDTH);

    let width = terminal_width().max(80);
    let fixed_width = id_width + state_width + created_width + 8;
    let flex_width = width.saturating_sub(fixed_width);
    let (title_width, branch_width) = split_flexible_width(
        flex_width,
        TITLE_MIN_WIDTH,
        branch_pref,
        BRANCH_MIN_WIDTH,
        BRANCH_MAX_WIDTH,
    );

    let columns = vec![
        TableColumn {
            header: "ID",
            width: id_width,
        },
        TableColumn {
            header: "STATE",
            width: state_width,
        },
        TableColumn {
            header: "TITLE",
            width: title_width,
        },
        TableColumn {
            header: "BRANCH",
            width: branch_width,
        },
        TableColumn {
            header: "CREATED",
            width: created_width,
        },
    ];

    let rows = prs
        .iter()
        .zip(state_strings.iter())
        .zip(branch_strings.iter())
        .zip(created_strings.iter())
        .map(|(((pr, state), branch), created)| {
            vec![
                TableCell {
                    text: format!("#{}", pr.number),
                    tone: CellTone::Id,
                    truncation: TruncationMode::None,
                },
                TableCell {
                    text: state.clone(),
                    tone: if pr.is_draft {
                        CellTone::StateDraft
                    } else {
                        CellTone::StateOpen
                    },
                    truncation: TruncationMode::None,
                },
                TableCell {
                    text: pr.title.clone(),
                    tone: CellTone::Default,
                    truncation: TruncationMode::End,
                },
                TableCell {
                    text: branch.clone(),
                    tone: CellTone::Branch,
                    truncation: TruncationMode::Middle,
                },
                TableCell {
                    text: created.clone(),
                    tone: CellTone::Secondary,
                    truncation: TruncationMode::None,
                },
            ]
        })
        .collect::<Vec<_>>();

    print_table(
        repo_label,
        &format!("{} open pull requests", prs.len()),
        "No open pull requests.",
        &columns,
        &rows,
    );
}
