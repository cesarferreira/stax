use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use crate::cache::CiCache;
use crate::config::Config;
use crate::engine::Stack;
use crate::git::GitRepo;
use crate::remote::RemoteInfo;

use super::protocol::{
    BranchSnapshot, DesktopError, PullRequestSnapshot, RecommendedAction, RepositorySnapshot,
    RepositoryState,
};

pub(super) fn build(repo_path: &Path) -> Result<RepositorySnapshot, DesktopError> {
    let repo = GitRepo::open_from_path(repo_path).map_err(|error| {
        DesktopError::operation(
            "invalid_repository",
            "The selected folder is not a Git repository.",
            error.to_string(),
            "choose_repository",
        )
    })?;

    build_from_repo(&repo).map_err(snapshot_error)
}

fn build_from_repo(repo: &GitRepo) -> anyhow::Result<RepositorySnapshot> {
    let workdir = repo.workdir()?;
    let canonical_workdir =
        std::fs::canonicalize(workdir).unwrap_or_else(|_| workdir.to_path_buf());
    let stack = Stack::load(repo)?;
    let current_branch = repo.current_branch()?;
    let dirty = repo.is_dirty()?;
    let conflicted_files = repo.conflicted_files()?;
    let repository_state = if !conflicted_files.is_empty() {
        RepositoryState::ConflictInProgress
    } else if repo.rebase_in_progress()? {
        RepositoryState::RebaseInProgress
    } else {
        RepositoryState::Normal
    };
    let cache = CiCache::load(repo.git_dir()?);
    let remote_info = Config::load()
        .ok()
        .and_then(|config| RemoteInfo::from_repo(repo, &config).ok());
    let ordered = display_order(&stack);
    let generation = snapshot_generation(repo, &canonical_workdir, &current_branch, &ordered)?;

    let mut branches = Vec::with_capacity(ordered.len());
    for (name, column) in ordered {
        let info = stack
            .branches
            .get(&name)
            .ok_or_else(|| anyhow::anyhow!("Stack branch '{name}' disappeared"))?;
        let is_trunk = name == stack.trunk;
        let is_current = name == current_branch;
        let (ahead, behind) = info
            .parent
            .as_deref()
            .map(|parent| repo.commits_ahead_behind(parent, &name))
            .transpose()?
            .unwrap_or((0, 0));
        let (unpushed, unpulled) = repo.commits_vs_remote(&name).unwrap_or((0, 0));
        let pull_request =
            info.pr_number
                .filter(|number| *number > 0)
                .map(|number| PullRequestSnapshot {
                    number,
                    state: info
                        .pr_state
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string()),
                    is_draft: info.pr_is_draft.unwrap_or(false),
                    url: remote_info.as_ref().map(|remote| remote.pr_url(number)),
                });
        let recommended_action = recommended_action(
            is_trunk,
            is_current,
            info.needs_restack,
            ahead,
            pull_request.is_some(),
        );

        branches.push(BranchSnapshot {
            name: name.clone(),
            parent: info.parent.clone(),
            column,
            is_current,
            is_trunk,
            ahead,
            behind,
            needs_restack: info.needs_restack,
            has_remote: repo.has_remote(&name),
            unpushed,
            unpulled,
            pull_request,
            ci_state: cache.get_ci_state(&name),
            recommended_action,
        });
    }

    let repository_name = canonical_workdir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| canonical_workdir.to_string_lossy().into_owned());

    Ok(RepositorySnapshot {
        generation,
        repository_path: canonical_workdir.to_string_lossy().into_owned(),
        repository_name,
        trunk: stack.trunk,
        current_branch,
        repository_state,
        dirty,
        branches,
    })
}

fn snapshot_error(error: anyhow::Error) -> DesktopError {
    DesktopError::operation(
        "snapshot_failed",
        "The repository could not be inspected.",
        format!("{error:#}"),
        "refresh",
    )
}

fn recommended_action(
    is_trunk: bool,
    is_current: bool,
    needs_restack: bool,
    ahead: usize,
    has_pull_request: bool,
) -> RecommendedAction {
    if is_trunk {
        RecommendedAction::None
    } else if !is_current {
        RecommendedAction::Checkout
    } else if needs_restack {
        RecommendedAction::Restack
    } else if !has_pull_request && ahead > 0 {
        RecommendedAction::SubmitStack
    } else if has_pull_request {
        RecommendedAction::OpenPr
    } else {
        RecommendedAction::None
    }
}

fn snapshot_generation(
    repo: &GitRepo,
    canonical_workdir: &Path,
    current_branch: &str,
    ordered: &[(String, usize)],
) -> anyhow::Result<String> {
    let mut hasher = DefaultHasher::new();
    canonical_workdir.hash(&mut hasher);
    current_branch.hash(&mut hasher);
    for (name, _) in ordered {
        name.hash(&mut hasher);
        repo.branch_commit(name)?.hash(&mut hasher);
    }
    Ok(format!("{:016x}", hasher.finish()))
}

fn display_order(stack: &Stack) -> Vec<(String, usize)> {
    #[derive(Clone)]
    struct Frame {
        branch: String,
        column: usize,
        expanded: bool,
    }

    let mut result = Vec::with_capacity(stack.branches.len());
    let mut frames = vec![Frame {
        branch: stack.trunk.clone(),
        column: 0,
        expanded: false,
    }];
    let mut visiting = HashSet::new();
    let mut emitted = HashSet::new();

    while let Some(frame) = frames.pop() {
        if frame.expanded {
            visiting.remove(&frame.branch);
            if emitted.insert(frame.branch.clone()) {
                result.push((frame.branch, frame.column));
            }
            continue;
        }

        if emitted.contains(&frame.branch) || !visiting.insert(frame.branch.clone()) {
            continue;
        }

        frames.push(Frame {
            branch: frame.branch.clone(),
            column: frame.column,
            expanded: true,
        });

        if let Some(info) = stack.branches.get(&frame.branch) {
            let mut children = info.children.iter().collect::<Vec<_>>();
            children.sort();
            for (index, child) in children.into_iter().enumerate().rev() {
                if emitted.contains(child) || visiting.contains(child) {
                    continue;
                }
                frames.push(Frame {
                    branch: child.clone(),
                    column: frame.column + index,
                    expanded: false,
                });
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::engine::stack::StackBranch;

    use super::*;

    fn branch(name: &str, parent: Option<&str>, children: &[&str]) -> StackBranch {
        StackBranch {
            name: name.to_string(),
            parent: parent.map(str::to_string),
            parent_revision: None,
            children: children.iter().map(|child| (*child).to_string()).collect(),
            needs_restack: false,
            pr_number: None,
            pr_state: None,
            pr_is_draft: None,
        }
    }

    #[test]
    fn display_order_terminates_when_metadata_contains_a_cycle() {
        let branches = HashMap::from([
            ("main".to_string(), branch("main", None, &["a"])),
            ("a".to_string(), branch("a", Some("main"), &["b"])),
            ("b".to_string(), branch("b", Some("a"), &["a"])),
        ]);
        let stack = Stack {
            branches,
            trunk: "main".to_string(),
        };

        assert_eq!(
            display_order(&stack),
            vec![
                ("b".to_string(), 0),
                ("a".to_string(), 0),
                ("main".to_string(), 0),
            ]
        );
    }
}
