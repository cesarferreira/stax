use crate::engine::BranchMetadata;
use crate::git::{GitRepo, RebaseResult};
use anyhow::Result;

pub(crate) fn fetch_remote_for_descendant_rebase(
    repo: &GitRepo,
    remote_name: &str,
) -> Result<bool> {
    repo.fetch_remote(remote_name)
}

pub(crate) fn rebase_descendant_onto_remote_trunk_with_provenance(
    repo: &GitRepo,
    branch: &str,
    trunk: &str,
    remote_name: &str,
) -> Result<RebaseResult> {
    rebase_descendant_onto_parent_with_provenance(repo, branch, trunk, remote_name, true)
}

pub(crate) fn rebase_descendant_onto_parent_with_provenance(
    repo: &GitRepo,
    branch: &str,
    parent: &str,
    remote_name: &str,
    use_remote_parent_ref: bool,
) -> Result<RebaseResult> {
    let onto_ref = if use_remote_parent_ref {
        format!("{}/{}", remote_name, parent)
    } else {
        parent.to_string()
    };

    let fallback_upstream = BranchMetadata::read(repo.inner(), branch)?
        .map(|meta| meta.parent_branch_revision)
        .unwrap_or_default();

    let result =
        repo.rebase_branch_onto_with_provenance(branch, &onto_ref, &fallback_upstream, false)?;

    if result == RebaseResult::Success {
        if let Some(meta) = BranchMetadata::read(repo.inner(), branch)? {
            let parent_commit = repo
                .resolve_ref(&onto_ref)
                .unwrap_or_else(|_| repo.branch_commit(parent).unwrap_or_default());
            let updated_meta = BranchMetadata {
                parent_branch_name: parent.to_string(),
                parent_branch_revision: parent_commit,
                ..meta
            };
            updated_meta.write(repo.inner(), branch)?;
        }
    }

    Ok(result)
}
