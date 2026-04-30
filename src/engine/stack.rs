use crate::engine::BranchMetadata;
use crate::git::{refs, GitRepo};
use anyhow::Result;
use git2::BranchType;
use std::collections::{HashMap, HashSet};

/// Represents a branch in the stack
#[derive(Debug, Clone)]
pub struct StackBranch {
    pub name: String,
    pub parent: Option<String>,
    /// Stored SHA of the parent at the time this branch was last rebased.
    /// `None` for trunk (which has no parent).
    pub parent_revision: Option<String>,
    pub children: Vec<String>,
    pub needs_restack: bool,
    pub pr_number: Option<u64>,
    pub pr_state: Option<String>,
    pub pr_is_draft: Option<bool>,
}

/// The full stack structure
#[derive(Debug, Clone)]
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
            // Metadata can outlive branches (e.g. interrupted delete). Ignore and prune it.
            if repo
                .inner()
                .find_branch(branch_name, BranchType::Local)
                .is_err()
            {
                let _ = BranchMetadata::delete(repo.inner(), branch_name);
                continue;
            }

            if let Some(meta) = BranchMetadata::read(repo.inner(), branch_name)? {
                let needs_restack = meta.needs_restack(repo.inner()).unwrap_or(false);
                branches.insert(
                    branch_name.clone(),
                    StackBranch {
                        name: branch_name.clone(),
                        parent: Some(meta.parent_branch_name.clone()),
                        parent_revision: Some(meta.parent_branch_revision.clone()),
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
                    // Parent doesn't exist - this branch is orphaned.
                    // Treat it as a direct child of trunk and mark it for restack so
                    // the restack loop will rebase it onto trunk.  We preserve
                    // parent_revision (the deleted branch's tip SHA) so that
                    // `git rebase --onto trunk <old_parent_sha> branch` replays only
                    // the branch's own commits and not the deleted parent's commits.
                    orphaned_branches.push(name.clone());
                }
            }
        }

        // Reparent orphaned branches to trunk in-memory so the restack loop
        // picks them up with the correct parent.
        for name in &orphaned_branches {
            if let Some(br) = branches.get_mut(name) {
                br.parent = Some(trunk.clone());
                br.needs_restack = true;
            }
        }

        // Collect direct children of trunk, including orphaned branches that
        // were reparented to trunk above.
        let trunk_children: Vec<String> = branches
            .values()
            .filter(|b| b.parent.as_ref() == Some(&trunk))
            .map(|b| b.name.clone())
            .collect();

        // Add trunk as a root
        branches.insert(
            trunk.clone(),
            StackBranch {
                name: trunk.clone(),
                parent: None,
                parent_revision: None,
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
        let mut visited = HashSet::from([current.clone()]);

        while let Some(b) = self.branches.get(&current) {
            if let Some(parent) = &b.parent {
                if !visited.insert(parent.clone()) {
                    break;
                }
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
        let mut visited = HashSet::from([branch.to_string()]);

        while let Some(current) = to_visit.pop() {
            if let Some(b) = self.branches.get(&current) {
                for child in &b.children {
                    if !visited.insert(child.clone()) {
                        continue;
                    }
                    result.push(child.clone());
                    to_visit.push(child.clone());
                }
            }
        }

        result
    }

    /// Get the current stack (ancestors + current + descendants)
    pub fn current_stack(&self, branch: &str) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        let mut ancestors = self.ancestors(branch);
        ancestors.reverse();

        for name in ancestors {
            if seen.insert(name.clone()) {
                result.push(name);
            }
        }

        if seen.insert(branch.to_string()) {
            result.push(branch.to_string());
        }

        for name in self.descendants(branch) {
            if seen.insert(name.clone()) {
                result.push(name);
            }
        }

        result
    }

    /// Get branches that need restacking
    pub fn needs_restack(&self) -> Vec<String> {
        self.branches
            .values()
            .filter(|b| b.needs_restack)
            .map(|b| b.name.clone())
            .collect()
    }

    /// Get siblings of a branch (other branches with the same parent)
    #[allow(dead_code)] // Useful utility for future features
    pub fn get_siblings(&self, branch: &str) -> Vec<String> {
        let branch_info = match self.branches.get(branch) {
            Some(b) => b,
            None => return vec![branch.to_string()],
        };

        let parent = match &branch_info.parent {
            Some(p) => p,
            None => return vec![branch.to_string()], // trunk has no siblings
        };

        // Get all children of the parent (including the branch itself)
        let parent_info = match self.branches.get(parent) {
            Some(p) => p,
            None => {
                // Parent not in stack - find other branches with same parent
                let mut siblings: Vec<String> = self
                    .branches
                    .values()
                    .filter(|b| b.parent.as_ref() == Some(&parent.to_string()))
                    .map(|b| b.name.clone())
                    .collect();
                siblings.sort();
                return siblings;
            }
        };

        let mut siblings = parent_info.children.clone();
        siblings.sort();
        siblings
    }

    /// Get direct children of a branch
    pub fn children(&self, branch: &str) -> Vec<String> {
        self.branches
            .get(branch)
            .map(|b| b.children.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_stack() -> Stack {
        // Creates a stack structure:
        // main (trunk)
        //  ├── feature-a
        //  │   └── feature-a-1
        //  │       └── feature-a-2
        //  └── feature-b
        let mut branches = HashMap::new();

        branches.insert(
            "main".to_string(),
            StackBranch {
                name: "main".to_string(),
                parent: None,
                parent_revision: None,
                children: vec!["feature-a".to_string(), "feature-b".to_string()],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );

        branches.insert(
            "feature-a".to_string(),
            StackBranch {
                name: "feature-a".to_string(),
                parent: Some("main".to_string()),
                parent_revision: Some("sha-main-0".to_string()),
                children: vec!["feature-a-1".to_string()],
                needs_restack: false,
                pr_number: Some(1),
                pr_state: Some("OPEN".to_string()),
                pr_is_draft: Some(false),
            },
        );

        branches.insert(
            "feature-a-1".to_string(),
            StackBranch {
                name: "feature-a-1".to_string(),
                parent: Some("feature-a".to_string()),
                parent_revision: Some("sha-feature-a-0".to_string()),
                children: vec!["feature-a-2".to_string()],
                needs_restack: true,
                pr_number: Some(2),
                pr_state: Some("OPEN".to_string()),
                pr_is_draft: Some(true),
            },
        );

        branches.insert(
            "feature-a-2".to_string(),
            StackBranch {
                name: "feature-a-2".to_string(),
                parent: Some("feature-a-1".to_string()),
                parent_revision: Some("sha-feature-a-1-0".to_string()),
                children: vec![],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );

        branches.insert(
            "feature-b".to_string(),
            StackBranch {
                name: "feature-b".to_string(),
                parent: Some("main".to_string()),
                parent_revision: Some("sha-main-0".to_string()),
                children: vec![],
                needs_restack: true,
                pr_number: Some(3),
                pr_state: Some("MERGED".to_string()),
                pr_is_draft: None,
            },
        );

        Stack {
            branches,
            trunk: "main".to_string(),
        }
    }

    #[test]
    fn test_ancestors_from_leaf() {
        let stack = create_test_stack();
        let ancestors = stack.ancestors("feature-a-2");
        assert_eq!(ancestors, vec!["feature-a-1", "feature-a", "main"]);
    }

    #[test]
    fn test_ancestors_from_middle() {
        let stack = create_test_stack();
        let ancestors = stack.ancestors("feature-a-1");
        assert_eq!(ancestors, vec!["feature-a", "main"]);
    }

    #[test]
    fn test_ancestors_from_first_level() {
        let stack = create_test_stack();
        let ancestors = stack.ancestors("feature-a");
        assert_eq!(ancestors, vec!["main"]);
    }

    #[test]
    fn test_ancestors_from_trunk() {
        let stack = create_test_stack();
        let ancestors = stack.ancestors("main");
        assert!(ancestors.is_empty());
    }

    #[test]
    fn test_ancestors_nonexistent() {
        let stack = create_test_stack();
        let ancestors = stack.ancestors("nonexistent");
        assert!(ancestors.is_empty());
    }

    #[test]
    fn test_descendants_from_trunk() {
        let stack = create_test_stack();
        let mut descendants = stack.descendants("main");
        descendants.sort();
        assert_eq!(
            descendants,
            vec!["feature-a", "feature-a-1", "feature-a-2", "feature-b"]
        );
    }

    #[test]
    fn test_descendants_from_middle() {
        let stack = create_test_stack();
        let mut descendants = stack.descendants("feature-a");
        descendants.sort();
        assert_eq!(descendants, vec!["feature-a-1", "feature-a-2"]);
    }

    #[test]
    fn test_descendants_from_leaf() {
        let stack = create_test_stack();
        let descendants = stack.descendants("feature-a-2");
        assert!(descendants.is_empty());
    }

    #[test]
    fn test_descendants_from_branch_with_no_children() {
        let stack = create_test_stack();
        let descendants = stack.descendants("feature-b");
        assert!(descendants.is_empty());
    }

    #[test]
    fn test_current_stack_from_leaf() {
        let stack = create_test_stack();
        let current = stack.current_stack("feature-a-2");
        assert_eq!(
            current,
            vec!["main", "feature-a", "feature-a-1", "feature-a-2"]
        );
    }

    #[test]
    fn test_current_stack_from_middle() {
        let stack = create_test_stack();
        let current = stack.current_stack("feature-a-1");
        assert_eq!(
            current,
            vec!["main", "feature-a", "feature-a-1", "feature-a-2"]
        );
    }

    #[test]
    fn test_current_stack_from_first_level() {
        let stack = create_test_stack();
        let current = stack.current_stack("feature-b");
        assert_eq!(current, vec!["main", "feature-b"]);
    }

    #[test]
    fn test_ancestors_breaks_parent_cycles() {
        let mut branches = HashMap::new();
        branches.insert(
            "main".to_string(),
            StackBranch {
                name: "main".to_string(),
                parent: None,
                parent_revision: None,
                children: vec![],
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
                parent: Some("b".to_string()),
                parent_revision: None,
                children: vec!["b".to_string()],
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
                parent: Some("a".to_string()),
                parent_revision: None,
                children: vec!["a".to_string()],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );

        let stack = Stack {
            branches,
            trunk: "main".to_string(),
        };

        assert_eq!(stack.ancestors("a"), vec!["b"]);
        assert_eq!(stack.current_stack("a"), vec!["b", "a"]);
    }

    #[test]
    fn test_descendants_breaks_child_cycles() {
        let mut branches = HashMap::new();
        branches.insert(
            "main".to_string(),
            StackBranch {
                name: "main".to_string(),
                parent: None,
                parent_revision: None,
                children: vec!["a".to_string()],
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
                parent_revision: None,
                children: vec!["b".to_string()],
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
                parent: Some("a".to_string()),
                parent_revision: None,
                children: vec!["a".to_string()],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );

        let stack = Stack {
            branches,
            trunk: "main".to_string(),
        };

        assert_eq!(stack.descendants("a"), vec!["b"]);
    }

    #[test]
    fn test_needs_restack() {
        let stack = create_test_stack();
        let mut needs = stack.needs_restack();
        needs.sort();
        assert_eq!(needs, vec!["feature-a-1", "feature-b"]);
    }

    #[test]
    fn test_get_siblings_with_one_sibling() {
        let stack = create_test_stack();
        let siblings = stack.get_siblings("feature-a");
        assert!(siblings.contains(&"feature-a".to_string()));
        assert!(siblings.contains(&"feature-b".to_string()));
        assert_eq!(siblings.len(), 2);
    }

    #[test]
    fn test_get_siblings_only_child() {
        let stack = create_test_stack();
        let siblings = stack.get_siblings("feature-a-1");
        assert_eq!(siblings, vec!["feature-a-1"]);
    }

    #[test]
    fn test_get_siblings_trunk() {
        let stack = create_test_stack();
        let siblings = stack.get_siblings("main");
        assert_eq!(siblings, vec!["main"]);
    }

    #[test]
    fn test_get_siblings_nonexistent() {
        let stack = create_test_stack();
        let siblings = stack.get_siblings("nonexistent");
        assert_eq!(siblings, vec!["nonexistent"]);
    }

    #[test]
    fn test_stack_branch_clone() {
        let branch = StackBranch {
            name: "test".to_string(),
            parent: Some("parent".to_string()),
            parent_revision: Some("abc123".to_string()),
            children: vec!["child".to_string()],
            needs_restack: true,
            pr_number: Some(42),
            pr_state: Some("OPEN".to_string()),
            pr_is_draft: Some(false),
        };
        let cloned = branch.clone();
        assert_eq!(cloned.name, branch.name);
        assert_eq!(cloned.pr_number, branch.pr_number);
    }

    #[test]
    fn test_stack_branch_debug() {
        let branch = StackBranch {
            name: "test".to_string(),
            parent: None,
            parent_revision: None,
            children: vec![],
            needs_restack: false,
            pr_number: None,
            pr_state: None,
            pr_is_draft: None,
        };
        let debug_str = format!("{:?}", branch);
        assert!(debug_str.contains("test"));
    }
}
