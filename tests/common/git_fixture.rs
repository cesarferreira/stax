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
}
