use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const CACHE_TTL_SECS: u64 = 300; // 5 minutes

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BranchCacheEntry {
    pub ci_state: Option<String>,
    pub pr_state: Option<String>,
    pub updated_at: u64,
}

#[derive(Serialize, Deserialize, Debug)]
#[derive(Default)]
pub struct CiCache {
    pub branches: HashMap<String, BranchCacheEntry>,
    #[serde(default)]
    pub last_refresh: u64,
}


impl CiCache {
    /// Get cache file path for current repo
    fn cache_path(git_dir: &std::path::Path) -> PathBuf {
        git_dir.join("stax").join("ci-cache.json")
    }

    /// Load cache from disk
    pub fn load(git_dir: &std::path::Path) -> Self {
        let path = Self::cache_path(git_dir);
        if !path.exists() {
            return Self::default();
        }

        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Save cache to disk
    pub fn save(&self, git_dir: &std::path::Path) -> Result<()> {
        let path = Self::cache_path(git_dir);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(&path, json)?;
        Ok(())
    }

    /// Get cached CI state for a branch
    pub fn get_ci_state(&self, branch: &str) -> Option<String> {
        self.branches.get(branch).and_then(|e| e.ci_state.clone())
    }

    /// Update cache entry for a branch
    pub fn update(&mut self, branch: &str, ci_state: Option<String>, pr_state: Option<String>) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        self.branches.insert(
            branch.to_string(),
            BranchCacheEntry {
                ci_state,
                pr_state,
                updated_at: now,
            },
        );
    }

    /// Check if cache is stale (older than TTL)
    pub fn is_stale(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Check if last refresh was more than TTL seconds ago
        now.saturating_sub(self.last_refresh) > CACHE_TTL_SECS
    }

    /// Mark cache as refreshed
    pub fn mark_refreshed(&mut self) {
        self.last_refresh = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
    }

    /// Remove branches that no longer exist
    pub fn cleanup(&mut self, valid_branches: &[String]) {
        let valid_set: std::collections::HashSet<_> = valid_branches.iter().collect();
        self.branches.retain(|k, _| valid_set.contains(k));
    }
}

