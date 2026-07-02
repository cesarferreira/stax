use crate::commands::ready::{ReadyBranch, ReadyRowState};
use std::collections::HashMap;

#[derive(Debug)]
pub enum ReadyTuiUpdate {
    Loaded {
        index: usize,
        row: crate::commands::ready::PrReadinessRow,
    },
    Unavailable {
        index: usize,
        branch: ReadyBranch,
        message: String,
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
            ReadyTuiUpdate::Loaded { index, row } => {
                if let Some(slot) = self.row_slot_mut(index, &row.branch) {
                    *slot = ReadyRowState::Loaded(row);
                }
                self.sort_rows_by_updated_at();
            }
            ReadyTuiUpdate::Unavailable {
                index,
                branch,
                message,
            } => {
                let branch_name = branch.name.clone();
                if let Some(slot) = self.row_slot_mut(index, &branch_name) {
                    *slot = ReadyRowState::Unavailable { branch, message };
                }
                self.sort_rows_by_updated_at();
            }
            ReadyTuiUpdate::Done => {
                self.loading = false;
            }
        }
    }

    fn row_slot_mut(&mut self, fallback_index: usize, branch: &str) -> Option<&mut ReadyRowState> {
        let index = self
            .rows
            .iter()
            .position(|row| row.branch() == branch)
            .unwrap_or(fallback_index);
        self.rows.get_mut(index)
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

        if let Some(selected_branch) = selected_branch {
            if let Some(index) = self
                .rows
                .iter()
                .position(|row| row.branch() == selected_branch)
            {
                self.selected_index = index;
            }
        }
    }

    pub fn reset_for_refresh(&mut self) {
        self.rows = self
            .rows
            .iter()
            .map(|row| ReadyRowState::Loading {
                branch: ReadyBranch {
                    name: row.branch().to_string(),
                    pr_number: row.pr_number(),
                },
            })
            .collect();
        self.loading = true;
        self.status_message = Some("Refreshing PR readiness...".to_string());
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

    pub fn selected_row(&self) -> Option<&ReadyRowState> {
        self.rows.get(self.selected_index)
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
            review_summary: "1 approval".to_string(),
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
            index: 1,
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

        app.apply_update(ReadyTuiUpdate::Loaded {
            index: 0,
            row: older,
        });
        app.apply_update(ReadyTuiUpdate::Loaded {
            index: 1,
            row: newer,
        });

        assert_eq!(app.rows[0].branch(), "feature/b");
        assert_eq!(app.rows[1].branch(), "feature/a");
    }

    #[test]
    fn ready_tui_selected_pr_url_comes_from_selected_loaded_row() {
        let mut app = ReadyTuiApp::new_for_test("owner/repo", "current stack", branches());
        app.apply_update(ReadyTuiUpdate::Loaded {
            index: 0,
            row: loaded_row("feature/a", 10),
        });

        assert_eq!(
            app.selected_pr_url(),
            Some("https://example.com/pull/10".to_string())
        );
    }

    #[test]
    fn ready_tui_refresh_resets_rows_to_loading() {
        let mut app = ReadyTuiApp::new_for_test("owner/repo", "current stack", branches());
        app.apply_update(ReadyTuiUpdate::Loaded {
            index: 0,
            row: loaded_row("feature/a", 10),
        });

        app.reset_for_refresh();

        assert!(
            app.rows
                .iter()
                .all(|row| matches!(row, ReadyRowState::Loading { .. }))
        );
    }
}
