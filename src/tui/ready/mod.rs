pub mod app;
pub mod ui;

use anyhow::Result;
use app::{ReadyTuiApp, ReadyTuiUpdate};
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use futures_util::{StreamExt, stream};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use crate::commands::open::open_url_in_browser;
use crate::commands::ready::{ReadyScope, ReadyScopeMode, fetch_row_for_branch, load_ready_scope};
use crate::engine::Stack;
use crate::forge::ForgeClient;
use crate::git::GitRepo;
use crate::remote::RemoteInfo;

pub fn run(scope_mode: ReadyScopeMode, interval: u64) -> Result<()> {
    let poll_interval = Duration::from_secs(interval.max(1));
    let mut scope = load_ready_scope(scope_mode)?;
    let mut app = ReadyTuiApp::from_parts(
        scope.repo_label.clone(),
        scope.scope_label.clone(),
        scope.branches.clone(),
    );
    let mut loader = Some(spawn_loader(scope.clone()));
    let mut last_refresh = Instant::now();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(
        &mut terminal,
        &mut app,
        scope_mode,
        &mut scope,
        &mut loader,
        poll_interval,
        &mut last_refresh,
    );

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

#[allow(clippy::too_many_arguments)]
fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut ReadyTuiApp,
    scope_mode: ReadyScopeMode,
    scope: &mut ReadyScope,
    loader: &mut Option<Receiver<ReadyTuiUpdate>>,
    poll_interval: Duration,
    last_refresh: &mut Instant,
) -> Result<()> {
    let mut draft_toggle: Option<Receiver<DraftToggleOutcome>> = None;

    loop {
        poll_loader(app, loader);
        poll_draft_toggle(
            app,
            &mut draft_toggle,
            scope_mode,
            scope,
            loader,
            last_refresh,
        );
        maybe_auto_refresh(app, scope_mode, scope, loader, poll_interval, last_refresh);
        terminal.draw(|f| ui::render(f, app))?;

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            handle_key(
                app,
                key.code,
                scope_mode,
                scope,
                loader,
                last_refresh,
                &mut draft_toggle,
            );
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn refresh(
    app: &mut ReadyTuiApp,
    scope_mode: ReadyScopeMode,
    scope: &mut ReadyScope,
    loader: &mut Option<Receiver<ReadyTuiUpdate>>,
    last_refresh: &mut Instant,
) {
    match load_ready_scope(scope_mode) {
        Ok(new_scope) => {
            app.reconcile_scope(&new_scope.branches);
            app.repo_label = new_scope.repo_label.clone();
            app.scope_label = new_scope.scope_label.clone();
            *scope = new_scope;
            app.begin_refresh();
            *loader = Some(spawn_loader(scope.clone()));
        }
        Err(error) => {
            app.status_message = Some(format!("Refresh failed: {error}"));
            app.begin_refresh();
            *loader = Some(spawn_loader(scope.clone()));
        }
    }
    *last_refresh = Instant::now();
}

fn maybe_auto_refresh(
    app: &mut ReadyTuiApp,
    scope_mode: ReadyScopeMode,
    scope: &mut ReadyScope,
    loader: &mut Option<Receiver<ReadyTuiUpdate>>,
    poll_interval: Duration,
    last_refresh: &mut Instant,
) {
    if loader.is_some() || app.loading {
        return;
    }
    if last_refresh.elapsed() < poll_interval {
        return;
    }

    refresh(app, scope_mode, scope, loader, last_refresh);
}

#[allow(clippy::too_many_arguments)]
fn handle_key(
    app: &mut ReadyTuiApp,
    code: KeyCode,
    scope_mode: ReadyScopeMode,
    scope: &mut ReadyScope,
    loader: &mut Option<Receiver<ReadyTuiUpdate>>,
    last_refresh: &mut Instant,
    draft_toggle: &mut Option<Receiver<DraftToggleOutcome>>,
) {
    if app.show_help {
        app.show_help = false;
        return;
    }

    match code {
        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
        KeyCode::Up | KeyCode::Char('k') => app.select_previous(),
        KeyCode::Down | KeyCode::Char('j') => app.select_next(),
        KeyCode::Char('?') => app.show_help = true,
        KeyCode::Char('r') => {
            refresh(app, scope_mode, scope, loader, last_refresh);
        }
        KeyCode::Enter | KeyCode::Char('o') => {
            if let Some(url) = app.selected_pr_url() {
                open_url_in_browser(&url);
                app.status_message = Some(format!("Opened {url}"));
            } else {
                app.status_message = Some("Selected row has no loaded PR URL yet".to_string());
            }
        }
        KeyCode::Char('d') => {
            if draft_toggle.is_some() {
                app.status_message = Some("Draft toggle already in progress".to_string());
            } else if let Some((pr_number, branch, is_draft)) = app.selected_draft_target() {
                let target_draft = !is_draft;
                app.status_message = Some(format!(
                    "{} PR #{}...",
                    if target_draft {
                        "Converting to draft"
                    } else {
                        "Marking ready for review"
                    },
                    pr_number
                ));
                *draft_toggle = Some(spawn_draft_toggle(
                    scope.clone(),
                    pr_number,
                    branch,
                    target_draft,
                ));
            } else {
                app.status_message = Some("Selected row has no loaded PR yet".to_string());
            }
        }
        _ => {}
    }
}

fn poll_draft_toggle(
    app: &mut ReadyTuiApp,
    draft_toggle: &mut Option<Receiver<DraftToggleOutcome>>,
    scope_mode: ReadyScopeMode,
    scope: &mut ReadyScope,
    loader: &mut Option<Receiver<ReadyTuiUpdate>>,
    last_refresh: &mut Instant,
) {
    let Some(receiver) = draft_toggle.as_ref() else {
        return;
    };

    match receiver.try_recv() {
        Ok(outcome) => {
            *draft_toggle = None;
            match outcome.error {
                Some(error) => {
                    app.status_message = Some(format!(
                        "Failed to update PR #{}: {error}",
                        outcome.pr_number
                    ));
                }
                None => {
                    app.status_message = Some(format!(
                        "PR #{} marked as {}",
                        outcome.pr_number,
                        if outcome.target_draft {
                            "draft"
                        } else {
                            "ready for review"
                        }
                    ));
                    refresh(app, scope_mode, scope, loader, last_refresh);
                }
            }
        }
        Err(TryRecvError::Empty) => {}
        Err(TryRecvError::Disconnected) => *draft_toggle = None,
    }
}

fn poll_loader(app: &mut ReadyTuiApp, loader: &mut Option<Receiver<ReadyTuiUpdate>>) {
    loop {
        let update = match loader.as_ref() {
            Some(receiver) => match receiver.try_recv() {
                Ok(update) => Some(update),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    *loader = None;
                    app.loading = false;
                    app.status_message = Some("Readiness loader disconnected".to_string());
                    None
                }
            },
            None => None,
        };

        let Some(update) = update else {
            break;
        };

        let done = matches!(update, ReadyTuiUpdate::Done);
        app.apply_update(update);
        if done {
            *loader = None;
            app.status_message = None;
        }
    }
}

fn spawn_loader(scope: ReadyScope) -> Receiver<ReadyTuiUpdate> {
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let repo = match GitRepo::open_from_path(&scope.git_dir) {
            Ok(repo) => repo,
            Err(error) => {
                send_all_unavailable(&sender, &scope, format!("Failed to open repo: {error}"));
                let _ = sender.send(ReadyTuiUpdate::Done);
                return;
            }
        };
        let stack = match Stack::load(&repo) {
            Ok(stack) => stack,
            Err(error) => {
                send_all_unavailable(&sender, &scope, format!("Failed to load stack: {error}"));
                let _ = sender.send(ReadyTuiUpdate::Done);
                return;
            }
        };
        let runtime = match tokio::runtime::Runtime::new() {
            Ok(runtime) => runtime,
            Err(error) => {
                send_all_unavailable(
                    &sender,
                    &scope,
                    format!("Failed to create runtime: {error}"),
                );
                let _ = sender.send(ReadyTuiUpdate::Done);
                return;
            }
        };
        let client = match create_loader_forge_client(&runtime, &scope.remote) {
            Ok(client) => client,
            Err(error) => {
                send_all_unavailable(
                    &sender,
                    &scope,
                    format!("Failed to create forge client: {error}"),
                );
                let _ = sender.send(ReadyTuiUpdate::Done);
                return;
            }
        };

        runtime.block_on(async {
            let mut pending =
                stream::iter(scope.branches.iter().enumerate().map(|(index, branch)| {
                    let repo = &repo;
                    let client = &client;
                    let remote = &scope.remote;
                    let stack = &stack;
                    let branch = branch.clone();
                    async move {
                        (
                            index,
                            branch.clone(),
                            fetch_row_for_branch(repo, client, remote, stack, &branch).await,
                        )
                    }
                }))
                .buffer_unordered(crate::parallel::IO_CONCURRENCY_LIMIT);

            while let Some((index, branch, result)) = pending.next().await {
                match result {
                    Ok(Some(row)) => {
                        let _ = sender.send(ReadyTuiUpdate::Loaded { index, row });
                    }
                    Ok(None) => {
                        let _ = sender.send(ReadyTuiUpdate::Unavailable {
                            index,
                            branch,
                            message: "No PR found for branch".to_string(),
                        });
                    }
                    Err(error) => {
                        let _ = sender.send(ReadyTuiUpdate::Unavailable {
                            index,
                            branch,
                            message: error.to_string(),
                        });
                    }
                }
            }
        });

        let _ = sender.send(ReadyTuiUpdate::Done);
    });

    receiver
}

struct DraftToggleOutcome {
    pr_number: u64,
    target_draft: bool,
    error: Option<String>,
}

fn spawn_draft_toggle(
    scope: ReadyScope,
    pr_number: u64,
    branch: String,
    target_draft: bool,
) -> Receiver<DraftToggleOutcome> {
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let outcome = (|| -> Result<()> {
            let repo = GitRepo::open_from_path(&scope.git_dir)?;
            let runtime = tokio::runtime::Runtime::new()?;
            let client = create_loader_forge_client(&runtime, &scope.remote)?;
            runtime.block_on(async { client.set_pr_draft(pr_number, target_draft).await })?;
            crate::commands::draft::update_local_pr_metadata(
                &repo,
                &branch,
                pr_number,
                target_draft,
            );
            Ok(())
        })();

        let _ = sender.send(DraftToggleOutcome {
            pr_number,
            target_draft,
            error: outcome.err().map(|error| error.to_string()),
        });
    });

    receiver
}

fn create_loader_forge_client(
    runtime: &tokio::runtime::Runtime,
    remote: &RemoteInfo,
) -> Result<ForgeClient> {
    let _enter = runtime.enter();
    ForgeClient::new(remote)
}

fn send_all_unavailable(
    sender: &mpsc::Sender<ReadyTuiUpdate>,
    scope: &ReadyScope,
    message: String,
) {
    for (index, branch) in scope.branches.iter().enumerate() {
        let _ = sender.send(ReadyTuiUpdate::Unavailable {
            index,
            branch: branch.clone(),
            message: message.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::{ForgeType, RemoteInfo};
    use std::env;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn ready_tui_constructs_forge_client_with_entered_runtime() {
        let _guard = ENV_LOCK.lock().unwrap();
        let original_token = env::var("STAX_GITHUB_TOKEN").ok();
        unsafe { env::set_var("STAX_GITHUB_TOKEN", "test-token") };

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let remote = RemoteInfo {
            name: "origin".to_string(),
            forge: ForgeType::GitHub,
            host: "github.com".to_string(),
            namespace: "owner".to_string(),
            repo: "repo".to_string(),
            base_url: "https://github.com".to_string(),
            api_base_url: Some("https://api.github.com".to_string()),
        };

        let result = create_loader_forge_client(&runtime, &remote);

        match original_token {
            Some(token) => unsafe { env::set_var("STAX_GITHUB_TOKEN", token) },
            None => unsafe { env::remove_var("STAX_GITHUB_TOKEN") },
        }
        assert!(result.is_ok());
    }
}
