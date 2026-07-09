use std::path::Path;

use crate::engine::Stack;
use crate::git::GitRepo;

use super::protocol::{
    DesktopError, DiffFileSnapshot, DiffLineKind, DiffLineSnapshot, DiffSnapshot,
    MAX_DIFF_TEXT_BYTES,
};
use super::snapshot;

pub(super) fn build(repo_path: &Path, branch: &str) -> Result<DiffSnapshot, DesktopError> {
    let repo = GitRepo::open_from_path(repo_path).map_err(|error| {
        DesktopError::operation(
            "invalid_repository",
            "The selected folder is not a Git repository.",
            error.to_string(),
            "choose_repository",
        )
    })?;
    let stack = Stack::load(&repo).map_err(diff_error)?;
    let Some(branch_info) = stack.branches.get(branch) else {
        return Err(DesktopError::operation(
            "branch_not_found",
            format!("Branch '{branch}' is not part of this stack."),
            "Refresh the repository and choose an available branch.",
            "refresh",
        ));
    };
    let parent = branch_info
        .parent
        .clone()
        .unwrap_or_else(|| branch.to_string());
    let generation = snapshot::generation(&repo, &stack).map_err(diff_error)?;
    let stats = repo.diff_stat(branch, &parent).map_err(diff_error)?;
    let additions = stats.iter().map(|(_, additions, _)| additions).sum();
    let deletions = stats.iter().map(|(_, _, deletions)| deletions).sum();
    let files = stats
        .into_iter()
        .map(|(path, additions, deletions)| DiffFileSnapshot {
            path,
            additions,
            deletions,
        })
        .collect();

    let raw_lines = repo
        .diff_against_parent(branch, &parent)
        .map_err(diff_error)?;
    let mut text_bytes = 0usize;
    let mut truncated = false;
    let mut lines = Vec::new();
    for line in raw_lines {
        let next_bytes = text_bytes.saturating_add(line.len()).saturating_add(1);
        if next_bytes > MAX_DIFF_TEXT_BYTES {
            truncated = true;
            break;
        }
        text_bytes = next_bytes;
        lines.push(DiffLineSnapshot {
            kind: classify(&line),
            text: line,
        });
    }

    Ok(DiffSnapshot {
        generation,
        branch: branch.to_string(),
        parent,
        additions,
        deletions,
        files,
        lines,
        truncated,
    })
}

fn diff_error(error: anyhow::Error) -> DesktopError {
    DesktopError::operation(
        "diff_failed",
        "The branch patch could not be loaded.",
        format!("{error:#}"),
        "refresh",
    )
}

fn classify(line: &str) -> DiffLineKind {
    if line.starts_with("diff --git ") {
        DiffLineKind::File
    } else if line.starts_with("@@") {
        DiffLineKind::Hunk
    } else if line.starts_with("+++") || line.starts_with("---") || line.starts_with("index ") {
        DiffLineKind::Metadata
    } else if line.starts_with('+') {
        DiffLineKind::Addition
    } else if line.starts_with('-') {
        DiffLineKind::Deletion
    } else {
        DiffLineKind::Context
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_headers_before_addition_and_deletion_markers() {
        assert!(matches!(classify("+++ b/file"), DiffLineKind::Metadata));
        assert!(matches!(classify("--- a/file"), DiffLineKind::Metadata));
        assert!(matches!(classify("+added"), DiffLineKind::Addition));
        assert!(matches!(classify("-removed"), DiffLineKind::Deletion));
    }
}
