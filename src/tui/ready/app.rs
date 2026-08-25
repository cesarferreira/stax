use crate::commands::ready::{ReadyBranch, ReadyRowState};
use std::collections::HashMap;

#[derive(Debug)]
pub enum ReadyTuiUpdate {
    Loaded {
        row: crate::commands::ready::PrReadinessRow,
    },
    Unavailable {
        branch: ReadyBranch,
        message: String,
    },
    Merged {
        branch: String,
    },
    Done,
}

pub struct ReadyTuiApp {
    pub repo_label: String,
    pub scope_label: String,
    pub rows: Vec<ReadyRowState>,
    branch_order: HashMap<String, usize>,
    pub selected_index: usize,
    pub status_message: Option<String>,
    pub show_help: bool,
    pub should_quit: bool,
    pub loading: bool,
}

impl ReadyTuiApp {
    #[cfg(test)]
    pub fn new_for_test(repo_label: &str, scope_label: &str, branches: Vec<ReadyBranch>) -> Self {
        Self::from_parts(repo_label.to_string(), scope_label.to_string(), branches)
    }

    pub fn from_parts(repo_label: String, scope_label: String, branches: Vec<ReadyBranch>) -> Self {
        let branch_order = branches
            .iter()
            .enumerate()
            .map(|(index, branch)| (branch.name.clone(), index))
            .collect::<HashMap<_, _>>();
        let rows = branches
            .into_iter()
            .map(|branch| ReadyRowState::Loading { branch })
            .collect::<Vec<_>>();

        Self {
            repo_label,
            scope_label,
            rows,
            branch_order,
            selected_index: 0,
            status_message: None,
            show_help: false,
            should_quit: false,
            loading: true,
        }
    }

    pub fn select_previous(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn select_next(&mut self) {
        if self.selected_index + 1 < self.rows.len() {
            self.selected_index += 1;
        }
    }

    pub fn apply_update(&mut self, update: ReadyTuiUpdate) {
        match update {
            ReadyTuiUpdate::Loaded { row } => {
                if let Some(slot) = self.row_slot_mut(&row.branch) {
                    *slot = ReadyRowState::Loaded(row);
                }
                if self.should_resort_rows() {
                    self.sort_rows_by_updated_at();
                }
            }
            ReadyTuiUpdate::Unavailable { branch, message } => {
                let branch_name = branch.name.clone();
                if let Some(slot) = self.row_slot_mut(&branch_name) {
                    *slot = ReadyRowState::Unavailable { branch, message };
                }
                if self.should_resort_rows() {
                    self.sort_rows_by_updated_at();
                }
            }
            ReadyTuiUpdate::Merged { branch } => self.remove_row(&branch),
            ReadyTuiUpdate::Done => {
                self.loading = false;
                self.sort_rows_by_updated_at();
            }
        }
    }

    fn should_resort_rows(&self) -> bool {
        !self.loading
            || self
                .rows
                .iter()
                .all(|row| matches!(row, ReadyRowState::Loading { .. }))
    }

    fn row_slot_mut(&mut self, branch: &str) -> Option<&mut ReadyRowState> {
        self.rows.iter_mut().find(|row| row.branch() == branch)
    }

    fn remove_row(&mut self, branch: &str) {
        let selected_branch = self
            .rows
            .get(self.selected_index)
            .map(|row| row.branch().to_string());
        let Some(index) = self.rows.iter().position(|row| row.branch() == branch) else {
            return;
        };

        self.rows.remove(index);
        self.selected_index = selected_branch
            .filter(|selected| selected != branch)
            .and_then(|selected| self.rows.iter().position(|row| row.branch() == selected))
            .unwrap_or(index.min(self.rows.len().saturating_sub(1)));
    }

    fn sort_rows_by_updated_at(&mut self) {
        let selected_branch = self
            .rows
            .get(self.selected_index)
            .map(|row| row.branch().to_string());

        self.rows.sort_by(|a, b| match (a, b) {
            (ReadyRowState::Loaded(a), ReadyRowState::Loaded(b)) => {
                b.updated_at.cmp(&a.updated_at).then_with(|| {
                    self.branch_order
                        .get(a.branch.as_str())
                        .copied()
                        .unwrap_or(usize::MAX)
                        .cmp(
                            &self
                                .branch_order
                                .get(b.branch.as_str())
                                .copied()
                                .unwrap_or(usize::MAX),
                        )
                })
            }
            (ReadyRowState::Loaded(_), _) => std::cmp::Ordering::Less,
            (_, ReadyRowState::Loaded(_)) => std::cmp::Ordering::Greater,
            _ => self
                .branch_order
                .get(a.branch())
                .copied()
                .unwrap_or(usize::MAX)
                .cmp(
                    &self
                        .branch_order
                        .get(b.branch())
                        .copied()
                        .unwrap_or(usize::MAX),
                ),
        });

        if let Some(selected_branch) = selected_branch
            && let Some(index) = self
                .rows
                .iter()
                .position(|row| row.branch() == selected_branch)
        {
            self.selected_index = index;
        }
    }

    pub fn begin_refresh(&mut self) {
        self.loading = true;
    }

    pub fn reconcile_scope(&mut self, branches: &[ReadyBranch]) {
        let selected_branch = self
            .rows
            .get(self.selected_index)
            .map(|row| row.branch().to_string());

        let mut existing: HashMap<String, ReadyRowState> = self
            .rows
            .drain(..)
            .map(|row| (row.branch().to_string(), row))
            .collect();

        self.branch_order = branches
            .iter()
            .enumerate()
            .map(|(index, branch)| (branch.name.clone(), index))
            .collect::<HashMap<_, _>>();

        self.rows = branches
            .iter()
            .map(|branch| {
                existing
                    .remove(&branch.name)
                    .unwrap_or_else(|| ReadyRowState::Loading {
                        branch: branch.clone(),
                    })
            })
            .collect();

        self.selected_index = selected_branch
            .and_then(|name| self.rows.iter().position(|row| row.branch() == name))
            .unwrap_or(0)
            .min(self.rows.len().saturating_sub(1));
    }

    pub fn loading_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| matches!(row, ReadyRowState::Loading { .. }))
            .count()
    }

    pub fn selected_pr_url(&self) -> Option<String> {
        match self.rows.get(self.selected_index) {
            Some(ReadyRowState::Loaded(row)) => row.pr_url.clone(),
            _ => None,
        }
    }

    /// `(pr_number, branch, is_draft)` for the selected row, if it has finished loading.
    pub fn selected_draft_target(&self) -> Option<(u64, String, bool)> {
        match self.rows.get(self.selected_index) {
            Some(ReadyRowState::Loaded(row)) => {
                Some((row.pr_number, row.branch.clone(), row.is_draft))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::ready::{
        PrReadinessRow, ReadyAction, ReadyBranch, ReadyReason, ReadyRowState,
    };

    fn branches() -> Vec<ReadyBranch> {
        vec![
            ReadyBranch {
                name: "feature/a".to_string(),
                pr_number: Some(10),
            },
            ReadyBranch {
                name: "feature/b".to_string(),
                pr_number: Some(11),
            },
        ]
    }

    fn loaded_row(branch: &str, pr_number: u64) -> PrReadinessRow {
        PrReadinessRow {
            branch: branch.to_string(),
            pr_number,
            title: "Ready PR".to_string(),
            updated_at: Some("2026-06-01T10:00:00Z".to_string()),
            action: ReadyAction::Merge,
            reason: ReadyReason::Ready,
            review_decision: Some("APPROVED".to_string()),
            approvals: 1,
            changes_requested: false,
            ci_status: "success".to_string(),
            ci_summary: "passed".to_string(),
            is_draft: false,
            mergeable: Some(true),
            mergeable_state: "clean".to_string(),
            review_summary: "approved".to_string(),
            pr_url: Some(format!("https://example.com/pull/{pr_number}")),
            pr_state: "open".to_string(),
        }
    }

    #[test]
    fn ready_tui_initializes_rows_as_loading_placeholders() {
        let app = ReadyTuiApp::new_for_test("owner/repo", "current stack", branches());

        assert_eq!(app.rows.len(), 2);
        assert_eq!(app.selected_index, 0);
        assert!(matches!(app.rows[0], ReadyRowState::Loading { .. }));
    }

    #[test]
    fn ready_tui_selection_stays_within_bounds() {
        let mut app = ReadyTuiApp::new_for_test("owner/repo", "current stack", branches());

        app.select_previous();
        assert_eq!(app.selected_index, 0);
        app.select_next();
        assert_eq!(app.selected_index, 1);
        app.select_next();
        assert_eq!(app.selected_index, 1);
    }

    #[test]
    fn ready_tui_applies_loaded_row_update() {
        let mut app = ReadyTuiApp::new_for_test("owner/repo", "current stack", branches());

        app.apply_update(ReadyTuiUpdate::Loaded {
            row: loaded_row("feature/b", 11),
        });

        let loaded = app
            .rows
            .iter()
            .find(|row| row.branch() == "feature/b")
            .expect("feature/b row");
        match loaded {
            ReadyRowState::Loaded(row) => assert_eq!(row.branch, "feature/b"),
            other => panic!("expected loaded row, got {other:?}"),
        }
    }

    #[test]
    fn ready_tui_sorts_loaded_rows_by_updated_at_newest_first() {
        let mut app = ReadyTuiApp::new_for_test("owner/repo", "current stack", branches());
        let mut older = loaded_row("feature/a", 10);
        older.updated_at = Some("2026-06-01T10:00:00Z".to_string());
        let mut newer = loaded_row("feature/b", 11);
        newer.updated_at = Some("2026-06-02T10:00:00Z".to_string());

        app.apply_update(ReadyTuiUpdate::Loaded { row: older });
        app.apply_update(ReadyTuiUpdate::Loaded { row: newer });
        app.apply_update(ReadyTuiUpdate::Done);

        assert_eq!(app.rows[0].branch(), "feature/b");
        assert_eq!(app.rows[1].branch(), "feature/a");
    }

    #[test]
    fn ready_tui_selected_pr_url_comes_from_selected_loaded_row() {
        let mut app = ReadyTuiApp::new_for_test("owner/repo", "current stack", branches());
        app.apply_update(ReadyTuiUpdate::Loaded {
            row: loaded_row("feature/a", 10),
        });

        assert_eq!(
            app.selected_pr_url(),
            Some("https://example.com/pull/10".to_string())
        );
    }

    #[test]
    fn ready_tui_selected_draft_target_reflects_loaded_row() {
        let mut app = ReadyTuiApp::new_for_test("owner/repo", "current stack", branches());

        assert_eq!(app.selected_draft_target(), None);

        app.apply_update(ReadyTuiUpdate::Loaded {
            row: loaded_row("feature/a", 10),
        });

        assert_eq!(
            app.selected_draft_target(),
            Some((10, "feature/a".to_string(), false))
        );
    }

    #[test]
    fn ready_tui_reconcile_scope_drops_removed_branch_and_keeps_loaded_row() {
        let mut app = ReadyTuiApp::new_for_test("owner/repo", "current stack", branches());
        app.apply_update(ReadyTuiUpdate::Loaded {
            row: loaded_row("feature/a", 10),
        });

        app.reconcile_scope(&[ReadyBranch {
            name: "feature/a".to_string(),
            pr_number: Some(10),
        }]);

        assert_eq!(app.rows.len(), 1);
        match &app.rows[0] {
            ReadyRowState::Loaded(row) => assert_eq!(row.branch, "feature/a"),
            other => panic!("expected loaded row, got {other:?}"),
        }
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn ready_tui_reconcile_scope_inserts_loading_row_for_new_branch() {
        let mut app = ReadyTuiApp::new_for_test("owner/repo", "current stack", branches());
        app.apply_update(ReadyTuiUpdate::Loaded {
            row: loaded_row("feature/a", 10),
        });

        app.reconcile_scope(&[
            ReadyBranch {
                name: "feature/a".to_string(),
                pr_number: Some(10),
            },
            ReadyBranch {
                name: "feature/c".to_string(),
                pr_number: Some(12),
            },
        ]);

        assert_eq!(app.rows.len(), 2);
        assert!(matches!(app.rows[0], ReadyRowState::Loaded(_)));
        match &app.rows[1] {
            ReadyRowState::Loading { branch } => assert_eq!(branch.name, "feature/c"),
            other => panic!("expected loading row, got {other:?}"),
        }
    }

    #[test]
    fn ready_tui_refresh_keeps_loaded_rows_visible() {
        let mut app = ReadyTuiApp::new_for_test("owner/repo", "current stack", branches());
        app.apply_update(ReadyTuiUpdate::Loaded {
            row: loaded_row("feature/a", 10),
        });
        app.loading = false;

        app.begin_refresh();

        assert!(app.loading);
        assert!(matches!(app.rows[0], ReadyRowState::Loaded(_)));
    }

    #[test]
    fn ready_tui_removes_initial_loading_placeholder_for_merged_pr() {
        let mut app = ReadyTuiApp::new_for_test("owner/repo", "current stack", branches());

        app.apply_update(ReadyTuiUpdate::Merged {
            branch: "feature/a".to_string(),
        });

        assert_eq!(app.rows.len(), 1);
        assert_eq!(app.rows[0].branch(), "feature/b");
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn ready_tui_refresh_removes_previously_loaded_merged_pr() {
        let mut app = ReadyTuiApp::new_for_test("owner/repo", "current stack", branches());
        app.apply_update(ReadyTuiUpdate::Loaded {
            row: loaded_row("feature/a", 10),
        });
        app.apply_update(ReadyTuiUpdate::Done);
        app.begin_refresh();

        app.apply_update(ReadyTuiUpdate::Merged {
            branch: "feature/a".to_string(),
        });
        app.apply_update(ReadyTuiUpdate::Done);

        assert_eq!(app.rows.len(), 1);
        assert_eq!(app.rows[0].branch(), "feature/b");
        assert!(!app.loading);
    }

    #[test]
    fn ready_tui_out_of_order_updates_use_branch_identity_after_removal() {
        let mut app = ReadyTuiApp::new_for_test("owner/repo", "current stack", branches());

        app.apply_update(ReadyTuiUpdate::Merged {
            branch: "feature/a".to_string(),
        });
        app.apply_update(ReadyTuiUpdate::Loaded {
            row: loaded_row("feature/b", 11),
        });

        assert_eq!(app.rows.len(), 1);
        assert!(matches!(
            &app.rows[0],
            ReadyRowState::Loaded(row) if row.branch == "feature/b"
        ));
    }

    #[test]
    fn ready_tui_removal_preserves_other_selection_and_clamps_selected_last_row() {
        let mut app = ReadyTuiApp::new_for_test("owner/repo", "current stack", branches());
        app.selected_index = 1;

        app.apply_update(ReadyTuiUpdate::Merged {
            branch: "feature/a".to_string(),
        });
        assert_eq!(app.selected_index, 0);
        assert_eq!(app.rows[0].branch(), "feature/b");

        app.apply_update(ReadyTuiUpdate::Merged {
            branch: "feature/b".to_string(),
        });
        assert!(app.rows.is_empty());
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn ready_tui_unavailable_update_remains_visible() {
        let mut app = ReadyTuiApp::new_for_test("owner/repo", "current stack", branches());

        app.apply_update(ReadyTuiUpdate::Unavailable {
            branch: branches()[0].clone(),
            message: "forge unavailable".to_string(),
        });

        assert!(matches!(
            &app.rows[0],
            ReadyRowState::Unavailable { message, .. } if message == "forge unavailable"
        ));
    }
}
