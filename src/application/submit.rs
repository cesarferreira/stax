#![allow(clippy::result_large_err)]
#![allow(dead_code)]

use super::RepositorySession;
use super::operation::{
    OperationError, OperationErrorDetails, OperationErrorKind, OperationEvent, OperationOutcome,
    OperationProgress, OperationReceipt, OperationReporter, OperationRequest, OperationResult,
    OperationSideEffects, OperationStage, PullRequestChange, PullRequestMode, PullRequestReceipt,
    TransactionSummary, report_operation,
};
use super::repository::{MutationLease, MutationTargets, require_blocking_network_context};
use crate::config::{
    Config, NativeStackMode, SingleStackMode, StackLinksMode, StackLinksWhenNative,
};
use crate::engine::Stack;
use crate::ops::receipt::{OpKind, PlanSummary};
use crate::ops::tx::Transaction;
use crate::remote::{RemoteInfo, TrustedRemoteInfo};
use anyhow::Context;
use std::collections::HashSet;
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
    remote: RemoteInfo,
    stack: Stack,
    current_branch: String,
    plans: Vec<PrPlan>,
    prompt_requests: Vec<SubmitPromptRequest>,
    preferences: SubmitPreferences,
    verify_hooks: bool,
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

impl RepositorySession {
    pub(crate) fn prepare_submit(
        &self,
        options: SubmitOptions,
        reporter: &mut dyn OperationReporter,
    ) -> Result<PreparedSubmit, OperationError> {
        let request = OperationRequest::SubmitStack {
            new_pull_requests: options.new_pull_requests,
        };
        require_blocking_network_context(&request)?;
        reporter.report(OperationEvent::Progress(OperationProgress {
            stage: OperationStage::Preparing,
            completed: 0,
            total: None,
            branch: None,
            message: "Preparing submit".into(),
        }));

        let repo = self.open_repo().map_err(|error| {
            submit_source_error(
                &request,
                OperationErrorKind::RepositoryUnavailable,
                "Could not open the repository",
                "Check the repository path and retry",
                error,
                OperationSideEffects::None,
                None,
            )
        })?;
        if !repo.is_initialized() {
            return Err(submit_error(
                &request,
                OperationErrorKind::InitializationRequired,
                "This repository has not been initialized for stax",
                "Run `st init` in the repository, then retry",
                "stax metadata refs are not initialized",
                OperationSideEffects::None,
                None,
            ));
        }
        let stack = Stack::load(&repo).map_err(|error| {
            submit_source_error(
                &request,
                OperationErrorKind::LocalGit,
                "Could not load the stack",
                "Resolve the stack metadata error and retry",
                error,
                OperationSideEffects::None,
                None,
            )
        })?;
        let current_branch = repo.current_branch().map_err(|error| {
            submit_source_error(
                &request,
                OperationErrorKind::LocalGit,
                "Could not determine the current branch",
                "Resolve the Git error and retry",
                error,
                OperationSideEffects::None,
                None,
            )
        })?;
        let branches = branches_for_submit_scope(&stack, &current_branch, options.scope);
        self.with_mutation(
            &request,
            MutationTargets::branches(branches.clone()),
            || Ok(()),
        )?;

        let trusted_network =
            Config::load_for_trusted_network(self.repository_root()).map_err(|error| {
                submit_source_error(
                    &request,
                    OperationErrorKind::Authentication,
                    "Could not load trusted submit configuration",
                    "Check global stax configuration and retry",
                    error,
                    OperationSideEffects::None,
                    None,
                )
            })?;
        let remote = submit_remote_info(&repo, &trusted_network, options.create_pull_requests)
            .map_err(|error| {
                submit_source_error(
                    &request,
                    OperationErrorKind::Authentication,
                    "Could not validate the submit remote",
                    "Configure a trusted remote and retry",
                    error,
                    OperationSideEffects::None,
                    None,
                )
            })?;
        let preferences = SubmitPreferences::from_config(
            &Config::load_repository_submit_preferences(self.repository_root()).unwrap_or_default(),
        );
        let remote_name = remote.name.clone();
        if options.fetch && !options.prefetched {
            reporter.report(OperationEvent::Progress(OperationProgress {
                stage: OperationStage::Preparing,
                completed: 0,
                total: None,
                branch: None,
                message: format!("Fetching {remote_name}"),
            }));
            let fetched = repo.fetch_remote(&remote_name).map_err(|error| {
                submit_source_error(
                    &request,
                    OperationErrorKind::Network,
                    "Could not fetch the submit remote; retry with `--no-fetch` to use existing remote-tracking refs",
                    "Check the remote, or retry with `--no-fetch` to use existing remote-tracking refs",
                    error,
                    OperationSideEffects::None,
                    None,
                )
            })?;
            if !fetched {
                return Err(submit_error(
                    &request,
                    OperationErrorKind::Network,
                    format!(
                        "git fetch {remote_name} failed; retry with `--no-fetch` to use existing remote-tracking refs"
                    ),
                    "Check the remote, or retry with `--no-fetch` to use existing remote-tracking refs",
                    format!("git fetch {remote_name} returned a non-zero status"),
                    OperationSideEffects::None,
                    None,
                ));
            }
        } else if !options.fetch {
            reporter.report(OperationEvent::Progress(OperationProgress {
                stage: OperationStage::Preparing,
                completed: 0,
                total: None,
                branch: None,
                message: "Skipping fetch (--no-fetch)".into(),
            }));
        }
        let lease = self.try_begin_mutation(&request)?;
        let plans = plans_for_branches(
            &repo,
            &stack,
            &branches,
            remote_name.as_str(),
            &remote,
            options.new_pull_requests,
        )
        .map_err(|error| {
            submit_source_error(
                &request,
                OperationErrorKind::LocalGit,
                "Could not prepare submit branches",
                "Resolve the Git error and retry",
                error,
                OperationSideEffects::None,
                None,
            )
        })?;

        Ok(PreparedSubmit {
            request,
            repository_root: self.repository_root().to_path_buf(),
            common_git_dir: self.common_git_dir().to_path_buf(),
            scope: options.scope,
            remote,
            stack,
            current_branch,
            plans,
            prompt_requests: Vec::new(),
            preferences,
            verify_hooks: options.verify_hooks,
            guards: PreparedSubmitGuards {
                resources: SubmitResources {
                    temporary_publish_refs: TemporaryPublishRefs::empty(self.repository_root()),
                    temporary_worktrees: Vec::new(),
                    #[cfg(test)]
                    after_cleanup: None,
                },
                _lease: lease,
            },
        })
    }

    pub(crate) fn execute_prepared_submit(
        &self,
        prepared: PreparedSubmit,
        answers: Vec<SubmitPromptAnswer>,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        if answers.len() != prepared.prompt_requests.len() {
            return Err(submit_error(
                &prepared.request,
                OperationErrorKind::InvalidInput,
                "Submit prompt answers did not match requested prompts",
                "Retry submit with one answer per prompt",
                "prompt answer count mismatch",
                OperationSideEffects::None,
                None,
            ));
        }
        execute_prepared_submit_inner(self, prepared, reporter)
    }

    pub fn submit_stack(
        &self,
        mode: PullRequestMode,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        let request = OperationRequest::SubmitStack {
            new_pull_requests: mode,
        };
        report_operation(request.clone(), reporter, |reporter| {
            require_blocking_network_context(&request)?;
            self.submit_stack_unframed(&request, mode, reporter)
        })
    }

    pub(super) fn submit_stack_unframed(
        &self,
        request: &OperationRequest,
        mode: PullRequestMode,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        let repo = self.open_repo().map_err(|error| {
            submit_source_error(
                request,
                OperationErrorKind::RepositoryUnavailable,
                "Could not open the repository",
                "Check the repository path and retry",
                error,
                OperationSideEffects::None,
                None,
            )
        })?;
        if !repo.is_initialized() {
            return Err(submit_error(
                request,
                OperationErrorKind::InitializationRequired,
                "This repository has not been initialized for stax",
                "Run `st init` in the repository, then retry",
                "stax metadata refs are not initialized",
                OperationSideEffects::None,
                None,
            ));
        }
        let stack = Stack::load(&repo).map_err(|error| {
            submit_source_error(
                request,
                OperationErrorKind::LocalGit,
                "Could not load the stack",
                "Resolve the stack metadata error and retry",
                error,
                OperationSideEffects::None,
                None,
            )
        })?;
        let current_branch = repo.current_branch().map_err(|error| {
            submit_source_error(
                request,
                OperationErrorKind::LocalGit,
                "Could not determine the current branch",
                "Resolve the Git error and retry",
                error,
                OperationSideEffects::None,
                None,
            )
        })?;
        let branches = branches_for_submit_scope(&stack, &current_branch, SubmitScope::Stack);
        let options = SubmitOptions {
            new_pull_requests: mode,
            ..SubmitOptions::gui_current_stack_draft()
        };

        self.with_mutation(request, MutationTargets::branches(branches), || Ok(()))?;
        execute_submit_stack(self, request, options, reporter)
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

fn submit_remote_info(
    repo: &crate::git::GitRepo,
    config: &Config,
    create_pull_requests: bool,
) -> anyhow::Result<RemoteInfo> {
    if create_pull_requests {
        return TrustedRemoteInfo::from_repo(repo, config).map(|remote| remote.remote().clone());
    }
    RemoteInfo::from_repo(repo, config)
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

fn execute_prepared_submit_inner(
    session: &RepositorySession,
    prepared: PreparedSubmit,
    reporter: &mut dyn OperationReporter,
) -> OperationResult {
    let request = prepared.request.clone();
    if prepared.repository_root != session.repository_root() {
        return Err(submit_error(
            &request,
            OperationErrorKind::PreconditionFailed,
            "Prepared submit belongs to a different repository",
            "Restart submit from the selected repository",
            "prepared repository root mismatch",
            OperationSideEffects::None,
            None,
        ));
    }
    execute_submit_plans(session, &request, prepared, reporter)
}

fn execute_submit_stack(
    session: &RepositorySession,
    request: &OperationRequest,
    options: SubmitOptions,
    reporter: &mut dyn OperationReporter,
) -> OperationResult {
    reporter.report(OperationEvent::Progress(OperationProgress {
        stage: OperationStage::Preparing,
        completed: 0,
        total: None,
        branch: None,
        message: "Preparing submit".into(),
    }));

    let repo = session.open_repo().map_err(|error| {
        submit_source_error(
            request,
            OperationErrorKind::RepositoryUnavailable,
            "Could not open the repository",
            "Check the repository path and retry",
            error,
            OperationSideEffects::None,
            None,
        )
    })?;
    let stack = Stack::load(&repo).map_err(|error| {
        submit_source_error(
            request,
            OperationErrorKind::LocalGit,
            "Could not load the stack",
            "Resolve the stack metadata error and retry",
            error,
            OperationSideEffects::None,
            None,
        )
    })?;
    let current_branch = repo.current_branch().map_err(|error| {
        submit_source_error(
            request,
            OperationErrorKind::LocalGit,
            "Could not determine the current branch",
            "Resolve the Git error and retry",
            error,
            OperationSideEffects::None,
            None,
        )
    })?;
    let trusted_network =
        Config::load_for_trusted_network(session.repository_root()).map_err(|error| {
            submit_source_error(
                request,
                OperationErrorKind::Authentication,
                "Could not load trusted submit configuration",
                "Check global stax configuration and retry",
                error,
                OperationSideEffects::None,
                None,
            )
        })?;
    let remote = submit_remote_info(&repo, &trusted_network, options.create_pull_requests)
        .map_err(|error| {
            submit_source_error(
                request,
                OperationErrorKind::Authentication,
                "Could not validate the submit remote",
                "Configure a trusted remote and retry",
                error,
                OperationSideEffects::None,
                None,
            )
        })?;
    let preferences = SubmitPreferences::from_config(
        &Config::load_repository_submit_preferences(session.repository_root()).unwrap_or_default(),
    );
    let remote_name = remote.name.clone();

    if options.fetch && !options.prefetched {
        let fetched = repo.fetch_remote(&remote_name).map_err(|error| {
            submit_source_error(
                request,
                OperationErrorKind::Network,
                "Could not fetch the submit remote; retry with `--no-fetch` to use existing remote-tracking refs",
                "Check the remote, or retry with `--no-fetch` to use existing remote-tracking refs",
                error,
                OperationSideEffects::None,
                None,
            )
        })?;
        if !fetched {
            return Err(submit_error(
                request,
                OperationErrorKind::Network,
                format!(
                    "git fetch {remote_name} failed; retry with `--no-fetch` to use existing remote-tracking refs"
                ),
                "Check the remote, or retry with `--no-fetch` to use existing remote-tracking refs",
                format!("git fetch {remote_name} returned a non-zero status"),
                OperationSideEffects::None,
                None,
            ));
        }
    }

    let branches = branches_for_submit_scope(&stack, &current_branch, options.scope);
    let plans = plans_for_branches(
        &repo,
        &stack,
        &branches,
        &remote_name,
        &remote,
        options.new_pull_requests,
    )
    .map_err(|error| {
        submit_source_error(
            request,
            OperationErrorKind::LocalGit,
            "Could not prepare submit branches",
            "Resolve the Git error and retry",
            error,
            OperationSideEffects::None,
            None,
        )
    })?;
    let lease = session.try_begin_mutation(request)?;
    let prepared = PreparedSubmit {
        request: request.clone(),
        repository_root: session.repository_root().to_path_buf(),
        common_git_dir: session.common_git_dir().to_path_buf(),
        scope: options.scope,
        remote,
        stack,
        current_branch,
        plans,
        prompt_requests: Vec::new(),
        preferences,
        verify_hooks: options.verify_hooks,
        guards: PreparedSubmitGuards {
            resources: SubmitResources {
                temporary_publish_refs: TemporaryPublishRefs::empty(session.repository_root()),
                temporary_worktrees: Vec::new(),
                #[cfg(test)]
                after_cleanup: None,
            },
            _lease: lease,
        },
    };
    execute_submit_plans(session, request, prepared, reporter)
}

fn execute_submit_plans(
    session: &RepositorySession,
    request: &OperationRequest,
    prepared: PreparedSubmit,
    reporter: &mut dyn OperationReporter,
) -> OperationResult {
    let repo = session.open_repo().map_err(|error| {
        submit_source_error(
            request,
            OperationErrorKind::RepositoryUnavailable,
            "Could not open the repository",
            "Check the repository path and retry",
            error,
            OperationSideEffects::None,
            None,
        )
    })?;
    let branches = prepared.branches();
    let remote_name = prepared.remote.name.clone();
    let mut push_specs = Vec::new();
    for plan in &prepared.plans {
        if plan.needs_push {
            push_specs.push(PushSpec {
                branch: plan.branch.clone(),
                source_ref: plan.publish_ref.clone(),
                oid: plan.publish_oid.clone(),
                expected_remote_oid: plan.remote_oid_after_fetch.clone(),
            });
        }
    }

    if push_specs.is_empty() {
        return Ok(submit_receipt(
            request,
            branches,
            existing_pull_request_receipts(&prepared),
            None,
            Vec::new(),
            OperationSideEffects::None,
        ));
    }

    let mut tx = Transaction::begin(OpKind::Submit, &repo, true).map_err(|error| {
        submit_source_error(
            request,
            OperationErrorKind::LocalGit,
            "Could not start the submit transaction",
            "Resolve the transaction error and retry",
            error,
            OperationSideEffects::None,
            None,
        )
    })?;
    for spec in &push_specs {
        tx.plan_remote_branch(&repo, &remote_name, &spec.branch)
            .map_err(|error| {
                submit_source_error(
                    request,
                    OperationErrorKind::LocalGit,
                    "Could not record the submit plan",
                    "Resolve the transaction error and retry",
                    error,
                    OperationSideEffects::None,
                    None,
                )
            })?;
    }
    tx.set_plan_summary(PlanSummary {
        branches_to_rebase: 0,
        branches_to_push: push_specs.len(),
        description: vec![format!("Submit {} branch(es)", push_specs.len())],
    });
    tx.snapshot().map_err(|error| {
        submit_source_error(
            request,
            OperationErrorKind::LocalGit,
            "Could not snapshot submit state",
            "Resolve the transaction error and retry",
            error,
            OperationSideEffects::None,
            None,
        )
    })?;

    reporter.report(OperationEvent::Progress(OperationProgress {
        stage: OperationStage::Pushing,
        completed: 0,
        total: Some(push_specs.len()),
        branch: None,
        message: format!("Pushing {} branch(es)", push_specs.len()),
    }));

    if let Err(error) = push_branches(
        repo.workdir().unwrap_or(session.repository_root()),
        &remote_name,
        &push_specs,
        !prepared_request_verify_hooks(&prepared),
    ) {
        let finalized = tx.finish_err_preserving_receipt(&error.to_string(), Some("push"), None);
        let receipt = submit_receipt(
            request,
            branches,
            existing_pull_request_receipts(&prepared),
            Some(TransactionSummary::from(&finalized.receipt)),
            Vec::new(),
            OperationSideEffects::RemoteMayHaveChanged,
        );
        return Err(submit_source_error(
            request,
            OperationErrorKind::PartialRemoteUpdate,
            format!("Submit failed after remote state may have changed\n{error}"),
            "Refresh the repository and retry after inspecting the remote",
            error,
            OperationSideEffects::RemoteMayHaveChanged,
            Some(receipt),
        ));
    }

    for (index, spec) in push_specs.iter().enumerate() {
        if let Some(oid) = &spec.oid {
            tx.record_remote_after(&remote_name, &spec.branch, oid);
        }
        reporter.report(OperationEvent::Progress(OperationProgress {
            stage: OperationStage::Pushing,
            completed: index + 1,
            total: Some(push_specs.len()),
            branch: Some(spec.branch.clone()),
            message: format!("Pushed {}", spec.branch),
        }));
    }

    reporter.report(OperationEvent::Progress(OperationProgress {
        stage: OperationStage::UpdatingPullRequests,
        completed: push_specs.len(),
        total: Some(push_specs.len()),
        branch: None,
        message: "Collecting pull request receipts".into(),
    }));

    let finalized = tx.finish_ok_preserving_receipt();
    let receipt = submit_receipt(
        request,
        branches,
        existing_pull_request_receipts(&prepared),
        Some(TransactionSummary::from(&finalized.receipt)),
        Vec::new(),
        OperationSideEffects::RemoteMayHaveChanged,
    );

    match finalized.persistence_error {
        Some(error) => Err(submit_source_error(
            request,
            OperationErrorKind::PartialRemoteUpdate,
            "Submit completed, but its receipt could not be persisted",
            "Refresh the repository and inspect the remote before retrying",
            error,
            OperationSideEffects::RemoteMayHaveChanged,
            Some(receipt),
        )),
        None => Ok(receipt),
    }
}

fn prepared_request_verify_hooks(prepared: &PreparedSubmit) -> bool {
    prepared.verify_hooks
}

pub(crate) fn branches_for_submit_scope(
    stack: &Stack,
    current_branch: &str,
    scope: SubmitScope,
) -> Vec<String> {
    let mut branches = match scope {
        SubmitScope::Branch => vec![current_branch.to_string()],
        SubmitScope::Downstack => {
            let mut ancestors = stack.ancestors(current_branch);
            ancestors.reverse();
            ancestors.retain(|branch| branch != &stack.trunk);
            ancestors.push(current_branch.to_string());
            ancestors
        }
        SubmitScope::Upstack => {
            let mut branches = vec![current_branch.to_string()];
            branches.extend(stack.descendants(current_branch));
            branches
        }
        SubmitScope::Stack => stack.current_stack(current_branch),
    };
    let mut seen = HashSet::new();
    branches.retain(|branch| branch != &stack.trunk && seen.insert(branch.clone()));
    branches
}

fn plans_for_branches(
    repo: &crate::git::GitRepo,
    stack: &Stack,
    branches: &[String],
    remote_name: &str,
    remote: &crate::remote::RemoteInfo,
    mode: PullRequestMode,
) -> anyhow::Result<Vec<PrPlan>> {
    branches
        .iter()
        .map(|branch| {
            let publish_oid = repo.branch_commit(branch)?;
            let remote_oid_after_fetch = repo
                .rev_parse(&format!("refs/remotes/{remote_name}/{branch}"))
                .ok();
            let parent = stack
                .branches
                .get(branch)
                .and_then(|branch| branch.parent.clone())
                .unwrap_or_else(|| stack.trunk.clone());
            let existing_pr = stack.branches.get(branch).and_then(|branch_info| {
                branch_info.pr_number.map(|number| ExistingPrSnapshot {
                    number,
                    head: branch.clone(),
                    base: parent.clone(),
                    title: branch.clone(),
                    state: branch_info
                        .pr_state
                        .clone()
                        .unwrap_or_else(|| "OPEN".into()),
                    is_draft: branch_info.pr_is_draft.unwrap_or(false),
                    url: remote.pr_url(number),
                })
            });
            Ok(PrPlan {
                branch: branch.clone(),
                parent,
                commit_range_base: String::new(),
                publish_ref: format!("refs/heads/{branch}"),
                publish_oid: Some(publish_oid.clone()),
                uses_temporary_publish_ref: false,
                remote_oid_after_fetch: remote_oid_after_fetch.clone(),
                existing_pr,
                tip_commit_subject: None,
                needs_title_update: false,
                title: None,
                body: None,
                ai_title_update: None,
                generated_body_update: None,
                is_draft: Some(matches!(mode, PullRequestMode::Draft)),
                needs_push: remote_oid_after_fetch.as_deref() != Some(publish_oid.as_str()),
                needs_pr_update: false,
                needs_base_update: false,
                is_empty: false,
                is_imported: false,
            })
        })
        .collect()
}

fn existing_pull_request_receipts(prepared: &PreparedSubmit) -> Vec<PullRequestReceipt> {
    prepared
        .plans
        .iter()
        .filter_map(|plan| {
            plan.existing_pr.as_ref().map(|pr| PullRequestReceipt {
                branch: plan.branch.clone(),
                number: pr.number,
                url: pr.url.clone(),
                change: PullRequestChange::Unchanged,
            })
        })
        .collect()
}

fn submit_receipt(
    request: &OperationRequest,
    affected_branches: Vec<String>,
    pull_requests: Vec<PullRequestReceipt>,
    transaction: Option<TransactionSummary>,
    warnings: Vec<super::OperationWarning>,
    side_effects: OperationSideEffects,
) -> OperationReceipt {
    let summary = if affected_branches.is_empty() {
        "No branches needed submit".to_string()
    } else {
        format!("Submitted {} branch(es)", affected_branches.len())
    };
    OperationReceipt {
        request: request.clone(),
        summary,
        affected_branches,
        outcome: OperationOutcome::Submitted { pull_requests },
        transaction,
        warnings,
        side_effects,
    }
}

fn submit_source_error(
    request: &OperationRequest,
    kind: OperationErrorKind,
    primary: impl Into<String>,
    action: impl Into<String>,
    source: anyhow::Error,
    side_effects: OperationSideEffects,
    receipt: Option<OperationReceipt>,
) -> OperationError {
    OperationError::from_source(
        request.clone(),
        kind,
        OperationErrorDetails::None,
        primary,
        action,
        &source,
        receipt,
        side_effects,
    )
}

fn submit_error(
    request: &OperationRequest,
    kind: OperationErrorKind,
    primary: impl Into<String>,
    action: impl Into<String>,
    diagnostic_chain: impl Into<String>,
    side_effects: OperationSideEffects,
    receipt: Option<OperationReceipt>,
) -> OperationError {
    OperationError {
        request: request.clone(),
        kind,
        details: OperationErrorDetails::None,
        primary: primary.into(),
        action: action.into(),
        diagnostic_chain: diagnostic_chain.into(),
        receipt,
        side_effects,
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

    fn branch_scope_test_stack() -> crate::engine::Stack {
        use crate::engine::stack::StackBranch;
        use std::collections::HashMap;

        // main (trunk)
        //  ├── a
        //  │   └── a1
        //  │       └── a2
        //  └── b
        let mut branches = HashMap::new();
        branches.insert(
            "main".to_string(),
            StackBranch {
                name: "main".to_string(),
                parent: None,
                parent_revision: None,
                children: vec!["a".to_string(), "b".to_string()],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );
        branches.insert(
            "a".to_string(),
            StackBranch {
                name: "a".to_string(),
                parent: Some("main".to_string()),
                parent_revision: Some("sha-main".to_string()),
                children: vec!["a1".to_string()],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );
        branches.insert(
            "a1".to_string(),
            StackBranch {
                name: "a1".to_string(),
                parent: Some("a".to_string()),
                parent_revision: Some("sha-a".to_string()),
                children: vec!["a2".to_string()],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );
        branches.insert(
            "a2".to_string(),
            StackBranch {
                name: "a2".to_string(),
                parent: Some("a1".to_string()),
                parent_revision: Some("sha-a1".to_string()),
                children: vec![],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );
        branches.insert(
            "b".to_string(),
            StackBranch {
                name: "b".to_string(),
                parent: Some("main".to_string()),
                parent_revision: Some("sha-main".to_string()),
                children: vec![],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );

        crate::engine::Stack {
            branches,
            trunk: "main".to_string(),
        }
    }

    #[test]
    fn branches_for_submit_scope_stack_from_middle() {
        let stack = branch_scope_test_stack();
        assert_eq!(
            super::branches_for_submit_scope(&stack, "a", super::SubmitScope::Stack),
            vec!["a", "a1", "a2"]
        );
    }

    #[test]
    fn branches_for_submit_scope_downstack_from_leaf() {
        let stack = branch_scope_test_stack();
        assert_eq!(
            super::branches_for_submit_scope(&stack, "a2", super::SubmitScope::Downstack),
            vec!["a", "a1", "a2"]
        );
    }

    #[test]
    fn branches_for_submit_scope_upstack_from_middle() {
        let stack = branch_scope_test_stack();
        assert_eq!(
            super::branches_for_submit_scope(&stack, "a1", super::SubmitScope::Upstack),
            vec!["a1", "a2"]
        );
    }

    #[test]
    fn branches_for_submit_scope_single() {
        let stack = branch_scope_test_stack();
        assert_eq!(
            super::branches_for_submit_scope(&stack, "a1", super::SubmitScope::Branch),
            vec!["a1"]
        );
    }

    #[test]
    fn branches_for_submit_scope_branch_on_trunk() {
        let stack = branch_scope_test_stack();
        let empty: Vec<String> = Vec::new();
        assert_eq!(
            super::branches_for_submit_scope(&stack, "main", super::SubmitScope::Branch),
            empty
        );
    }
}
