use std::fs;
use std::path::Path;

/// Commit currently being replayed, read from Git's rebase state directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RebaseCommitProgress {
    pub current: u32,
    pub total: u32,
    pub subject: String,
}

impl RebaseCommitProgress {
    pub(crate) fn detail_line(&self) -> String {
        format_rebase_commit_detail(self.current, self.total, &self.subject)
    }
}

/// One-line status for the commit Git is applying, such as
/// `Rebasing commit 12/31 · Fix Kotlin compiler warnings`.
pub(crate) fn format_rebase_commit_detail(current: u32, total: u32, subject: &str) -> String {
    let subject = commit_subject(subject);
    if subject.is_empty() {
        format!("Rebasing commit {current}/{total}")
    } else {
        format!("Rebasing commit {current}/{total} · {subject}")
    }
}

/// Read the in-progress rebase from `git_dir`, if one is visible on disk.
///
/// `git rebase` does not stream the current commit on stdout while stax waits
/// for it. The merge backend writes `rebase-merge/msgnum`, `end`, and
/// `message`; the apply backend writes `rebase-apply/next`, `last`, and `msg`.
pub(crate) fn rebase_detail_line(git_dir: &Path) -> Option<String> {
    read_rebase_commit_progress(git_dir).map(|progress| progress.detail_line())
}

pub(crate) fn read_rebase_commit_progress(git_dir: &Path) -> Option<RebaseCommitProgress> {
    read_merge_backend(git_dir).or_else(|| read_apply_backend(git_dir))
}

fn read_merge_backend(git_dir: &Path) -> Option<RebaseCommitProgress> {
    let dir = git_dir.join("rebase-merge");
    let current = read_positive_u32(&dir.join("msgnum"))?;
    let total = read_positive_u32(&dir.join("end"))?;
    Some(RebaseCommitProgress {
        current,
        total,
        subject: commit_subject_file(&dir.join("message")),
    })
}

fn read_apply_backend(git_dir: &Path) -> Option<RebaseCommitProgress> {
    let dir = git_dir.join("rebase-apply");
    let current = read_positive_u32(&dir.join("next"))?;
    let total = read_positive_u32(&dir.join("last"))?;
    let subject = commit_subject_file(&dir.join("msg"));
    let subject = if subject.is_empty() {
        commit_subject_file(&dir.join("final-commit"))
    } else {
        subject
    };
    Some(RebaseCommitProgress {
        current,
        total,
        subject,
    })
}

fn read_positive_u32(path: &Path) -> Option<u32> {
    let text = fs::read_to_string(path).ok()?;
    let value = text.trim().parse::<u32>().ok()?;
    (value > 0).then_some(value)
}

fn commit_subject_file(path: &Path) -> String {
    fs::read_to_string(path)
        .map(|text| commit_subject(&text))
        .unwrap_or_default()
}

fn commit_subject(message: &str) -> String {
    message
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &Path, name: &str, contents: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(name), contents).unwrap();
    }

    #[test]
    fn merge_backend_reads_counts_and_subject() {
        let root = TempDir::new().unwrap();
        let dir = root.path().join("rebase-merge");
        write(&dir, "msgnum", "12\n");
        write(&dir, "end", "31\n");
        write(
            &dir,
            "message",
            "Fix Kotlin compiler warnings\n\nMore detail.\n",
        );

        let progress = read_rebase_commit_progress(root.path()).unwrap();
        assert_eq!(
            progress.detail_line(),
            "Rebasing commit 12/31 · Fix Kotlin compiler warnings"
        );
    }

    #[test]
    fn merge_backend_omits_blank_subject() {
        let root = TempDir::new().unwrap();
        let dir = root.path().join("rebase-merge");
        write(&dir, "msgnum", "1\n");
        write(&dir, "end", "4\n");

        assert_eq!(
            rebase_detail_line(root.path()).as_deref(),
            Some("Rebasing commit 1/4")
        );
    }

    #[test]
    fn apply_backend_reads_next_last_and_msg() {
        let root = TempDir::new().unwrap();
        let dir = root.path().join("rebase-apply");
        write(&dir, "next", "2\n");
        write(&dir, "last", "5\n");
        write(&dir, "msg", "Rename the picker\n");
        write(&dir, "final-commit", "ignored when msg exists\n");

        assert_eq!(
            rebase_detail_line(root.path()).as_deref(),
            Some("Rebasing commit 2/5 · Rename the picker")
        );
    }

    #[test]
    fn apply_backend_falls_back_to_final_commit() {
        let root = TempDir::new().unwrap();
        let dir = root.path().join("rebase-apply");
        write(&dir, "next", "3\n");
        write(&dir, "last", "3\n");
        write(&dir, "final-commit", "  Drop the old flag  \n");

        assert_eq!(
            rebase_detail_line(root.path()).as_deref(),
            Some("Rebasing commit 3/3 · Drop the old flag")
        );
    }

    #[test]
    fn missing_or_zero_state_is_hidden() {
        let root = TempDir::new().unwrap();
        assert_eq!(read_rebase_commit_progress(root.path()), None);

        let dir = root.path().join("rebase-merge");
        write(&dir, "msgnum", "0\n");
        write(&dir, "end", "4\n");
        assert_eq!(read_rebase_commit_progress(root.path()), None);
    }
}
