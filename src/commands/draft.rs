use crate::config::Config;
use crate::engine::metadata::PrInfo;
use crate::engine::{BranchMetadata, Stack};
use crate::forge::ForgeClient;
use crate::git::GitRepo;
use crate::remote::RemoteInfo;
use anyhow::Result;
use colored::Colorize;

pub fn run(branch: Option<String>, stack: bool, is_draft: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let stack_data = Stack::load(&repo)?;
    let config = Config::load()?;
    let current = repo.current_branch()?;

    let branches = if stack {
        stack_data
            .current_stack(&current)
            .into_iter()
            .filter(|name| name != &stack_data.trunk)
            .collect::<Vec<_>>()
    } else {
        let target = branch.unwrap_or(current);
        let branch_info = stack_data.branches.get(&target);
        if branch_info.is_none() {
            anyhow::bail!(
                "Branch '{}' is not tracked. Use {} to track it first.",
                target,
                "stax branch track".cyan()
            );
        }
        vec![target]
    };

    let remote_info = RemoteInfo::from_repo(&repo, &config)?;
    let rt = tokio::runtime::Runtime::new()?;
    let _enter = rt.enter();
    let client = ForgeClient::new(&remote_info)?;

    let mut skipped_without_pr = Vec::new();
    let mut processed = 0usize;

    for target in branches {
        let Some(pr_number) =
            super::resolve_pr::resolve_pr_number(&repo, &stack_data, &target, &config)?
        else {
            skipped_without_pr.push(target);
            continue;
        };

        process_branch(&repo, &rt, &client, &target, pr_number, is_draft)?;
        processed += 1;
    }

    if processed == 0 {
        anyhow::bail!(
            "No PRs found in {}. Use {} to create one.",
            if stack {
                "the current stack"
            } else {
                "this branch"
            },
            "stax submit".cyan()
        );
    }

    if stack && !skipped_without_pr.is_empty() {
        eprintln!(
            "Skipped {} without a PR: {}",
            skipped_without_pr.len(),
            skipped_without_pr.join(", ").dimmed()
        );
    }

    Ok(())
}

fn process_branch(
    repo: &GitRepo,
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    branch: &str,
    pr_number: u64,
    is_draft: bool,
) -> Result<()> {
    let remote_pr = rt.block_on(async { client.get_pr(pr_number).await })?;

    if remote_pr.is_draft == is_draft {
        update_local_pr_metadata(repo, branch, pr_number, is_draft);
        let state = if is_draft {
            "already a draft"
        } else {
            "already published"
        };
        println!("PR #{} on {} is {}.", pr_number, branch.cyan(), state);
        return Ok(());
    }

    rt.block_on(async { client.set_pr_draft(pr_number, is_draft).await })?;

    update_local_pr_metadata(repo, branch, pr_number, is_draft);

    if is_draft {
        println!(
            "PR #{} on {} marked as {}.",
            pr_number.to_string().cyan(),
            branch.cyan(),
            "draft".yellow()
        );
    } else {
        println!(
            "PR #{} on {} marked as {}.",
            pr_number.to_string().cyan(),
            branch.cyan(),
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
