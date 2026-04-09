use crate::config::Config;
use crate::engine::metadata::BranchMetadata;
use crate::engine::Stack;
use crate::forge::ForgeClient;
use crate::git::GitRepo;
use crate::remote::RemoteInfo;
use anyhow::Result;

/// Resolve the PR number for a tracked branch.
///
/// 1. Returns the locally stored `pr_number` when available.
/// 2. Falls back to a forge lookup (`find_pr`) when metadata is missing.
/// 3. Persists the discovered PR number to branch metadata so future
///    commands skip the network round-trip.
///
/// Returns `None` when no PR exists locally or remotely, or when the
/// forge is unreachable (missing token, network error, etc.).
pub fn resolve_pr_number(
    repo: &GitRepo,
    stack: &Stack,
    branch: &str,
    config: &Config,
) -> Result<Option<u64>> {
    // Check local metadata first
    if let Some(branch_info) = stack.branches.get(branch) {
        if let Some(pr_number) = branch_info.pr_number {
            return Ok(Some(pr_number));
        }
    }

    // Fall back to forge lookup — all failures are non-fatal so that
    // missing tokens, network errors, etc. degrade gracefully.
    let remote_info = match RemoteInfo::from_repo(repo, config) {
        Ok(info) => info,
        Err(_) => return Ok(None),
    };
    let rt = tokio::runtime::Runtime::new()?;
    let _enter = rt.enter();
    let client = match ForgeClient::new(&remote_info) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };

    let pr_number = match rt.block_on(async { client.find_pr(branch).await }) {
        Ok(Some(pr_info)) => pr_info.number,
        _ => return Ok(None),
    };

    // Persist discovered PR number to metadata
    if let Ok(Some(mut meta)) = BranchMetadata::read(repo.inner(), branch) {
        if meta.pr_info.is_none() {
            meta.pr_info = Some(crate::engine::metadata::PrInfo {
                number: pr_number,
                state: "open".to_string(),
                is_draft: None,
            });
            let _ = meta.write(repo.inner(), branch);
        }
    }

    Ok(Some(pr_number))
}
