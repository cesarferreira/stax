pub mod app;
pub mod ui;

use anyhow::Result;
use app::{BoardApp, BoardMode, BoardTab, BoardTarget, BoardUpdate};
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use crate::ci::CheckRunInfo;
use crate::commands::board::{BoardScope, BoardTabSelection};
use crate::commands::open::open_url_in_browser;
use crate::config::Config;
use crate::forge::{MergeMethod, PrComment};
use crate::git::GitRepo;
use crate::github::client::GitHubClient;

const PAGE_STEP: usize = 10;

pub fn run(
    scope: BoardScope,
    tab: BoardTabSelection,
    interval: u64,
    mine_only: bool,
) -> Result<()> {
    let poll_interval = Duration::from_secs(interval.max(1));
    let mut app = BoardApp::new(scope.repo_label.clone(), tab, mine_only);
    let (req_tx, rx) = spawn_worker(scope);
    let _ = req_tx.send(BoardRequest::ListPrs);
    let _ = req_tx.send(BoardRequest::ListIssues);
    let _ = req_tx.send(BoardRequest::ViewerLogin);
    let mut last_refresh = Instant::now();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(
        &mut terminal,
        &mut app,
        &req_tx,
        &rx,
        poll_interval,
        &mut last_refresh,
    );

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

enum BoardRequest {
    ListPrs,
    ListIssues,
    PrDetail(u64),
    IssueDetail(u64),
    Comments(BoardTarget),
    Diff(u64),
    RepoLabels,
    ViewerLogin,
    AddLabel { target: BoardTarget, label: String },
    RemoveLabel { target: BoardTarget, label: String },
    ToggleDraft { number: u64, to_draft: bool },
    Merge { number: u64, sha: String },
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut BoardApp,
    req_tx: &Sender<BoardRequest>,
    rx: &Receiver<BoardUpdate>,
    poll_interval: Duration,
    last_refresh: &mut Instant,
) -> Result<()> {
    loop {
        poll_updates(app, rx, req_tx);
        maybe_auto_refresh(app, req_tx, poll_interval, last_refresh);
        sync_selected_detail(app, req_tx);
        terminal.draw(|f| ui::render(f, app))?;

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            handle_key(app, key.code, key.modifiers, req_tx);
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn poll_updates(app: &mut BoardApp, rx: &Receiver<BoardUpdate>, req_tx: &Sender<BoardRequest>) {
    loop {
        match rx.try_recv() {
            Ok(update) => {
                let refresh_after = matches!(update, BoardUpdate::ActionDone { refresh: true, .. });
                app.apply_update(update);
                if refresh_after {
                    let _ = req_tx.send(BoardRequest::ListPrs);
                    let _ = req_tx.send(BoardRequest::ListIssues);
                }
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => break,
        }
    }
}

fn maybe_auto_refresh(
    app: &mut BoardApp,
    req_tx: &Sender<BoardRequest>,
    poll_interval: Duration,
    last_refresh: &mut Instant,
) {
    if app.loading_list || last_refresh.elapsed() < poll_interval {
        return;
    }
    app.loading_list = true;
    let _ = req_tx.send(BoardRequest::ListPrs);
    let _ = req_tx.send(BoardRequest::ListIssues);
    *last_refresh = Instant::now();
}

fn handle_key(
    app: &mut BoardApp,
    code: KeyCode,
    modifiers: KeyModifiers,
    req_tx: &Sender<BoardRequest>,
) {
    match app.mode {
        BoardMode::List => handle_list_key(app, code, modifiers, req_tx),
        BoardMode::Detail => handle_detail_key(app, code, req_tx),
        BoardMode::Diff | BoardMode::Comments => handle_scroll_key(app, code),
        BoardMode::Filter => handle_filter_key(app, code),
        BoardMode::LabelPicker => handle_label_key(app, code, req_tx),
        BoardMode::ConfirmMerge => handle_confirm_merge_key(app, code, req_tx),
        BoardMode::Help => handle_help_key(app, code),
    }
}

fn handle_list_key(
    app: &mut BoardApp,
    code: KeyCode,
    modifiers: KeyModifiers,
    req_tx: &Sender<BoardRequest>,
) {
    match code {
        KeyCode::Tab => {
            let next = match app.tab {
                BoardTab::PullRequests => BoardTab::Issues,
                BoardTab::Issues => BoardTab::PullRequests,
            };
            app.switch_tab(next);
        }
        KeyCode::Char('1') => app.switch_tab(BoardTab::PullRequests),
        KeyCode::Char('2') => app.switch_tab(BoardTab::Issues),
        KeyCode::Char('j') | KeyCode::Down => app.select_next(),
        KeyCode::Char('k') | KeyCode::Up => app.select_previous(),
        KeyCode::Char('g') => app.select_first(),
        KeyCode::Char('G') => app.select_last(),
        KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => page(app, true),
        KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => page(app, false),
        KeyCode::Enter => open_detail(app, req_tx),
        KeyCode::Char('d') => {
            request_diff(app, req_tx);
            if app.tab == BoardTab::PullRequests {
                app.mode = BoardMode::Diff;
            }
        }
        KeyCode::Char('c') => {
            request_comments(app, req_tx);
            app.mode = BoardMode::Comments;
        }
        KeyCode::Char('l') => open_label_picker(app, req_tx),
        KeyCode::Char('t') => toggle_draft(app, req_tx),
        KeyCode::Char('m') => {
            if app.merge_target().is_some() {
                app.mode = BoardMode::ConfirmMerge;
            } else {
                app.status_message = Some("Open the PR detail before merging".to_string());
            }
        }
        KeyCode::Char('o') => open_selected_in_browser(app),
        KeyCode::Char('r') => request_full_refresh(app, req_tx),
        KeyCode::Char('/') => app.mode = BoardMode::Filter,
        KeyCode::Char('v') => app.toggle_show_detail(),
        KeyCode::Char('a') => toggle_mine_only(app),
        KeyCode::Char('?') => app.mode = BoardMode::Help,
        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
        _ => {}
    }
}

fn handle_detail_key(app: &mut BoardApp, code: KeyCode, req_tx: &Sender<BoardRequest>) {
    match code {
        KeyCode::Char('j') | KeyCode::Down => {
            app.detail_scroll = app.detail_scroll.saturating_add(1)
        }
        KeyCode::Char('k') | KeyCode::Up => app.detail_scroll = app.detail_scroll.saturating_sub(1),
        KeyCode::Char('d') => {
            request_diff(app, req_tx);
            if app.tab == BoardTab::PullRequests {
                app.mode = BoardMode::Diff;
            }
        }
        KeyCode::Char('c') => {
            request_comments(app, req_tx);
            app.mode = BoardMode::Comments;
        }
        KeyCode::Char('l') => open_label_picker(app, req_tx),
        KeyCode::Char('t') => toggle_draft(app, req_tx),
        KeyCode::Char('m') => {
            if app.merge_target().is_some() {
                app.mode = BoardMode::ConfirmMerge;
            } else {
                app.status_message = Some("PR detail is still loading".to_string());
            }
        }
        KeyCode::Char('o') => open_selected_in_browser(app),
        KeyCode::Char('r') => request_detail_refresh(app, req_tx),
        KeyCode::Char('v') => app.toggle_show_detail(),
        KeyCode::Char('?') => app.mode = BoardMode::Help,
        KeyCode::Char('q') | KeyCode::Esc => {
            app.mode = BoardMode::List;
            app.detail_scroll = 0;
        }
        _ => {}
    }
}

fn handle_scroll_key(app: &mut BoardApp, code: KeyCode) {
    let max = match app.mode {
        BoardMode::Diff => match app.selected_target() {
            Some(BoardTarget::Pr(number)) => app.diff_lines.get(&number).map(Vec::len).unwrap_or(0),
            _ => 0,
        },
        BoardMode::Comments => app
            .selected_target()
            .and_then(|target| app.comments.get(&target))
            .map(Vec::len)
            .unwrap_or(0),
        _ => 0,
    };

    match code {
        KeyCode::Char('j') | KeyCode::Down => {
            let scroll = scroll_field(app);
            if (*scroll as usize) + 1 < max.max(1) {
                *scroll += 1;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            let scroll = scroll_field(app);
            *scroll = scroll.saturating_sub(1);
        }
        KeyCode::Char('g') => *scroll_field(app) = 0,
        KeyCode::Char('G') => *scroll_field(app) = max.saturating_sub(1) as u16,
        KeyCode::Esc | KeyCode::Char('q') => app.mode = return_mode_after_overlay(app),
        _ => {}
    }
}

fn scroll_field(app: &mut BoardApp) -> &mut u16 {
    match app.mode {
        BoardMode::Diff => &mut app.diff_scroll,
        BoardMode::Comments => &mut app.comments_scroll,
        _ => &mut app.detail_scroll,
    }
}

fn handle_filter_key(app: &mut BoardApp, code: KeyCode) {
    match code {
        KeyCode::Enter => app.mode = BoardMode::List,
        KeyCode::Esc => {
            app.filter.clear();
            app.mode = BoardMode::List;
        }
        KeyCode::Backspace => {
            app.filter.pop();
        }
        KeyCode::Char(ch) => app.filter.push(ch),
        _ => {}
    }
}

fn handle_label_key(app: &mut BoardApp, code: KeyCode, req_tx: &Sender<BoardRequest>) {
    match code {
        KeyCode::Char('j') | KeyCode::Down => {
            if app.label_cursor + 1 < app.repo_labels.len() {
                app.label_cursor += 1;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.label_cursor = app.label_cursor.saturating_sub(1);
        }
        KeyCode::Char(' ') => toggle_label(app, req_tx),
        KeyCode::Esc | KeyCode::Char('q') => {
            app.mode = return_mode_after_overlay(app);
        }
        _ => {}
    }
}

fn toggle_label(app: &mut BoardApp, req_tx: &Sender<BoardRequest>) {
    let Some(target) = app.selected_target() else {
        return;
    };
    let Some(label) = app.repo_labels.get(app.label_cursor).cloned() else {
        return;
    };

    if app.active_labels.contains(&label) {
        let _ = req_tx.send(BoardRequest::RemoveLabel { target, label });
    } else {
        let _ = req_tx.send(BoardRequest::AddLabel { target, label });
    }
}

fn handle_confirm_merge_key(app: &mut BoardApp, code: KeyCode, req_tx: &Sender<BoardRequest>) {
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            if let Some((number, sha)) = app.merge_target() {
                app.status_message = Some(format!("Merging PR #{number} (squash)..."));
                let _ = req_tx.send(BoardRequest::Merge { number, sha });
            }
            app.mode = return_mode_after_overlay(app);
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.mode = return_mode_after_overlay(app);
        }
        _ => {}
    }
}

fn handle_help_key(app: &mut BoardApp, _code: KeyCode) {
    app.mode = return_mode_after_overlay(app);
}

fn return_mode_after_overlay(app: &BoardApp) -> BoardMode {
    let has_detail = match app.selected_target() {
        Some(BoardTarget::Pr(number)) => app.pr_details.contains_key(&number),
        Some(BoardTarget::Issue(number)) => app.issue_details.contains_key(&number),
        None => false,
    };
    if has_detail {
        BoardMode::Detail
    } else {
        BoardMode::List
    }
}

fn page(app: &mut BoardApp, down: bool) {
    match app.tab {
        BoardTab::PullRequests => {
            let len = app.visible_pr_indices().len();
            if len == 0 {
                return;
            }
            app.pr_selected = if down {
                (app.pr_selected + PAGE_STEP).min(len - 1)
            } else {
                app.pr_selected.saturating_sub(PAGE_STEP)
            };
        }
        BoardTab::Issues => {
            let len = app.visible_issue_indices().len();
            if len == 0 {
                return;
            }
            app.issue_selected = if down {
                (app.issue_selected + PAGE_STEP).min(len - 1)
            } else {
                app.issue_selected.saturating_sub(PAGE_STEP)
            };
        }
    }
}

fn toggle_mine_only(app: &mut BoardApp) {
    app.toggle_mine_only();
    if let Err(error) = Config::set_board_mine_only(app.mine_only) {
        app.status_message = Some(format!("Failed to save preference: {error}"));
    }
}

fn open_detail(app: &mut BoardApp, req_tx: &Sender<BoardRequest>) {
    let Some(target) = app.selected_target() else {
        return;
    };
    // Enter is an explicit request to view detail, so it always works even
    // if the ambient preview panel was hidden via `v`.
    app.show_detail = true;
    app.mode = BoardMode::Detail;
    request_detail_if_needed(app, target, req_tx);
}

/// The detail pane is always visible alongside the list (at normal terminal
/// widths) as a live preview of the current selection, not something gated
/// behind pressing Enter — so it must keep the fetch for the current
/// selection in sync every tick, not just when `open_detail` runs. Without
/// this, the pane renders "Loading #N…" for whatever is selected even though
/// no request was ever sent, which looks identical to a genuine hang.
/// Skipped entirely while the panel is hidden (`v`), so hiding it also stops
/// the eager prefetching, not just the rendering.
fn sync_selected_detail(app: &mut BoardApp, req_tx: &Sender<BoardRequest>) {
    if !app.show_detail {
        return;
    }
    let Some(target) = app.selected_target() else {
        return;
    };
    request_detail_if_needed(app, target, req_tx);
}

fn request_detail_if_needed(
    app: &mut BoardApp,
    target: BoardTarget,
    req_tx: &Sender<BoardRequest>,
) {
    let already_loaded = match target {
        BoardTarget::Pr(number) => app.pr_details.contains_key(&number),
        BoardTarget::Issue(number) => app.issue_details.contains_key(&number),
    };
    if already_loaded || app.loading_detail == Some(target) {
        return;
    }
    app.loading_detail = Some(target);
    app.detail_error.remove(&target);
    let _ = req_tx.send(match target {
        BoardTarget::Pr(number) => BoardRequest::PrDetail(number),
        BoardTarget::Issue(number) => BoardRequest::IssueDetail(number),
    });
}

fn request_detail_refresh(app: &mut BoardApp, req_tx: &Sender<BoardRequest>) {
    let Some(target) = app.selected_target() else {
        return;
    };
    app.loading_detail = Some(target);
    app.detail_error.remove(&target);
    let _ = req_tx.send(match target {
        BoardTarget::Pr(number) => BoardRequest::PrDetail(number),
        BoardTarget::Issue(number) => BoardRequest::IssueDetail(number),
    });
}

fn request_diff(app: &mut BoardApp, req_tx: &Sender<BoardRequest>) {
    let Some(BoardTarget::Pr(number)) = app.selected_target() else {
        return;
    };
    let cached_for_selection = app.diff_lines.contains_key(&number);
    if cached_for_selection || app.loading_diff == Some(number) {
        return;
    }
    app.loading_diff = Some(number);
    let _ = req_tx.send(BoardRequest::Diff(number));
}

fn request_comments(app: &mut BoardApp, req_tx: &Sender<BoardRequest>) {
    let Some(target) = app.selected_target() else {
        return;
    };
    let cached_for_selection = app.comments.contains_key(&target);
    if cached_for_selection || app.loading_comments == Some(target) {
        return;
    }
    app.loading_comments = Some(target);
    let _ = req_tx.send(BoardRequest::Comments(target));
}

fn open_label_picker(app: &mut BoardApp, req_tx: &Sender<BoardRequest>) {
    if app.selected_target().is_none() {
        return;
    }
    app.active_labels = match app.tab {
        BoardTab::PullRequests => app
            .selected_pr()
            .map(|pr| pr.labels.clone())
            .unwrap_or_default(),
        BoardTab::Issues => app
            .selected_issue()
            .map(|issue| issue.labels.clone())
            .unwrap_or_default(),
    };
    app.label_cursor = 0;
    app.mode = BoardMode::LabelPicker;
    if app.repo_labels.is_empty() {
        let _ = req_tx.send(BoardRequest::RepoLabels);
    }
}

fn toggle_draft(app: &mut BoardApp, req_tx: &Sender<BoardRequest>) {
    if app.tab != BoardTab::PullRequests {
        return;
    }
    let Some(pr) = app.selected_pr() else {
        return;
    };
    let number = pr.number;
    let to_draft = !pr.is_draft;
    app.status_message = Some(format!(
        "{} PR #{number}...",
        if to_draft {
            "Converting to draft"
        } else {
            "Marking ready for review"
        }
    ));
    let _ = req_tx.send(BoardRequest::ToggleDraft { number, to_draft });
}

fn open_selected_in_browser(app: &mut BoardApp) {
    if let Some(url) = app.selected_url() {
        open_url_in_browser(&url);
        app.status_message = Some(format!("Opened {url}"));
    } else {
        app.status_message = Some("Nothing selected to open".to_string());
    }
}

fn request_full_refresh(app: &mut BoardApp, req_tx: &Sender<BoardRequest>) {
    app.loading_list = true;
    let _ = req_tx.send(BoardRequest::ListPrs);
    let _ = req_tx.send(BoardRequest::ListIssues);
}

fn spawn_worker(scope: BoardScope) -> (Sender<BoardRequest>, Receiver<BoardUpdate>) {
    let (req_tx, req_rx) = mpsc::channel::<BoardRequest>();
    let (update_tx, update_rx) = mpsc::channel::<BoardUpdate>();

    thread::spawn(move || {
        let runtime = match tokio::runtime::Runtime::new() {
            Ok(runtime) => runtime,
            Err(error) => {
                let _ = update_tx.send(BoardUpdate::Error(format!(
                    "Failed to create async runtime: {error}"
                )));
                return;
            }
        };
        let client = match create_board_client(&runtime, &scope) {
            Ok(client) => client,
            Err(error) => {
                let _ = update_tx.send(BoardUpdate::Error(format!(
                    "Failed to create GitHub client: {error}"
                )));
                return;
            }
        };

        while let Ok(request) = req_rx.recv() {
            runtime.block_on(handle_request(&client, &scope, request, &update_tx));
        }
    });

    (req_tx, update_rx)
}

fn create_board_client(
    runtime: &tokio::runtime::Runtime,
    scope: &BoardScope,
) -> Result<GitHubClient> {
    let _enter = runtime.enter();
    GitHubClient::new_for_trusted_remote(
        scope.remote.owner(),
        &scope.remote.repo,
        scope.remote.api_base_url.clone(),
        &scope.config,
        &scope.remote.host,
    )
}

async fn handle_request(
    client: &GitHubClient,
    scope: &BoardScope,
    request: BoardRequest,
    tx: &Sender<BoardUpdate>,
) {
    match request {
        BoardRequest::ListPrs => match client.board_list_open_prs(scope.limit).await {
            Ok(prs) => {
                let _ = tx.send(BoardUpdate::Prs(prs));
            }
            Err(error) => {
                let _ = tx.send(BoardUpdate::Error(format!(
                    "Failed to list pull requests: {error}"
                )));
            }
        },
        BoardRequest::ListIssues => match client.board_list_open_issues(scope.limit).await {
            Ok(issues) => {
                let _ = tx.send(BoardUpdate::Issues(issues));
            }
            Err(error) => {
                let _ = tx.send(BoardUpdate::Error(format!(
                    "Failed to list issues: {error}"
                )));
            }
        },
        BoardRequest::PrDetail(number) => {
            let (detail_result, files_result) = tokio::join!(
                client.board_get_pr_detail(number),
                client.board_list_pr_files(number)
            );
            match (detail_result, files_result) {
                (Ok(detail), Ok(files)) => {
                    let checks = fetch_pr_checks(client, scope, &detail.head_sha).await;
                    let _ = tx.send(BoardUpdate::PrDetail {
                        number,
                        detail: Box::new(detail),
                        files,
                        checks,
                    });
                }
                (Err(error), _) | (_, Err(error)) => {
                    let _ = tx.send(BoardUpdate::DetailError {
                        target: BoardTarget::Pr(number),
                        message: format!("Failed to load PR #{number}: {error}"),
                    });
                }
            }
        }
        BoardRequest::IssueDetail(number) => match client.board_get_issue_detail(number).await {
            Ok(detail) => {
                let _ = tx.send(BoardUpdate::IssueDetail {
                    number,
                    detail: Box::new(detail),
                });
            }
            Err(error) => {
                let _ = tx.send(BoardUpdate::DetailError {
                    target: BoardTarget::Issue(number),
                    message: format!("Failed to load issue #{number}: {error}"),
                });
            }
        },
        BoardRequest::Comments(target) => {
            let result = match target {
                BoardTarget::Pr(number) => client.list_all_comments(number).await,
                BoardTarget::Issue(number) => client
                    .board_list_issue_comments(number)
                    .await
                    .map(|comments| comments.into_iter().map(PrComment::Issue).collect()),
            };
            match result {
                Ok(comments) => {
                    let _ = tx.send(BoardUpdate::Comments { target, comments });
                }
                Err(error) => {
                    let _ = tx.send(BoardUpdate::Error(format!(
                        "Failed to load comments: {error}"
                    )));
                }
            }
        }
        BoardRequest::Diff(number) => match client.board_get_pr_diff(number).await {
            Ok(diff) => {
                let _ = tx.send(BoardUpdate::Diff { number, diff });
            }
            Err(error) => {
                let _ = tx.send(BoardUpdate::Error(format!("Failed to load diff: {error}")));
            }
        },
        BoardRequest::RepoLabels => match client.board_list_repo_labels().await {
            Ok(labels) => {
                let _ = tx.send(BoardUpdate::RepoLabels(labels));
            }
            Err(error) => {
                let _ = tx.send(BoardUpdate::Error(format!(
                    "Failed to load labels: {error}"
                )));
            }
        },
        BoardRequest::ViewerLogin => match client.get_current_user().await {
            Ok(login) => {
                let _ = tx.send(BoardUpdate::ViewerLogin(login));
            }
            Err(error) => {
                // "Mine only" fails open (shows everything) without a
                // viewer login, so this is worth surfacing but not fatal.
                let _ = tx.send(BoardUpdate::Error(format!(
                    "Couldn't determine your GitHub username, \"mine only\" won't filter: {error}"
                )));
            }
        },
        BoardRequest::AddLabel { target, label } => {
            let number = target_number(target);
            match client
                .add_labels(number, std::slice::from_ref(&label))
                .await
            {
                Ok(()) => {
                    let _ = tx.send(BoardUpdate::LabelMutation {
                        target,
                        label: label.clone(),
                        added: true,
                        message: format!("Added label \"{label}\""),
                    });
                }
                Err(error) => {
                    let _ = tx.send(BoardUpdate::Error(format!("Failed to add label: {error}")));
                }
            }
        }
        BoardRequest::RemoveLabel { target, label } => {
            let number = target_number(target);
            match client.remove_label(number, &label).await {
                Ok(()) => {
                    let _ = tx.send(BoardUpdate::LabelMutation {
                        target,
                        label: label.clone(),
                        added: false,
                        message: format!("Removed label \"{label}\""),
                    });
                }
                Err(error) => {
                    let _ = tx.send(BoardUpdate::Error(format!(
                        "Failed to remove label: {error}"
                    )));
                }
            }
        }
        BoardRequest::ToggleDraft { number, to_draft } => {
            match client.set_pr_draft(number, to_draft).await {
                Ok(()) => {
                    let message = format!(
                        "PR #{number} marked as {}",
                        if to_draft {
                            "draft"
                        } else {
                            "ready for review"
                        }
                    );
                    let _ = tx.send(BoardUpdate::ActionDone {
                        message,
                        refresh: true,
                    });
                }
                Err(error) => {
                    let _ = tx.send(BoardUpdate::Error(format!(
                        "Failed to update PR #{number}: {error}"
                    )));
                }
            }
        }
        BoardRequest::Merge { number, sha } => {
            match client
                .merge_pr(number, MergeMethod::Squash, None, None, Some(sha))
                .await
            {
                Ok(()) => {
                    let message = format!(
                        "Merged PR #{number} (squash). Run `stax sync` to update local branches."
                    );
                    let _ = tx.send(BoardUpdate::ActionDone {
                        message,
                        refresh: true,
                    });
                }
                Err(error) => {
                    let _ = tx.send(BoardUpdate::Error(format!(
                        "Failed to merge PR #{number}: {error}"
                    )));
                }
            }
        }
    }
}

fn target_number(target: BoardTarget) -> u64 {
    match target {
        BoardTarget::Pr(number) | BoardTarget::Issue(number) => number,
    }
}

async fn fetch_pr_checks(
    client: &GitHubClient,
    scope: &BoardScope,
    sha: &str,
) -> Vec<CheckRunInfo> {
    let Ok(repo) = GitRepo::open_from_path(&scope.git_dir) else {
        return Vec::new();
    };
    let checks = client
        .fetch_checks(&repo, sha)
        .await
        .map(|(_, checks)| checks)
        .unwrap_or_default();
    drop(repo);
    checks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::github::board::BoardPrSummary;
    use crate::remote::{ForgeType, RemoteInfo};
    use chrono::Utc;
    use std::env;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn pr(number: u64, labels: &[&str]) -> BoardPrSummary {
        BoardPrSummary {
            number,
            title: "title".to_string(),
            author: "octocat".to_string(),
            head_branch: "feature".to_string(),
            base_branch: "main".to_string(),
            is_draft: false,
            labels: labels.iter().map(|label| label.to_string()).collect(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            url: format!("https://github.com/o/r/pull/{number}"),
        }
    }

    #[test]
    fn label_picker_waits_for_success_before_changing_active_labels() {
        let mut app = BoardApp::new("o/r".to_string(), BoardTabSelection::PullRequests, false);
        app.prs = vec![pr(1, &["bug"])];
        app.active_labels = vec!["bug".to_string()];
        app.repo_labels = vec!["bug".to_string()];
        let (tx, rx) = mpsc::channel();

        toggle_label(&mut app, &tx);

        assert!(matches!(
            rx.recv().expect("label request"),
            BoardRequest::RemoveLabel { target: BoardTarget::Pr(1), label } if label == "bug"
        ));
        assert_eq!(app.active_labels, vec!["bug"]);
    }

    #[test]
    fn board_worker_constructs_github_client_with_entered_runtime() {
        let _guard = ENV_LOCK.lock().unwrap();
        let original_token = env::var("STAX_GITHUB_TOKEN").ok();
        unsafe { env::set_var("STAX_GITHUB_TOKEN", "test-token") };

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let scope = BoardScope {
            git_dir: std::path::PathBuf::from("."),
            remote: RemoteInfo {
                name: "origin".to_string(),
                forge: ForgeType::GitHub,
                host: "github.com".to_string(),
                namespace: "owner".to_string(),
                repo: "repo".to_string(),
                base_url: "https://github.com".to_string(),
                api_base_url: Some("https://api.github.com".to_string()),
            },
            config: Config::default(),
            repo_label: "owner/repo".to_string(),
            limit: 30,
        };

        let result = create_board_client(&runtime, &scope);

        match original_token {
            Some(token) => unsafe { env::set_var("STAX_GITHUB_TOKEN", token) },
            None => unsafe { env::remove_var("STAX_GITHUB_TOKEN") },
        }
        assert!(result.is_ok());
    }
}
