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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_cache_path() {
        let temp = TempDir::new().unwrap();
        let path = CiCache::cache_path(temp.path());
        assert!(path.to_string_lossy().contains("stax"));
        assert!(path.to_string_lossy().contains("ci-cache.json"));
    }

    #[test]
    fn test_cache_default() {
        let cache = CiCache::default();
        assert!(cache.branches.is_empty());
        assert_eq!(cache.last_refresh, 0);
    }

    #[test]
    fn test_cache_load_nonexistent() {
        let temp = TempDir::new().unwrap();
        let cache = CiCache::load(temp.path());
        assert!(cache.branches.is_empty());
    }

    #[test]
    fn test_cache_save_and_load() {
        let temp = TempDir::new().unwrap();
        let mut cache = CiCache::default();
        cache.update("feature-1", Some("success".to_string()), Some("OPEN".to_string()));
        cache.save(temp.path()).unwrap();

        let loaded = CiCache::load(temp.path());
        assert!(loaded.branches.contains_key("feature-1"));
        assert_eq!(loaded.get_ci_state("feature-1"), Some("success".to_string()));
    }

    #[test]
    fn test_cache_update() {
        let mut cache = CiCache::default();
        cache.update("branch-1", Some("pending".to_string()), Some("DRAFT".to_string()));

        assert!(cache.branches.contains_key("branch-1"));
        let entry = cache.branches.get("branch-1").unwrap();
        assert_eq!(entry.ci_state, Some("pending".to_string()));
        assert_eq!(entry.pr_state, Some("DRAFT".to_string()));
        assert!(entry.updated_at > 0);
    }

    #[test]
    fn test_cache_get_ci_state() {
        let mut cache = CiCache::default();
        assert_eq!(cache.get_ci_state("nonexistent"), None);

        cache.update("feature", Some("success".to_string()), None);
        assert_eq!(cache.get_ci_state("feature"), Some("success".to_string()));
    }

    #[test]
    fn test_cache_is_stale() {
        let cache = CiCache::default();
        // Default cache with last_refresh = 0 should be stale
        assert!(cache.is_stale());
    }

    #[test]
    fn test_cache_mark_refreshed() {
        let mut cache = CiCache::default();
        cache.mark_refreshed();
        // After marking refreshed, should not be stale (within TTL)
        assert!(!cache.is_stale());
        assert!(cache.last_refresh > 0);
    }

    #[test]
    fn test_cache_cleanup() {
        let mut cache = CiCache::default();
        cache.update("keep-1", Some("success".to_string()), None);
        cache.update("keep-2", Some("success".to_string()), None);
        cache.update("remove-1", Some("failure".to_string()), None);
        cache.update("remove-2", Some("pending".to_string()), None);

        let valid = vec!["keep-1".to_string(), "keep-2".to_string()];
        cache.cleanup(&valid);

        assert!(cache.branches.contains_key("keep-1"));
        assert!(cache.branches.contains_key("keep-2"));
        assert!(!cache.branches.contains_key("remove-1"));
        assert!(!cache.branches.contains_key("remove-2"));
    }

    #[test]
    fn test_cache_cleanup_empty_valid() {
        let mut cache = CiCache::default();
        cache.update("branch-1", Some("success".to_string()), None);
        cache.update("branch-2", Some("success".to_string()), None);

        cache.cleanup(&[]);
        assert!(cache.branches.is_empty());
    }

    #[test]
    fn test_branch_cache_entry_serialization() {
        let entry = BranchCacheEntry {
            ci_state: Some("success".to_string()),
            pr_state: Some("OPEN".to_string()),
            updated_at: 1234567890,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("success"));
        assert!(json.contains("OPEN"));
        assert!(json.contains("1234567890"));

        let deserialized: BranchCacheEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.ci_state, entry.ci_state);
        assert_eq!(deserialized.pr_state, entry.pr_state);
        assert_eq!(deserialized.updated_at, entry.updated_at);
    }

    #[test]
    fn test_cache_serialization() {
        let mut cache = CiCache::default();
        cache.update("branch", Some("success".to_string()), Some("MERGED".to_string()));
        cache.mark_refreshed();

        let json = serde_json::to_string(&cache).unwrap();
        let deserialized: CiCache = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.branches.len(), 1);
        assert!(deserialized.last_refresh > 0);
    }
}

