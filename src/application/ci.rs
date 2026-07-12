use super::{CiSummary, RepositorySession};
use crate::ci::history;
use crate::config::Config;
use crate::forge::ForgeClient;
use crate::remote::RemoteInfo;
use anyhow::{Context, Result, anyhow};
use std::future::Future;

impl RepositorySession {
    /// Loads provider-neutral CI state for a branch in this repository.
    pub fn load_ci(&self, branch: &str) -> Result<CiSummary> {
        let repo = self.open_repo()?;
        let config = Config::load_for_repo(self.repository_root()).map_err(|_| {
            anyhow!(
                "Failed to load stax config for repository '{}'; check the global config and repository stax.toml",
                self.repository_root().display()
            )
        })?;
        let remote_name = config.remote_name().to_string();
        let remote = RemoteInfo::from_repo(&repo, &config).map_err(|_| {
            anyhow!(
                "Unable to load CI for branch '{branch}': configure a git remote named \
                 '{remote_name}' with a supported GitHub, GitLab, or Gitea URL"
            )
        })?;
        let sha = repo.branch_commit(branch).map_err(|_| {
            anyhow!(
                "Unable to resolve branch '{branch}' to a commit while loading CI; \
                 refresh the repository and verify the branch still exists"
            )
        })?;

        let forge = remote.forge;
        let repo_ref = &repo;
        let sha_ref = sha.as_str();
        let (overall_status, checks) = run_in_tokio_runtime_with(
            || tokio::runtime::Runtime::new().map_err(Into::into),
            || {
                let client = ForgeClient::new_with_config(&remote, &config)
                    .map_err(|error| provider_error(branch, forge, &error))?;
                Ok(async move {
                    client
                        .fetch_checks(repo_ref, sha_ref)
                        .await
                        .map_err(|error| provider_error(branch, forge, &error))
                })
            },
        )
        .with_context(|| format!("Failed to load CI for branch '{branch}'"))?;

        let average_secs = history::estimate_run_average(&repo, &checks)
            .or_else(|| checks.iter().filter_map(|check| check.average_secs).max());
        Ok(CiSummary::from_checks(
            overall_status,
            &checks,
            average_secs,
        ))
    }
}

fn run_in_tokio_runtime_with<T, R, F, Fut>(runtime_factory: R, operation: F) -> Result<T>
where
    R: FnOnce() -> Result<tokio::runtime::Runtime>,
    F: FnOnce() -> Result<Fut>,
    Fut: Future<Output = Result<T>>,
{
    let runtime = runtime_factory()?;
    runtime.block_on(async move {
        let future = operation()?;
        future.await
    })
}

fn provider_error(
    branch: &str,
    forge: crate::remote::ForgeType,
    error: &anyhow::Error,
) -> anyhow::Error {
    let message = error.to_string().to_ascii_lowercase();
    if message.contains("auth")
        || message.contains("token")
        || message.contains("unauthorized")
        || message.contains("401")
        || message.contains("403")
        || message.contains("bad credentials")
    {
        anyhow!(
            "{forge} authentication failed while loading CI for branch '{branch}'; \
             run `stax auth` or refresh the configured provider credentials"
        )
    } else if message.contains("timeout")
        || message.contains("connect")
        || message.contains("network")
        || message.contains("dns")
        || message.contains("request")
    {
        anyhow!(
            "Could not reach {forge} while loading CI for branch '{branch}'; \
             check the network connection and provider URL"
        )
    } else {
        anyhow!(
            "{forge} could not load CI for branch '{branch}'; \
             verify the provider configuration and credentials"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::run_in_tokio_runtime_with;
    use crate::application::RepositorySession;
    use anyhow::{Result, anyhow};
    use std::env;
    use std::future::{Ready, ready};
    use std::path::Path;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tempfile::TempDir;

    struct ConfigDirGuard {
        previous: Option<String>,
    }

    impl ConfigDirGuard {
        fn set(path: &Path) -> Self {
            let previous = env::var("STAX_CONFIG_DIR").ok();
            unsafe { env::set_var("STAX_CONFIG_DIR", path) };
            Self { previous }
        }
    }

    impl Drop for ConfigDirGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(previous) => unsafe { env::set_var("STAX_CONFIG_DIR", previous) },
                None => unsafe { env::remove_var("STAX_CONFIG_DIR") },
            }
        }
    }

    fn test_session(remote_url: Option<&str>) -> (TempDir, RepositorySession, ConfigDirGuard) {
        let dir = tempfile::tempdir().unwrap();
        let mut options = git2::RepositoryInitOptions::new();
        options.initial_head("main");
        let repo = git2::Repository::init_opts(dir.path(), &options).unwrap();
        if let Some(url) = remote_url {
            repo.remote("origin", url).unwrap();
        }

        let config_dir = dir.path().join("isolated-config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            "[remote]\nname = \"origin\"\n",
        )
        .unwrap();
        let guard = ConfigDirGuard::set(&config_dir);
        let session = RepositorySession::open(dir.path()).unwrap();
        (dir, session, guard)
    }

    #[test]
    fn repository_without_remote_returns_actionable_configure_remote_error() {
        let (_dir, session, _config) = test_session(None);

        let error = session.load_ci("main").unwrap_err();
        let message = format!("{error:#}");

        assert!(message.contains("configure a git remote"));
        assert!(message.contains("origin"));
    }

    #[test]
    fn unresolved_branch_returns_actionable_branch_context_before_network() {
        let (_dir, session, _config) = test_session(Some("https://example.invalid/owner/repo.git"));

        let error = session.load_ci("missing-branch").unwrap_err();
        let message = format!("{error:#}");

        assert!(message.contains("missing-branch"));
        assert!(message.contains("resolve"));
    }

    #[test]
    fn runtime_is_available_to_setup_and_async_work() {
        let setup_has_runtime = Arc::new(AtomicBool::new(false));
        let future_has_runtime = Arc::new(AtomicBool::new(false));
        let setup_has_runtime_clone = Arc::clone(&setup_has_runtime);
        let future_has_runtime_clone = Arc::clone(&future_has_runtime);

        let result = run_in_tokio_runtime_with(
            || tokio::runtime::Runtime::new().map_err(Into::into),
            || {
                setup_has_runtime_clone.store(
                    tokio::runtime::Handle::try_current().is_ok(),
                    Ordering::SeqCst,
                );
                Ok(async move {
                    future_has_runtime_clone.store(
                        tokio::runtime::Handle::try_current().is_ok(),
                        Ordering::SeqCst,
                    );
                    Ok::<_, anyhow::Error>(42usize)
                })
            },
        )
        .unwrap();

        assert_eq!(result, 42);
        assert!(setup_has_runtime.load(Ordering::SeqCst));
        assert!(future_has_runtime.load(Ordering::SeqCst));
    }

    #[test]
    fn runtime_factory_errors_propagate_without_panicking() {
        let error = run_in_tokio_runtime_with::<(), _, _, Ready<Result<()>>>(
            || Err(anyhow!("runtime factory failed")),
            || Ok(ready(Ok(()))),
        )
        .unwrap_err();

        assert!(format!("{error:#}").contains("runtime factory failed"));
    }

    #[test]
    fn runtime_setup_errors_propagate_without_panicking() {
        let error = run_in_tokio_runtime_with::<(), _, _, Ready<Result<()>>>(
            || tokio::runtime::Runtime::new().map_err(Into::into),
            || Err(anyhow!("runtime setup failed")),
        )
        .unwrap_err();

        assert!(format!("{error:#}").contains("runtime setup failed"));
    }

    #[test]
    fn async_fetch_errors_propagate_without_panicking() {
        let error = run_in_tokio_runtime_with(
            || tokio::runtime::Runtime::new().map_err(Into::into),
            || Ok(async { Err::<(), _>(anyhow!("fetch failed")) }),
        )
        .unwrap_err();

        assert!(format!("{error:#}").contains("fetch failed"));
    }
}
