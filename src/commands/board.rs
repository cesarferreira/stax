use crate::config::Config;
use crate::forge::{ForgeClient, forge_token};
use crate::git::GitRepo;
use crate::remote::{ForgeType, RemoteInfo};
use anyhow::Result;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardTabSelection {
    PullRequests,
    Issues,
}

#[derive(Clone)]
pub struct BoardScope {
    pub git_dir: PathBuf,
    pub remote: RemoteInfo,
    pub config: Config,
    pub repo_label: String,
    pub limit: u8,
}

pub fn load_board_scope(limit: u8) -> Result<BoardScope> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let remote = RemoteInfo::from_repo(&repo, &config)?;

    if remote.forge != ForgeType::GitHub {
        anyhow::bail!(
            "`stax board` supports GitHub only; use `stax pr list` / `stax issue list` on {}.",
            remote.forge
        );
    }

    if forge_token(remote.forge).is_none() {
        anyhow::bail!(
            "{} auth not configured; the board dashboard cannot be fetched.",
            remote.forge
        );
    }

    let repo_label = format!("{}/{}", remote.namespace, remote.repo);
    let git_dir = repo.git_dir()?.to_path_buf();

    Ok(BoardScope {
        git_dir,
        remote,
        config,
        repo_label,
        limit,
    })
}

pub fn run_tui(limit: u8, tab: BoardTabSelection, interval: u64) -> Result<()> {
    let scope = load_board_scope(limit)?;
    let mine_only = scope.config.board.mine_only;
    crate::tui::board::run(scope, tab, interval, mine_only)
}

pub fn run_plain(limit: u8) -> Result<()> {
    // Reuses `load_board_scope`'s GitHub-only + auth gate rather than
    // re-deriving `RemoteInfo` here, so plain and interactive mode enforce
    // the same preconditions. `ForgeClient` (not `GitHubClient`) is used for
    // the actual listing since `print_pr_table`/`print_issue_table` already
    // work off the forge-neutral `RepoPrListItem`/`RepoIssueListItem` types.
    let scope = load_board_scope(limit)?;

    let rt = tokio::runtime::Runtime::new()?;
    let _enter = rt.enter();
    let client = ForgeClient::new(&scope.remote)?;
    let (prs, issues) = rt.block_on(async {
        tokio::try_join!(
            client.list_open_pull_requests(scope.limit),
            client.list_open_issues(scope.limit)
        )
    })?;

    crate::commands::pr::print_pr_table(&scope.repo_label, &prs);
    crate::commands::issue::print_issue_table(&scope.repo_label, &issues);
    Ok(())
}
