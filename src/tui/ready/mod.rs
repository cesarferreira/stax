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
use std::time::Duration;

use crate::commands::open::open_url_in_browser;
use crate::commands::ready::{ReadyScope, ReadyScopeMode, fetch_row_for_branch, load_ready_scope};
use crate::engine::Stack;
use crate::forge::ForgeClient;
use crate::git::GitRepo;
use crate::remote::RemoteInfo;
use crate::tui::keys::{self, KeyScope};

pub fn run(scope_mode: ReadyScopeMode) -> Result<()> {
    let scope = load_ready_scope(scope_mode)?;
    let mut app = ReadyTuiApp::from_parts(
        scope.repo_label.clone(),
        scope.scope_label.clone(),
        scope.branches.clone(),
    );
    let mut loader = Some(spawn_loader(scope.clone()));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, &mut app, &scope, &mut loader);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut ReadyTuiApp,
    scope: &ReadyScope,
    loader: &mut Option<Receiver<ReadyTuiUpdate>>,
) -> Result<()> {
    loop {
        poll_loader(app, loader);
        terminal.draw(|f| ui::render(f, app))?;

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            // This TUI is a list view only, so motion bindings are all that apply.
            let key = keys::normalize(key, KeyScope::Navigation);
            handle_key(app, key.code, scope, loader);
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn handle_key(
    app: &mut ReadyTuiApp,
    code: KeyCode,
    scope: &ReadyScope,
    loader: &mut Option<Receiver<ReadyTuiUpdate>>,
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
            app.reset_for_refresh();
            *loader = Some(spawn_loader(scope.clone()));
        }
        KeyCode::Enter | KeyCode::Char('o') => {
            if let Some(url) = app.selected_pr_url() {
                open_url_in_browser(&url);
                app.status_message = Some(format!("Opened {url}"));
            } else {
                app.status_message = Some("Selected row has no loaded PR URL yet".to_string());
            }
        }
        _ => {}
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
