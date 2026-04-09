use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::ops::receipt::{OpKind, PlanSummary};
use crate::ops::tx::{self, Transaction};
use crate::tui::split_hunk::diff_parser::{parse_diff, reconstruct_full_patch, DiffFile};
use anyhow::{bail, Context, Result};
use ratatui::widgets::ListState;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Interaction mode for the hunk split TUI
#[derive(Debug, Clone, PartialEq)]
pub enum HunkSplitMode {
    List,
    Sequential,
    Naming,
    ConfirmAbort,
    Help,
}

/// An item in the flat navigation list (file header or individual hunk)
#[derive(Debug, Clone, Copy)]
pub enum FlatItem {
    FileHeader { file_idx: usize },
    Hunk { file_idx: usize, hunk_idx: usize },
}

#[derive(Debug, Clone)]
enum UndoAction {
    ToggleHunk {
        file_idx: usize,
        hunk_idx: usize,
        was_selected: bool,
    },
    ToggleFile {
        file_idx: usize,
        prev_states: Vec<bool>,
    },
}

/// Main application state for hunk-based split TUI
pub struct HunkSplitApp {
    pub workdir: PathBuf,
    pub original_branch: String,
    pub parent_branch: String,
    children: Vec<String>,
    stashed: bool,
    pub files: Vec<DiffFile>,
    pub selected: Vec<Vec<bool>>,
    pub flat_items: Vec<FlatItem>,
    pub cursor: usize,
    pub list_state: ListState,
    pub diff_scroll: u16,
    pub diff_line_count: u16,
    pub diff_viewport_height: u16,
    pub mode: HunkSplitMode,
    pub round: usize,
    created_branches: Vec<String>,
    undo_stack: Vec<UndoAction>,
    pub input_buffer: String,
    pub input_cursor: usize,
    pub status_message: Option<String>,
    pub should_quit: bool,
    pub round_complete: bool,
    pub all_done: bool,
    existing_branches: Vec<String>,
    no_verify: bool,
}

fn git(workdir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workdir)
        .output()
        .with_context(|| format!("Failed to run git {}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!("git {} failed: {}", args.join(" "), stderr);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_ok(workdir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(workdir)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

impl HunkSplitApp {
    /// Initialize the hunk split: validate state, flatten branch, parse diff.
    pub fn new(no_verify: bool) -> Result<Self> {
        let repo = GitRepo::open()?;
        let stack = Stack::load(&repo)?;
        let current = repo.current_branch()?;

        if current == stack.trunk {
            bail!("Cannot split trunk branch.");
        }

        let branch_info = stack
            .branches
            .get(&current)
            .context("Branch is not tracked.")?;

        let parent = branch_info
            .parent
            .clone()
            .context("Branch has no parent.")?;

        let children: Vec<String> = stack
            .branches
            .values()
            .filter(|b| b.parent.as_deref() == Some(&current))
            .map(|b| b.name.clone())
            .collect();

        let workdir = repo.workdir()?.to_path_buf();
        let existing_branches = repo.list_branches()?;

        let stashed = repo.is_dirty()?;
        if stashed {
            git(&workdir, &["add", "-A"])?;
            let mut commit_args = vec!["commit", "-m", "WIP: uncommitted changes for split"];
            if no_verify {
                commit_args.push("--no-verify");
            }
            git(&workdir, &commit_args)?;
        }

        let parent_sha = repo.merge_base(&parent, &current)?;
        drop(repo);

        let tip = git(&workdir, &["rev-parse", "HEAD"])?;
        git(&workdir, &["switch", "-d", &tip])?;

        let files = match (|| -> Result<Vec<DiffFile>> {
            git(&workdir, &["reset", "-Nq", &parent_sha])?;
            let diff_output = git(&workdir, &["diff"])?;
            let files = parse_diff(&diff_output);
            if files.is_empty() {
                bail!("No diff hunks found between parent and branch tip.");
            }
            Ok(files)
        })() {
            Ok(files) => files,
            Err(e) => {
                let _ = git(&workdir, &["checkout", "-f", &current]);
                return Err(e);
            }
        };

        let selected: Vec<Vec<bool>> = files.iter().map(|f| vec![false; f.hunks.len()]).collect();
        let flat_items = build_flat_items(&files);

        Ok(Self {
            workdir,
            original_branch: current,
            parent_branch: parent,
            children,
            stashed,
            files,
            selected,
            flat_items,
            cursor: 0,
            list_state: ListState::default(),
            diff_scroll: 0,
            diff_line_count: 0,
            diff_viewport_height: 0,
            mode: HunkSplitMode::List,
            round: 1,
            created_branches: Vec::new(),
            undo_stack: Vec::new(),
            input_buffer: String::new(),
            input_cursor: 0,
            status_message: None,
            should_quit: false,
            round_complete: false,
            all_done: false,
            existing_branches,
            no_verify,
        })
    }

    /// Get the item at the current cursor position
    pub fn current_item(&self) -> Option<&FlatItem> {
        self.flat_items.get(self.cursor)
    }

    /// Move cursor up one item
    pub fn move_cursor_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
        self.diff_scroll = 0;
    }

    /// Move cursor down one item
    pub fn move_cursor_down(&mut self) {
        if self.cursor < self.flat_items.len().saturating_sub(1) {
            self.cursor += 1;
        }
        self.diff_scroll = 0;
    }

    /// Toggle selection of the hunk at cursor
    pub fn toggle_current(&mut self) {
        if let Some(FlatItem::Hunk { file_idx, hunk_idx }) = self.current_item().copied() {
            let was = self.selected[file_idx][hunk_idx];
            self.selected[file_idx][hunk_idx] = !was;
            self.undo_stack.push(UndoAction::ToggleHunk {
                file_idx,
                hunk_idx,
                was_selected: was,
            });
        }
    }

    /// Toggle all hunks in the file at cursor
    pub fn toggle_file(&mut self) {
        let file_idx = match self.current_item() {
            Some(FlatItem::FileHeader { file_idx }) => *file_idx,
            Some(FlatItem::Hunk { file_idx, .. }) => *file_idx,
            None => return,
        };

        let prev_states = self.selected[file_idx].clone();
        let all_selected = prev_states.iter().all(|&s| s);
        let new_val = !all_selected;
        for s in &mut self.selected[file_idx] {
            *s = new_val;
        }
        self.undo_stack.push(UndoAction::ToggleFile {
            file_idx,
            prev_states,
        });
    }

    /// Undo the last toggle operation
    pub fn undo(&mut self) {
        if let Some(action) = self.undo_stack.pop() {
            match action {
                UndoAction::ToggleHunk {
                    file_idx,
                    hunk_idx,
                    was_selected,
                } => {
                    self.selected[file_idx][hunk_idx] = was_selected;
                }
                UndoAction::ToggleFile {
                    file_idx,
                    prev_states,
                } => {
                    self.selected[file_idx] = prev_states;
                }
            }
        }
    }

    pub fn scroll_diff_down(&mut self) {
        let max = self
            .diff_line_count
            .saturating_sub(self.diff_viewport_height);
        if self.diff_scroll < max {
            self.diff_scroll += 1;
        }
    }

    pub fn scroll_diff_up(&mut self) {
        if self.diff_scroll > 0 {
            self.diff_scroll -= 1;
        }
    }

    /// Select the current hunk and advance to the next (sequential mode)
    pub fn accept_and_advance(&mut self) {
        if let Some(FlatItem::Hunk { file_idx, hunk_idx }) = self.current_item().copied() {
            if !self.selected[file_idx][hunk_idx] {
                self.selected[file_idx][hunk_idx] = true;
                self.undo_stack.push(UndoAction::ToggleHunk {
                    file_idx,
                    hunk_idx,
                    was_selected: false,
                });
            }
        }
        self.advance_to_next_hunk();
        self.diff_scroll = 0;
    }

    /// Skip the current hunk and advance to the next (sequential mode)
    pub fn skip_and_advance(&mut self) {
        if let Some(FlatItem::Hunk { file_idx, hunk_idx }) = self.current_item().copied() {
            if self.selected[file_idx][hunk_idx] {
                self.selected[file_idx][hunk_idx] = false;
                self.undo_stack.push(UndoAction::ToggleHunk {
                    file_idx,
                    hunk_idx,
                    was_selected: true,
                });
            }
        }
        self.advance_to_next_hunk();
        self.diff_scroll = 0;
    }

    fn advance_to_next_hunk(&mut self) {
        let start = self.cursor + 1;
        for i in start..self.flat_items.len() {
            if matches!(self.flat_items[i], FlatItem::Hunk { .. }) {
                self.cursor = i;
                return;
            }
        }
        if self.selected_count() > 0 {
            self.mode = HunkSplitMode::Naming;
            self.input_buffer = self.suggest_branch_name();
            self.input_cursor = self.input_buffer.len();
        }
    }

    /// Advance cursor past all hunks in the current file
    pub fn advance_past_current_file(&mut self) {
        let current_file_idx = match self.current_item() {
            Some(FlatItem::FileHeader { file_idx }) => *file_idx,
            Some(FlatItem::Hunk { file_idx, .. }) => *file_idx,
            None => return,
        };

        let start = self.cursor + 1;
        for i in start..self.flat_items.len() {
            let item_file_idx = match self.flat_items[i] {
                FlatItem::FileHeader { file_idx } => file_idx,
                FlatItem::Hunk { file_idx, .. } => file_idx,
            };
            if item_file_idx != current_file_idx {
                self.cursor = i;
                return;
            }
        }
        if self.selected_count() > 0 {
            self.mode = HunkSplitMode::Naming;
            self.input_buffer = self.suggest_branch_name();
            self.input_cursor = self.input_buffer.len();
        }
    }

    /// Count of currently selected hunks
    pub fn selected_count(&self) -> usize {
        self.selected
            .iter()
            .flat_map(|v| v.iter())
            .filter(|&&s| s)
            .count()
    }

    /// Total number of hunks across all files
    pub fn total_hunk_count(&self) -> usize {
        self.selected.iter().map(|v| v.len()).sum()
    }

    /// Auto-suggest a branch name for the current round
    pub fn suggest_branch_name(&self) -> String {
        if self.selected_count() == self.total_hunk_count() && !self.created_branches.is_empty() {
            return self.original_branch.clone();
        }
        format!("{}_split_{}", self.original_branch, self.round)
    }

    /// Validate a proposed branch name
    pub fn validate_branch_name(&self, name: &str) -> Result<(), String> {
        if name.trim().is_empty() {
            return Err("Branch name cannot be empty".to_string());
        }
        if name != self.original_branch
            && (self.existing_branches.iter().any(|b| b == name)
                || self.created_branches.iter().any(|b| b == name))
        {
            return Err(format!("Branch '{}' already exists", name));
        }
        Ok(())
    }

    /// Stage and commit the selected hunks for this round.
    /// Returns `Ok(true)` if there are remaining hunks for another round.
    pub fn commit_round(&mut self, branch_name: &str) -> Result<bool> {
        let selected_count = self.selected_count();
        let total_count = self.total_hunk_count();

        let mut selections: Vec<(usize, Vec<usize>)> = Vec::new();
        for (fi, file_sel) in self.selected.iter().enumerate() {
            let hunks: Vec<usize> = file_sel
                .iter()
                .enumerate()
                .filter(|(_, &s)| s)
                .map(|(hi, _)| hi)
                .collect();
            if !hunks.is_empty() {
                selections.push((fi, hunks));
            }
        }

        if selections.is_empty() {
            bail!("No hunks selected");
        }

        let patch = reconstruct_full_patch(&self.files, &selections);

        let debug = std::env::var("STAX_SPLIT_DEBUG").is_ok();
        let debug_log_path = std::env::temp_dir().join("stax-split-debug.log");
        if debug {
            let mut log = if self.round == 1 {
                String::new()
            } else {
                std::fs::read_to_string(&debug_log_path).unwrap_or_default()
            };
            log.push_str(&format!(
                "=== Round {} commit ===\nBranch: {}\nSelected: {}/{}\nSelections: {:?}\n",
                self.round, branch_name, selected_count, total_count, selections,
            ));
            log.push_str(&format!("--- patch ---\n{}\n--- end patch ---\n", patch));
            let _ = std::fs::write(&debug_log_path, &log);
        }

        git(&self.workdir, &["reset"])?;

        let tmpdir = tempfile::tempdir()?;
        let patch_path = tmpdir.path().join("split.patch");
        std::fs::write(&patch_path, &patch)?;

        let patch_str = patch_path.to_string_lossy().to_string();
        git(&self.workdir, &["apply", "--cached", &patch_str])?;
        let mut commit_args = vec!["commit", "-m", branch_name];
        if self.no_verify {
            commit_args.push("--no-verify");
        }
        git(&self.workdir, &commit_args)?;

        self.created_branches.push(branch_name.to_string());
        if branch_name != self.original_branch {
            self.existing_branches.push(branch_name.to_string());
        }

        git(&self.workdir, &["add", "-N", "."])?;
        let diff_output = git(&self.workdir, &["diff"])?;
        let files = parse_diff(&diff_output);
        let has_remaining = !files.is_empty();

        if debug {
            let mut log = std::fs::read_to_string(&debug_log_path).unwrap_or_default();
            log.push_str(&format!(
                "--- post-commit diff ({} files remaining) ---\n{}\n--- end diff ---\n",
                files.len(),
                if diff_output.is_empty() {
                    "(empty)".to_string()
                } else {
                    diff_output.clone()
                },
            ));
            if selected_count < total_count && !has_remaining {
                log.push_str("!!! BUG: partial selection but no remaining hunks !!!\n");
            }
            let _ = std::fs::write(&debug_log_path, &log);
        }

        if has_remaining {
            self.selected = files.iter().map(|f| vec![false; f.hunks.len()]).collect();
            self.files = files;
            self.flat_items = build_flat_items(&self.files);
            self.cursor = 0;
            self.diff_scroll = 0;
            self.undo_stack.clear();
            self.round += 1;
        } else {
            self.files = files;
        }

        Ok(has_remaining)
    }

    /// Create branch pointers and stax metadata for the split branches.
    /// Uses the transaction system for undo support and crash recovery.
    pub fn finalize(&mut self) -> Result<()> {
        let split_tip = git(&self.workdir, &["rev-parse", "HEAD"])?;
        git(&self.workdir, &["checkout", &self.parent_branch])?;
        let repo = GitRepo::open_from_path(&self.workdir)?;

        let mut affected: Vec<String> = self.created_branches.clone();
        affected.push(self.original_branch.clone());

        let mut tx = Transaction::begin(OpKind::Split, &repo, false)?;
        tx.plan_branches(&repo, &affected)?;

        let summary = PlanSummary {
            branches_to_rebase: 0,
            branches_to_push: 0,
            description: vec![format!(
                "Hunk split into {} new branches",
                self.created_branches.len()
            )],
        };
        tx::print_plan(tx.kind(), &summary, false);
        tx.set_plan_summary(summary);

        let num_branches = self.created_branches.len();
        for (i, name) in self.created_branches.iter().enumerate() {
            let offset = num_branches - 1 - i;
            let rev = format!("{}~{}", split_tip, offset);
            if name == &self.original_branch {
                git(&self.workdir, &["branch", "-f", name, &rev])?;
            } else {
                git(&self.workdir, &["branch", name, &rev])?;
            }
        }

        let mut prev_parent = self.parent_branch.clone();
        for name in &self.created_branches {
            let parent_rev = repo.branch_commit(&prev_parent)?;
            let meta = BranchMetadata::new(&prev_parent, &parent_rev);
            meta.write(repo.inner(), name)?;
            prev_parent = name.clone();
        }

        let original_reused = self.created_branches.contains(&self.original_branch);

        for child in &self.children {
            let last_branch = self
                .created_branches
                .last()
                .expect("at least one branch created");
            if let Some(mut meta) = BranchMetadata::read(repo.inner(), child)? {
                let parent_rev = repo.branch_commit(last_branch)?;
                meta.parent_branch_name = last_branch.clone();
                meta.parent_branch_revision = parent_rev;
                meta.write(repo.inner(), child)?;
            }
        }

        if !original_reused && git_ok(&self.workdir, &["branch", "-D", &self.original_branch]) {
            let _ = BranchMetadata::delete(repo.inner(), &self.original_branch);
        }

        let checkout_target = self
            .created_branches
            .last()
            .expect("at least one branch created")
            .clone();
        git(&self.workdir, &["checkout", &checkout_target])?;

        tx.snapshot()?;
        tx.finish_ok()?;

        Ok(())
    }

    /// Rollback: restore the original branch state
    pub fn rollback(&self) {
        let _ = git(&self.workdir, &["checkout", "-f", &self.original_branch]);
        let repo = GitRepo::open_from_path(&self.workdir).ok();
        for name in &self.created_branches {
            if name != &self.original_branch {
                let _ = git(&self.workdir, &["branch", "-D", name]);
                if let Some(ref repo) = repo {
                    let _ = BranchMetadata::delete(repo.inner(), name);
                }
            }
        }
        if self.stashed {
            let _ = git(&self.workdir, &["reset", "HEAD~1"]);
        }
    }

    /// Count of selected hunks in a specific file
    pub fn file_selected_count(&self, file_idx: usize) -> usize {
        self.selected[file_idx].iter().filter(|&&s| s).count()
    }

    /// Total hunk count for a specific file
    pub fn file_hunk_count(&self, file_idx: usize) -> usize {
        self.selected[file_idx].len()
    }
}

fn build_flat_items(files: &[DiffFile]) -> Vec<FlatItem> {
    let mut items = Vec::new();
    for (fi, file) in files.iter().enumerate() {
        items.push(FlatItem::FileHeader { file_idx: fi });
        for hi in 0..file.hunks.len() {
            items.push(FlatItem::Hunk {
                file_idx: fi,
                hunk_idx: hi,
            });
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::split_hunk::diff_parser::DiffHunk;

    fn make_hunk(start: u32, count: u32) -> DiffHunk {
        DiffHunk {
            header: format!("@@ -1,1 +{},{} @@", start, count),
            lines: vec![format!("+line at {}", start)],
            new_start: start,
            new_count: count,
        }
    }

    fn make_file(path: &str, num_hunks: usize) -> DiffFile {
        DiffFile {
            path: path.to_string(),
            header_lines: vec![format!("diff --git a/{path} b/{path}")],
            is_new: false,
            is_deleted: false,
            hunks: (0..num_hunks)
                .map(|i| make_hunk((i * 10 + 1) as u32, 3))
                .collect(),
        }
    }

    #[test]
    fn test_build_flat_items_structure() {
        let files = vec![make_file("a.rs", 2), make_file("b.rs", 1)];
        let items = build_flat_items(&files);

        assert_eq!(items.len(), 5);
        assert!(matches!(items[0], FlatItem::FileHeader { file_idx: 0 }));
        assert!(matches!(
            items[1],
            FlatItem::Hunk {
                file_idx: 0,
                hunk_idx: 0
            }
        ));
        assert!(matches!(
            items[2],
            FlatItem::Hunk {
                file_idx: 0,
                hunk_idx: 1
            }
        ));
        assert!(matches!(items[3], FlatItem::FileHeader { file_idx: 1 }));
        assert!(matches!(
            items[4],
            FlatItem::Hunk {
                file_idx: 1,
                hunk_idx: 0
            }
        ));
    }

    #[test]
    fn test_build_flat_items_empty() {
        let items = build_flat_items(&[]);
        assert!(items.is_empty());
    }
}
