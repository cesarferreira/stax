use crate::commands::github_list::{
    format_relative_time, print_table, split_flexible_width, terminal_width, CellTone, TableCell,
    TableColumn, TruncationMode,
};
use crate::config::Config;
use crate::forge::{ForgeClient, RepoIssueListItem};
use crate::git::GitRepo;
use crate::remote::RemoteInfo;
use anyhow::Result;

const TITLE_MIN_WIDTH: usize = 24;
const LABELS_MIN_WIDTH: usize = 12;
const LABELS_MAX_WIDTH: usize = 24;

pub fn run_list(limit: u8, json: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let remote_info = RemoteInfo::from_repo(&repo, &config)?;
    let repo_label = format!("{}/{}", remote_info.namespace, remote_info.repo);

    let rt = tokio::runtime::Runtime::new()?;
    let issues = rt.block_on(async {
        let client = ForgeClient::new(&remote_info)?;
        client.list_open_issues(limit).await
    })?;

    if json {
        println!("{}", serde_json::to_string_pretty(&issues)?);
        return Ok(());
    }

    print_issue_table(&repo_label, &issues);
    Ok(())
}

fn print_issue_table(repo_label: &str, issues: &[RepoIssueListItem]) {
    let updated_strings: Vec<String> = issues
        .iter()
        .map(|issue| format_relative_time(issue.updated_at))
        .collect();
    let label_strings: Vec<String> = issues
        .iter()
        .map(|issue| {
            if issue.labels.is_empty() {
                "—".to_string()
            } else {
                issue.labels.join(", ")
            }
        })
        .collect();

    let id_width = issues
        .iter()
        .map(|issue| format!("#{}", issue.number).len())
        .max()
        .unwrap_or(2)
        .max("ID".len());
    let updated_width = updated_strings
        .iter()
        .map(|value| value.len())
        .max()
        .unwrap_or("UPDATED".len())
        .max("UPDATED".len());
    let label_pref = label_strings
        .iter()
        .map(|value| value.len())
        .max()
        .unwrap_or("LABELS".len())
        .clamp(LABELS_MIN_WIDTH, LABELS_MAX_WIDTH);

    let width = terminal_width().max(72);
    let fixed_width = id_width + updated_width + 6;
    let flex_width = width.saturating_sub(fixed_width);
    let (title_width, labels_width) = split_flexible_width(
        flex_width,
        TITLE_MIN_WIDTH,
        label_pref,
        LABELS_MIN_WIDTH,
        LABELS_MAX_WIDTH,
    );

    let columns = vec![
        TableColumn {
            header: "ID",
            width: id_width,
        },
        TableColumn {
            header: "TITLE",
            width: title_width,
        },
        TableColumn {
            header: "LABELS",
            width: labels_width,
        },
        TableColumn {
            header: "UPDATED",
            width: updated_width,
        },
    ];

    let rows = issues
        .iter()
        .zip(label_strings.iter())
        .zip(updated_strings.iter())
        .map(|((issue, labels), updated)| {
            vec![
                TableCell {
                    text: format!("#{}", issue.number),
                    tone: CellTone::Id,
                    truncation: TruncationMode::None,
                },
                TableCell {
                    text: issue.title.clone(),
                    tone: CellTone::Default,
                    truncation: TruncationMode::End,
                },
                TableCell {
                    text: labels.clone(),
                    tone: if issue.labels.is_empty() {
                        CellTone::Secondary
                    } else {
                        CellTone::Label
                    },
                    truncation: TruncationMode::End,
                },
                TableCell {
                    text: updated.clone(),
                    tone: CellTone::Secondary,
                    truncation: TruncationMode::None,
                },
            ]
        })
        .collect::<Vec<_>>();

    print_table(
        repo_label,
        &format!("{} open issues", issues.len()),
        "No open issues.",
        &columns,
        &rows,
    );
}
