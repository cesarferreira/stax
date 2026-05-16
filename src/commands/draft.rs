use crate::config::Config;
use crate::engine::metadata::PrInfo;
use crate::engine::{BranchMetadata, Stack};
use crate::forge::ForgeClient;
use crate::git::GitRepo;
use crate::remote::RemoteInfo;
use anyhow::Result;
use colored::Colorize;

pub fn run(branch: Option<String>, is_draft: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let stack = Stack::load(&repo)?;
    let config = Config::load()?;

    let target = branch.unwrap_or_else(|| repo.current_branch().unwrap_or_default());

    let branch_info = stack.branches.get(&target);
    if branch_info.is_none() {
        anyhow::bail!(
            "Branch '{}' is not tracked. Use {} to track it first.",
            target,
            "stax branch track".cyan()
        );
    }

    let pr_number = super::resolve_pr::resolve_pr_number(&repo, &stack, &target, &config)?;
    let Some(pr_number) = pr_number else {
        anyhow::bail!(
            "No PR found for branch '{}'. Use {} to create one.",
            target,
            "stax submit".cyan()
        );
    };

    let remote_info = RemoteInfo::from_repo(&repo, &config)?;
    let rt = tokio::runtime::Runtime::new()?;
    let _enter = rt.enter();
    let client = ForgeClient::new(&remote_info)?;

    let remote_pr = rt.block_on(async { client.get_pr(pr_number).await })?;

    if remote_pr.is_draft == is_draft {
        update_local_pr_metadata(&repo, &target, pr_number, is_draft);
        let state = if is_draft {
            "already a draft"
        } else {
            "already published"
        };
        println!("PR #{} is {}.", pr_number, state);
        return Ok(());
    }

    rt.block_on(async { client.set_pr_draft(pr_number, is_draft).await })?;

    update_local_pr_metadata(&repo, &target, pr_number, is_draft);

    if is_draft {
        println!(
            "PR #{} on {} marked as {}.",
            pr_number.to_string().cyan(),
            target.cyan(),
            "draft".yellow()
        );
    } else {
        println!(
            "PR #{} on {} marked as {}.",
            pr_number.to_string().cyan(),
            target.cyan(),
            "ready for review".green()
        );
    }

    Ok(())
}

fn update_local_pr_metadata(repo: &GitRepo, branch: &str, pr_number: u64, is_draft: bool) {
    if let Ok(Some(mut meta)) = BranchMetadata::read(repo.inner(), branch) {
        if let Some(ref mut pr_info) = meta.pr_info {
            pr_info.is_draft = Some(is_draft);
        } else {
            meta.pr_info = Some(PrInfo {
                number: pr_number,
                state: "open".to_string(),
                is_draft: Some(is_draft),
            });
        }
        let _ = meta.write(repo.inner(), branch);
    }
}
