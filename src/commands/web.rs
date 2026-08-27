use anyhow::{Context, Result};
use std::path::PathBuf;

/// Launch the `st web` localhost workspace server.
pub fn run(path: Option<PathBuf>, port: u16, no_open: bool) -> Result<()> {
    let repo_root = resolve_repo_root(path)?;
    let rt = tokio::runtime::Runtime::new().context("Failed to create tokio runtime")?;
    rt.block_on(async move { crate::web::run_server(repo_root, port, no_open).await })
}

fn resolve_repo_root(path: Option<PathBuf>) -> Result<PathBuf> {
    let path = match path {
        Some(p) => p,
        None => std::env::current_dir().context("Failed to resolve current directory")?,
    };
    let repo = git2::Repository::discover(&path).with_context(|| {
        format!(
            "Failed to discover git repository from '{}'",
            path.display()
        )
    })?;
    let workdir = repo.workdir().with_context(|| {
        format!(
            "Git repository discovered from '{}' has no working directory",
            path.display()
        )
    })?;
    workdir.canonicalize().with_context(|| {
        format!(
            "Failed to canonicalize git repository root '{}' discovered from '{}'",
            workdir.display(),
            path.display()
        )
    })
}
