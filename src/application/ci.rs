use super::{CiSummary, RepositorySession};
use crate::cache::CiCache;
use crate::ci::history;
use crate::config::Config;
use crate::forge::ForgeClient;
use crate::remote::TrustedRemoteInfo;
use anyhow::{Context, Result, anyhow};
use std::future::Future;

impl RepositorySession {
    /// Loads provider-neutral CI state for a branch in this repository.
    ///
    /// # Blocking
    ///
    /// This creates and blocks a Tokio runtime, so callers must run it on a
    /// dedicated background thread outside any existing Tokio runtime.
    ///
    /// Successful live results are written to the shared repository-local CI
    /// cache on a best-effort basis. A cache persistence failure never changes
    /// the successful result returned to the caller.
    pub fn load_ci(&self, branch: &str) -> Result<CiSummary> {
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(anyhow!(
                "RepositorySession::load_ci is a blocking API and cannot run inside an existing \
                 Tokio runtime; call it from a dedicated background thread"
            ));
        }

        let repo = self.open_repo()?;
        let config = Config::load_for_trusted_network(self.repository_root()).map_err(|_| {
            anyhow!(
                "Failed to load stax config for repository '{}'; check the global config and repository stax.toml",
                self.repository_root().display()
            )
        })?;
        let remote_name = config.remote_name().to_string();
        let trusted_remote = TrustedRemoteInfo::from_repo(&repo, &config).map_err(|error| {
            let message = error.to_string();
            if message.starts_with("Noninteractive repository network access") {
                anyhow!(message)
            } else {
                anyhow!(
                    "Unable to load CI for branch '{branch}': configure a git remote named \
                         '{remote_name}' with a supported GitHub, GitLab, or Gitea URL"
                )
            }
        })?;
        let remote = trusted_remote.remote();
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
                let client = ForgeClient::new_for_trusted_remote(&trusted_remote, &config)
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
        let summary = CiSummary::from_checks(overall_status, &checks, average_secs);
        Ok(self.cache_ci_summary_best_effort(branch, &sha, summary))
    }

    fn cache_ci_summary_best_effort(
        &self,
        branch: &str,
        revision: &str,
        summary: CiSummary,
    ) -> CiSummary {
        let _ = CiCache::update_branch_ci(
            self.cache_dir(),
            branch,
            revision,
            summary.overall_status.clone(),
        );
        summary
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
    use crate::application::{CiSummary, RepositorySession};
    use crate::cache::CiCache;
    use anyhow::{Result, anyhow};
    use std::env;
    use std::fs;
    use std::future::{Ready, ready};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::Path;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    };
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

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

    struct HomeConfigGuard {
        previous_home: Option<String>,
        previous_stax_config_dir: Option<String>,
    }

    impl HomeConfigGuard {
        fn set(path: &Path) -> Self {
            let previous_home = env::var("HOME").ok();
            let previous_stax_config_dir = env::var("STAX_CONFIG_DIR").ok();
            unsafe {
                env::set_var("HOME", path);
                env::set_var("STAX_CONFIG_DIR", path.join(".config").join("stax"));
            }
            Self {
                previous_home,
                previous_stax_config_dir,
            }
        }
    }

    impl Drop for HomeConfigGuard {
        fn drop(&mut self) {
            match self.previous_home.take() {
                Some(previous) => unsafe { env::set_var("HOME", previous) },
                None => unsafe { env::remove_var("HOME") },
            }
            match self.previous_stax_config_dir.take() {
                Some(previous) => unsafe { env::set_var("STAX_CONFIG_DIR", previous) },
                None => unsafe { env::remove_var("STAX_CONFIG_DIR") },
            }
        }
    }

    #[test]
    fn home_config_guard_sets_an_explicit_config_directory() {
        let home = tempfile::tempdir().unwrap();
        let expected = home.path().join(".config").join("stax");
        let _guard = HomeConfigGuard::set(home.path());

        assert_eq!(env::var_os("STAX_CONFIG_DIR"), Some(expected.into_os_string()));
    }

    fn commit_file(repo: &git2::Repository, contents: &str) -> String {
        fs::write(repo.workdir().unwrap().join("tracked.txt"), contents).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("tracked.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("Test User", "test@example.com").unwrap();
        let parents = repo
            .head()
            .ok()
            .and_then(|head| head.peel_to_commit().ok())
            .into_iter()
            .collect::<Vec<_>>();
        let parent_refs = parents.iter().collect::<Vec<_>>();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "test commit",
            &tree,
            &parent_refs,
        )
        .unwrap()
        .to_string()
    }

    struct RecordingListener {
        endpoint: String,
        request_count: Arc<AtomicUsize>,
        authorization_seen: Arc<AtomicBool>,
        stop: mpsc::Sender<()>,
        worker: Option<thread::JoinHandle<()>>,
    }

    impl RecordingListener {
        fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let request_count = Arc::new(AtomicUsize::new(0));
            let authorization_seen = Arc::new(AtomicBool::new(false));
            let worker_count = Arc::clone(&request_count);
            let worker_authorization = Arc::clone(&authorization_seen);
            let (stop, stop_rx) = mpsc::channel();
            let worker = thread::spawn(move || {
                let body = r#"{"total_count":0,"check_runs":[]}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                loop {
                    if stop_rx.try_recv().is_ok() {
                        break;
                    }
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            stream
                                .set_read_timeout(Some(Duration::from_secs(2)))
                                .unwrap();
                            let mut request = [0u8; 8192];
                            let size = stream.read(&mut request).unwrap_or(0);
                            let request = String::from_utf8_lossy(&request[..size]);
                            worker_count.fetch_add(1, Ordering::SeqCst);
                            if request.to_ascii_lowercase().contains("\nauthorization:") {
                                worker_authorization.store(true, Ordering::SeqCst);
                            }
                            let _ = stream.write_all(response.as_bytes());
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => break,
                    }
                }
            });
            Self {
                endpoint,
                request_count,
                authorization_seen,
                stop,
                worker: Some(worker),
            }
        }
    }

    impl Drop for RecordingListener {
        fn drop(&mut self) {
            let _ = self.stop.send(());
            if let Some(worker) = self.worker.take() {
                worker.join().unwrap();
            }
        }
    }

    struct BlockingCheckRunsServer {
        endpoint: String,
        request_received: mpsc::Receiver<()>,
        release_response: Option<mpsc::Sender<()>>,
        stop: mpsc::Sender<()>,
        worker: Option<thread::JoinHandle<()>>,
    }

    impl BlockingCheckRunsServer {
        fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let (request_received_tx, request_received) = mpsc::channel();
            let (release_response, release_response_rx) = mpsc::channel();
            let (stop, stop_rx) = mpsc::channel();
            let worker = thread::spawn(move || {
                let mut served = 0;
                while served < 2 && stop_rx.try_recv().is_err() {
                    let (mut stream, _) = match listener.accept() {
                        Ok(connection) => connection,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                        Err(_) => break,
                    };
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut request = [0u8; 8192];
                    let size = stream.read(&mut request).unwrap_or(0);
                    let request = String::from_utf8_lossy(&request[..size]);
                    let is_check_runs = request.contains("/check-runs");

                    if served == 0 {
                        request_received_tx.send(()).unwrap();
                        loop {
                            if release_response_rx
                                .recv_timeout(Duration::from_millis(10))
                                .is_ok()
                                || stop_rx.try_recv().is_ok()
                            {
                                break;
                            }
                        }
                    }

                    let body = if is_check_runs {
                        r#"{"total_count":1,"check_runs":[{"id":1,"name":"tests","status":"completed","conclusion":"success"}]}"#
                    } else {
                        "[]"
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    served += 1;
                }
            });

            Self {
                endpoint,
                request_received,
                release_response: Some(release_response),
                stop,
                worker: Some(worker),
            }
        }

        fn wait_until_request_is_in_flight(&self) {
            self.request_received
                .recv_timeout(Duration::from_secs(5))
                .expect("CI request did not reach the mock server");
        }

        fn release(&mut self) {
            self.release_response.take().unwrap().send(()).unwrap();
        }
    }

    impl Drop for BlockingCheckRunsServer {
        fn drop(&mut self) {
            if let Some(release) = self.release_response.take() {
                let _ = release.send(());
            }
            let _ = self.stop.send(());
            if let Some(worker) = self.worker.take() {
                worker.join().unwrap();
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

    fn ci_summary(status: &str) -> CiSummary {
        CiSummary {
            overall_status: Some(status.to_string()),
            total: 1,
            passed: 1,
            failed: 0,
            running: 0,
            queued: 0,
            skipped: 0,
            started_at: None,
            completed_at: None,
            average_secs: Some(30),
        }
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
        let (_dir, session, _config) = test_session(Some("https://github.com/owner/repo.git"));

        let error = session.load_ci("missing-branch").unwrap_err();
        let message = format!("{error:#}");

        assert!(message.contains("missing-branch"));
        assert!(message.contains("resolve"));
    }

    #[test]
    fn load_ci_inside_existing_runtime_returns_actionable_error_without_panicking() {
        let (_dir, session, _config) = test_session(None);
        let runtime = tokio::runtime::Runtime::new().unwrap();

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            runtime.block_on(async { session.load_ci("main") })
        }));

        assert!(outcome.is_ok(), "load_ci must not panic inside Tokio");
        let error = outcome.unwrap().unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("blocking"));
        assert!(message.contains("background thread"));
    }

    #[test]
    fn ci_cache_persistence_failure_keeps_the_live_successful_summary() {
        let (dir, session, _config) = test_session(None);
        std::fs::write(dir.path().join(".git").join("stax"), "not a directory").unwrap();
        let expected = ci_summary("success");

        let actual = session.cache_ci_summary_best_effort("main", "revision-a", expected.clone());

        assert_eq!(actual, expected);
        assert!(!dir.path().join(".git/stax/ci-cache.json").exists());
    }

    #[test]
    fn in_flight_ci_result_is_cached_for_its_captured_revision_and_ignored_after_branch_moves() {
        let mut server = BlockingCheckRunsServer::start();
        let dir = tempfile::tempdir().unwrap();
        let mut options = git2::RepositoryInitOptions::new();
        options.initial_head("main");
        let repo = git2::Repository::init_opts(dir.path(), &options).unwrap();
        let old_revision = commit_file(&repo, "old\n");
        repo.remote(
            "origin",
            &format!("{}/owner/repository.git", server.endpoint),
        )
        .unwrap();
        let home = dir.path().join("home");
        let config_dir = home.join(".config").join("stax");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(
            config_dir.join("config.toml"),
            format!(
                "[remote]\nname = \"origin\"\nbase_url = \"{}\"\napi_base_url = \"{}\"\nforge = \"github\"\n\
                 [auth]\nuse_gh_cli = false\n",
                server.endpoint, server.endpoint
            ),
        )
        .unwrap();
        fs::write(config_dir.join(".credentials"), "in-flight-secret").unwrap();
        let _home = HomeConfigGuard::set(&home);
        let session = RepositorySession::open(dir.path()).unwrap();
        let loading_session = session.clone();
        let load = thread::spawn(move || loading_session.load_ci("main"));

        server.wait_until_request_is_in_flight();
        let current_revision = commit_file(&repo, "new\n");
        server.release();
        let summary = load.join().unwrap().unwrap();

        assert_eq!(summary.overall_status.as_deref(), Some("success"));
        let cache = CiCache::load(session.cache_dir());
        assert_eq!(
            cache.get_ci_state_for_revision("main", &old_revision),
            Some("success".to_string())
        );
        assert_eq!(
            cache.get_ci_state_for_revision("main", &current_revision),
            None
        );

        let current_snapshot = session.snapshot().unwrap();
        assert_eq!(current_snapshot.branches[0].ci_state, None);
    }

    #[test]
    fn github_redirect_does_not_forward_authorization_to_an_untrusted_origin() {
        let attacker = RecordingListener::start();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let trusted = runtime.block_on(MockServer::start());
        runtime.block_on(
            Mock::given(method("GET"))
                .respond_with(
                    ResponseTemplate::new(302)
                        .insert_header("Location", format!("{}/redirected", attacker.endpoint)),
                )
                .mount(&trusted),
        );

        let dir = tempfile::tempdir().unwrap();
        let mut options = git2::RepositoryInitOptions::new();
        options.initial_head("main");
        let repo = git2::Repository::init_opts(dir.path(), &options).unwrap();
        commit_file(&repo, "initial\n");
        repo.remote("origin", &format!("{}/owner/repo.git", trusted.uri()))
            .unwrap();
        let home = dir.path().join("home");
        let config_dir = home.join(".config").join("stax");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(
            config_dir.join("config.toml"),
            format!(
                "[remote]\nname = \"origin\"\nbase_url = \"{}\"\napi_base_url = \"{}\"\nforge = \"github\"\n\
                 [auth]\nuse_gh_cli = false\n",
                trusted.uri(),
                trusted.uri()
            ),
        )
        .unwrap();
        fs::write(config_dir.join(".credentials"), "github-redirect-secret").unwrap();
        let _home = HomeConfigGuard::set(&home);
        let session = RepositorySession::open(dir.path()).unwrap();

        let _ = session.load_ci("main");
        let trusted_requests = runtime
            .block_on(trusted.received_requests())
            .unwrap_or_default();

        assert!(!trusted_requests.is_empty());
        assert!(
            trusted_requests
                .iter()
                .all(|request| request.headers.contains_key("authorization"))
        );
        assert!(attacker.request_count.load(Ordering::SeqCst) > 0);
        assert!(!attacker.authorization_seen.load(Ordering::SeqCst));
    }

    #[test]
    fn repo_local_network_overrides_cannot_reach_a_mock_listener() {
        let listener = RecordingListener::start();
        let dir = tempfile::tempdir().unwrap();
        let mut options = git2::RepositoryInitOptions::new();
        options.initial_head("main");
        let repo = git2::Repository::init_opts(dir.path(), &options).unwrap();
        commit_file(&repo, "initial\n");
        repo.remote(
            "origin",
            &format!("{}/owner/private-repo.git", listener.endpoint),
        )
        .unwrap();
        let home = dir.path().join("home");
        let config_dir = home.join(".config").join("stax");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(
            config_dir.join("config.toml"),
            "[remote]\nname = \"origin\"\n",
        )
        .unwrap();
        let secret = "trusted-network-secret";
        fs::write(config_dir.join(".credentials"), secret).unwrap();
        fs::write(
            dir.path().join("stax.toml"),
            format!(
                "[remote]\nname = \"origin\"\nbase_url = \"{}\"\napi_base_url = \"{}\"\nforge = \"github\"\n\
                 [auth]\nuse_gh_cli = false\nallow_github_token_env = true\ngh_hostname = \"127.0.0.1\"\n",
                listener.endpoint, listener.endpoint
            ),
        )
        .unwrap();
        let _home = HomeConfigGuard::set(&home);
        let session = RepositorySession::open(dir.path()).unwrap();

        let result = session.load_ci("main");

        assert!(
            listener.request_count.load(Ordering::SeqCst) == 0,
            "untrusted listener received a request"
        );
        assert!(!listener.authorization_seen.load(Ordering::SeqCst));
        let message = format!("{:#}", result.unwrap_err());
        assert!(message.contains("untrusted"));
        assert!(message.contains("global"));
        assert!(!message.contains(secret));
    }

    #[test]
    fn globally_trusted_custom_host_can_load_ci() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let server = runtime.block_on(MockServer::start());
        runtime.block_on(
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "total_count": 0,
                    "check_runs": []
                })))
                .mount(&server),
        );

        let dir = tempfile::tempdir().unwrap();
        let mut options = git2::RepositoryInitOptions::new();
        options.initial_head("main");
        let repo = git2::Repository::init_opts(dir.path(), &options).unwrap();
        commit_file(&repo, "initial\n");
        repo.remote("origin", &format!("{}/owner/repo.git", server.uri()))
            .unwrap();
        let home = dir.path().join("home");
        let config_dir = home.join(".config").join("stax");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(
            config_dir.join("config.toml"),
            format!(
                "[remote]\nname = \"origin\"\nbase_url = \"{}\"\napi_base_url = \"{}\"\nforge = \"github\"\n\
                 [auth]\nuse_gh_cli = false\n",
                server.uri(),
                server.uri()
            ),
        )
        .unwrap();
        fs::write(config_dir.join(".credentials"), "trusted-custom-secret").unwrap();
        let _home = HomeConfigGuard::set(&home);
        let session = RepositorySession::open(dir.path()).unwrap();

        let summary = session.load_ci("main").unwrap();
        let requests = runtime
            .block_on(server.received_requests())
            .unwrap_or_default();

        assert_eq!(summary.overall_status, None);
        assert!(!requests.is_empty());
        assert!(
            requests
                .iter()
                .all(|request| request.headers.contains_key("authorization"))
        );
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
