use stax::application::{
    BranchDetails, BranchDiff, BranchSummary, CiSummary, DetailRequestToken, RepositorySession,
};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

pub(crate) type HydrationFuture<T> =
    Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'static>>;

pub(crate) trait BranchHydrationService: Send + Sync {
    fn load_details(
        &self,
        repository: PathBuf,
        branch: BranchSummary,
    ) -> HydrationFuture<BranchDetails>;

    fn load_cached_diff(
        &self,
        repository: PathBuf,
        branch: String,
        parent: String,
    ) -> HydrationFuture<Option<BranchDiff>>;

    fn load_diff(
        &self,
        repository: PathBuf,
        branch: String,
        parent: String,
    ) -> HydrationFuture<BranchDiff>;

    fn load_ci(&self, repository: PathBuf, branch: String) -> HydrationFuture<CiSummary>;
}

pub(crate) struct NativeBranchHydrationService;

impl BranchHydrationService for NativeBranchHydrationService {
    fn load_details(
        &self,
        repository: PathBuf,
        branch: BranchSummary,
    ) -> HydrationFuture<BranchDetails> {
        Box::pin(async move {
            RepositorySession::open(repository)
                .and_then(|session| session.branch_details(&branch))
                .map_err(|error| format!("{error:#}"))
        })
    }

    fn load_cached_diff(
        &self,
        repository: PathBuf,
        branch: String,
        parent: String,
    ) -> HydrationFuture<Option<BranchDiff>> {
        Box::pin(async move {
            RepositorySession::open(repository)
                .and_then(|session| session.cached_diff(&branch, &parent))
                .map_err(|error| format!("{error:#}"))
        })
    }

    fn load_diff(
        &self,
        repository: PathBuf,
        branch: String,
        parent: String,
    ) -> HydrationFuture<BranchDiff> {
        Box::pin(async move {
            RepositorySession::open(repository)
                .and_then(|session| session.refresh_diff(&branch, &parent))
                .map_err(|error| format!("{error:#}"))
        })
    }

    fn load_ci(&self, repository: PathBuf, branch: String) -> HydrationFuture<CiSummary> {
        Box::pin(async move {
            RepositorySession::open(repository)
                .and_then(|session| session.load_ci(&branch))
                .map_err(|error| format!("{error:#}"))
        })
    }
}

#[derive(Clone)]
pub(crate) struct DetailsHydrationRequest {
    pub(crate) token: DetailRequestToken,
    pub(crate) branch: BranchSummary,
}

#[derive(Clone)]
pub(crate) struct DiffHydrationRequest {
    pub(crate) token: DetailRequestToken,
    pub(crate) parent: Option<String>,
}

#[derive(Clone)]
pub(crate) struct CiHydrationRequest {
    pub(crate) token: DetailRequestToken,
}

struct BoundedStream<T> {
    active: bool,
    queued: Option<T>,
}

impl<T> Default for BoundedStream<T> {
    fn default() -> Self {
        Self {
            active: false,
            queued: None,
        }
    }
}

impl<T> BoundedStream<T> {
    fn enqueue(&mut self, request: T) -> Option<T> {
        if self.active {
            self.queued = Some(request);
            None
        } else {
            self.active = true;
            Some(request)
        }
    }

    fn finish(&mut self) -> Option<T> {
        match self.queued.take() {
            Some(request) => Some(request),
            None => {
                self.active = false;
                None
            }
        }
    }

    fn has_queued(&self) -> bool {
        self.queued.is_some()
    }

    fn clear_queued(&mut self) {
        self.queued = None;
    }
}

/// Owns at most one active request and one latest queued request per stream.
#[derive(Default)]
pub(crate) struct HydrationCoordinator {
    details: BoundedStream<DetailsHydrationRequest>,
    diff: BoundedStream<DiffHydrationRequest>,
    ci: BoundedStream<CiHydrationRequest>,
}

impl HydrationCoordinator {
    pub(crate) fn enqueue_details(
        &mut self,
        token: DetailRequestToken,
        branch: BranchSummary,
    ) -> Option<DetailsHydrationRequest> {
        self.details
            .enqueue(DetailsHydrationRequest { token, branch })
    }

    pub(crate) fn finish_details(&mut self) -> Option<DetailsHydrationRequest> {
        self.details.finish()
    }

    pub(crate) fn enqueue_diff(
        &mut self,
        token: DetailRequestToken,
        parent: Option<String>,
    ) -> Option<DiffHydrationRequest> {
        self.diff.enqueue(DiffHydrationRequest { token, parent })
    }

    pub(crate) fn diff_has_queued(&self) -> bool {
        self.diff.has_queued()
    }

    pub(crate) fn finish_diff(&mut self) -> Option<DiffHydrationRequest> {
        self.diff.finish()
    }

    pub(crate) fn enqueue_ci(&mut self, token: DetailRequestToken) -> Option<CiHydrationRequest> {
        self.ci.enqueue(CiHydrationRequest { token })
    }

    pub(crate) fn finish_ci(&mut self) -> Option<CiHydrationRequest> {
        self.ci.finish()
    }

    pub(crate) fn clear_queued_ci(&mut self) {
        self.ci.clear_queued();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BoundedStream, BranchHydrationService, HydrationFuture, NativeBranchHydrationService,
    };
    use stax::application::RepositorySession;
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use std::task::{Context, Poll, Waker};
    use tempfile::TempDir;

    fn git(path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(["-C", path.to_str().unwrap()])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn resolve<T>(mut future: HydrationFuture<T>) -> Result<T, String> {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        match future.as_mut().poll(&mut context) {
            Poll::Ready(result) => result,
            Poll::Pending => panic!("native repository hydration unexpectedly yielded"),
        }
    }

    #[test]
    fn bounded_stream_keeps_only_the_latest_queued_request() {
        let mut stream = BoundedStream::default();

        assert_eq!(stream.enqueue(1), Some(1));
        assert_eq!(stream.enqueue(2), None);
        assert_eq!(stream.enqueue(3), None);
        assert_eq!(stream.finish(), Some(3));
        assert_eq!(stream.finish(), None);
        assert_eq!(stream.enqueue(4), Some(4));
    }

    #[test]
    fn native_hydration_returns_seeded_cache_then_recomputes_fresh_diff() {
        let temp = TempDir::new().unwrap();
        git(temp.path(), &["init", "-b", "main"]);
        git(temp.path(), &["config", "user.email", "test@example.com"]);
        git(temp.path(), &["config", "user.name", "Test User"]);
        fs::write(temp.path().join("file.txt"), "main\n").unwrap();
        git(temp.path(), &["add", "file.txt"]);
        git(temp.path(), &["commit", "-m", "main"]);
        git(temp.path(), &["switch", "-c", "feature"]);
        fs::write(temp.path().join("file.txt"), "main\nfeature\n").unwrap();
        git(temp.path(), &["add", "file.txt"]);
        git(temp.path(), &["commit", "-m", "feature"]);

        let session = RepositorySession::open(temp.path()).unwrap();
        let actual = session.diff("feature", "main").unwrap();
        let cache_dir = temp.path().join(".git/stax/diff-cache/v1");
        let mut cache_entries = fs::read_dir(&cache_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension().and_then(|extension| extension.to_str()) == Some("json")
            })
            .collect::<Vec<_>>();
        assert_eq!(cache_entries.len(), 1);
        let cache_path = cache_entries.pop().unwrap();
        let mut stored: serde_json::Value =
            serde_json::from_slice(&fs::read(&cache_path).unwrap()).unwrap();
        stored["stat"] = serde_json::json!([]);
        stored["lines"] = serde_json::json!([
            {"content": "incorrect cached patch", "line_type": "context"}
        ]);
        fs::write(&cache_path, serde_json::to_vec_pretty(&stored).unwrap()).unwrap();

        let service = NativeBranchHydrationService;
        let cached = resolve(service.load_cached_diff(
            temp.path().to_path_buf(),
            "feature".into(),
            "main".into(),
        ))
        .unwrap()
        .unwrap();
        assert_eq!(cached.lines[0].content, "incorrect cached patch");

        let fresh =
            resolve(service.load_diff(temp.path().to_path_buf(), "feature".into(), "main".into()))
                .unwrap();

        assert_eq!(fresh, actual);
        assert_eq!(
            session.cached_diff("feature", "main").unwrap(),
            Some(actual)
        );
    }
}
