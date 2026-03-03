use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentWorktree {
    pub name: String,
    pub branch: String,
    pub path: PathBuf,
    pub created_at: String,
}

pub struct Registry {
    path: PathBuf,
    pub entries: Vec<AgentWorktree>,
}

impl Registry {
    pub fn load(git_dir: &Path) -> Result<Self> {
        let path = git_dir.join("stax").join("agent-worktrees.json");
        let entries = if path.exists() {
            let content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read registry: {}", path.display()))?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(Self { path, entries })
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(&self.entries)?;
        fs::write(&self.path, content)?;
        Ok(())
    }

    pub fn add(&mut self, entry: AgentWorktree) {
        self.entries.retain(|e| e.name != entry.name);
        self.entries.push(entry);
    }

    pub fn remove_by_name(&mut self, name: &str) {
        self.entries.retain(|e| e.name != name);
    }

    pub fn find_by_name(&self, name_or_slug: &str) -> Option<&AgentWorktree> {
        self.entries.iter().find(|e| {
            e.name == name_or_slug
                || e.branch == name_or_slug
                || e.branch.ends_with(&format!("/{}", name_or_slug))
        })
    }

    /// Remove entries whose paths no longer exist; returns the number pruned.
    pub fn prune(&mut self) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| e.path.exists());
        before - self.entries.len()
    }
}
