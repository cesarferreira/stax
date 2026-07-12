#![allow(clippy::result_large_err)]
#![allow(dead_code)]

use super::operation::PullRequestMode;
use super::repository::MutationLease;
use crate::config::{
    Config, NativeStackMode, SingleStackMode, StackLinksMode, StackLinksWhenNative,
};
use crate::engine::Stack;
use crate::remote::TrustedRemoteInfo;
use anyhow::Context;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubmitScope {
    Branch,
    Downstack,
    Upstack,
    Stack,
}

impl SubmitScope {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Branch => "branch",
            Self::Downstack => "downstack",
            Self::Upstack => "upstack",
            Self::Stack => "stack",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SubmitOptions {
    pub scope: SubmitScope,
    pub new_pull_requests: PullRequestMode,
    pub fetch: bool,
    pub prefetched: bool,
    pub verify_hooks: bool,
    pub create_pull_requests: bool,
    pub reviewers: Vec<String>,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub rerequest_review: bool,
    pub native_stack_override: Option<NativeStackMode>,
    pub update_title: bool,
}

impl SubmitOptions {
    pub(crate) fn gui_current_stack_draft() -> Self {
        Self {
            scope: SubmitScope::Stack,
            new_pull_requests: PullRequestMode::Draft,
            fetch: true,
            prefetched: false,
            verify_hooks: true,
            create_pull_requests: true,
            reviewers: Vec::new(),
            labels: Vec::new(),
            assignees: Vec::new(),
            rerequest_review: false,
            native_stack_override: None,
            update_title: false,
        }
    }
}

pub(crate) struct SubmitConfigSources {
    pub trusted_network: Config,
    pub preferences: SubmitPreferences,
}

impl SubmitConfigSources {
    pub(crate) fn load(repository_root: &Path) -> anyhow::Result<Self> {
        let trusted_network = Config::load_for_trusted_network(repository_root)?;
        let submit_preferences = Config::load_repository_submit_preferences(repository_root)?;
        Ok(Self {
            trusted_network,
            preferences: SubmitPreferences::from_config(&submit_preferences),
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SubmitPreferences {
    pub stack_links: StackLinksMode,
    pub single_stack: SingleStackMode,
    pub stack_links_when_native: StackLinksWhenNative,
    pub native_stack: NativeStackMode,
}

impl SubmitPreferences {
    fn from_config(config: &Config) -> Self {
        Self {
            stack_links: config.submit.stack_links,
            single_stack: config.submit.single_stack,
            stack_links_when_native: config.submit.stack_links_when_native,
            native_stack: config.submit.native_stack,
        }
    }
}

#[allow(dead_code)]
pub(crate) struct PreparedSubmit {
    request: super::OperationRequest,
    repository_root: PathBuf,
    common_git_dir: PathBuf,
    scope: SubmitScope,
    remote: TrustedRemoteInfo,
    stack: Stack,
    current_branch: String,
    plans: Vec<PrPlan>,
    prompt_requests: Vec<SubmitPromptRequest>,
    preferences: SubmitPreferences,
    guards: PreparedSubmitGuards,
}

impl PreparedSubmit {
    pub(crate) fn prompt_requests(&self) -> &[SubmitPromptRequest] {
        &self.prompt_requests
    }

    pub(crate) fn branches(&self) -> Vec<String> {
        self.plans.iter().map(|plan| plan.branch.clone()).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SubmitPromptRequest {
    pub branch: String,
    pub suggested_title: String,
    pub suggested_body: String,
    pub suggested_mode: PullRequestMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SubmitPromptAnswer {
    pub branch: String,
    pub title: String,
    pub body: String,
    pub mode: PullRequestMode,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct PrPlan {
    branch: String,
    parent: String,
    commit_range_base: String,
    publish_ref: String,
    publish_oid: Option<String>,
    uses_temporary_publish_ref: bool,
    remote_oid_after_fetch: Option<String>,
    existing_pr: Option<ExistingPrSnapshot>,
    tip_commit_subject: Option<String>,
    needs_title_update: bool,
    title: Option<String>,
    body: Option<String>,
    ai_title_update: Option<String>,
    generated_body_update: Option<String>,
    is_draft: Option<bool>,
    needs_push: bool,
    needs_pr_update: bool,
    needs_base_update: bool,
    is_empty: bool,
    is_imported: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct ExistingPrSnapshot {
    number: u64,
    head: String,
    base: String,
    title: String,
    state: String,
    is_draft: bool,
    url: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PushSpec {
    pub(crate) branch: String,
    pub(crate) source_ref: String,
    pub(crate) oid: Option<String>,
    pub(crate) expected_remote_oid: Option<String>,
}

struct PreparedSubmitGuards {
    resources: SubmitResources,
    _lease: MutationLease,
}

struct SubmitResources {
    temporary_publish_refs: TemporaryPublishRefs,
    temporary_worktrees: Vec<TemporarySubmitWorktree>,
    #[cfg(test)]
    after_cleanup: Option<Box<dyn FnOnce()>>,
}

impl SubmitResources {
    fn cleanup_best_effort(&mut self) {
        for worktree in self.temporary_worktrees.iter_mut().rev() {
            let _ = worktree.remove();
        }
        self.temporary_publish_refs.cleanup();
    }
}

impl Drop for SubmitResources {
    fn drop(&mut self) {
        self.cleanup_best_effort();
        #[cfg(test)]
        if let Some(after_cleanup) = self.after_cleanup.take() {
            after_cleanup();
        }
    }
}

pub(crate) struct TemporaryPublishRefs {
    pub(crate) workdir: PathBuf,
    pub(crate) refs: Vec<String>,
}

impl TemporaryPublishRefs {
    pub(crate) fn empty(workdir: &Path) -> Self {
        Self {
            workdir: workdir.to_path_buf(),
            refs: Vec::new(),
        }
    }

    pub(crate) fn cleanup(&mut self) {
        for refname in self.refs.drain(..) {
            let _ = Command::new("git")
                .args(["update-ref", "-d", &refname])
                .current_dir(&self.workdir)
                .output();
        }
    }
}

impl Drop for TemporaryPublishRefs {
    fn drop(&mut self) {
        self.cleanup();
    }
}

pub(crate) struct TemporarySubmitWorktree {
    pub(crate) workdir: PathBuf,
    pub(crate) path: PathBuf,
    pub(crate) active: bool,
}

impl TemporarySubmitWorktree {
    pub(crate) fn new(workdir: &Path, path: &Path) -> Self {
        Self {
            workdir: workdir.to_path_buf(),
            path: path.to_path_buf(),
            active: true,
        }
    }

    pub(crate) fn remove(&mut self) -> anyhow::Result<()> {
        if !self.active {
            return Ok(());
        }
        let remove = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&self.path)
            .current_dir(&self.workdir)
            .output()
            .context("Failed to remove temporary submit worktree")?;
        if !remove.status.success() {
            anyhow::bail!("{}", command_output_details("git worktree remove", &remove));
        }
        self.active = false;
        Ok(())
    }
}

impl Drop for TemporarySubmitWorktree {
    fn drop(&mut self) {
        if self.active {
            let _ = Command::new("git")
                .args(["worktree", "remove", "--force"])
                .arg(&self.path)
                .current_dir(&self.workdir)
                .output();
        }
    }
}

fn command_output_details(command: &str, output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let details = push_failure_details(&stdout, &stderr);
    if details.is_empty() {
        format!("{command} failed")
    } else {
        format!("{command} failed:\n{details}")
    }
}

pub(crate) fn push_branches(
    workdir: &Path,
    remote: &str,
    specs: &[PushSpec],
    no_verify: bool,
) -> anyhow::Result<()> {
    let mut args = vec!["push", "--porcelain"];
    let lease_args = specs
        .iter()
        .map(|spec| {
            format!(
                "--force-with-lease=refs/heads/{}:{}",
                spec.branch,
                spec.expected_remote_oid.as_deref().unwrap_or("")
            )
        })
        .collect::<Vec<_>>();
    args.extend(lease_args.iter().map(String::as_str));
    if no_verify {
        args.push("--no-verify");
    }
    args.extend(["-u", remote]);
    let refspecs = specs
        .iter()
        .map(|spec| format!("{}:refs/heads/{}", spec.source_ref, spec.branch))
        .collect::<Vec<_>>();
    args.extend(refspecs.iter().map(String::as_str));

    let output = Command::new("git")
        .args(args)
        .current_dir(workdir)
        .output()
        .context("Failed to push branches")?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let rejected = rejected_push_branches(&stdout, specs);
        let details = push_failure_details(&stdout, &stderr);
        let branch_list = specs
            .iter()
            .map(|spec| spec.branch.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        if !rejected.is_empty() {
            let mut message = format!(
                "Failed to push branches {branch_list}: rejected {}",
                rejected.join(", ")
            );
            if !details.is_empty() {
                message.push('\n');
                message.push_str(&details);
            }
            anyhow::bail!("{message}");
        }
        if details.is_empty() {
            anyhow::bail!("Failed to push branches: {branch_list}");
        }
        anyhow::bail!("Failed to push branches {branch_list}:\n{details}");
    }
    Ok(())
}

pub(crate) fn rejected_push_branches(porcelain: &str, specs: &[PushSpec]) -> Vec<String> {
    porcelain
        .lines()
        .filter(|line| line.starts_with("!\t"))
        .filter_map(|line| {
            let local_ref = line.split('\t').nth(1)?.split(':').next()?;
            specs
                .iter()
                .find(|spec| spec.source_ref == local_ref)
                .map(|spec| spec.branch.clone())
                .or_else(|| local_ref.strip_prefix("refs/heads/").map(str::to_string))
        })
        .filter(|branch| specs.iter().any(|spec| spec.branch == *branch))
        .collect()
}

pub(crate) fn push_failure_details(stdout: &str, stderr: &str) -> String {
    let stdout = stdout.trim();
    let stderr = stderr.trim();

    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => String::new(),
        (false, true) => stdout.to_string(),
        (true, false) => stderr.to_string(),
        (false, false) => format!("stdout:\n{stdout}\nstderr:\n{stderr}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PreparedSubmitGuards, SubmitResources, TemporaryPublishRefs, TemporarySubmitWorktree,
    };
    use crate::application::{
        OperationErrorKind, OperationRequest, PullRequestMode, RepositorySession,
    };
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn prepared_submit_drops_resources_before_mutation_lease() {
        let root = tempfile::tempdir().expect("repository tempdir");
        let init = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(root.path())
            .output()
            .expect("git init");
        assert!(init.status.success());
        for args in [
            ["config", "user.name", "Test User"],
            ["config", "user.email", "test@example.com"],
        ] {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(root.path())
                .output()
                .expect("git config");
            assert!(output.status.success());
        }
        std::fs::write(root.path().join("README.md"), "test\n").unwrap();
        let add = std::process::Command::new("git")
            .args(["add", "README.md"])
            .current_dir(root.path())
            .output()
            .expect("git add setup");
        assert!(add.status.success());
        let commit = std::process::Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(root.path())
            .output()
            .expect("git commit setup");
        assert!(commit.status.success());
        let temporary_ref = "refs/stax/submit/cleanup-order";
        let head = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(root.path())
            .output()
            .expect("rev-parse HEAD");
        assert!(head.status.success());
        let head = String::from_utf8_lossy(&head.stdout).trim().to_string();
        let update_ref = std::process::Command::new("git")
            .args(["update-ref", temporary_ref, &head])
            .current_dir(root.path())
            .output()
            .expect("create temporary ref");
        assert!(update_ref.status.success());
        let temporary_worktree = root.path().join("temporary-submit-worktree");
        let add_worktree = std::process::Command::new("git")
            .args(["worktree", "add", "--detach"])
            .arg(&temporary_worktree)
            .arg("HEAD")
            .current_dir(root.path())
            .output()
            .expect("create temporary worktree");
        assert!(add_worktree.status.success());

        let request = OperationRequest::SubmitStack {
            new_pull_requests: PullRequestMode::Draft,
        };
        let first = RepositorySession::open(root.path()).unwrap();
        let second = RepositorySession::open(root.path()).unwrap();
        let lease = first.try_begin_mutation(&request).unwrap();
        let cleanup_observed_while_locked = Rc::new(Cell::new(false));
        let observed = Rc::clone(&cleanup_observed_while_locked);
        let root_for_probe = root.path().to_path_buf();
        let request_for_probe = request.clone();

        let guards = PreparedSubmitGuards {
            resources: SubmitResources {
                temporary_publish_refs: TemporaryPublishRefs {
                    workdir: root.path().to_path_buf(),
                    refs: vec![temporary_ref.to_string()],
                },
                temporary_worktrees: vec![TemporarySubmitWorktree {
                    workdir: root.path().to_path_buf(),
                    path: temporary_worktree.clone(),
                    active: true,
                }],
                after_cleanup: Some(Box::new(move || {
                    let ref_lookup = std::process::Command::new("git")
                        .args(["show-ref", "--verify", temporary_ref])
                        .current_dir(&root_for_probe)
                        .output()
                        .expect("verify temporary ref cleanup");
                    assert!(!ref_lookup.status.success());
                    assert!(!temporary_worktree.exists());
                    let probe = RepositorySession::open(&root_for_probe).unwrap();
                    let error = probe.try_begin_mutation(&request_for_probe).unwrap_err();
                    assert_eq!(error.kind, OperationErrorKind::Busy);
                    observed.set(true);
                })),
            },
            _lease: lease,
        };

        drop(guards);

        assert!(cleanup_observed_while_locked.get());
        assert!(second.try_begin_mutation(&request).is_ok());
    }
}
