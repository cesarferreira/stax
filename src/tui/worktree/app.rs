use crate::commands::worktree::remove::remove_worktree_with_hooks;
use crate::commands::worktree::shared::{
    compute_worktree_details, default_tmux_session_name, find_worktree, list_tmux_sessions,
    status_labels, TmuxSession, WorktreeDetails,
};
use crate::config::Config;
use crate::git::repo::WorktreeInfo;
use crate::git::GitRepo;
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DashboardMode {
    Normal,
    Help,
    CreateInput,
    ConfirmDelete,
    ConfirmForceDelete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxState {
    Loading,
    Unavailable,
    Missing,
    Detached,
    Attached(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TmuxAvailability {
    Loading,
    Available,
    Unavailable,
}

#[derive(Debug)]
enum LoaderUpdate {
    TmuxProbe(Result<Vec<TmuxSession>, String>),
    Details {
        index: usize,
        details: Box<WorktreeDetails>,
        status_labels: Vec<String>,
    },
    DetailError {
        index: usize,
        error: String,
    },
    Done,
}

#[derive(Debug)]
enum RemovalUpdate {
    RunningPreHook,
    RemovingWorktree,
    #[allow(dead_code)]
    RunningPostHook,
    Success {
        removed_name: String,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone)]
pub struct WorktreeRecord {
    pub info: WorktreeInfo,
    pub branch_label: String,
    pub details: Option<WorktreeDetails>,
    pub load_error: Option<String>,
    pub tmux_session: String,
    pub tmux_state: TmuxState,
    pub status_labels: Vec<String>,
}

impl WorktreeRecord {
    fn new(info: WorktreeInfo) -> Self {
        let branch_label = info
            .branch
            .clone()
            .unwrap_or_else(|| "(detached)".to_string());
        let tmux_session =
            default_tmux_session_name(&info.name).unwrap_or_else(|_| info.name.clone());

        Self {
            info,
            branch_label,
            details: None,
            load_error: None,
            tmux_session,
            tmux_state: TmuxState::Loading,
            status_labels: vec!["loading".to_string()],
        }
    }

    pub fn is_loading(&self) -> bool {
        self.details.is_none() && self.load_error.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingCommand {
    Go { name: String },
    Create { name: Option<String> },
    Restack,
}

impl PendingCommand {
    pub fn args(&self) -> Vec<String> {
        match self {
            Self::Go { name } => vec!["wt".into(), "go".into(), name.clone(), "--tmux".into()],
            Self::Create { name } => {
                let mut args = vec!["wt".into(), "c".into()];
                if let Some(name) = name {
                    args.push(name.clone());
                }
                args.push("--tmux".into());
                args
            }
            Self::Restack => vec!["wt".into(), "rs".into()],
        }
    }
}

pub struct WorktreeApp {
    pub records: Vec<WorktreeRecord>,
    pub selected_index: usize,
    pub mode: DashboardMode,
    pub input_buffer: String,
    pub input_cursor: usize,
    pub status_message: Option<String>,
    pub should_quit: bool,
    pub pending_command: Option<PendingCommand>,
    tmux_availability: TmuxAvailability,
    loader: Option<Receiver<LoaderUpdate>>,
    removal_operation: Option<Receiver<RemovalUpdate>>,
    removal_in_progress: bool,
    pub removal_status: Option<String>,
    repo_path: PathBuf,
}

impl WorktreeApp {
    pub fn new(
        initial_status: Option<String>,
        preferred_selection: Option<String>,
    ) -> Result<Self> {
        let repo = GitRepo::open()?;
        let repo_path = repo.git_dir()?.to_path_buf();
        let worktrees = repo.list_worktrees()?;
        let records = worktrees
            .into_iter()
            .map(WorktreeRecord::new)
            .collect::<Vec<_>>();
        let selected_index = default_selection(&records, preferred_selection.as_deref());
        let loader = if records.is_empty() {
            None
        } else {
            Some(spawn_loader(
                repo_path.clone(),
                records.iter().map(|record| record.info.clone()).collect(),
            ))
        };

        Ok(Self {
            records,
            selected_index,
            mode: DashboardMode::Normal,
            input_buffer: String::new(),
            input_cursor: 0,
            status_message: initial_status,
            should_quit: false,
            pending_command: None,
            tmux_availability: TmuxAvailability::Loading,
            loader,
            removal_operation: None,
            removal_in_progress: false,
            removal_status: None,
            repo_path,
        })
    }

    pub fn selected(&self) -> Option<&WorktreeRecord> {
        self.records.get(self.selected_index)
    }

    pub fn select_previous(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn select_next(&mut self) {
        if self.selected_index + 1 < self.records.len() {
            self.selected_index += 1;
        }
    }

    pub fn set_status<T: Into<String>>(&mut self, message: T) {
        self.status_message = Some(message.into());
    }

    pub fn request_go(&mut self) {
        let Some(record) = self.selected() else {
            return;
        };

        match self.tmux_availability {
            TmuxAvailability::Loading => {
                self.set_status("Still probing tmux; try again in a moment");
                return;
            }
            TmuxAvailability::Unavailable => {
                self.set_status("tmux not available; install tmux or use the CLI directly");
                return;
            }
            TmuxAvailability::Available => {}
        }

        if record.info.is_prunable || !record.info.path.exists() {
            self.set_status("Worktree path is missing; run `st wt prune` first");
            return;
        }

        self.pending_command = Some(PendingCommand::Go {
            name: record.info.name.clone(),
        });
        self.should_quit = true;
    }

    pub fn request_create(&mut self) {
        match self.tmux_availability {
            TmuxAvailability::Loading => {
                self.set_status("Still probing tmux; try again in a moment");
                return;
            }
            TmuxAvailability::Unavailable => {
                self.set_status("tmux not available; install tmux or use `st wt c` manually");
                return;
            }
            TmuxAvailability::Available => {}
        }
        self.input_buffer.clear();
        self.input_cursor = 0;
        self.mode = DashboardMode::CreateInput;
    }

    pub fn confirm_create(&mut self) {
        let name = self.input_buffer.trim().to_string();
        self.pending_command = Some(PendingCommand::Create {
            name: if name.is_empty() { None } else { Some(name) },
        });
        self.should_quit = true;
    }

    pub fn request_delete(&mut self) {
        let Some(record) = self.selected() else {
            return;
        };

        if record.info.is_main {
            self.set_status("Cannot remove the main worktree");
            return;
        }
        if record.info.is_current {
            self.set_status("Cannot remove the current worktree from the dashboard");
            return;
        }
        if record.info.is_prunable || !record.info.path.exists() {
            self.set_status("Missing worktree entries should be cleaned with `st wt prune`");
            return;
        }

        self.mode = DashboardMode::ConfirmDelete;
    }

    pub fn confirm_delete(&mut self) {
        if let Some(record) = self.selected() {
            // Check if worktree is dirty and details are loaded
            if let Some(details) = &record.details {
                if details.dirty {
                    // Dirty worktree: show force confirmation
                    self.mode = DashboardMode::ConfirmForceDelete;
                    return;
                }
            }

            // Clean worktree or details not loaded: proceed with removal
            self.start_removal(false);
            self.mode = DashboardMode::Normal;
        }
    }

    pub fn confirm_force_delete(&mut self) {
        self.start_removal(true);
        self.mode = DashboardMode::Normal;
    }

    fn start_removal(&mut self, force: bool) {
        let Some(record) = self.selected() else {
            return;
        };

        let worktree_name = record.info.name.clone();
        let repo_path = self.repo_path.clone();

        self.removal_operation = Some(spawn_removal_operation(repo_path, worktree_name, force));
        self.removal_in_progress = true;
    }

    pub fn request_restack(&mut self) {
        if self.records.iter().any(|record| {
            record
                .details
                .as_ref()
                .is_some_and(|details| details.is_managed)
        }) {
            self.pending_command = Some(PendingCommand::Restack);
            self.should_quit = true;
            return;
        }

        if self.records.iter().any(WorktreeRecord::is_loading) {
            self.set_status("Still loading stack metadata; try again in a moment");
            return;
        }

        if self
            .records
            .iter()
            .any(|record| record.load_error.as_ref().is_some())
        {
            self.set_status("Some worktree metadata failed to load; use the CLI directly");
            return;
        }

        if !self.records.iter().any(|record| {
            record
                .details
                .as_ref()
                .is_some_and(|details| details.is_managed)
        }) {
            self.set_status("No stax-managed worktrees to restack");
        }
    }

    pub fn refresh_background(&mut self) {
        // Process loader updates
        loop {
            let update = match self.loader.as_ref() {
                Some(loader) => match loader.try_recv() {
                    Ok(update) => Some(update),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => {
                        self.loader = None;
                        None
                    }
                },
                None => None,
            };

            let Some(update) = update else {
                break;
            };
            self.apply_loader_update(update);
        }

        // Process removal updates
        loop {
            let update = match self.removal_operation.as_ref() {
                Some(operation) => match operation.try_recv() {
                    Ok(update) => Some(update),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => {
                        self.removal_operation = None;
                        None
                    }
                },
                None => None,
            };

            let Some(update) = update else {
                break;
            };
            self.apply_removal_update(update);
        }
    }

    pub fn is_loading(&self) -> bool {
        matches!(self.tmux_availability, TmuxAvailability::Loading)
            || self.records.iter().any(WorktreeRecord::is_loading)
    }

    pub fn loading_summary(&self) -> Option<String> {
        if !self.is_loading() {
            return None;
        }

        let loaded = self
            .records
            .iter()
            .filter(|record| !record.is_loading())
            .count();
        Some(format!(
            "Loading worktree details... ({}/{})",
            loaded,
            self.records.len()
        ))
    }

    fn apply_loader_update(&mut self, update: LoaderUpdate) {
        match update {
            LoaderUpdate::TmuxProbe(Ok(sessions)) => {
                let tmux_map = sessions
                    .into_iter()
                    .map(|session| (session.name.clone(), session))
                    .collect::<HashMap<_, _>>();
                self.tmux_availability = TmuxAvailability::Available;
                for record in &mut self.records {
                    record.tmux_state = tmux_state_for(true, &tmux_map, &record.tmux_session);
                }
            }
            LoaderUpdate::TmuxProbe(Err(_)) => {
                self.tmux_availability = TmuxAvailability::Unavailable;
                for record in &mut self.records {
                    record.tmux_state = TmuxState::Unavailable;
                }
            }
            LoaderUpdate::Details {
                index,
                details,
                status_labels,
            } => {
                if let Some(record) = self.records.get_mut(index) {
                    record.details = Some(*details);
                    record.load_error = None;
                    record.status_labels = status_labels;
                }
            }
            LoaderUpdate::DetailError { index, error } => {
                if let Some(record) = self.records.get_mut(index) {
                    record.load_error = Some(error);
                    record.status_labels = vec!["error".to_string()];
                }
            }
            LoaderUpdate::Done => {
                self.loader = None;
            }
        }
    }

    fn apply_removal_update(&mut self, update: RemovalUpdate) {
        match update {
            RemovalUpdate::RunningPreHook => {
                self.removal_status = Some("Running pre-remove hook...".to_string());
            }
            RemovalUpdate::RemovingWorktree => {
                self.removal_status = Some("Removing worktree...".to_string());
            }
            RemovalUpdate::RunningPostHook => {
                self.removal_status = Some("Running post-remove hook...".to_string());
            }
            RemovalUpdate::Success { removed_name } => {
                self.removal_in_progress = false;
                self.removal_operation = None;
                self.removal_status = None;
                self.set_status(format!("Removed '{}'", removed_name));
                self.reload_worktrees();
            }
            RemovalUpdate::Error { message } => {
                self.removal_in_progress = false;
                self.removal_operation = None;
                self.removal_status = None;
                self.set_status(format!("Error: {}", message));
            }
        }
    }

    fn reload_worktrees(&mut self) {
        let repo = match GitRepo::open() {
            Ok(r) => r,
            Err(e) => {
                self.set_status(format!("Failed to reload: {}", e));
                return;
            }
        };

        let repo_path = match repo.git_dir() {
            Ok(p) => p.to_path_buf(),
            Err(e) => {
                self.set_status(format!("Failed to get repo path: {}", e));
                return;
            }
        };

        let worktrees = match repo.list_worktrees() {
            Ok(wts) => wts,
            Err(e) => {
                self.set_status(format!("Failed to list worktrees: {}", e));
                return;
            }
        };

        self.records = worktrees.into_iter().map(WorktreeRecord::new).collect();

        if self.selected_index >= self.records.len() && !self.records.is_empty() {
            self.selected_index = self.records.len() - 1;
        }

        if !self.records.is_empty() {
            self.loader = Some(spawn_loader(
                repo_path,
                self.records.iter().map(|r| r.info.clone()).collect(),
            ));
        } else {
            self.loader = None;
        }
    }
}

fn spawn_loader(repo_path: PathBuf, worktrees: Vec<WorktreeInfo>) -> Receiver<LoaderUpdate> {
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let repo = match GitRepo::open_from_path(&repo_path) {
            Ok(repo) => repo,
            Err(error) => {
                for index in 0..worktrees.len() {
                    let _ = sender.send(LoaderUpdate::DetailError {
                        index,
                        error: format!("Failed to open repository: {error}"),
                    });
                }
                let _ = sender.send(LoaderUpdate::TmuxProbe(Err(error.to_string())));
                let _ = sender.send(LoaderUpdate::Done);
                return;
            }
        };

        let _ = sender.send(LoaderUpdate::TmuxProbe(
            list_tmux_sessions().map_err(|error| error.to_string()),
        ));

        for (index, worktree) in worktrees.into_iter().enumerate() {
            match compute_worktree_details(&repo, worktree) {
                Ok(details) => {
                    let labels = status_labels(&details);
                    let _ = sender.send(LoaderUpdate::Details {
                        index,
                        details: Box::new(details),
                        status_labels: labels,
                    });
                }
                Err(error) => {
                    let _ = sender.send(LoaderUpdate::DetailError {
                        index,
                        error: error.to_string(),
                    });
                }
            }
        }

        let _ = sender.send(LoaderUpdate::Done);
    });

    receiver
}

fn spawn_removal_operation(
    repo_path: PathBuf,
    worktree_name: String,
    force: bool,
) -> Receiver<RemovalUpdate> {
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let repo = match GitRepo::open_from_path(&repo_path) {
            Ok(repo) => repo,
            Err(e) => {
                let _ = sender.send(RemovalUpdate::Error {
                    message: format!("Failed to open repository: {}", e),
                });
                return;
            }
        };

        let config = match Config::load() {
            Ok(config) => config,
            Err(e) => {
                let _ = sender.send(RemovalUpdate::Error {
                    message: format!("Failed to load config: {}", e),
                });
                return;
            }
        };

        let worktree = match find_worktree(&repo, &worktree_name) {
            Ok(Some(wt)) => wt,
            Ok(None) => {
                let _ = sender.send(RemovalUpdate::Error {
                    message: format!("Worktree '{}' not found", worktree_name),
                });
                return;
            }
            Err(e) => {
                let _ = sender.send(RemovalUpdate::Error {
                    message: format!("Failed to find worktree: {}", e),
                });
                return;
            }
        };

        let _ = sender.send(RemovalUpdate::RunningPreHook);
        let _ = sender.send(RemovalUpdate::RemovingWorktree);

        match remove_worktree_with_hooks(&repo, &config, &worktree, force) {
            Ok(display_name) => {
                let _ = sender.send(RemovalUpdate::Success {
                    removed_name: display_name,
                });
            }
            Err(e) => {
                let _ = sender.send(RemovalUpdate::Error {
                    message: format!("Removal failed: {}", e),
                });
            }
        }
    });

    receiver
}

fn tmux_state_for(
    tmux_available: bool,
    sessions: &HashMap<String, TmuxSession>,
    session_name: &str,
) -> TmuxState {
    if !tmux_available {
        return TmuxState::Unavailable;
    }

    match sessions.get(session_name) {
        Some(session) if session.attached_clients > 0 => {
            TmuxState::Attached(session.attached_clients)
        }
        Some(_) => TmuxState::Detached,
        None => TmuxState::Missing,
    }
}

pub fn default_selection(records: &[WorktreeRecord], preferred: Option<&str>) -> usize {
    if let Some(preferred) = preferred {
        if let Some(index) = records
            .iter()
            .position(|record| record.info.name == preferred)
        {
            return index;
        }
    }

    records
        .iter()
        .position(|record| record.info.is_current)
        .unwrap_or(0)
}

pub fn worktree_badges(record: &WorktreeRecord) -> Vec<String> {
    let mut badges = Vec::new();

    if record.info.is_current {
        badges.push("current".to_string());
    }
    if record.info.is_main {
        badges.push("main".to_string());
    }
    if record.info.branch.is_none() {
        badges.push("detached".to_string());
    }
    if let Some(details) = record.details.as_ref() {
        if details.is_managed {
            badges.push("managed".to_string());
        } else {
            badges.push("unmanaged".to_string());
        }
        if details.dirty {
            badges.push("dirty".to_string());
        }
        if details.rebase_in_progress {
            badges.push("rebase".to_string());
        }
        if details.merge_in_progress {
            badges.push("merge".to_string());
        }
        if details.has_conflicts {
            badges.push("conflicts".to_string());
        }
    } else if record.load_error.is_some() {
        badges.push("error".to_string());
    } else {
        badges.push("loading".to_string());
    }
    if record.info.is_locked {
        badges.push("locked".to_string());
    }
    if record.info.is_prunable {
        badges.push("prunable".to_string());
    }

    badges
}

#[cfg(test)]
mod tests {
    use super::{default_selection, worktree_badges, PendingCommand, TmuxState, WorktreeRecord};
    use crate::git::repo::WorktreeInfo;
    use std::path::PathBuf;

    fn record(name: &str) -> WorktreeRecord {
        let mut record = WorktreeRecord::new(WorktreeInfo {
            name: name.to_string(),
            path: PathBuf::from(format!("/tmp/{}", name)),
            branch: Some(name.to_string()),
            is_main: false,
            is_current: false,
            is_locked: false,
            lock_reason: None,
            is_prunable: false,
            prunable_reason: None,
        });
        record.tmux_state = TmuxState::Missing;
        record
    }

    #[test]
    fn pending_command_builds_tmux_args() {
        assert_eq!(
            PendingCommand::Go {
                name: "lane".to_string()
            }
            .args(),
            vec!["wt", "go", "lane", "--tmux"]
        );
        assert_eq!(
            PendingCommand::Create {
                name: Some("lane".to_string())
            }
            .args(),
            vec!["wt", "c", "lane", "--tmux"]
        );
    }

    #[test]
    fn default_selection_prefers_named_worktree() {
        let mut first = record("alpha");
        first.info.is_current = true;
        let second = record("beta");
        assert_eq!(default_selection(&[first, second], Some("beta")), 1);
    }

    #[test]
    fn worktree_badges_show_loading_before_details_arrive() {
        let mut record = record("lane");
        record.info.is_current = true;
        record.info.branch = None;
        record.info.is_prunable = true;

        let badges = worktree_badges(&record);
        assert!(badges.contains(&"current".to_string()));
        assert!(badges.contains(&"detached".to_string()));
        assert!(badges.contains(&"loading".to_string()));
        assert!(badges.contains(&"prunable".to_string()));
    }
}
