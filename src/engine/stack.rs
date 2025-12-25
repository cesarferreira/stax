use crate::engine::BranchMetadata;
use crate::git::{refs, GitRepo};
use anyhow::Result;
use std::collections::HashMap;

/// Represents a branch in the stack
#[derive(Debug, Clone)]
pub struct StackBranch {
    pub name: String,
    pub parent: Option<String>,
    pub children: Vec<String>,
    pub needs_restack: bool,
    pub pr_number: Option<u64>,
    pub pr_state: Option<String>,
    pub pr_is_draft: Option<bool>,
}

/// The full stack structure
pub struct Stack {
    pub branches: HashMap<String, StackBranch>,
    pub trunk: String,
}

impl Stack {
    /// Load the stack from git metadata
    pub fn load(repo: &GitRepo) -> Result<Self> {
        let trunk = repo.trunk_branch()?;
        let tracked_branches = refs::list_metadata_branches(repo.inner())?;

        let mut branches: HashMap<String, StackBranch> = HashMap::new();

        // First pass: load all metadata
        for branch_name in &tracked_branches {
            if let Some(meta) = BranchMetadata::read(repo.inner(), branch_name)? {
                let needs_restack = meta.needs_restack(repo.inner()).unwrap_or(false);
                branches.insert(
                    branch_name.clone(),
                    StackBranch {
                        name: branch_name.clone(),
                        parent: Some(meta.parent_branch_name.clone()),
                        children: Vec::new(),
                        needs_restack,
                        pr_number: meta.pr_info.as_ref().map(|p| p.number),
                        pr_state: meta.pr_info.as_ref().map(|p| p.state.clone()),
                        pr_is_draft: meta.pr_info.as_ref().and_then(|p| p.is_draft),
                    },
                );
            }
        }

        // Second pass: populate children and find orphans
        let branch_names: Vec<String> = branches.keys().cloned().collect();
        let mut orphaned_branches: Vec<String> = Vec::new();

        for name in branch_names {
            if let Some(parent_name) = branches.get(&name).and_then(|b| b.parent.clone()) {
                if parent_name == trunk {
                    // Direct child of trunk - will be handled below
                    continue;
                }
                if let Some(parent) = branches.get_mut(&parent_name) {
                    parent.children.push(name.clone());
                } else {
                    // Parent doesn't exist - this branch is orphaned
                    // Treat it as a direct child of trunk
                    orphaned_branches.push(name.clone());
                }
            }
        }

        // Collect direct children of trunk (including orphaned branches)
        let mut trunk_children: Vec<String> = branches
            .values()
            .filter(|b| b.parent.as_ref() == Some(&trunk))
            .map(|b| b.name.clone())
            .collect();
        trunk_children.extend(orphaned_branches);

        // Add trunk as a root
        branches.insert(
            trunk.clone(),
            StackBranch {
                name: trunk.clone(),
                parent: None,
                children: trunk_children,
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );

        Ok(Self { branches, trunk })
    }

    /// Get the ancestors of a branch (up to trunk)
    pub fn ancestors(&self, branch: &str) -> Vec<String> {
        let mut result = Vec::new();
        let mut current = branch.to_string();

        while let Some(b) = self.branches.get(&current) {
            if let Some(parent) = &b.parent {
                result.push(parent.clone());
                current = parent.clone();
            } else {
                break;
            }
        }

        result
    }

    /// Get all descendants of a branch
    pub fn descendants(&self, branch: &str) -> Vec<String> {
        let mut result = Vec::new();
        let mut to_visit = vec![branch.to_string()];

        while let Some(current) = to_visit.pop() {
            if let Some(b) = self.branches.get(&current) {
                for child in &b.children {
                    result.push(child.clone());
                    to_visit.push(child.clone());
                }
            }
        }

        result
    }

    /// Get the current stack (ancestors + current + descendants)
    pub fn current_stack(&self, branch: &str) -> Vec<String> {
        let mut ancestors = self.ancestors(branch);
        ancestors.reverse();
        ancestors.push(branch.to_string());
        ancestors.extend(self.descendants(branch));
        ancestors
    }

    /// Get branches that need restacking
    pub fn needs_restack(&self) -> Vec<String> {
        self.branches
            .values()
            .filter(|b| b.needs_restack)
            .map(|b| b.name.clone())
            .collect()
    }

}
