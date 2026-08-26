use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::Result;

/// Consistent local stack state shared by CLI, TUI, and desktop consumers.
#[derive(Debug, Clone)]
pub struct StackSnapshot {
    pub stack: Stack,
    pub current_branch: String,
}

impl StackSnapshot {
    pub fn load(repo: &GitRepo) -> Result<Self> {
        let stack = Stack::load(repo)?;
        let current_branch = repo.current_branch()?;
        Ok(Self {
            stack,
            current_branch,
        })
    }
}
