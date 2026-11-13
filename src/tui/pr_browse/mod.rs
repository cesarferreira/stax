//! Interactive pull request browser and reviewer behind `stax pr browse`.

pub mod app;
pub mod markdown;
pub mod ui;

use crate::commands::open::open_url_in_browser;
use crate::forge::MergeMethod;
use crate::github::GitHubClient;
use crate::github::pr_browse::{self, Action, Detail, Entry, PrRef, Source, State};
use anyhow::Result;
use app::{App, Overlay, ParsedPatch, PatchState, Screen, Tab};
use crossterm::{
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::collections::HashMap;
use std::io;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Where the browser runs: the repository and branch of the current folder
/// (if it is a GitHub checkout) and the GitHub it talks to.
pub struct Scope {
    pub here: Option<String>,
    pub branch: Option<String>,
    pub trunk: Option<String>,
    /// The current checkout's remote, once validated as a trusted network
    /// destination. Pull requests that live there (`PrRef::on_trusted`) are
    /// reached through its API URL; everything else goes to github.com.
    pub trusted: Option<TrustedRemote>,
    pub config: crate::config::Config,
    /// Web address of the checkout's host, for links: `https://github.com`.
    pub web_base: String,
    clients: Mutex<HashMap<(String, bool), GitHubClient>>,
}

pub struct TrustedRemote {
    pub repo: String,
    pub host: String,
    pub api_base_url: Option<String>,
}

impl Scope {
    pub fn new(
        here: Option<String>,
        branch: Option<String>,
        trunk: Option<String>,
        trusted: Option<TrustedRemote>,
        config: crate::config::Config,
        web_base: String,
    ) -> Self {
        Scope {
            here,
            branch,
            trunk,
            trusted,
            config,
            web_base,
            clients: Mutex::new(HashMap::new()),
        }
    }

    /// A client for `repo` (`owner/name`). Always built through the
    /// host-scoped token resolver, which refuses a token configured for a
    /// different host: the checkout's validated host for its own pull
    /// requests, github.com for everything else. A pasted link or a
    /// look-alike repository name can never steer the token elsewhere.
    /// Clients are kept, so hovering rows doesn't rebuild one per request.
    fn client_for(&self, repo: &str, on_trusted: bool) -> Result<GitHubClient> {
        let key = (repo.to_ascii_lowercase(), on_trusted);
        if let Some(client) = self.clients.lock().ok().and_then(|c| c.get(&key).cloned()) {
            return Ok(client);
        }
        let (owner, name) = repo
            .split_once('/')
            .ok_or_else(|| anyhow::anyhow!("'{repo}' isn't an owner/name repository"))?;
        let client = match (&self.trusted, on_trusted) {
            (Some(t), true) => GitHubClient::new_for_trusted_remote(
                owner,
                name,
                t.api_base_url.clone(),
                &self.config,
                &t.host,
            )?,
            _ => {
                GitHubClient::new_for_trusted_remote(owner, name, None, &self.config, "github.com")?
            }
        };
        if let Ok(mut clients) = self.clients.lock() {
            clients.insert(key, client.clone());
        }
        Ok(client)
    }

    /// The client for searches: the checkout's host when there is one.
    fn search_client(&self) -> Result<GitHubClient> {
        match &self.trusted {
            Some(t) => self.client_for(&t.repo, true),
            None => self.client_for("_/_", false),
        }
    }
}

enum Request {
    Search {
        index: usize,
        source: Source,
    },
    Detail {
        pr: PrRef,
        seq: u64,
    },
    Patch {
        pr: PrRef,
        head: String,
    },
    Act {
        pr: PrRef,
        action: Action,
        head: String,
    },
}

enum Update {
    Group {
        index: usize,
        result: Result<Vec<Entry>, String>,
    },
    Detail {
        pr: PrRef,
        seq: u64,
        result: Result<Box<Detail>, String>,
    },
    Patch {
        pr: PrRef,
        head: String,
        result: Result<String, String>,
    },
    Acted {
        pr: PrRef,
        action: Action,
        result: Result<(), String>,
    },
}

fn describe(error: &anyhow::Error) -> String {
    format!("{error:#}")
}

/// Restores the terminal however the browser ends, panics included.
struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        if let Err(error) = execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste) {
            let _ = disable_raw_mode();
            return Err(error.into());
        }
        Ok(TerminalGuard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), DisableBracketedPaste, LeaveAlternateScreen);
    }
}

/// Runs the browser: the list of pull requests that concern you, or `initial`
/// opened directly.
pub fn run(scope: Scope, initial: Option<PrRef>) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    // Clients build their transport on the current runtime, so this and the
    // auth check both happen inside it.
    let search_client = {
        let _enter = runtime.enter();
        scope.search_client()?
    };

    let sources = if initial.is_some() {
        Vec::new()
    } else {
        pr_browse::sources(
            scope.here.as_deref(),
            scope.branch.as_deref(),
            scope.trunk.as_deref(),
        )
    };
    let mut app = App::new(sources, initial.clone(), scope.web_base.clone());
    let (req_tx, rx) = spawn_worker(runtime, search_client, Arc::new(scope));

    for index in 0..app.groups.len() {
        let source = app.groups[index].source.clone();
        let _ = req_tx.send(Request::Search { index, source });
    }
    if let Some(pr) = initial {
        request_detail(&mut app, &req_tx, &pr);
    }

    let _guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    run_app(&mut terminal, &mut app, &req_tx, &rx)
}

fn spawn_worker(
    runtime: tokio::runtime::Runtime,
    search_client: GitHubClient,
    scope: Arc<Scope>,
) -> (Sender<Request>, Receiver<Update>) {
    let (req_tx, req_rx) = mpsc::channel::<Request>();
    let (up_tx, up_rx) = mpsc::channel::<Update>();
    std::thread::spawn(move || {
        let viewer = Arc::new(tokio::sync::OnceCell::<String>::new());
        let search_client = Arc::new(search_client);
        while let Ok(request) = req_rx.recv() {
            let tx = up_tx.clone();
            let scope = scope.clone();
            let viewer = viewer.clone();
            let search_client = search_client.clone();
            runtime.spawn(async move {
                handle_request(request, tx, scope, viewer, search_client).await;
            });
        }
    });
    (req_tx, up_rx)
}

async fn handle_request(
    request: Request,
    tx: Sender<Update>,
    scope: Arc<Scope>,
    viewer: Arc<tokio::sync::OnceCell<String>>,
    search_client: Arc<GitHubClient>,
) {
    match request {
        Request::Search { index, source } => {
            let result = async {
                let login = if matches!(source, Source::Owned) {
                    viewer
                        .get_or_try_init(|| search_client.get_current_user())
                        .await?
                        .clone()
                } else {
                    String::new()
                };
                pr_browse::search(&search_client, &source, &login, scope.trusted.is_some()).await
            }
            .await
            .map_err(|e| describe(&e));
            let _ = tx.send(Update::Group { index, result });
        }
        Request::Detail { pr, seq } => {
            let result = async {
                let client = scope.client_for(&pr.repo, pr.on_trusted)?;
                pr_browse::fetch_detail(&client, &pr).await.map(Box::new)
            }
            .await
            .map_err(|e| describe(&e));
            let _ = tx.send(Update::Detail { pr, seq, result });
        }
        Request::Patch { pr, head } => {
            let result = async {
                let client = scope.client_for(&pr.repo, pr.on_trusted)?;
                pr_browse::fetch_patch(&client, &pr).await
            }
            .await
            .map_err(|e| describe(&e));
            let _ = tx.send(Update::Patch { pr, head, result });
        }
        Request::Act { pr, action, head } => {
            let result = async {
                let client = scope.client_for(&pr.repo, pr.on_trusted)?;
                pr_browse::perform(&client, &pr, &action, &head).await
            }
            .await
            .map_err(|e| describe(&e));
            let _ = tx.send(Update::Acted { pr, action, result });
        }
    }
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    req_tx: &Sender<Request>,
    rx: &Receiver<Update>,
) -> Result<()> {
    loop {
        poll_updates(app, rx, req_tx);
        sync_requests(app, req_tx);
        terminal.draw(|f| ui::render(f, app))?;

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    handle_key(app, key, req_tx);
                }
                // A paste is text, never commands: it only lands in the comment box.
                Event::Paste(text) => {
                    if let Overlay::Comment(comment) = &mut app.overlay {
                        comment.push_str(&text.replace("\r\n", "\n").replace('\r', "\n"));
                    }
                }
                _ => {}
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn poll_updates(app: &mut App, rx: &Receiver<Update>, req_tx: &Sender<Request>) {
    while let Ok(update) = rx.try_recv() {
        apply_update(app, update, req_tx);
    }
}

fn apply_update(app: &mut App, update: Update, req_tx: &Sender<Request>) {
    match update {
        Update::Group { index, result } => app.apply_group(index, result),
        Update::Detail { pr, seq, result } => {
            // A newer read of the same pull request supersedes this one.
            if app.latest_detail.get(&pr) == Some(&seq) {
                app.apply_detail(&pr, result.map(|d| *d));
            }
        }
        Update::Patch { pr, head, result } => {
            if app.current.as_ref() == Some(&pr)
                && app.detail.as_ref().is_some_and(|d| d.head_sha == head)
            {
                app.patch = match result {
                    Ok(text) => PatchState::Ready(ParsedPatch::parse(&text)),
                    Err(error) => PatchState::Failed(error),
                };
                app.patch_head = head;
            }
        }
        Update::Acted { pr, action, result } => {
            app.busy = None;
            match result {
                Ok(()) => {
                    app.status = Some(action.done(&pr));
                    app.list_dirty = true;
                    refresh_detail(app, req_tx, &pr);
                }
                Err(error) => {
                    app.status = Some(format!(
                        "Couldn't do that: {}",
                        error.lines().next().unwrap_or("unknown error")
                    ));
                }
            }
        }
    }
}

/// Asks for what the screen needs and doesn't have: the pull request under the
/// cursor read ahead, and the diff once Code is open.
fn sync_requests(app: &mut App, req_tx: &Sender<Request>) {
    match app.screen {
        Screen::List => {
            if let Some(entry) = app.selected_entry()
                && app.prefetched.insert(entry.pr.clone())
            {
                request_detail(app, req_tx, &entry.pr);
            }
        }
        Screen::View => {
            if app.tab == Tab::Code
                && matches!(app.patch, PatchState::None)
                && let (Some(pr), Some(detail)) = (app.current.clone(), &app.detail)
            {
                let head = detail.head_sha.clone();
                app.patch = PatchState::Loading;
                app.patch_head = head.clone();
                let _ = req_tx.send(Request::Patch { pr, head });
            }
        }
    }
}

fn request_detail(app: &mut App, req_tx: &Sender<Request>, pr: &PrRef) {
    if !app.inflight.contains(pr) {
        refresh_detail(app, req_tx, pr);
    }
}

/// Reads the pull request again even if a read is under way: after an action,
/// the older read may predate it, and its answer is dropped.
fn refresh_detail(app: &mut App, req_tx: &Sender<Request>, pr: &PrRef) {
    app.next_seq += 1;
    let seq = app.next_seq;
    app.latest_detail.insert(pr.clone(), seq);
    app.inflight.insert(pr.clone());
    let _ = req_tx.send(Request::Detail {
        pr: pr.clone(),
        seq,
    });
}

fn reload_list(app: &mut App, req_tx: &Sender<Request>) {
    app.list_dirty = false;
    app.cache.clear();
    app.prefetched.clear();
    for index in 0..app.groups.len() {
        app.groups[index].status = app::GroupStatus::Loading;
        let source = app.groups[index].source.clone();
        let _ = req_tx.send(Request::Search { index, source });
    }
}

fn copy_to_clipboard(app: &mut App, what: &str, text: &str) {
    let result = arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_owned()));
    app.status = Some(match result {
        Ok(()) => format!("Copied {what}"),
        Err(error) => format!("Couldn't copy {what}: {error}"),
    });
}

fn handle_key(app: &mut App, key: KeyEvent, req_tx: &Sender<Request>) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return;
    }
    match app.overlay.clone() {
        Overlay::None => match app.screen {
            Screen::List => handle_list_key(app, key, req_tx),
            Screen::View => handle_view_key(app, key, req_tx),
        },
        Overlay::Help => app.overlay = Overlay::None,
        Overlay::MergeMethod(methods) => handle_merge_key(app, key.code, &methods),
        Overlay::Confirm(action) => {
            app.overlay = Overlay::None;
            if matches!(key.code, KeyCode::Char('y' | 'Y')) {
                perform(app, req_tx, action);
            }
        }
        Overlay::Comment(text) => handle_comment_key(app, key, text, req_tx),
    }
}

fn handle_list_key(app: &mut App, key: KeyEvent, req_tx: &Sender<Request>) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let page = app.list_height.max(2) as isize / 2;
    app.status = None;
    match key.code {
        KeyCode::Esc => {
            if app.filter.is_empty() {
                app.should_quit = true;
            } else {
                app.filter.clear();
                app.clamp_cursor();
            }
        }
        KeyCode::Up => app.move_cursor(-1),
        KeyCode::Down => app.move_cursor(1),
        KeyCode::Char('p') if ctrl => app.move_cursor(-1),
        KeyCode::Char('n') if ctrl => app.move_cursor(1),
        KeyCode::PageUp => app.move_cursor(-page),
        KeyCode::PageDown => app.move_cursor(page),
        KeyCode::Home => app.cursor = 0,
        KeyCode::End => app.cursor = usize::MAX,
        KeyCode::Enter => {
            if let Some(entry) = app.selected_entry() {
                request_detail(app, req_tx, &entry.pr);
                app.open(entry.pr);
            }
        }
        KeyCode::Char('y') if ctrl => {
            if let Some(entry) = app.selected_entry() {
                let number = entry.pr.number.to_string();
                copy_to_clipboard(app, &format!("#{number}"), &number);
            }
        }
        KeyCode::Char('r') if ctrl => reload_list(app, req_tx),
        KeyCode::Backspace => {
            app.filter.pop();
            app.cursor = 0;
        }
        KeyCode::Char(c) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => {
            app.filter.push(c);
            app.cursor = 0;
        }
        _ => {}
    }
    app.clamp_cursor();
}

fn handle_view_key(app: &mut App, key: KeyEvent, req_tx: &Sender<Request>) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let half = (app.view_height / 2).max(1) as isize;
    let full = app.view_height.saturating_sub(1).max(1) as isize;
    if app.busy.is_some() {
        return;
    }
    app.status = None;

    let set_tab = |app: &mut App, tab: Tab| {
        app.tab = tab;
        app.hscroll = 0;
    };

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => {
            if app.direct {
                app.should_quit = true;
            } else {
                let dirty = app.list_dirty;
                app.back_to_list();
                if dirty {
                    reload_list(app, req_tx);
                }
            }
        }
        KeyCode::Right | KeyCode::Tab => set_tab(app, app.tab.step(1)),
        KeyCode::Left | KeyCode::BackTab => set_tab(app, app.tab.step(-1)),
        KeyCode::Char('1') => set_tab(app, Tab::Summary),
        KeyCode::Char('2') => set_tab(app, Tab::Timeline),
        KeyCode::Char('3') => set_tab(app, Tab::Code),
        KeyCode::Char('j') | KeyCode::Down => app.scroll_by(1),
        KeyCode::Char('k') | KeyCode::Up => app.scroll_by(-1),
        KeyCode::Char('d') if ctrl => app.scroll_by(half),
        KeyCode::Char('u') if ctrl => app.scroll_by(-half),
        KeyCode::Char(' ') | KeyCode::PageDown => app.scroll_by(full),
        KeyCode::Char('b') | KeyCode::PageUp => app.scroll_by(-full),
        KeyCode::Char('g') | KeyCode::Home => app.scroll_by(isize::MIN / 2),
        KeyCode::Char('G') | KeyCode::End => app.scroll_by(isize::MAX / 2),
        KeyCode::Char('l') if app.tab == Tab::Code => app.hscroll += 8,
        KeyCode::Char('h') if app.tab == Tab::Code => {
            app.hscroll = app.hscroll.saturating_sub(8);
        }
        KeyCode::Char(']') => jump_file(app, true),
        KeyCode::Char('[') => jump_file(app, false),
        KeyCode::Char('e') => app.expanded = !app.expanded,
        KeyCode::Char('?') => app.overlay = Overlay::Help,
        KeyCode::Char('R') => {
            if let Some(pr) = app.current.clone() {
                app.detail_error = None;
                app.patch = PatchState::None;
                refresh_detail(app, req_tx, &pr);
                app.status = Some("Reloading…".to_owned());
            }
        }
        KeyCode::Char('o') => {
            if let Some(pr) = &app.current {
                open_url_in_browser(&app.url(pr));
            }
        }
        KeyCode::Char('y') => {
            if let Some(pr) = app.current.clone() {
                let url = app.url(&pr);
                copy_to_clipboard(app, "the link", &url);
            }
        }
        KeyCode::Char('Y') => {
            if let Some(pr) = app.current.clone() {
                let command = format!("gh pr checkout {} -R {}", pr.number, pr.repo);
                copy_to_clipboard(app, "the checkout command", &command);
            }
        }
        KeyCode::Char('n') => {
            if let Some(pr) = app.current.clone() {
                copy_to_clipboard(app, &format!("#{}", pr.number), &pr.number.to_string());
            }
        }
        KeyCode::Char('m') => start_merge(app),
        KeyCode::Char('x') => start_close_or_reopen(app),
        KeyCode::Char('r') => toggle_draft(app, req_tx),
        KeyCode::Char('a') => start_approve(app, req_tx),
        KeyCode::Char('c') => {
            if app.detail.is_some() {
                app.overlay = Overlay::Comment(String::new());
            } else {
                app.status = Some("Wait for the pull request to load.".to_owned());
            }
        }
        _ => {}
    }
}

fn jump_file(app: &mut App, forward: bool) {
    if app.tab != Tab::Code {
        return;
    }
    let PatchState::Ready(patch) = &app.patch else {
        return;
    };
    let here = app.scroll[Tab::Code.index()];
    let target = if forward {
        patch.files.iter().copied().find(|row| *row > here)
    } else {
        patch.files.iter().copied().rev().find(|row| *row < here)
    };
    if let Some(row) = target {
        app.scroll[Tab::Code.index()] = row.min(app.max_scroll());
    }
}

fn start_merge(app: &mut App) {
    let Some(detail) = &app.detail else {
        app.status = Some("Wait for the pull request to load.".to_owned());
        return;
    };
    let message = match detail.state {
        State::Merged => Some("Already merged."),
        State::Closed => Some("Closed; reopen it first (x)."),
        State::Draft => Some("A draft can't be merged; mark it ready first (r)."),
        State::Open if !detail.can_write => Some("You don't have write access to merge this."),
        State::Open if detail.methods.is_empty() => Some("This repository allows no merge method."),
        State::Open => None,
    };
    if let Some(message) = message {
        app.status = Some(message.to_owned());
        return;
    }
    let methods = detail.methods.clone();
    if let [only] = methods.as_slice() {
        app.overlay = Overlay::Confirm(Action::Merge(*only));
    } else {
        app.overlay = Overlay::MergeMethod(methods);
    }
}

fn handle_merge_key(app: &mut App, code: KeyCode, methods: &[MergeMethod]) {
    app.overlay = Overlay::None;
    if let KeyCode::Char(c) = code
        && let Some(method) = methods
            .iter()
            .find(|m| pr_browse::method_key(**m) == c.to_ascii_lowercase())
    {
        app.overlay = Overlay::Confirm(Action::Merge(*method));
    }
}

fn start_close_or_reopen(app: &mut App) {
    let Some(detail) = &app.detail else {
        app.status = Some("Wait for the pull request to load.".to_owned());
        return;
    };
    match detail.state {
        State::Merged => app.status = Some("Already merged.".to_owned()),
        _ if !detail.can_write => {
            app.status = Some("You don't have write access to close or reopen this.".to_owned());
        }
        State::Closed => app.overlay = Overlay::Confirm(Action::Reopen),
        State::Open | State::Draft => app.overlay = Overlay::Confirm(Action::Close),
    }
}

fn toggle_draft(app: &mut App, req_tx: &Sender<Request>) {
    let Some(detail) = &app.detail else {
        app.status = Some("Wait for the pull request to load.".to_owned());
        return;
    };
    let action = match detail.state {
        State::Draft => Action::Ready,
        State::Open => Action::Draft,
        State::Merged | State::Closed => {
            app.status = Some("Only an open pull request can change draft state.".to_owned());
            return;
        }
    };
    perform(app, req_tx, action);
}

fn start_approve(app: &mut App, req_tx: &Sender<Request>) {
    let Some(detail) = &app.detail else {
        app.status = Some("Wait for the pull request to load.".to_owned());
        return;
    };
    if matches!(detail.state, State::Merged | State::Closed) {
        app.status = Some("Only an open pull request can be approved.".to_owned());
        return;
    }
    perform(app, req_tx, Action::Approve);
}

fn handle_comment_key(app: &mut App, key: KeyEvent, mut text: String, req_tx: &Sender<Request>) {
    match key.code {
        KeyCode::Esc => app.overlay = Overlay::None,
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::ALT) => {
            text.push('\n');
            app.overlay = Overlay::Comment(text);
        }
        KeyCode::Enter => {
            if text.trim().is_empty() {
                app.overlay = Overlay::None;
            } else {
                app.overlay = Overlay::None;
                perform(app, req_tx, Action::Comment(text.trim_end().to_owned()));
            }
        }
        KeyCode::Backspace => {
            text.pop();
            app.overlay = Overlay::Comment(text);
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            text.push(c);
            app.overlay = Overlay::Comment(text);
        }
        _ => app.overlay = Overlay::Comment(text),
    }
}

fn perform(app: &mut App, req_tx: &Sender<Request>, action: Action) {
    let (Some(pr), Some(detail)) = (app.current.clone(), &app.detail) else {
        return;
    };
    let head = detail.head_sha.clone();
    app.busy = Some(
        match &action {
            Action::Merge(_) => "Merging…",
            Action::Close => "Closing…",
            Action::Reopen => "Reopening…",
            Action::Ready | Action::Draft => "Updating…",
            Action::Approve => "Approving…",
            Action::Comment(_) => "Posting…",
        }
        .to_owned(),
    );
    let _ = req_tx.send(Request::Act { pr, action, head });
}
