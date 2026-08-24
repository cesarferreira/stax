//! Shared session state for the `st web` workspace.

use crate::application::OperationReceipt;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Persisted pane preferences stored in `.git/stax/web-state.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct WebPrefs {
    #[serde(default = "default_true")]
    show_stack: bool,
    #[serde(default = "default_true")]
    show_changes: bool,
    #[serde(default = "default_true")]
    show_inspector: bool,
    #[serde(default)]
    recent_projects: Vec<PathBuf>,
}

fn default_true() -> bool {
    true
}

impl Default for WebPrefs {
    fn default() -> Self {
        Self {
            show_stack: true,
            show_changes: true,
            show_inspector: true,
            recent_projects: Vec::new(),
        }
    }
}

/// Per-session state shared across all Axum handlers.
#[derive(Debug, Clone)]
pub struct WebSession {
    pub repository_root: PathBuf,
    pub session_token: String,
    pub csrf_token: String,
    /// Currently highlighted branch (not necessarily checked out).
    pub selected_branch: Option<String>,
    pub search_query: String,
    /// Whether the stack pane is visible.
    pub show_stack: bool,
    /// Whether the changes pane is visible.
    pub show_changes: bool,
    /// Whether the inspector pane is visible.
    pub show_inspector: bool,
    /// Last completed operation receipt (for undo/redo state).
    pub last_receipt: Option<OperationReceipt>,
    /// Whether a mutation is currently in flight.
    pub active_operation: bool,
    /// Recently opened repository roots (up to 10, most recent first).
    pub recent_projects: Vec<PathBuf>,
    /// Optional last SSE event message.
    pub last_event: Option<String>,
    /// Path to the persisted prefs file (`.git/stax/web-state.json`).
    prefs_path: Option<PathBuf>,
}

impl WebSession {
    pub fn new(repository_root: PathBuf, session_token: String, csrf_token: String) -> Self {
        let prefs_path = prefs_file_path(&repository_root);
        let prefs = prefs_path
            .as_ref()
            .and_then(|p| load_prefs(p).ok())
            .unwrap_or_default();

        // Seed recent_projects: current repo at top, then saved list (deduped, max 10).
        let mut recent = vec![repository_root.clone()];
        for p in prefs.recent_projects {
            if p != repository_root && recent.len() < 10 {
                recent.push(p);
            }
        }

        Self {
            repository_root,
            session_token,
            csrf_token,
            selected_branch: None,
            search_query: String::new(),
            show_stack: prefs.show_stack,
            show_changes: prefs.show_changes,
            show_inspector: prefs.show_inspector,
            last_receipt: None,
            active_operation: false,
            recent_projects: recent,
            last_event: None,
            prefs_path,
        }
    }

    /// Persist current pane visibility and recent-projects list to disk.
    pub fn save_prefs(&self) {
        let Some(ref path) = self.prefs_path else {
            return;
        };
        let prefs = WebPrefs {
            show_stack: self.show_stack,
            show_changes: self.show_changes,
            show_inspector: self.show_inspector,
            recent_projects: self.recent_projects.clone(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&prefs) {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(path, json);
        }
    }

    /// Switch the active repository root, refresh prefs path, and bump recent projects.
    pub fn switch_repository(&mut self, root: PathBuf, selected_branch: Option<String>) {
        self.repository_root = root.clone();
        self.selected_branch = selected_branch;
        self.search_query.clear();
        self.last_receipt = None;
        self.prefs_path = prefs_file_path(&root);
        if let Some(ref path) = self.prefs_path
            && let Ok(prefs) = load_prefs(path)
        {
            self.show_stack = prefs.show_stack;
            self.show_changes = prefs.show_changes;
            self.show_inspector = prefs.show_inspector;
        }
        self.recent_projects.retain(|p| p != &root);
        self.recent_projects.insert(0, root);
        self.recent_projects.truncate(10);
        self.save_prefs();
    }
}

fn prefs_file_path(repo_root: &std::path::Path) -> Option<PathBuf> {
    // Try to find the .git directory via git2 (handles worktrees correctly).
    if let Ok(repo) = git2::Repository::open(repo_root) {
        let git_dir = repo.path().to_path_buf();
        return Some(git_dir.join("stax").join("web-state.json"));
    }
    None
}

fn load_prefs(path: &std::path::Path) -> anyhow::Result<WebPrefs> {
    let data = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&data)?)
}

pub type SharedSession = Arc<Mutex<WebSession>>;

pub fn make_shared(session: WebSession) -> SharedSession {
    Arc::new(Mutex::new(session))
}

pub fn generate_token() -> String {
    use getrandom::fill;
    let mut bytes = [0u8; 24];
    fill(&mut bytes).expect("getrandom failed");
    hex_encode(&bytes)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
