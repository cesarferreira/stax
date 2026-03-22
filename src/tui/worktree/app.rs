use crate::commands::worktree::shared::{
    compute_worktree_details, default_tmux_session_name, list_tmux_sessions, status_labels,
    TmuxSession, WorktreeDetails,
};
use crate::git::GitRepo;
use anyhow::Result;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DashboardMode {
    Normal,
    Help,
    CreateInput,
    ConfirmDelete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxState {
    Unavailable,
    Missing,
    Detached,
    Attached(usize),
}

#[derive(Debug, Clone)]
pub struct WorktreeRecord {
    pub details: WorktreeDetails,
    pub tmux_session: String,
    pub tmux_state: TmuxState,
    pub status_labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingCommand {
    Go { name: String },
    Create { name: Option<String> },
    Remove { name: String },
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
            Self::Remove { name } => vec!["wt".into(), "rm".into(), name.clone()],
            Self::Restack => vec!["wt".into(), "rs".into()],
        }
    }
}

pub struct WorktreeApp {
    #[allow(dead_code)]
    pub repo: GitRepo,
    pub records: Vec<WorktreeRecord>,
    pub selected_index: usize,
    pub mode: DashboardMode,
    pub input_buffer: String,
    pub input_cursor: usize,
    pub status_message: Option<String>,
    pub should_quit: bool,
    pub pending_command: Option<PendingCommand>,
    pub tmux_available: bool,
}

impl WorktreeApp {
    pub fn new(
        initial_status: Option<String>,
        preferred_selection: Option<String>,
    ) -> Result<Self> {
        let repo = GitRepo::open()?;
        let (records, tmux_available) = load_records(&repo)?;
        let selected_index = default_selection(&records, preferred_selection.as_deref());

        Ok(Self {
            repo,
            records,
            selected_index,
            mode: DashboardMode::Normal,
            input_buffer: String::new(),
            input_cursor: 0,
            status_message: initial_status,
            should_quit: false,
            pending_command: None,
            tmux_available,
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

        if !self.tmux_available {
            self.set_status("tmux not available; install tmux or use the CLI directly");
            return;
        }
        if record.details.info.is_prunable || !record.details.info.path.exists() {
            self.set_status("Worktree path is missing; run `st wt prune` first");
            return;
        }

        self.pending_command = Some(PendingCommand::Go {
            name: record.details.info.name.clone(),
        });
        self.should_quit = true;
    }

    pub fn request_create(&mut self) {
        if !self.tmux_available {
            self.set_status("tmux not available; install tmux or use `st wt c` manually");
            return;
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

        if record.details.info.is_main {
            self.set_status("Cannot remove the main worktree");
            return;
        }
        if record.details.info.is_current {
            self.set_status("Cannot remove the current worktree from the dashboard");
            return;
        }
        if record.details.info.is_prunable || !record.details.info.path.exists() {
            self.set_status("Missing worktree entries should be cleaned with `st wt prune`");
            return;
        }

        self.mode = DashboardMode::ConfirmDelete;
    }

    pub fn confirm_delete(&mut self) {
        if let Some(record) = self.selected() {
            self.pending_command = Some(PendingCommand::Remove {
                name: record.details.info.name.clone(),
            });
            self.should_quit = true;
        }
    }

    pub fn request_restack(&mut self) {
        if !self.records.iter().any(|record| record.details.is_managed) {
            self.set_status("No stax-managed worktrees to restack");
            return;
        }

        self.pending_command = Some(PendingCommand::Restack);
        self.should_quit = true;
    }
}

fn load_records(repo: &GitRepo) -> Result<(Vec<WorktreeRecord>, bool)> {
    let tmux_sessions = list_tmux_sessions().ok();
    let tmux_available = tmux_sessions.is_some();
    let tmux_map = tmux_sessions
        .unwrap_or_default()
        .into_iter()
        .map(|session| (session.name.clone(), session))
        .collect::<HashMap<_, _>>();

    let records = repo
        .list_worktrees()?
        .into_iter()
        .map(|worktree| {
            let details = compute_worktree_details(repo, worktree)?;
            let tmux_session = default_tmux_session_name(&details.info.name)
                .unwrap_or_else(|_| details.info.name.clone());
            let tmux_state = tmux_state_for(tmux_available, &tmux_map, &tmux_session);
            let status_labels = status_labels(&details);
            Ok(WorktreeRecord {
                details,
                tmux_session,
                tmux_state,
                status_labels,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok((records, tmux_available))
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
            .position(|record| record.details.info.name == preferred)
        {
            return index;
        }
    }

    records
        .iter()
        .position(|record| record.details.info.is_current)
        .unwrap_or(0)
}

pub fn worktree_badges(record: &WorktreeRecord) -> Vec<String> {
    let mut badges = Vec::new();

    if record.details.info.is_current {
        badges.push("current".to_string());
    }
    if record.details.info.is_main {
        badges.push("main".to_string());
    }
    if record.details.info.branch.is_none() {
        badges.push("detached".to_string());
    }
    if record.details.is_managed {
        badges.push("managed".to_string());
    } else {
        badges.push("unmanaged".to_string());
    }
    if record.details.dirty {
        badges.push("dirty".to_string());
    }
    if record.details.rebase_in_progress {
        badges.push("rebase".to_string());
    }
    if record.details.merge_in_progress {
        badges.push("merge".to_string());
    }
    if record.details.has_conflicts {
        badges.push("conflicts".to_string());
    }
    if record.details.info.is_locked {
        badges.push("locked".to_string());
    }
    if record.details.info.is_prunable {
        badges.push("prunable".to_string());
    }

    badges
}

#[cfg(test)]
mod tests {
    use super::{default_selection, worktree_badges, PendingCommand, TmuxState, WorktreeRecord};
    use crate::commands::worktree::shared::WorktreeDetails;
    use crate::git::repo::WorktreeInfo;
    use std::path::PathBuf;

    fn record(name: &str) -> WorktreeRecord {
        WorktreeRecord {
            details: WorktreeDetails {
                info: WorktreeInfo {
                    name: name.to_string(),
                    path: PathBuf::from(format!("/tmp/{}", name)),
                    branch: Some(name.to_string()),
                    is_main: false,
                    is_current: false,
                    is_locked: false,
                    lock_reason: None,
                    is_prunable: false,
                    prunable_reason: None,
                },
                branch_label: name.to_string(),
                is_managed: true,
                stack_parent: Some("main".to_string()),
                dirty: false,
                rebase_in_progress: false,
                merge_in_progress: false,
                has_conflicts: false,
                marker: None,
                ahead: Some(1),
                behind: Some(0),
            },
            tmux_session: name.to_string(),
            tmux_state: TmuxState::Missing,
            status_labels: vec!["managed".to_string()],
        }
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
        first.details.info.is_current = true;
        let second = record("beta");
        assert_eq!(default_selection(&[first, second], Some("beta")), 1);
    }

    #[test]
    fn worktree_badges_cover_unmanaged_prunable_detached_dirty_current() {
        let mut record = record("lane");
        record.details.info.is_current = true;
        record.details.info.branch = None;
        record.details.is_managed = false;
        record.details.dirty = true;
        record.details.info.is_prunable = true;

        let badges = worktree_badges(&record);
        assert!(badges.contains(&"current".to_string()));
        assert!(badges.contains(&"detached".to_string()));
        assert!(badges.contains(&"unmanaged".to_string()));
        assert!(badges.contains(&"dirty".to_string()));
        assert!(badges.contains(&"prunable".to_string()));
    }
}
