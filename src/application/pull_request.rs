#![allow(clippy::result_large_err)]

use super::operation::report_operation;
use super::{
    OperationError, OperationErrorDetails, OperationErrorKind, OperationEvent, OperationOutcome,
    OperationProgress, OperationReceipt, OperationReporter, OperationRequest, OperationResult,
    OperationSideEffects, OperationStage, RepositorySession,
};
use crate::application::repository::require_blocking_network_context;
use crate::config::Config;
use crate::engine::metadata::BranchMetadata;
use crate::forge::ForgeClient;
use crate::github::pr::PrInfoWithHead;
use crate::remote::TrustedRemoteInfo;
use anyhow::Result;
use git2::BranchType;
use std::future::Future;
use std::pin::Pin;

trait PullRequestLookup {
    fn find_open_by_head<'a>(
        &'a self,
        branch: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<PrInfoWithHead>>> + Send + 'a>>;
}

impl PullRequestLookup for ForgeClient {
    fn find_open_by_head<'a>(
        &'a self,
        branch: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<PrInfoWithHead>>> + Send + 'a>> {
        Box::pin(async move { self.find_open_pr_by_head(branch).await })
    }
}

impl RepositorySession {
    pub fn resolve_pull_request_url(
        &self,
        branch: &str,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        let request = OperationRequest::ResolvePullRequestUrl {
            branch: branch.to_owned(),
        };
        report_operation(request.clone(), reporter, |reporter| {
            self.resolve_pull_request_url_unframed(&request, branch, reporter)
        })
    }

    pub(super) fn resolve_pull_request_url_unframed(
        &self,
        request: &OperationRequest,
        branch: &str,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        if let Some(receipt) = resolve_metadata_only(self, request, branch, reporter)? {
            return Ok(receipt);
        }

        require_blocking_network_context(request)?;
        resolve_with_lookup(
            self,
            request,
            branch,
            ForgeClient::new_for_trusted_remote,
            reporter,
        )
    }
}

fn resolve_metadata_only(
    session: &RepositorySession,
    request: &OperationRequest,
    branch: &str,
    reporter: &mut dyn OperationReporter,
) -> Result<Option<OperationReceipt>, OperationError> {
    let (repo, branch) = open_initialized_branch(session, request, branch)?;
    let Some(metadata) = BranchMetadata::read(repo.inner(), branch).map_err(|error| {
        OperationError::from_source(
            request.clone(),
            OperationErrorKind::LocalGit,
            OperationErrorDetails::PullRequest {
                branch: branch.to_string(),
            },
            format!("Could not read pull-request metadata for branch '{branch}'"),
            "Refresh repository metadata and retry",
            &error,
            None,
            OperationSideEffects::None,
        )
    })?
    else {
        return Ok(None);
    };
    let Some(pr_info) = metadata.pr_info.filter(|pr| pr.number > 0) else {
        return Ok(None);
    };
    let url = pull_request_url(session, request, &repo, pr_info.number)?;
    reporter.report(OperationEvent::Progress(OperationProgress {
        stage: OperationStage::ResolvingPullRequest,
        completed: 1,
        total: Some(1),
        branch: Some(branch.to_string()),
        message: format!("Resolved pull request for {branch}"),
    }));
    Ok(Some(pull_request_receipt(request, branch, url)))
}

fn resolve_with_lookup<F, L>(
    session: &RepositorySession,
    request: &OperationRequest,
    branch: &str,
    create_lookup: F,
    reporter: &mut dyn OperationReporter,
) -> OperationResult
where
    F: FnOnce(&TrustedRemoteInfo, &Config) -> Result<L>,
    L: PullRequestLookup,
{
    if let Some(receipt) = resolve_metadata_only(session, request, branch, reporter)? {
        return Ok(receipt);
    }
    let (repo, branch) = open_initialized_branch(session, request, branch)?;
    let config = Config::load_for_trusted_network(session.repository_root()).map_err(|error| {
        OperationError::from_source(
            request.clone(),
            OperationErrorKind::Authorization,
            OperationErrorDetails::None,
            "Could not load trusted repository network configuration",
            "Check the global stax config and repository stax.toml, then retry",
            &error,
            None,
            OperationSideEffects::None,
        )
    })?;
    let trusted_remote = TrustedRemoteInfo::from_repo(&repo, &config).map_err(|error| {
        OperationError::from_source(
            request.clone(),
            OperationErrorKind::Authorization,
            OperationErrorDetails::None,
            "Repository network access is not trusted for this remote",
            "Configure trusted global remote settings and retry",
            &error,
            None,
            OperationSideEffects::None,
        )
    })?;

    reporter.report(OperationEvent::Progress(OperationProgress {
        stage: OperationStage::ResolvingPullRequest,
        completed: 0,
        total: Some(1),
        branch: Some(branch.to_string()),
        message: format!("Resolving pull request for {branch}"),
    }));
    let runtime = tokio::runtime::Runtime::new().map_err(|error| {
        operation_error(
            request,
            OperationErrorKind::Runtime,
            OperationErrorDetails::None,
            "Could not create a runtime for pull-request lookup",
            "Retry from a blocking/background executor",
            error.to_string(),
            OperationSideEffects::None,
        )
    })?;
    let found = runtime
        .block_on(async {
            let lookup = create_lookup(&trusted_remote, &config)?;
            lookup.find_open_by_head(branch).await
        })
        .map_err(|error| operation_error_from_source(request, &error))?;
    let pr = found.ok_or_else(|| {
        operation_error(
            request,
            OperationErrorKind::PreconditionFailed,
            OperationErrorDetails::PullRequest {
                branch: branch.to_string(),
            },
            format!("No open pull request found for branch '{branch}'"),
            "Submit the branch or choose a branch with an open pull request",
            "forge lookup returned no open pull request",
            OperationSideEffects::None,
        )
    })?;
    let url = trusted_remote.remote().pr_url(pr.info.number);
    Ok(pull_request_receipt(request, branch, url))
}

fn open_initialized_branch<'a>(
    session: &RepositorySession,
    request: &OperationRequest,
    branch: &'a str,
) -> Result<(crate::git::GitRepo, &'a str), OperationError> {
    let branch = branch.trim();
    if branch.is_empty() {
        return Err(operation_error(
            request,
            OperationErrorKind::InvalidInput,
            OperationErrorDetails::None,
            "Branch name is required",
            "Choose a local branch and retry",
            "pull-request branch input was empty",
            OperationSideEffects::None,
        ));
    }
    let repo = session.open_repo().map_err(|error| {
        OperationError::from_source(
            request.clone(),
            OperationErrorKind::RepositoryUnavailable,
            OperationErrorDetails::None,
            "Could not open the repository",
            "Check the repository path and retry",
            &error,
            None,
            OperationSideEffects::None,
        )
    })?;
    if !repo.is_initialized() {
        return Err(operation_error(
            request,
            OperationErrorKind::InitializationRequired,
            OperationErrorDetails::None,
            "This repository has not been initialized for stax",
            "Run `st init` in the repository, then retry",
            "stax metadata refs are not initialized",
            OperationSideEffects::None,
        ));
    }
    if repo.inner().find_branch(branch, BranchType::Local).is_err() {
        return Err(operation_error(
            request,
            OperationErrorKind::InvalidInput,
            OperationErrorDetails::Branch {
                branch: branch.to_string(),
            },
            format!("Branch '{branch}' does not exist locally"),
            "Choose an existing local branch and retry",
            "local branch lookup failed",
            OperationSideEffects::None,
        ));
    }
    Ok((repo, branch))
}

fn pull_request_url(
    session: &RepositorySession,
    request: &OperationRequest,
    repo: &crate::git::GitRepo,
    number: u64,
) -> Result<String, OperationError> {
    let config = Config::load_for_trusted_network(session.repository_root()).map_err(|error| {
        OperationError::from_source(
            request.clone(),
            OperationErrorKind::Authorization,
            OperationErrorDetails::None,
            "Could not load trusted repository network configuration",
            "Check the global stax config and repository stax.toml, then retry",
            &error,
            None,
            OperationSideEffects::None,
        )
    })?;
    let trusted_remote = TrustedRemoteInfo::from_repo(repo, &config).map_err(|error| {
        OperationError::from_source(
            request.clone(),
            OperationErrorKind::Authorization,
            OperationErrorDetails::None,
            "Repository network access is not trusted for this remote",
            "Configure trusted global remote settings and retry",
            &error,
            None,
            OperationSideEffects::None,
        )
    })?;
    Ok(trusted_remote.remote().pr_url(number))
}

fn pull_request_receipt(request: &OperationRequest, branch: &str, url: String) -> OperationReceipt {
    OperationReceipt {
        request: request.clone(),
        summary: format!("Resolved pull request for {branch}"),
        affected_branches: vec![branch.to_string()],
        outcome: OperationOutcome::PullRequestResolved {
            branch: branch.to_string(),
            url,
        },
        transaction: None,
        warnings: Vec::new(),
        side_effects: OperationSideEffects::None,
    }
}

fn operation_error_from_source(
    request: &OperationRequest,
    error: &anyhow::Error,
) -> OperationError {
    let message = format!("{error:#}");
    let lowercase = message.to_ascii_lowercase();
    let kind = if lowercase.contains("403")
        || lowercase.contains("forbidden")
        || lowercase.contains("permission")
        || lowercase.contains("untrusted")
        || lowercase.contains("provider mismatch")
    {
        OperationErrorKind::Authorization
    } else if lowercase.contains("401")
        || lowercase.contains("unauthorized")
        || lowercase.contains("bad credentials")
        || lowercase.contains("not configured")
        || lowercase.contains("token")
        || lowercase.contains("auth")
    {
        OperationErrorKind::Authentication
    } else if lowercase.contains("timeout")
        || lowercase.contains("connect")
        || lowercase.contains("network")
        || lowercase.contains("dns")
        || lowercase.contains("request")
        || lowercase.contains("500")
        || lowercase.contains("502")
        || lowercase.contains("503")
        || lowercase.contains("504")
    {
        OperationErrorKind::Network
    } else {
        OperationErrorKind::Network
    };
    OperationError::from_source(
        request.clone(),
        kind,
        OperationErrorDetails::PullRequest {
            branch: match request {
                OperationRequest::ResolvePullRequestUrl { branch } => branch.clone(),
                _ => String::new(),
            },
        },
        "Could not resolve the pull request",
        "Check provider access and retry",
        error,
        None,
        OperationSideEffects::None,
    )
}

fn operation_error(
    request: &OperationRequest,
    kind: OperationErrorKind,
    details: OperationErrorDetails,
    primary: impl Into<String>,
    action: impl Into<String>,
    diagnostic_chain: impl Into<String>,
    side_effects: OperationSideEffects,
) -> OperationError {
    OperationError {
        request: request.clone(),
        kind,
        details,
        primary: primary.into(),
        action: action.into(),
        diagnostic_chain: diagnostic_chain.into(),
        receipt: None,
        side_effects,
    }
}

#[cfg(test)]
mod tests {
    use super::{PullRequestLookup, resolve_with_lookup};
    use crate::application::{
        NoopOperationReporter, OperationOutcome, OperationRequest, OperationSideEffects,
        RepositorySession,
    };
    use crate::engine::metadata::{BranchMetadata, PrInfo};
    use crate::git::GitRepo;
    use crate::github::pr::{PrInfo as ForgePrInfo, PrInfoWithHead};
    use anyhow::Result;
    use std::future::Future;
    use std::path::Path;
    use std::pin::Pin;

    struct FakePullRequestLookup {
        result: Option<PrInfoWithHead>,
    }

    impl PullRequestLookup for FakePullRequestLookup {
        fn find_open_by_head<'a>(
            &'a self,
            _branch: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<Option<PrInfoWithHead>>> + Send + 'a>> {
            Box::pin(async move { Ok(self.result.clone()) })
        }
    }

    fn git(cwd: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env(
                "GIT_CONFIG_SYSTEM",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn test_repo() -> (tempfile::TempDir, GitRepo, RepositorySession) {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "main"]);
        git(dir.path(), &["config", "user.name", "Test User"]);
        git(dir.path(), &["config", "user.email", "test@example.com"]);
        std::fs::write(dir.path().join("README.md"), "initial\n").unwrap();
        git(dir.path(), &["add", "README.md"]);
        git(dir.path(), &["commit", "-m", "initial"]);
        git(
            dir.path(),
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/test-owner/test-repo.git",
            ],
        );
        git(dir.path(), &["checkout", "-b", "feature"]);
        std::fs::write(dir.path().join("feature.txt"), "feature\n").unwrap();
        git(dir.path(), &["add", "feature.txt"]);
        git(dir.path(), &["commit", "-m", "feature"]);
        git(dir.path(), &["checkout", "main"]);

        let repo = GitRepo::open_from_path(dir.path()).unwrap();
        repo.set_trunk("main").unwrap();
        let parent_revision = repo.branch_commit("main").unwrap();
        let metadata = BranchMetadata::new("main", &parent_revision);
        metadata.write(repo.inner(), "feature").unwrap();
        let session = RepositorySession::open(dir.path()).unwrap();
        (dir, repo, session)
    }

    fn write_pr_metadata(repo: &GitRepo, branch: &str, number: u64) {
        let mut metadata = BranchMetadata::read(repo.inner(), branch).unwrap().unwrap();
        metadata.pr_info = Some(PrInfo {
            number,
            state: "OPEN".into(),
            is_draft: Some(false),
        });
        metadata.write(repo.inner(), branch).unwrap();
    }

    fn metadata_bytes(repo: &GitRepo, branch: &str) -> Vec<u8> {
        let reference = crate::git::refs::metadata_refname(branch);
        let object = repo.inner().revparse_single(&reference).unwrap();
        repo.inner()
            .find_blob(object.id())
            .unwrap()
            .content()
            .to_vec()
    }

    fn request(branch: &str) -> OperationRequest {
        OperationRequest::ResolvePullRequestUrl {
            branch: branch.to_string(),
        }
    }

    fn fake_pr(number: u64) -> PrInfoWithHead {
        PrInfoWithHead {
            info: ForgePrInfo {
                number,
                state: "OPEN".into(),
                is_draft: false,
                base: "main".into(),
            },
            head: "feature".into(),
            head_label: None,
            title: "Feature".into(),
        }
    }

    #[test]
    fn pull_request_resolution_from_metadata_is_read_only() {
        let (_dir, repo, session) = test_repo();
        write_pr_metadata(&repo, "feature", 42);
        let before = metadata_bytes(&repo, "feature");

        let receipt = resolve_with_lookup(
            &session,
            &request("feature"),
            "feature",
            |_, _| Ok(FakePullRequestLookup { result: None }),
            &mut NoopOperationReporter,
        )
        .unwrap();

        assert_eq!(metadata_bytes(&repo, "feature"), before);
        assert_eq!(receipt.side_effects, OperationSideEffects::None);
        assert_eq!(
            receipt.outcome,
            OperationOutcome::PullRequestResolved {
                branch: "feature".into(),
                url: "https://github.com/test-owner/test-repo/pull/42".into(),
            }
        );
    }

    #[test]
    fn pull_request_fallback_does_not_persist_metadata() {
        let (_dir, repo, session) = test_repo();
        let before = metadata_bytes(&repo, "feature");

        let receipt = resolve_with_lookup(
            &session,
            &request("feature"),
            "feature",
            |_, _| {
                Ok(FakePullRequestLookup {
                    result: Some(fake_pr(42)),
                })
            },
            &mut NoopOperationReporter,
        )
        .unwrap();

        assert_eq!(metadata_bytes(&repo, "feature"), before);
        assert_eq!(receipt.side_effects, OperationSideEffects::None);
        assert_eq!(
            receipt.outcome,
            OperationOutcome::PullRequestResolved {
                branch: "feature".into(),
                url: "https://github.com/test-owner/test-repo/pull/42".into(),
            }
        );
    }
}
