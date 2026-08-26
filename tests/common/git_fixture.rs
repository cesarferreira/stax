use anyhow::{Context, Result};
use git2::{Repository, RepositoryInitOptions, Signature};
use std::fs;
use std::path::Path;

pub(crate) fn init_test_repo(path: &Path) -> Result<()> {
    let mut options = RepositoryInitOptions::new();
    options.initial_head("main");
    let repo = Repository::init_opts(path, &options)
        .with_context(|| format!("initialize test repository at {}", path.display()))?;

    let mut config = repo.config().context("open test repository config")?;
    config.set_str("user.name", "Test User")?;
    config.set_str("user.email", "test@test.com")?;
    config.set_bool("commit.gpgSign", false)?;

    fs::write(path.join("README.md"), "# Test Repo\n").context("write test repository README")?;
    let mut index = repo.index().context("open test repository index")?;
    index.add_path(Path::new("README.md"))?;
    index.write()?;
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;
    let signature = Signature::now("Test User", "test@test.com")?;
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        "Initial commit",
        &tree,
        &[],
    )?;
    Ok(())
}

pub(crate) fn commit_all(path: &Path, message: &str) -> Result<git2::Oid> {
    let repo = Repository::open(path)
        .with_context(|| format!("open test repository at {}", path.display()))?;
    let parent = repo.head()?.peel_to_commit()?;
    let mut index = repo.index()?;
    let mut nested_worktrees = Vec::new();
    let worktrees = repo.worktrees()?;
    for name in worktrees.iter() {
        let Some(name) = name? else {
            continue;
        };
        let worktree = repo.find_worktree(name)?;
        if let Ok(relative_path) = worktree.path().strip_prefix(path) {
            nested_worktrees.push(relative_path.to_path_buf());
        }
    }
    index.update_all(["*"], None)?;
    let mut status_options = git2::StatusOptions::new();
    status_options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .exclude_submodules(true);
    let statuses = repo.statuses(Some(&mut status_options))?;
    for entry in statuses.iter() {
        if !entry.status().contains(git2::Status::WT_NEW) {
            continue;
        }
        let relative_path = Path::new(entry.path()?);
        if nested_worktrees
            .iter()
            .any(|worktree| relative_path.starts_with(worktree))
        {
            continue;
        }
        let metadata = fs::symlink_metadata(path.join(relative_path))?;
        if metadata.is_file() || metadata.file_type().is_symlink() {
            index.add_path(relative_path)?;
        }
    }
    index.write()?;
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;
    if tree.id() == parent.tree_id() {
        anyhow::bail!("no fixture changes to commit");
    }
    let signature = Signature::now("Test User", "test@test.com")?;
    Ok(repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        message,
        &tree,
        &[&parent],
    )?)
}

#[cfg(test)]
mod tests {
    use super::init_test_repo;
    use tempfile::{NamedTempFile, tempdir};

    #[test]
    fn initializes_clean_main_repo_with_deterministic_identity() {
        let dir = tempdir().expect("temp dir");
        init_test_repo(dir.path()).expect("initialize fixture");

        let repo = git2::Repository::open(dir.path()).expect("open fixture");
        assert_eq!(repo.head().unwrap().shorthand().unwrap(), "main");
        assert_eq!(
            repo.head()
                .unwrap()
                .peel_to_commit()
                .unwrap()
                .message()
                .unwrap(),
            "Initial commit"
        );
        assert!(repo.statuses(None).unwrap().is_empty());

        let config = repo.config().unwrap();
        assert_eq!(config.get_string("user.name").unwrap(), "Test User");
        assert_eq!(config.get_string("user.email").unwrap(), "test@test.com");
    }

    #[test]
    fn reports_repository_path_when_initialization_fails() {
        let file = NamedTempFile::new().expect("temp file");
        let error = init_test_repo(file.path()).unwrap_err().to_string();
        assert!(error.contains("initialize test repository"));
        assert!(error.contains(&file.path().display().to_string()));
    }

    #[test]
    fn commits_added_modified_and_deleted_files() {
        let dir = tempdir().expect("temp dir");
        init_test_repo(dir.path()).expect("initialize fixture");
        std::fs::write(dir.path().join("deleted.txt"), "delete me\n").unwrap();
        super::commit_all(dir.path(), "add deletion candidate").unwrap();

        std::fs::write(dir.path().join("added.txt"), "added\n").unwrap();
        std::fs::write(dir.path().join("README.md"), "changed\n").unwrap();
        std::fs::remove_file(dir.path().join("deleted.txt")).unwrap();

        let oid = super::commit_all(dir.path(), "fixture update").expect("commit fixture");
        let repo = git2::Repository::open(dir.path()).unwrap();
        let commit = repo.find_commit(oid).unwrap();
        assert_eq!(commit.message().unwrap(), "fixture update");
        let tree = commit.tree().unwrap();
        assert!(tree.get_name("added.txt").is_some());
        assert!(tree.get_name("deleted.txt").is_none());
        let readme = tree.get_name("README.md").unwrap();
        assert_eq!(repo.find_blob(readme.id()).unwrap().content(), b"changed\n");
        assert!(repo.statuses(None).unwrap().is_empty());
    }

    #[test]
    fn commit_all_rejects_an_empty_change() {
        let dir = tempdir().expect("temp dir");
        init_test_repo(dir.path()).expect("initialize fixture");
        let error = super::commit_all(dir.path(), "empty")
            .unwrap_err()
            .to_string();
        assert!(error.contains("no fixture changes to commit"));
    }

    #[test]
    fn commit_all_ignores_nested_linked_worktrees() {
        let dir = tempdir().expect("temp dir");
        init_test_repo(dir.path()).expect("initialize fixture");
        let repo = git2::Repository::open(dir.path()).unwrap();
        let nested_worktree = dir.path().join("nested-worktree");
        repo.worktree("nested-worktree", &nested_worktree, None)
            .expect("create nested linked worktree");
        std::fs::write(dir.path().join("main.txt"), "main change\n").unwrap();

        super::commit_all(dir.path(), "main update").expect("commit main worktree change");
        assert_eq!(
            repo.status_file(std::path::Path::new("main.txt")).unwrap(),
            git2::Status::CURRENT
        );
        assert!(
            repo.head()
                .unwrap()
                .peel_to_tree()
                .unwrap()
                .get_name("main.txt")
                .is_some()
        );
    }
}
