use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::tui::split_hunk::diff_parser::{parse_diff, reconstruct_full_patch, DiffFile};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub enum HunkSplitMode {
    List,
    Sequential,
    Naming,
    ConfirmAbort,
    Help,
}

#[derive(Debug, Clone)]
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
    pub fn new() -> Result<Self> {
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

        let stashed = has_dirty_workdir(&workdir);
        if stashed {
            git(&workdir, &["add", "-A"])?;
            git(
                &workdir,
                &["commit", "-m", "WIP: uncommitted changes for split"],
            )?;
        }

        let parent_sha = repo.branch_commit(&parent)?;
        drop(repo);
        drop(stack);

        let tip = git(&workdir, &["rev-parse", "HEAD"])?;
        git(&workdir, &["switch", "-d", &tip])?;
        git(&workdir, &["reset", "-Nq", &parent_sha])?;

        let diff_output = git(&workdir, &["diff"])?;
        let files = parse_diff(&diff_output);

        if files.is_empty() {
            git(&workdir, &["checkout", "-f", &current])?;
            bail!("No diff hunks found between parent and branch tip.");
        }

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
        })
    }

    pub fn current_item(&self) -> Option<&FlatItem> {
        self.flat_items.get(self.cursor)
    }

    pub fn move_cursor_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_cursor_down(&mut self) {
        if self.cursor < self.flat_items.len().saturating_sub(1) {
            self.cursor += 1;
        }
    }

    pub fn toggle_current(&mut self) {
        if let Some(FlatItem::Hunk { file_idx, hunk_idx }) = self.current_item().cloned() {
            let was = self.selected[file_idx][hunk_idx];
            self.selected[file_idx][hunk_idx] = !was;
            self.undo_stack.push(UndoAction::ToggleHunk {
                file_idx,
                hunk_idx,
                was_selected: was,
            });
        }
    }

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

    pub fn accept_and_advance(&mut self) {
        if let Some(FlatItem::Hunk { file_idx, hunk_idx }) = self.current_item().cloned() {
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
    }

    pub fn skip_and_advance(&mut self) {
        if let Some(FlatItem::Hunk { file_idx, hunk_idx }) = self.current_item().cloned() {
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

    pub fn advance_past_current_file(&mut self) {
        let current_file_idx = match self.current_item() {
            Some(FlatItem::FileHeader { file_idx }) => *file_idx,
            Some(FlatItem::Hunk { file_idx, .. }) => *file_idx,
            None => return,
        };

        let start = self.cursor + 1;
        for i in start..self.flat_items.len() {
            match &self.flat_items[i] {
                FlatItem::FileHeader { file_idx } if *file_idx != current_file_idx => {
                    self.cursor = i;
                    return;
                }
                FlatItem::Hunk { file_idx, .. } if *file_idx != current_file_idx => {
                    self.cursor = i;
                    return;
                }
                _ => continue,
            }
        }
        if self.selected_count() > 0 {
            self.mode = HunkSplitMode::Naming;
            self.input_buffer = self.suggest_branch_name();
            self.input_cursor = self.input_buffer.len();
        }
    }

    pub fn selected_count(&self) -> usize {
        self.selected
            .iter()
            .flat_map(|v| v.iter())
            .filter(|&&s| s)
            .count()
    }

    pub fn total_hunk_count(&self) -> usize {
        self.selected.iter().map(|v| v.len()).sum()
    }

    pub fn suggest_branch_name(&self) -> String {
        if self.selected_count() == self.total_hunk_count() && !self.created_branches.is_empty() {
            return self.original_branch.clone();
        }
        format!("{}_split_{}", self.original_branch, self.round)
    }

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

    pub fn commit_round(&mut self, branch_name: &str) -> Result<bool> {
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

        git(&self.workdir, &["reset"])?;

        let tmpdir = tempfile::tempdir()?;
        let patch_path = tmpdir.path().join("split.patch");
        std::fs::write(&patch_path, &patch)?;

        let patch_str = patch_path.to_string_lossy().to_string();
        git(&self.workdir, &["apply", "--cached", &patch_str])?;
        git(&self.workdir, &["commit", "-m", branch_name])?;

        self.created_branches.push(branch_name.to_string());
        if branch_name != self.original_branch {
            self.existing_branches.push(branch_name.to_string());
        }

        remove_committed_hunks(&mut self.files, &mut self.selected, &selections);

        let has_remaining = self.files.iter().any(|f| !f.hunks.is_empty());

        if has_remaining {
            self.flat_items = build_flat_items(&self.files);
            self.cursor = 0;
            self.undo_stack.clear();
            self.round += 1;
        }

        Ok(has_remaining)
    }

    pub fn finalize(&mut self) -> Result<()> {
        let repo = GitRepo::open_from_path(&self.workdir)?;

        let num_branches = self.created_branches.len();
        for (i, name) in self.created_branches.iter().enumerate() {
            let offset = num_branches - i;
            let rev = format!("HEAD~{}", offset);
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
            let last_branch = self.created_branches.last().unwrap();
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

        let checkout_target = self.created_branches.last().unwrap().clone();
        git(&self.workdir, &["checkout", &checkout_target])?;

        Ok(())
    }

    pub fn rollback(&self) {
        let _ = git(&self.workdir, &["checkout", "-f", &self.original_branch]);
        for name in &self.created_branches {
            if name != &self.original_branch {
                let _ = git(&self.workdir, &["branch", "-D", name]);
            }
        }
        if self.stashed {
            let _ = git(&self.workdir, &["reset", "HEAD~1"]);
        }
    }

    pub fn file_selected_count(&self, file_idx: usize) -> usize {
        self.selected[file_idx].iter().filter(|&&s| s).count()
    }

    pub fn file_hunk_count(&self, file_idx: usize) -> usize {
        self.selected[file_idx].len()
    }
}

fn has_dirty_workdir(workdir: &Path) -> bool {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(workdir)
        .output();
    match output {
        Ok(o) => !String::from_utf8_lossy(&o.stdout).trim().is_empty(),
        Err(_) => false,
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

fn remove_committed_hunks(
    files: &mut Vec<DiffFile>,
    selected: &mut Vec<Vec<bool>>,
    selections: &[(usize, Vec<usize>)],
) {
    for (fi, hunk_indices) in selections.iter().rev() {
        for hi in hunk_indices.iter().rev() {
            files[*fi].hunks.remove(*hi);
            selected[*fi].remove(*hi);
        }
    }

    let mut i = files.len();
    while i > 0 {
        i -= 1;
        if files[i].hunks.is_empty() {
            files.remove(i);
            selected.remove(i);
        }
    }
}
