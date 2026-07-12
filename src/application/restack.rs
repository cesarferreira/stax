#![allow(clippy::result_large_err)]

use super::operation::report_operation;
use super::{
    OperationError, OperationErrorDetails, OperationErrorKind, OperationEvent, OperationOutcome,
    OperationProgress, OperationReceipt, OperationReporter, OperationRequest, OperationResult,
    OperationSideEffects, OperationStage, OperationWarning, RepositorySession, RestackScope,
    TransactionSummary,
};
use crate::application::repository::MutationTargets;
use crate::config::Config;
use crate::engine::restack_preflight::choose_rebase_upstream;
use crate::engine::{BranchMetadata, PrInfo, Stack};
use crate::git::{GitRepo, RebaseResult};
use crate::ops::receipt::{OpKind, PlanSummary};
use crate::ops::tx::Transaction;
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RestackExecutionOptions {
    pub scope: RestackScope,
    pub auto_stash: bool,
    pub restore_branch: Option<String>,
    pub completed_from_receipt: HashSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OwnedStash {
    worktree: PathBuf,
    restored: bool,
}

struct RestackFailureState {
    transaction: Option<Transaction>,
    completed_branches: Vec<String>,
    stashes: Vec<OwnedStash>,
    side_effects: OperationSideEffects,
}

impl RepositorySession {
    pub fn restack(
        &self,
        scope: RestackScope,
        auto_stash: bool,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        let options = RestackExecutionOptions {
            scope: scope.clone(),
            auto_stash,
            restore_branch: None,
            completed_from_receipt: HashSet::new(),
        };
        self.restack_with_options(options, reporter)
    }

    pub(crate) fn restack_with_options(
        &self,
        options: RestackExecutionOptions,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        let request = OperationRequest::Restack {
            scope: options.scope.clone(),
            auto_stash: options.auto_stash,
        };
        report_operation(request.clone(), reporter, |reporter| {
            self.restack_with_options_unframed(&request, options, reporter)
        })
    }

    #[allow(dead_code)]
    pub(super) fn restack_unframed(
        &self,
        request: &OperationRequest,
        scope: RestackScope,
        auto_stash: bool,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        let options = RestackExecutionOptions {
            scope,
            auto_stash,
            restore_branch: None,
            completed_from_receipt: HashSet::new(),
        };
        self.restack_with_options_unframed(request, options, reporter)
    }

    fn restack_with_options_unframed(
        &self,
        request: &OperationRequest,
        options: RestackExecutionOptions,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        let repo = self.open_repo().map_err(|error| {
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
        let stack = Stack::load(&repo).map_err(|error| {
            OperationError::from_source(
                request.clone(),
                OperationErrorKind::LocalGit,
                OperationErrorDetails::None,
                "Could not load the stack",
                "Resolve the stack metadata error and retry",
                &error,
                None,
                OperationSideEffects::None,
            )
        })?;
        let scope_branches = branches_for_scope(&stack, &options.scope)?;

        self.with_mutation(
            request,
            MutationTargets::branches(scope_branches.clone()),
            || restack_inner(self, request, options, scope_branches, reporter),
        )
    }
}

pub(crate) fn branches_for_scope(
    stack: &Stack,
    scope: &RestackScope,
) -> Result<Vec<String>, OperationError> {
    match scope {
        RestackScope::Branch(branch) => {
            ensure_restack_branch(stack, scope, branch)?;
            Ok(vec![branch.clone()])
        }
        RestackScope::ThroughBranch(branch) => {
            ensure_restack_branch(stack, scope, branch)?;
            let mut branches = stack.ancestors(branch);
            branches.reverse();
            branches.retain(|candidate| candidate != &stack.trunk);
            branches.push(branch.clone());
            Ok(branches)
        }
        RestackScope::StackContaining(branch) => {
            ensure_restack_branch(stack, scope, branch)?;
            let mut branches = stack.ancestors(branch);
            branches.reverse();
            branches.retain(|candidate| candidate != &stack.trunk);
            branches.push(branch.clone());
            collect_descendants_preorder(stack, branch, &mut branches, &mut HashSet::new());
            Ok(branches)
        }
        RestackScope::All => {
            let mut branches = Vec::new();
            let mut visited = HashSet::new();
            let mut roots = stack
                .branches
                .get(&stack.trunk)
                .map(|branch| branch.children.clone())
                .unwrap_or_default();
            roots.sort();
            for root in roots {
                if root != stack.trunk && visited.insert(root.clone()) {
                    branches.push(root.clone());
                    collect_descendants_preorder(stack, &root, &mut branches, &mut visited);
                }
            }
            Ok(branches)
        }
    }
}

fn ensure_restack_branch(
    stack: &Stack,
    scope: &RestackScope,
    branch: &str,
) -> Result<(), OperationError> {
    if branch == stack.trunk {
        return Err(scope_error(
            scope,
            format!("Branch '{branch}' is the trunk branch"),
            "Choose a stacked branch and retry",
            "restack scope selected trunk",
        ));
    }
    if !stack.branches.contains_key(branch) {
        return Err(scope_error(
            scope,
            format!("Branch '{branch}' is not tracked in the stack"),
            "Choose a tracked branch and retry",
            "restack scope selected a missing branch",
        ));
    }
    Ok(())
}

fn collect_descendants_preorder(
    stack: &Stack,
    branch: &str,
    output: &mut Vec<String>,
    visited: &mut HashSet<String>,
) {
    let mut children = stack.children(branch);
    children.sort();
    for child in children {
        if child == stack.trunk || !visited.insert(child.clone()) {
            continue;
        }
        output.push(child.clone());
        collect_descendants_preorder(stack, &child, output, visited);
    }
}

fn restack_inner(
    session: &RepositorySession,
    request: &OperationRequest,
    options: RestackExecutionOptions,
    mut scope_branches: Vec<String>,
    reporter: &mut dyn OperationReporter,
) -> OperationResult {
    let repo = open_operation_repo(session, request)?;
    let current = repo.current_branch().map_err(|error| {
        OperationError::from_source(
            request.clone(),
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "Could not determine the current branch",
            "Check out a branch and retry",
            &error,
            None,
            OperationSideEffects::None,
        )
    })?;
    let restore_branch = options.restore_branch.clone().unwrap_or(current);
    let current_workdir = normalized_workdir(&repo).map_err(|error| {
        local_git_error(
            request,
            "Could not determine the current worktree",
            "Resolve the Git worktree error and retry",
            error,
            None,
        )
    })?;

    let mut stashes = Vec::new();
    let mut stashed_worktree_set = HashSet::new();
    if repo.is_dirty().map_err(|error| {
        local_git_error(
            request,
            "Could not inspect the working tree",
            "Resolve the Git status error and retry",
            error,
            None,
        )
    })? {
        if !options.auto_stash {
            return Err(operation_error(
                request,
                OperationErrorKind::DirtyWorktree,
                OperationErrorDetails::None,
                "Working tree has uncommitted changes",
                "Commit, stash, or enable auto-stash before retrying",
                "current working tree is dirty",
                OperationSideEffects::None,
            ));
        }
        if repo.stash_push().map_err(|error| {
            local_git_error(
                request,
                "Could not stash working tree changes",
                "Resolve the stash error and retry",
                error,
                None,
            )
        })? {
            stashed_worktree_set.insert(current_workdir.clone());
            stashes.push(OwnedStash {
                worktree: current_workdir.clone(),
                restored: false,
            });
        }
    }

    let stack = Stack::load(&repo).map_err(|error| {
        local_git_error(
            request,
            "Could not load the stack",
            "Resolve the stack metadata error and retry",
            error,
            None,
        )
    })?;
    let mut skipped_frozen = Vec::new();
    scope_branches.retain(|branch| {
        let frozen = BranchMetadata::is_frozen(repo.inner(), branch).unwrap_or(false);
        if frozen {
            skipped_frozen.push(branch.clone());
        }
        !frozen
    });
    let branches_to_restack = branches_needing_restack(&stack, &scope_branches);
    let mut warnings = Vec::new();

    if branches_to_restack.is_empty() {
        warnings.extend(restore_stashed_worktrees(&repo, &mut stashes));
        return Ok(restack_receipt(
            request,
            Vec::new(),
            skipped_frozen,
            None,
            warnings,
            OperationSideEffects::None,
        ));
    }

    if !options.auto_stash {
        for branch in &branches_to_restack {
            let target_workdir = repo.branch_rebase_target_workdir(branch).map_err(|error| {
                local_git_error(
                    request,
                    format!("Could not find the rebase worktree for '{branch}'"),
                    "Resolve the Git worktree error and retry",
                    error,
                    Some(branch),
                )
            })?;
            if repo.is_dirty_at(&target_workdir).map_err(|error| {
                local_git_error(
                    request,
                    format!("Could not inspect worktree '{}'", target_workdir.display()),
                    "Resolve the Git status error and retry",
                    error,
                    Some(branch),
                )
            })? {
                return Err(operation_error(
                    request,
                    OperationErrorKind::DirtyWorktree,
                    OperationErrorDetails::Rebase {
                        branch: Some(branch.clone()),
                        worktree: target_workdir,
                    },
                    format!("Worktree for '{branch}' has uncommitted changes"),
                    "Commit, stash, or enable auto-stash before retrying",
                    "target worktree is dirty before restack",
                    OperationSideEffects::None,
                ));
            }
        }
    }

    reporter.report(OperationEvent::Progress(OperationProgress {
        stage: OperationStage::Preparing,
        completed: 0,
        total: Some(scope_branches.len()),
        branch: None,
        message: format!("Preparing to restack {} branch(es)", scope_branches.len()),
    }));

    let mut tx = Transaction::begin(OpKind::Restack, &repo, true).map_err(|error| {
        local_git_error(
            request,
            "Could not start the restack transaction",
            "Resolve the transaction error and retry",
            error,
            None,
        )
    })?;
    tx.plan_branches(&repo, &scope_branches).map_err(|error| {
        local_git_error(
            request,
            "Could not record the restack plan",
            "Resolve the transaction error and retry",
            error,
            None,
        )
    })?;
    tx.set_plan_summary(PlanSummary {
        branches_to_rebase: scope_branches.len(),
        branches_to_push: 0,
        description: vec![format!("Restack up to {} branch(es)", scope_branches.len())],
    });
    tx.set_auto_stash_pop(options.auto_stash);
    tx.snapshot().map_err(|error| {
        local_git_error(
            request,
            "Could not snapshot branches before restack",
            "Resolve the transaction error and retry",
            error,
            None,
        )
    })?;
    let mut tx = Some(tx);

    let config = Config::load().unwrap_or_default();
    let mut completed = Vec::new();
    let mut live_stack = Stack::load(&repo).map_err(|error| {
        finish_transaction_error(
            request,
            &repo,
            &mut tx,
            &completed,
            &stashes,
            "Could not reload the stack",
            "Resolve the stack metadata error and retry",
            error,
            None,
        )
    })?;

    for (index, branch) in scope_branches.iter().enumerate() {
        if options.completed_from_receipt.contains(branch) {
            continue;
        }
        let needs_restack = live_stack
            .branches
            .get(branch)
            .map(|branch| branch.needs_restack)
            .unwrap_or(false);
        if !needs_restack {
            continue;
        }

        let (parent_branch_name, parent_branch_revision) = match live_stack.branches.get(branch) {
            Some(branch) if branch.parent.is_some() && branch.parent_revision.is_some() => (
                branch.parent.clone().unwrap(),
                branch.parent_revision.clone().unwrap(),
            ),
            _ => match BranchMetadata::read(repo.inner(), branch).map_err(|error| {
                finish_transaction_error(
                    request,
                    &repo,
                    &mut tx,
                    &completed,
                    &stashes,
                    format!("Could not read metadata for '{branch}'"),
                    "Resolve the metadata error and retry",
                    error,
                    Some(branch),
                )
            })? {
                Some(metadata) => (metadata.parent_branch_name, metadata.parent_branch_revision),
                None => continue,
            },
        };

        reporter.report(OperationEvent::Progress(OperationProgress {
            stage: OperationStage::Restacking,
            completed: index,
            total: Some(scope_branches.len()),
            branch: Some(branch.clone()),
            message: format!("Restacking {branch} onto {parent_branch_name}"),
        }));

        let (rebase_upstream, warning) = choose_rebase_upstream_data(
            &repo,
            &config,
            branch,
            &parent_branch_name,
            &parent_branch_revision,
        );
        warnings.extend(warning);

        let target_workdir = repo.branch_rebase_target_workdir(branch).map_err(|error| {
            finish_transaction_error(
                request,
                &repo,
                &mut tx,
                &completed,
                &stashes,
                format!("Could not find the rebase worktree for '{branch}'"),
                "Resolve the Git worktree error and retry",
                error,
                Some(branch),
            )
        })?;
        if options.auto_stash
            && !stashed_worktree_set.contains(&target_workdir)
            && repo.is_dirty_at(&target_workdir).map_err(|error| {
                finish_transaction_error(
                    request,
                    &repo,
                    &mut tx,
                    &completed,
                    &stashes,
                    format!("Could not inspect worktree '{}'", target_workdir.display()),
                    "Resolve the Git status error and retry",
                    error,
                    Some(branch),
                )
            })?
            && repo.stash_push_at(&target_workdir).map_err(|error| {
                finish_transaction_error(
                    request,
                    &repo,
                    &mut tx,
                    &completed,
                    &stashes,
                    format!("Could not stash worktree '{}'", target_workdir.display()),
                    "Resolve the stash error and retry",
                    error,
                    Some(branch),
                )
            })?
        {
            stashed_worktree_set.insert(target_workdir.clone());
            stashes.push(OwnedStash {
                worktree: target_workdir.clone(),
                restored: false,
            });
        }

        let pr_state = live_stack
            .branches
            .get(branch)
            .and_then(|branch| branch.pr_state.as_deref())
            .unwrap_or("");
        let pr_is_open = matches!(pr_state.to_uppercase().as_str(), "OPEN" | "DRAFT");
        let rebase_result = if pr_is_open {
            repo.rebase_branch_onto_with_provenance_no_squash_check(
                branch,
                &parent_branch_name,
                &rebase_upstream,
                options.auto_stash,
            )
        } else {
            repo.rebase_branch_onto_with_provenance(
                branch,
                &parent_branch_name,
                &rebase_upstream,
                options.auto_stash,
            )
        }
        .map_err(|error| {
            finish_transaction_error(
                request,
                &repo,
                &mut tx,
                &completed,
                &stashes,
                format!("Could not rebase '{branch}'"),
                "Resolve the Git rebase error and retry",
                error,
                Some(branch),
            )
        })?;

        match rebase_result {
            RebaseResult::Success => {
                let new_parent_rev = repo.branch_commit(&parent_branch_name).map_err(|error| {
                    finish_transaction_error(
                        request,
                        &repo,
                        &mut tx,
                        &completed,
                        &stashes,
                        format!("Could not resolve parent '{parent_branch_name}'"),
                        "Resolve the Git ref error and retry",
                        error,
                        Some(branch),
                    )
                })?;
                let existing_metadata =
                    BranchMetadata::read(repo.inner(), branch).map_err(|error| {
                        finish_transaction_error(
                            request,
                            &repo,
                            &mut tx,
                            &completed,
                            &stashes,
                            format!("Could not read metadata for '{branch}'"),
                            "Resolve the metadata error and retry",
                            error,
                            Some(branch),
                        )
                    })?;
                let source_remote = existing_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.source_remote.clone());
                let frozen = existing_metadata.is_some_and(|metadata| metadata.frozen);
                let updated_metadata = BranchMetadata {
                    parent_branch_name: parent_branch_name.clone(),
                    parent_branch_revision: new_parent_rev.clone(),
                    source_remote,
                    frozen,
                    pr_info: live_stack.branches.get(branch).and_then(|branch| {
                        branch.pr_number.map(|number| PrInfo {
                            number,
                            state: branch.pr_state.clone().unwrap_or_default(),
                            is_draft: branch.pr_is_draft,
                        })
                    }),
                };
                updated_metadata
                    .write(repo.inner(), branch)
                    .map_err(|error| {
                        finish_transaction_error(
                            request,
                            &repo,
                            &mut tx,
                            &completed,
                            &stashes,
                            format!("Could not update metadata for '{branch}'"),
                            "Resolve the metadata error and retry",
                            error,
                            Some(branch),
                        )
                    })?;

                if let Some(branch_info) = live_stack.branches.get_mut(branch) {
                    branch_info.needs_restack = false;
                    branch_info.parent_revision = Some(new_parent_rev.clone());
                }
                let children = live_stack
                    .branches
                    .get(branch)
                    .map(|branch| branch.children.clone())
                    .unwrap_or_default();
                for child in &children {
                    if let Some(child_info) = live_stack.branches.get_mut(child) {
                        child_info.needs_restack = child_info
                            .parent_revision
                            .as_deref()
                            .map(|revision| revision != new_parent_rev)
                            .unwrap_or(true);
                    }
                }

                transaction_mut(&mut tx)
                    .record_after(&repo, branch)
                    .map_err(|error| {
                        finish_transaction_error(
                            request,
                            &repo,
                            &mut tx,
                            &completed,
                            &stashes,
                            format!("Could not record the new tip for '{branch}'"),
                            "Resolve the transaction error and retry",
                            error,
                            Some(branch),
                        )
                    })?;
                transaction_mut(&mut tx).push_completed_branch(branch);
                completed.push(branch.clone());
            }
            RebaseResult::Conflict => {
                if !repo.has_conflicts_in(&target_workdir).unwrap_or(true) {
                    return Err(finalize_restack_failure(
                        request,
                        RestackFailureState {
                            transaction: tx.take(),
                            completed_branches: completed,
                            stashes,
                            side_effects: OperationSideEffects::RepositoryChanged,
                        },
                        OperationErrorKind::LocalGit,
                        OperationErrorDetails::Branch {
                            branch: branch.clone(),
                        },
                        format!("Could not finish rebasing '{branch}'"),
                        "Inspect the rebase state, then run `st continue` or `st abort`"
                            .to_string(),
                        anyhow::anyhow!(
                            "git rebase stopped for '{branch}' without unresolved file conflicts"
                        ),
                        "rebase",
                        Some(branch),
                    ));
                }
                return Err(finalize_restack_failure(
                    request,
                    RestackFailureState {
                        transaction: tx.take(),
                        completed_branches: completed,
                        stashes,
                        side_effects: OperationSideEffects::RepositoryChanged,
                    },
                    OperationErrorKind::RebaseConflict,
                    OperationErrorDetails::Rebase {
                        branch: Some(branch.clone()),
                        worktree: target_workdir,
                    },
                    format!("Restack stopped on a conflict in '{branch}'"),
                    "Resolve the conflicts and run `st continue`, or run `st abort`".into(),
                    anyhow::anyhow!(
                        "rebase conflict while rebasing '{branch}' onto '{parent_branch_name}'"
                    ),
                    "rebase",
                    Some(branch),
                ));
            }
        }
    }

    repo.checkout(&restore_branch).map_err(|error| {
        finish_transaction_error(
            request,
            &repo,
            &mut tx,
            &completed,
            &stashes,
            format!("Could not restore checkout to '{restore_branch}'"),
            "Inspect the repository checkout state and retry",
            error,
            None,
        )
    })?;
    let stash_warnings = restore_stashed_worktrees(&repo, &mut stashes);
    if !stash_warnings.is_empty() {
        let diagnostic = stash_warnings
            .iter()
            .map(|warning| match warning {
                OperationWarning::StashRestoreFailed {
                    worktree,
                    diagnostic,
                } => format!("{}: {diagnostic}", worktree.display()),
                _ => String::new(),
            })
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        return Err(finalize_restack_failure(
            request,
            RestackFailureState {
                transaction: tx.take(),
                completed_branches: completed,
                stashes,
                side_effects: OperationSideEffects::RepositoryChanged,
            },
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "Could not restore stashed worktree changes".to_string(),
            "Inspect the listed worktrees and restore their stashes manually".to_string(),
            anyhow::anyhow!(diagnostic),
            "stash",
            None,
        ));
    }
    warnings.extend(stash_warnings);
    let finalized = tx
        .take()
        .expect("restack transaction should be present for successful finalization")
        .finish_ok_preserving_receipt();
    let transaction = TransactionSummary::from(&finalized.receipt);
    let receipt = restack_receipt(
        request,
        completed,
        skipped_frozen,
        Some(transaction),
        warnings,
        OperationSideEffects::RepositoryChanged,
    );
    if let Some(error) = finalized.persistence_error {
        return Err(OperationError::from_source(
            request.clone(),
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "Restack completed but its receipt could not be saved",
            "Inspect the repository and retry if needed",
            &error,
            Some(receipt),
            OperationSideEffects::RepositoryChanged,
        ));
    }
    Ok(receipt)
}

fn choose_rebase_upstream_data(
    repo: &GitRepo,
    config: &Config,
    branch: &str,
    parent: &str,
    stored_revision: &str,
) -> (String, Option<OperationWarning>) {
    let decision = choose_rebase_upstream(repo, config, branch, parent, stored_revision, true);
    let warning = (decision.adjusted && config.restack.preflight_warn).then(|| {
        OperationWarning::RestackBoundaryAdjusted {
            branch: branch.to_string(),
            reason: decision.reason.clone().unwrap_or_default(),
        }
    });
    (decision.upstream, warning)
}

fn normalized_workdir(repo: &GitRepo) -> anyhow::Result<PathBuf> {
    Ok(GitRepo::normalize_path(repo.workdir()?))
}

fn transaction_mut(transaction: &mut Option<Transaction>) -> &mut Transaction {
    transaction
        .as_mut()
        .expect("restack transaction should be present before finalization")
}

fn restore_stashed_worktrees(repo: &GitRepo, stashes: &mut [OwnedStash]) -> Vec<OperationWarning> {
    let mut warnings = Vec::new();
    for stash in stashes.iter_mut().rev() {
        if stash.restored {
            continue;
        }
        if let Err(error) = repo.stash_pop_at(&stash.worktree) {
            warnings.push(OperationWarning::StashRestoreFailed {
                worktree: stash.worktree.clone(),
                diagnostic: format!("{error:#}"),
            });
        } else {
            stash.restored = true;
        }
    }
    warnings
}

fn restore_failure_stashes(repo: &GitRepo, stashes: &mut [OwnedStash]) {
    for stash in stashes.iter_mut().rev() {
        if stash.restored {
            continue;
        }
        if repo.rebase_in_progress_in(&stash.worktree).unwrap_or(false) {
            continue;
        }
        if repo.stash_pop_at(&stash.worktree).is_ok() {
            stash.restored = true;
        }
    }
}

fn branches_needing_restack(stack: &Stack, scope: &[String]) -> Vec<String> {
    scope
        .iter()
        .filter(|branch| {
            stack
                .branches
                .get(*branch)
                .map(|branch| branch.needs_restack)
                .unwrap_or(false)
        })
        .cloned()
        .collect()
}

fn restack_receipt(
    request: &OperationRequest,
    branches: Vec<String>,
    skipped_frozen: Vec<String>,
    transaction: Option<TransactionSummary>,
    warnings: Vec<OperationWarning>,
    side_effects: OperationSideEffects,
) -> OperationReceipt {
    let summary = if branches.is_empty() {
        "Stack is up to date, nothing to restack".to_string()
    } else {
        format!("Restacked {} branch(es)", branches.len())
    };
    let mut affected_branches = branches.clone();
    for branch in &skipped_frozen {
        if !affected_branches.contains(branch) {
            affected_branches.push(branch.clone());
        }
    }
    OperationReceipt {
        request: request.clone(),
        summary,
        affected_branches,
        outcome: OperationOutcome::Restacked {
            branches,
            skipped_frozen,
        },
        transaction,
        warnings,
        side_effects,
    }
}

fn open_operation_repo(
    session: &RepositorySession,
    request: &OperationRequest,
) -> Result<GitRepo, OperationError> {
    session.open_repo().map_err(|error| {
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
    })
}

fn finish_transaction_error(
    request: &OperationRequest,
    repo: &GitRepo,
    transaction: &mut Option<Transaction>,
    completed_branches: &[String],
    stashes: &[OwnedStash],
    primary: impl Into<String>,
    action: impl Into<String>,
    source: anyhow::Error,
    branch: Option<&str>,
) -> OperationError {
    let mut stashes = stashes.to_vec();
    restore_failure_stashes(repo, &mut stashes);
    let primary = primary.into();
    let action = action.into();
    finalize_restack_failure(
        request,
        RestackFailureState {
            transaction: transaction.take(),
            completed_branches: completed_branches.to_vec(),
            stashes,
            side_effects: OperationSideEffects::RepositoryChanged,
        },
        OperationErrorKind::LocalGit,
        branch
            .map(|branch| OperationErrorDetails::Branch {
                branch: branch.to_string(),
            })
            .unwrap_or(OperationErrorDetails::None),
        primary,
        action,
        source,
        "rebase",
        branch,
    )
}

fn finalize_restack_failure(
    request: &OperationRequest,
    state: RestackFailureState,
    kind: OperationErrorKind,
    details: OperationErrorDetails,
    primary: String,
    action: String,
    source: anyhow::Error,
    failed_step: &'static str,
    failed_branch: Option<&str>,
) -> OperationError {
    let mut diagnostic_chain = format!("{source:#}");
    let transaction = state.transaction.map(|transaction| {
        let finalized =
            transaction.finish_err_preserving_receipt(&primary, Some(failed_step), failed_branch);
        if let Some(error) = finalized.persistence_error {
            diagnostic_chain.push_str("\nreceipt persistence failure: ");
            diagnostic_chain.push_str(&format!("{error:#}"));
        }
        TransactionSummary::from(&finalized.receipt)
    });
    let receipt = transaction.map(|transaction| {
        restack_receipt(
            request,
            state.completed_branches,
            Vec::new(),
            Some(transaction),
            Vec::new(),
            state.side_effects,
        )
    });
    let pending_stashes = state
        .stashes
        .iter()
        .filter(|stash| !stash.restored)
        .map(|stash| stash.worktree.display().to_string())
        .collect::<Vec<_>>();
    let action = if pending_stashes.is_empty() {
        action
    } else {
        format!(
            "{action}. Operation-owned stashes remain in: {}",
            pending_stashes.join(", ")
        )
    };

    OperationError {
        request: request.clone(),
        kind,
        details,
        primary,
        action,
        diagnostic_chain,
        receipt,
        side_effects: state.side_effects,
    }
}

fn local_git_error(
    request: &OperationRequest,
    primary: impl Into<String>,
    action: impl Into<String>,
    source: anyhow::Error,
    branch: Option<&str>,
) -> OperationError {
    OperationError::from_source(
        request.clone(),
        OperationErrorKind::LocalGit,
        branch
            .map(|branch| OperationErrorDetails::Branch {
                branch: branch.to_string(),
            })
            .unwrap_or(OperationErrorDetails::None),
        primary,
        action,
        &source,
        None,
        OperationSideEffects::None,
    )
}

fn scope_error(
    scope: &RestackScope,
    primary: impl Into<String>,
    action: impl Into<String>,
    diagnostic_chain: impl Into<String>,
) -> OperationError {
    let request = OperationRequest::Restack {
        scope: scope.clone(),
        auto_stash: false,
    };
    operation_error(
        &request,
        OperationErrorKind::InvalidInput,
        OperationErrorDetails::None,
        primary,
        action,
        diagnostic_chain,
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
    use super::branches_for_scope;
    use crate::application::{OperationErrorKind, RestackScope};
    use crate::engine::Stack;
    use crate::engine::stack::StackBranch;
    use std::collections::HashMap;

    fn branch(name: &str, parent: Option<&str>, children: &[&str]) -> StackBranch {
        StackBranch {
            name: name.to_string(),
            parent: parent.map(str::to_string),
            parent_revision: parent.map(|parent| format!("{parent}-revision")),
            children: children.iter().map(|child| (*child).to_string()).collect(),
            needs_restack: true,
            pr_number: None,
            pr_state: None,
            pr_is_draft: None,
        }
    }

    fn forked_stack() -> Stack {
        let mut branches = HashMap::new();
        branches.insert("main".into(), branch("main", None, &["base"]));
        branches.insert(
            "base".into(),
            branch("base", Some("main"), &["selected", "unrelated-sibling"]),
        );
        branches.insert(
            "selected".into(),
            branch("selected", Some("base"), &["child-b", "child-a"]),
        );
        branches.insert(
            "child-a".into(),
            branch("child-a", Some("selected"), &["grandchild"]),
        );
        branches.insert(
            "grandchild".into(),
            branch("grandchild", Some("child-a"), &[]),
        );
        branches.insert("child-b".into(), branch("child-b", Some("selected"), &[]));
        branches.insert(
            "unrelated-sibling".into(),
            branch("unrelated-sibling", Some("base"), &[]),
        );
        Stack {
            branches,
            trunk: "main".into(),
        }
    }

    #[test]
    fn branches_for_scope_stack_containing_uses_selected_subtree_only() {
        let stack = forked_stack();

        assert_eq!(
            branches_for_scope(&stack, &RestackScope::StackContaining("selected".into())).unwrap(),
            vec!["base", "selected", "child-a", "grandchild", "child-b"],
        );
    }

    #[test]
    fn branches_for_scope_through_branch_excludes_descendants() {
        let stack = forked_stack();

        assert_eq!(
            branches_for_scope(&stack, &RestackScope::ThroughBranch("selected".into())).unwrap(),
            vec!["base", "selected"],
        );
    }

    #[test]
    fn branches_for_scope_all_uses_depth_first_lexical_order() {
        let stack = forked_stack();

        assert_eq!(
            branches_for_scope(&stack, &RestackScope::All).unwrap(),
            vec![
                "base",
                "selected",
                "child-a",
                "grandchild",
                "child-b",
                "unrelated-sibling",
            ],
        );
    }

    #[test]
    fn branches_for_scope_rejects_trunk_and_missing_branches() {
        let stack = forked_stack();

        let trunk = branches_for_scope(&stack, &RestackScope::Branch("main".into())).unwrap_err();
        assert_eq!(trunk.kind, OperationErrorKind::InvalidInput);

        let missing =
            branches_for_scope(&stack, &RestackScope::Branch("missing".into())).unwrap_err();
        assert_eq!(missing.kind, OperationErrorKind::InvalidInput);
    }
}
