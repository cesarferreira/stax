use anyhow::{Context, Result};
use fs4::FileExt;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::NamedTempFile;

const MAX_TUI_DIFF_CACHE_ENTRIES: usize = 128;
const MAX_TUI_DIFF_CACHE_BYTES: u64 = 100 * 1024 * 1024;

enum LockMode {
    Shared,
    Exclusive,
}

struct CacheLock {
    file: File,
}

impl Drop for CacheLock {
    fn drop(&mut self) {
        let _ = <File as FileExt>::unlock(&self.file);
    }
}

fn acquire_cache_lock(cache_path: &Path, mode: LockMode) -> Result<CacheLock> {
    let parent = cache_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create cache directory {}", parent.display()))?;
    let lock_path = cache_lock_path(cache_path);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("failed to open cache lock {}", lock_path.display()))?;
    match mode {
        LockMode::Shared => <File as FileExt>::lock_shared(&file),
        LockMode::Exclusive => <File as FileExt>::lock(&file),
    }
    .with_context(|| format!("failed to lock cache {}", cache_path.display()))?;
    Ok(CacheLock { file })
}

fn cache_lock_path(cache_path: &Path) -> PathBuf {
    let parent = cache_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = cache_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("cache");
    parent.join(format!(".{file_name}.lock"))
}

fn load_json_unlocked<T>(path: &Path) -> Result<T>
where
    T: DeserializeOwned + Default,
{
    let contents = match fs::read(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read cache {}", path.display()));
        }
    };
    serde_json::from_slice(&contents)
        .with_context(|| format!("failed to parse cache {}", path.display()))
}

fn persist_json_atomic<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temporary cache in {}", parent.display()))?;
    serde_json::to_writer_pretty(temporary.as_file_mut(), value)
        .with_context(|| format!("failed to serialize cache {}", path.display()))?;
    temporary
        .as_file_mut()
        .write_all(b"\n")
        .with_context(|| format!("failed to write cache {}", path.display()))?;
    temporary
        .as_file_mut()
        .flush()
        .with_context(|| format!("failed to flush cache {}", path.display()))?;
    temporary
        .as_file()
        .sync_all()
        .with_context(|| format!("failed to sync cache {}", path.display()))?;
    let persisted = temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to atomically replace cache {}", path.display()))?;
    drop(persisted);
    sync_parent_directory(parent)?;
    Ok(())
}

fn sync_parent_directory(parent: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let directory = File::open(parent)
            .with_context(|| format!("failed to open cache directory {}", parent.display()))?;
        match directory.sync_all() {
            Ok(()) => {}
            #[cfg(target_os = "macos")]
            Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to sync cache directory {}", parent.display())
                });
            }
        }
    }
    #[cfg(not(unix))]
    let _ = parent;
    Ok(())
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BranchCacheEntry {
    #[serde(default)]
    pub ci_revision: Option<String>,
    pub ci_state: Option<String>,
    pub pr_state: Option<String>,
    pub updated_at: u64,
}

#[derive(Serialize, Deserialize, Debug, Default)]
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
        Self::load_strict(git_dir).unwrap_or_default()
    }

    pub(crate) fn load_strict(git_dir: &std::path::Path) -> Result<Self> {
        let path = Self::cache_path(git_dir);
        let _lock = acquire_cache_lock(&path, LockMode::Shared)?;
        load_json_unlocked(&path)
    }

    /// Persist an exact snapshot for isolated tests.
    #[cfg(test)]
    pub fn save(&self, git_dir: &std::path::Path) -> Result<()> {
        let path = Self::cache_path(git_dir);
        let _lock = acquire_cache_lock(&path, LockMode::Exclusive)?;
        persist_json_atomic(&path, self)
    }

    fn transaction(git_dir: &std::path::Path, update: impl FnOnce(&mut Self)) -> Result<()> {
        let path = Self::cache_path(git_dir);
        let _lock = acquire_cache_lock(&path, LockMode::Exclusive)?;
        let mut stored = load_json_unlocked::<Self>(&path)?;
        update(&mut stored);
        persist_json_atomic(&path, &stored)
    }

    /// Atomically update one branch while preserving all other cached entries.
    pub(crate) fn update_branch_ci(
        git_dir: &std::path::Path,
        branch: &str,
        revision: &str,
        ci_state: Option<String>,
    ) -> Result<()> {
        Self::transaction(git_dir, |stored| {
            stored.update_ci(branch, revision, ci_state)
        })
    }

    /// Atomically update one branch's pull-request state while preserving CI.
    pub(crate) fn update_branch_pr(
        git_dir: &std::path::Path,
        branch: &str,
        pr_state: Option<String>,
    ) -> Result<()> {
        Self::transaction(git_dir, |stored| stored.update_pr(branch, pr_state))
    }

    /// Atomically refresh both live fields and the cache timestamp for one branch.
    pub(crate) fn refresh_branch_states(
        git_dir: &std::path::Path,
        branch: &str,
        revision: &str,
        ci_state: Option<String>,
        pr_state: Option<String>,
    ) -> Result<()> {
        Self::transaction(git_dir, |stored| {
            stored.update(branch, revision, ci_state, pr_state);
            stored.mark_refreshed();
        })
    }

    /// Atomically replace the refreshed branch set in one cache transaction.
    pub(crate) fn refresh_branches(
        git_dir: &std::path::Path,
        updates: &[(String, String, Option<String>, Option<String>)],
        valid_branches: &[String],
    ) -> Result<()> {
        Self::transaction(git_dir, |stored| {
            for (branch, revision, ci_state, pr_state) in updates {
                stored.update(branch, revision, ci_state.clone(), pr_state.clone());
            }
            stored.cleanup(valid_branches);
            stored.mark_refreshed();
        })
    }

    /// Get cached CI state only when it belongs to the branch's current commit.
    pub fn get_ci_state_for_revision(&self, branch: &str, revision: &str) -> Option<String> {
        self.branches.get(branch).and_then(|entry| {
            (entry.ci_revision.as_deref() == Some(revision))
                .then(|| entry.ci_state.clone())
                .flatten()
        })
    }

    /// Update both cached fields for one exact branch revision.
    pub fn update(
        &mut self,
        branch: &str,
        revision: &str,
        ci_state: Option<String>,
        pr_state: Option<String>,
    ) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        self.branches.insert(
            branch.to_string(),
            BranchCacheEntry {
                ci_revision: Some(revision.to_string()),
                ci_state,
                pr_state,
                updated_at: now,
            },
        );
    }

    /// Update only CI state while retaining any cached pull-request state.
    pub fn update_ci(&mut self, branch: &str, revision: &str, ci_state: Option<String>) {
        let pr_state = self
            .branches
            .get(branch)
            .and_then(|entry| entry.pr_state.clone());
        self.update(branch, revision, ci_state, pr_state);
    }

    /// Update only pull-request state while retaining cached CI and its revision.
    fn update_pr(&mut self, branch: &str, pr_state: Option<String>) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if let Some(entry) = self.branches.get_mut(branch) {
            entry.pr_state = pr_state;
            entry.updated_at = now;
        } else {
            self.branches.insert(
                branch.to_string(),
                BranchCacheEntry {
                    ci_revision: None,
                    ci_state: None,
                    pr_state,
                    updated_at: now,
                },
            );
        }
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DiskDiffLine {
    pub content: String,
    pub line_type: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DiskDiffStat {
    pub file: String,
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DiskCachedDiff {
    pub stat: Vec<DiskDiffStat>,
    pub lines: Vec<DiskDiffLine>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TuiDiffCacheEntry {
    pub diff: DiskCachedDiff,
    pub updated_at: u64,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct TuiDiffCache {
    pub entries: HashMap<String, TuiDiffCacheEntry>,
}

impl TuiDiffCache {
    fn cache_path(git_dir: &std::path::Path) -> PathBuf {
        git_dir.join("stax").join("tui-diff-cache.json")
    }

    fn entries_dir(git_dir: &Path) -> PathBuf {
        git_dir.join("stax").join("diff-cache").join("v1")
    }

    fn entry_path(git_dir: &Path, key: &str) -> PathBuf {
        Self::entries_dir(git_dir).join(format!("{}.json", key.replace(':', "-")))
    }

    fn coordination_path(entries_dir: &Path) -> PathBuf {
        entries_dir.join("directory")
    }

    pub fn key(
        _parent: &str,
        _branch: &str,
        parent_oid: &str,
        branch_oid: &str,
        merge_base_oid: &str,
    ) -> String {
        format!("v1:{parent_oid}:{branch_oid}:{merge_base_oid}")
    }

    #[cfg(test)]
    pub fn load(git_dir: &std::path::Path) -> Self {
        Self::load_strict(git_dir).unwrap_or_default()
    }

    #[cfg(test)]
    pub(crate) fn load_strict(git_dir: &std::path::Path) -> Result<Self> {
        let mut cache = Self::default();
        let entries_dir = Self::entries_dir(git_dir);
        let _directory_lock =
            acquire_cache_lock(&Self::coordination_path(&entries_dir), LockMode::Shared)?;
        let entries = match fs::read_dir(&entries_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(cache),
            Err(error) => return Err(error).context("failed to read diff cache directory"),
        };
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if !entry.file_type()?.is_file()
                || path.extension().and_then(|extension| extension.to_str()) != Some("json")
            {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let _lock = acquire_cache_lock(&path, LockMode::Shared)?;
            let Some(diff) = load_json_unlocked::<Option<DiskCachedDiff>>(&path)? else {
                continue;
            };
            let updated_at = entry
                .metadata()?
                .modified()
                .ok()
                .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs())
                .unwrap_or_default();
            cache.entries.insert(
                stem.replace('-', ":"),
                TuiDiffCacheEntry { diff, updated_at },
            );
        }
        Ok(cache)
    }

    #[cfg(test)]
    pub fn save(&self, git_dir: &std::path::Path) -> Result<()> {
        for (key, entry) in &self.entries {
            Self::insert_persisted(git_dir, key.clone(), entry.diff.clone())?;
        }
        Ok(())
    }

    pub(crate) fn read_persisted(
        git_dir: &std::path::Path,
        key: &str,
    ) -> Result<Option<DiskCachedDiff>> {
        let path = Self::entry_path(git_dir, key);
        let entries_dir = Self::entries_dir(git_dir);
        let result = (|| {
            let _directory_lock =
                acquire_cache_lock(&Self::coordination_path(&entries_dir), LockMode::Shared)?;
            if !path.exists() {
                return Ok(None);
            }
            let _entry_lock = acquire_cache_lock(&path, LockMode::Exclusive)?;
            match load_json_unlocked::<Option<DiskCachedDiff>>(&path) {
                Ok(diff) => {
                    if diff.is_some()
                        && let Ok(file) = OpenOptions::new().read(true).write(true).open(&path)
                    {
                        let _ =
                            file.set_times(fs::FileTimes::new().set_modified(SystemTime::now()));
                    }
                    Ok(diff)
                }
                Err(error) => {
                    let _ = fs::remove_file(&path);
                    Err(error)
                }
            }
        })();
        if result.is_err() {
            let _ = Self::cleanup_orphaned_entry_lock(&entries_dir, &path);
        }
        result
    }

    /// Atomically insert one diff while preserving all other cached entries.
    pub(crate) fn insert_persisted(
        git_dir: &std::path::Path,
        key: String,
        diff: DiskCachedDiff,
    ) -> Result<()> {
        Self::insert_persisted_with_limits(
            git_dir,
            key,
            diff,
            MAX_TUI_DIFF_CACHE_ENTRIES,
            MAX_TUI_DIFF_CACHE_BYTES,
        )
    }

    fn insert_persisted_with_limits(
        git_dir: &Path,
        key: String,
        diff: DiskCachedDiff,
        max_entries: usize,
        max_bytes: u64,
    ) -> Result<()> {
        let path = Self::entry_path(git_dir, &key);
        let entries_dir = Self::entries_dir(git_dir);
        let write_result = (|| {
            let _directory_lock =
                acquire_cache_lock(&Self::coordination_path(&entries_dir), LockMode::Shared)?;
            let _entry_lock = acquire_cache_lock(&path, LockMode::Exclusive)?;
            persist_json_atomic(&path, &diff)?;
            let _ = fs::remove_file(Self::cache_path(git_dir));
            Ok(())
        })();
        if let Err(error) = write_result {
            let _ = Self::cleanup_orphaned_entry_lock(&entries_dir, &path);
            return Err(error);
        }
        let _ = Self::cleanup_entries(&entries_dir, max_entries, max_bytes);
        Ok(())
    }

    fn cleanup_orphaned_entry_lock(entries_dir: &Path, entry_path: &Path) -> Result<()> {
        let _directory_lock =
            acquire_cache_lock(&Self::coordination_path(entries_dir), LockMode::Exclusive)?;
        match fs::metadata(entry_path) {
            Ok(metadata) if metadata.is_file() => return Ok(()),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to inspect diff cache entry {}",
                        entry_path.display()
                    )
                });
            }
        }
        match fs::remove_file(cache_lock_path(entry_path)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).with_context(|| {
                format!(
                    "failed to remove orphaned diff cache entry lock {}",
                    entry_path.display()
                )
            }),
        }
    }

    fn cleanup_entries(entries_dir: &Path, max_entries: usize, max_bytes: u64) -> Result<()> {
        let _directory_lock =
            acquire_cache_lock(&Self::coordination_path(entries_dir), LockMode::Exclusive)?;
        Self::cleanup_orphaned_entry_locks(entries_dir)?;
        let entries = match fs::read_dir(entries_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to read diff cache directory {}",
                        entries_dir.display()
                    )
                });
            }
        };
        let mut cached = Vec::new();
        let mut total_bytes = 0_u64;
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error).context("failed to inspect diff cache entry"),
            };
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error).context("failed to inspect diff cache entry type"),
            };
            if !file_type.is_file()
                || path.extension().and_then(|extension| extension.to_str()) != Some("json")
            {
                continue;
            }
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("failed to inspect diff cache entry {}", path.display())
                    });
                }
            };
            let bytes = metadata.len();
            total_bytes = total_bytes.saturating_add(bytes);
            cached.push((metadata.modified().unwrap_or(UNIX_EPOCH), path, bytes));
        }
        cached.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));

        let mut remove_count = cached.len().saturating_sub(max_entries);
        for (_, path, bytes) in cached {
            if remove_count == 0 && total_bytes <= max_bytes {
                break;
            }
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("failed to remove diff cache entry {}", path.display())
                    });
                }
            }
            match fs::remove_file(cache_lock_path(&path)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("failed to remove diff cache entry lock {}", path.display())
                    });
                }
            }
            total_bytes = total_bytes.saturating_sub(bytes);
            remove_count = remove_count.saturating_sub(1);
        }
        Ok(())
    }

    fn cleanup_orphaned_entry_locks(entries_dir: &Path) -> Result<()> {
        let entries = match fs::read_dir(entries_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to read diff cache directory {}",
                        entries_dir.display()
                    )
                });
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error).context("failed to inspect diff cache entry"),
            };
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error).context("failed to inspect diff cache entry type"),
            };
            if !file_type.is_file() {
                continue;
            }
            let path = entry.path();
            let Some(lock_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some(payload_name) = lock_name
                .strip_prefix('.')
                .and_then(|name| name.strip_suffix(".lock"))
            else {
                continue;
            };
            if !payload_name.ends_with(".json") {
                continue;
            }
            let payload_path = entries_dir.join(payload_name);
            match fs::metadata(&payload_path) {
                Ok(metadata) if metadata.is_file() => continue,
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "failed to inspect diff cache entry {}",
                            payload_path.display()
                        )
                    });
                }
            }
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "failed to remove orphaned diff cache entry lock {}",
                            path.display()
                        )
                    });
                }
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn get(&self, key: &str) -> Option<&DiskCachedDiff> {
        self.entries.get(key).map(|entry| &entry.diff)
    }

    #[cfg(test)]
    pub fn insert(&mut self, key: String, diff: DiskCachedDiff) {
        self.entries.insert(
            key,
            TuiDiffCacheEntry {
                diff,
                updated_at: current_unix_time(),
            },
        );
        self.prune_old_entries();
    }

    #[cfg(test)]
    fn prune_old_entries(&mut self) {
        if self.entries.len() <= MAX_TUI_DIFF_CACHE_ENTRIES {
            return;
        }

        let mut entries = self
            .entries
            .iter()
            .map(|(key, entry)| (key.clone(), entry.updated_at))
            .collect::<Vec<_>>();
        entries.sort_by_key(|(_, updated_at)| *updated_at);

        let remove_count = self.entries.len() - MAX_TUI_DIFF_CACHE_ENTRIES;
        for (key, _) in entries.into_iter().take(remove_count) {
            self.entries.remove(&key);
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TuiPaneVisibilityState {
    pub stack: bool,
    pub summary: bool,
    pub patch: bool,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct TuiStateCache {
    #[serde(default)]
    pub panes: Option<TuiPaneVisibilityState>,
}

impl TuiStateCache {
    fn cache_path(git_dir: &std::path::Path) -> PathBuf {
        git_dir.join("stax").join("tui-state.json")
    }

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

    pub fn save(&self, git_dir: &std::path::Path) -> Result<()> {
        let path = Self::cache_path(git_dir);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(&path, json)?;
        Ok(())
    }
}

/// Cache for ahead/behind commit counts, keyed by (base_sha:head_sha).
///
/// The key encodes the current tip OIDs of both refs, so the cache
/// self-invalidates automatically: if either branch moves (push, rebase,
/// fetch), the SHA changes and the entry becomes a miss.
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct AheadBehindCache {
    /// "base_sha:head_sha" → (ahead, behind)
    pub entries: HashMap<String, (usize, usize)>,
}

impl AheadBehindCache {
    fn cache_path(git_dir: &std::path::Path) -> PathBuf {
        git_dir.join("stax").join("ahead-behind-cache.json")
    }

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

    pub fn save(&self, git_dir: &std::path::Path) -> Result<()> {
        let path = Self::cache_path(git_dir);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, serde_json::to_string(self)?)?;
        Ok(())
    }

    pub fn get(&self, base_sha: &str, head_sha: &str) -> Option<(usize, usize)> {
        self.entries
            .get(&format!("{}:{}", base_sha, head_sha))
            .copied()
    }

    pub fn set(&mut self, base_sha: &str, head_sha: &str, ahead: usize, behind: usize) {
        self.entries
            .insert(format!("{}:{}", base_sha, head_sha), (ahead, behind));
    }
}

#[cfg(test)]
fn current_unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use tempfile::TempDir;

    fn disk_diff(content: &str) -> DiskCachedDiff {
        DiskCachedDiff {
            stat: Vec::new(),
            lines: vec![DiskDiffLine {
                content: content.to_string(),
                line_type: "context".to_string(),
            }],
        }
    }

    fn assert_cleanup_limits(
        max_entries: usize,
        max_bytes: u64,
        entries: &[(&str, usize)],
        expected: &[&str],
    ) {
        let dir = tempfile::tempdir().unwrap();
        let entries_dir = TuiDiffCache::entries_dir(dir.path());
        fs::create_dir_all(&entries_dir).unwrap();
        for (name, size) in entries {
            fs::write(entries_dir.join(format!("{name}.json")), vec![b'x'; *size]).unwrap();
        }
        fs::write(entries_dir.join("keep.txt"), b"unrelated").unwrap();

        TuiDiffCache::cleanup_entries(&entries_dir, max_entries, max_bytes).unwrap();

        let mut remaining = fs::read_dir(&entries_dir)
            .unwrap()
            .filter_map(|entry| {
                let name = entry.unwrap().file_name().into_string().unwrap();
                name.ends_with(".json").then_some(name)
            })
            .collect::<Vec<_>>();
        remaining.sort();
        assert_eq!(remaining, expected);
        assert_eq!(
            fs::read(entries_dir.join("keep.txt")).unwrap(),
            b"unrelated"
        );
    }

    #[test]
    fn persisted_diffs_use_independent_portable_json_paths() {
        let dir = tempfile::tempdir().unwrap();
        let first_key = "v1:aaa:bbb:ccc";
        let second_key = "v1:ddd:eee:fff";

        TuiDiffCache::insert_persisted(dir.path(), first_key.into(), disk_diff("first")).unwrap();
        fs::write(TuiDiffCache::cache_path(dir.path()), b"legacy aggregate").unwrap();
        TuiDiffCache::insert_persisted(dir.path(), second_key.into(), disk_diff("second")).unwrap();

        assert_eq!(
            serde_json::from_slice::<DiskCachedDiff>(
                &fs::read(TuiDiffCache::entry_path(dir.path(), first_key)).unwrap()
            )
            .unwrap(),
            disk_diff("first")
        );
        assert_eq!(
            serde_json::from_slice::<DiskCachedDiff>(
                &fs::read(TuiDiffCache::entry_path(dir.path(), second_key)).unwrap()
            )
            .unwrap(),
            disk_diff("second")
        );
        assert_eq!(
            TuiDiffCache::entry_path(dir.path(), first_key)
                .file_name()
                .unwrap(),
            "v1-aaa-bbb-ccc.json"
        );
        assert!(!TuiDiffCache::cache_path(dir.path()).exists());
    }

    #[test]
    fn requested_diff_does_not_parse_unrelated_entries() {
        let dir = tempfile::tempdir().unwrap();
        TuiDiffCache::insert_persisted(dir.path(), "v1:a:b:c".into(), disk_diff("kept")).unwrap();
        fs::write(
            TuiDiffCache::entries_dir(dir.path()).join("broken.json"),
            b"{",
        )
        .unwrap();

        assert_eq!(
            TuiDiffCache::read_persisted(dir.path(), "v1:a:b:c").unwrap(),
            Some(disk_diff("kept"))
        );
    }

    #[test]
    fn malformed_requested_diff_is_removed_and_can_be_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let key = "v1:a:b:c";
        let path = TuiDiffCache::entry_path(dir.path(), key);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"{").unwrap();

        assert!(TuiDiffCache::read_persisted(dir.path(), key).is_err());
        assert!(!path.exists());
        assert!(!cache_lock_path(&path).exists());

        TuiDiffCache::insert_persisted(dir.path(), key.into(), disk_diff("replacement")).unwrap();
        assert_eq!(
            TuiDiffCache::read_persisted(dir.path(), key).unwrap(),
            Some(disk_diff("replacement"))
        );
    }

    #[test]
    fn failed_diff_write_without_an_entry_removes_its_lock() {
        let dir = tempfile::tempdir().unwrap();
        let key = "v1:a:b:c";
        let path = TuiDiffCache::entry_path(dir.path(), key);
        fs::create_dir_all(&path).unwrap();

        assert!(
            TuiDiffCache::insert_persisted(dir.path(), key.into(), disk_diff("unwritten")).is_err()
        );

        assert!(!cache_lock_path(&path).exists());
    }

    #[test]
    fn reading_a_diff_touches_it_for_oldest_first_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let first_key = "v1:a:b:c";
        let second_key = "v1:d:e:f";
        TuiDiffCache::insert_persisted(dir.path(), first_key.into(), disk_diff("first")).unwrap();
        TuiDiffCache::insert_persisted(dir.path(), second_key.into(), disk_diff("second")).unwrap();
        let first_path = TuiDiffCache::entry_path(dir.path(), first_key);
        let second_path = TuiDiffCache::entry_path(dir.path(), second_key);
        File::open(&first_path)
            .unwrap()
            .set_times(
                fs::FileTimes::new().set_modified(UNIX_EPOCH + std::time::Duration::from_secs(1)),
            )
            .unwrap();
        File::open(&second_path)
            .unwrap()
            .set_times(
                fs::FileTimes::new().set_modified(UNIX_EPOCH + std::time::Duration::from_secs(2)),
            )
            .unwrap();

        assert_eq!(
            TuiDiffCache::read_persisted(dir.path(), first_key).unwrap(),
            Some(disk_diff("first"))
        );
        TuiDiffCache::cleanup_entries(&TuiDiffCache::entries_dir(dir.path()), 1, u64::MAX).unwrap();

        assert!(first_path.exists());
        assert!(!second_path.exists());
    }

    #[test]
    fn diff_cache_cleanup_enforces_count_and_byte_limits() {
        assert_cleanup_limits(
            2,
            100,
            &[("a", 10), ("b", 10), ("c", 10)],
            &["b.json", "c.json"],
        );
        assert_cleanup_limits(10, 45, &[("a", 10), ("b", 20), ("c", 30)], &["c.json"]);
    }

    #[test]
    fn diff_cache_cleanup_removes_seeded_orphan_entry_lock() {
        let dir = tempfile::tempdir().unwrap();
        let entries_dir = TuiDiffCache::entries_dir(dir.path());
        fs::create_dir_all(&entries_dir).unwrap();
        let orphan_payload = entries_dir.join("orphan.json");
        let orphan_lock = cache_lock_path(&orphan_payload);
        fs::write(&orphan_lock, b"").unwrap();

        TuiDiffCache::cleanup_entries(&entries_dir, usize::MAX, u64::MAX).unwrap();

        assert!(!orphan_lock.exists());
    }

    #[test]
    fn diff_cache_cleanup_retains_entry_lock_for_live_payload() {
        let dir = tempfile::tempdir().unwrap();
        let entries_dir = TuiDiffCache::entries_dir(dir.path());
        fs::create_dir_all(&entries_dir).unwrap();
        let live_payload = entries_dir.join("live.json");
        let live_lock = cache_lock_path(&live_payload);
        fs::write(&live_payload, b"{}").unwrap();
        fs::write(&live_lock, b"").unwrap();

        TuiDiffCache::cleanup_entries(&entries_dir, usize::MAX, u64::MAX).unwrap();

        assert!(live_payload.exists());
        assert!(live_lock.exists());
    }

    #[test]
    fn diff_cache_cleanup_ignores_non_hidden_lock_like_file_and_enforces_count_limit() {
        let dir = tempfile::tempdir().unwrap();
        let entries_dir = TuiDiffCache::entries_dir(dir.path());
        fs::create_dir_all(&entries_dir).unwrap();
        let first = entries_dir.join("first.json");
        let second = entries_dir.join("second.json");
        let unrelated = entries_dir.join("notes.json.lock");
        fs::write(&first, vec![b'a'; 10]).unwrap();
        fs::write(&second, vec![b'b'; 10]).unwrap();
        fs::write(&unrelated, b"keep me").unwrap();
        File::open(&first)
            .unwrap()
            .set_times(
                fs::FileTimes::new().set_modified(UNIX_EPOCH + std::time::Duration::from_secs(1)),
            )
            .unwrap();
        File::open(&second)
            .unwrap()
            .set_times(
                fs::FileTimes::new().set_modified(UNIX_EPOCH + std::time::Duration::from_secs(2)),
            )
            .unwrap();

        TuiDiffCache::cleanup_entries(&entries_dir, 1, u64::MAX).unwrap();

        assert_eq!(fs::read(&unrelated).unwrap(), b"keep me");
        assert!(!first.exists());
        assert!(second.exists());
    }

    #[test]
    fn diff_cache_cleanup_ignores_lock_like_directory_and_enforces_byte_limit() {
        let dir = tempfile::tempdir().unwrap();
        let entries_dir = TuiDiffCache::entries_dir(dir.path());
        fs::create_dir_all(&entries_dir).unwrap();
        let first = entries_dir.join("first.json");
        let second = entries_dir.join("second.json");
        let unrelated = entries_dir.join(".notes.json.lock");
        fs::write(&first, vec![b'a'; 10]).unwrap();
        fs::write(&second, vec![b'b'; 20]).unwrap();
        fs::create_dir(&unrelated).unwrap();
        File::open(&first)
            .unwrap()
            .set_times(
                fs::FileTimes::new().set_modified(UNIX_EPOCH + std::time::Duration::from_secs(1)),
            )
            .unwrap();
        File::open(&second)
            .unwrap()
            .set_times(
                fs::FileTimes::new().set_modified(UNIX_EPOCH + std::time::Duration::from_secs(2)),
            )
            .unwrap();

        TuiDiffCache::cleanup_entries(&entries_dir, usize::MAX, 20).unwrap();

        assert!(unrelated.is_dir());
        assert!(!first.exists());
        assert!(second.exists());
    }

    #[test]
    fn concurrent_diff_writers_enforce_cleanup_limit_and_remove_evicted_locks() {
        let temp = TempDir::new().unwrap();
        let root = Arc::new(temp.path().to_path_buf());
        let barrier = Arc::new(Barrier::new(25));
        let writers = (0..24)
            .map(|index| {
                let root = Arc::clone(&root);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    TuiDiffCache::insert_persisted_with_limits(
                        &root,
                        format!("key-{index}"),
                        disk_diff(&format!("patch-{index}")),
                        4,
                        u64::MAX,
                    )
                    .unwrap();
                })
            })
            .collect::<Vec<_>>();

        barrier.wait();
        for writer in writers {
            writer.join().unwrap();
        }

        let entries_dir = TuiDiffCache::entries_dir(&root);
        let names = fs::read_dir(entries_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        let entry_count = names.iter().filter(|name| name.ends_with(".json")).count();
        let entry_lock_count = names
            .iter()
            .filter(|name| name.ends_with(".json.lock"))
            .count();

        assert!(entry_count <= 4, "found {entry_count} persisted entries");
        assert_eq!(entry_lock_count, entry_count);
    }

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
        cache.update(
            "feature-1",
            "revision-1",
            Some("success".to_string()),
            Some("OPEN".to_string()),
        );
        cache.save(temp.path()).unwrap();

        let loaded = CiCache::load(temp.path());
        assert!(loaded.branches.contains_key("feature-1"));
        assert_eq!(
            loaded.get_ci_state_for_revision("feature-1", "revision-1"),
            Some("success".to_string())
        );
    }

    #[test]
    fn test_cache_update() {
        let mut cache = CiCache::default();
        cache.update(
            "branch-1",
            "revision-1",
            Some("pending".to_string()),
            Some("DRAFT".to_string()),
        );

        assert!(cache.branches.contains_key("branch-1"));
        let entry = cache.branches.get("branch-1").unwrap();
        assert_eq!(entry.ci_revision.as_deref(), Some("revision-1"));
        assert_eq!(entry.ci_state, Some("pending".to_string()));
        assert_eq!(entry.pr_state, Some("DRAFT".to_string()));
        assert!(entry.updated_at > 0);
    }

    #[test]
    fn updating_ci_preserves_the_cached_pull_request_state() {
        let mut cache = CiCache::default();
        cache.update(
            "branch-1",
            "revision-1",
            Some("pending".to_string()),
            Some("OPEN".to_string()),
        );

        cache.update_ci("branch-1", "revision-2", Some("success".to_string()));

        let entry = cache.branches.get("branch-1").unwrap();
        assert_eq!(entry.ci_revision.as_deref(), Some("revision-2"));
        assert_eq!(entry.ci_state.as_deref(), Some("success"));
        assert_eq!(entry.pr_state.as_deref(), Some("OPEN"));
    }

    #[test]
    fn test_cache_get_ci_state() {
        let mut cache = CiCache::default();
        assert_eq!(
            cache.get_ci_state_for_revision("nonexistent", "revision"),
            None
        );

        cache.update("feature", "revision", Some("success".to_string()), None);
        assert_eq!(
            cache.get_ci_state_for_revision("feature", "revision"),
            Some("success".to_string())
        );
    }

    #[test]
    fn legacy_ci_cache_entries_are_not_current_for_any_revision() {
        let entry: BranchCacheEntry =
            serde_json::from_str(r#"{"ci_state":"success","pr_state":"OPEN","updated_at":123}"#)
                .unwrap();
        let mut cache = CiCache::default();
        cache.branches.insert("feature".to_string(), entry);

        assert_eq!(cache.get_ci_state_for_revision("feature", "deadbeef"), None);
        assert_eq!(
            cache
                .branches
                .get("feature")
                .and_then(|entry| entry.ci_revision.as_deref()),
            None
        );
    }

    #[test]
    fn ci_cache_lookup_requires_the_exact_commit_revision() {
        let mut cache = CiCache::default();
        cache.update_ci("feature", "revision-a", Some("success".to_string()));

        assert_eq!(
            cache
                .get_ci_state_for_revision("feature", "revision-a")
                .as_deref(),
            Some("success")
        );
        assert_eq!(
            cache.get_ci_state_for_revision("feature", "revision-b"),
            None
        );
    }

    #[test]
    fn pull_request_only_transaction_preserves_ci_revision_and_state() {
        let temp = TempDir::new().unwrap();
        CiCache::update_branch_ci(
            temp.path(),
            "feature",
            "revision-a",
            Some("success".to_string()),
        )
        .unwrap();

        CiCache::update_branch_pr(temp.path(), "feature", Some("DRAFT".to_string())).unwrap();

        let cache = CiCache::load_strict(temp.path()).unwrap();
        let entry = cache.branches.get("feature").unwrap();
        assert_eq!(entry.ci_revision.as_deref(), Some("revision-a"));
        assert_eq!(entry.ci_state.as_deref(), Some("success"));
        assert_eq!(entry.pr_state.as_deref(), Some("DRAFT"));
    }

    #[test]
    fn test_cache_mark_refreshed() {
        let mut cache = CiCache::default();
        cache.mark_refreshed();
        assert!(cache.last_refresh > 0);
    }

    #[test]
    fn test_cache_cleanup() {
        let mut cache = CiCache::default();
        cache.update("keep-1", "revision", Some("success".to_string()), None);
        cache.update("keep-2", "revision", Some("success".to_string()), None);
        cache.update("remove-1", "revision", Some("failure".to_string()), None);
        cache.update("remove-2", "revision", Some("pending".to_string()), None);

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
        cache.update("branch-1", "revision", Some("success".to_string()), None);
        cache.update("branch-2", "revision", Some("success".to_string()), None);

        cache.cleanup(&[]);
        assert!(cache.branches.is_empty());
    }

    #[test]
    fn test_branch_cache_entry_serialization() {
        let entry = BranchCacheEntry {
            ci_revision: Some("revision-1".to_string()),
            ci_state: Some("success".to_string()),
            pr_state: Some("OPEN".to_string()),
            updated_at: 1234567890,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("success"));
        assert!(json.contains("OPEN"));
        assert!(json.contains("1234567890"));

        let deserialized: BranchCacheEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.ci_revision, entry.ci_revision);
        assert_eq!(deserialized.ci_state, entry.ci_state);
        assert_eq!(deserialized.pr_state, entry.pr_state);
        assert_eq!(deserialized.updated_at, entry.updated_at);
    }

    #[test]
    fn test_cache_serialization() {
        let mut cache = CiCache::default();
        cache.update(
            "branch",
            "revision",
            Some("success".to_string()),
            Some("MERGED".to_string()),
        );
        cache.mark_refreshed();

        let json = serde_json::to_string(&cache).unwrap();
        let deserialized: CiCache = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.branches.len(), 1);
        assert!(deserialized.last_refresh > 0);
    }

    #[test]
    fn concurrent_ci_transactions_preserve_different_branches() {
        let temp = TempDir::new().unwrap();
        let root = Arc::new(temp.path().to_path_buf());
        let barrier = Arc::new(Barrier::new(3));
        let writers = [("feature-a", "success"), ("feature-b", "failure")]
            .into_iter()
            .map(|(branch, status)| {
                let root = Arc::clone(&root);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    CiCache::update_branch_ci(
                        &root,
                        branch,
                        &format!("revision-{branch}"),
                        Some(status.to_string()),
                    )
                    .unwrap();
                })
            })
            .collect::<Vec<_>>();

        barrier.wait();
        for writer in writers {
            writer.join().unwrap();
        }

        let cache = CiCache::load_strict(&root).unwrap();
        assert_eq!(
            cache
                .get_ci_state_for_revision("feature-a", "revision-feature-a")
                .as_deref(),
            Some("success")
        );
        assert_eq!(
            cache
                .get_ci_state_for_revision("feature-b", "revision-feature-b")
                .as_deref(),
            Some("failure")
        );
    }

    #[test]
    fn stale_snapshot_writer_cannot_revert_newer_ci_or_pr_fields() {
        let temp = TempDir::new().unwrap();
        CiCache::refresh_branch_states(
            temp.path(),
            "branch-a",
            "revision-a-old",
            Some("pending".into()),
            Some("DRAFT".into()),
        )
        .unwrap();
        CiCache::refresh_branch_states(
            temp.path(),
            "branch-b",
            "revision-b-old",
            Some("pending".into()),
            Some("DRAFT".into()),
        )
        .unwrap();
        let mut stale = CiCache::load_strict(temp.path()).unwrap();

        CiCache::refresh_branch_states(
            temp.path(),
            "branch-a",
            "revision-a-new",
            Some("success".into()),
            Some("OPEN".into()),
        )
        .unwrap();
        assert_eq!(
            stale
                .branches
                .get("branch-a")
                .and_then(|entry| entry.ci_state.as_deref()),
            Some("pending")
        );

        let stale_branch_b = stale.branches.get_mut("branch-b").unwrap();
        stale_branch_b.ci_state = Some("failure".into());
        stale_branch_b.pr_state = Some("MERGED".into());
        let stale_branch_b_ci = stale_branch_b.ci_state.clone();
        let stale_branch_b_pr = stale_branch_b.pr_state.clone();
        CiCache::update_branch_ci(temp.path(), "branch-b", "revision-b-old", stale_branch_b_ci)
            .unwrap();
        CiCache::update_branch_pr(temp.path(), "branch-b", stale_branch_b_pr).unwrap();

        let current = CiCache::load_strict(temp.path()).unwrap();
        let branch_a = current.branches.get("branch-a").unwrap();
        assert_eq!(branch_a.ci_state.as_deref(), Some("success"));
        assert_eq!(branch_a.pr_state.as_deref(), Some("OPEN"));
        let branch_b = current.branches.get("branch-b").unwrap();
        assert_eq!(branch_b.ci_state.as_deref(), Some("failure"));
        assert_eq!(branch_b.pr_state.as_deref(), Some("MERGED"));
    }

    #[test]
    fn linked_worktrees_share_ci_cache_and_preserve_concurrent_field_updates() {
        let temp = TempDir::new().unwrap();
        let repo = git2::Repository::init(temp.path()).unwrap();
        let signature = git2::Signature::now("Test User", "test@example.com").unwrap();
        let mut index = repo.index().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &signature, &signature, "initial", &tree, &[])
            .unwrap();
        drop(tree);
        drop(index);
        let linked_path = temp.path().join("linked");
        repo.worktree("linked", &linked_path, None).unwrap();
        drop(repo);

        let main_repo = crate::git::GitRepo::open_from_path(temp.path()).unwrap();
        let linked_repo = crate::git::GitRepo::open_from_path(&linked_path).unwrap();
        let main_cache_dir = main_repo.common_git_dir().unwrap();
        let linked_cache_dir = linked_repo.common_git_dir().unwrap();
        assert_eq!(main_cache_dir, linked_cache_dir);

        CiCache::update_branch_ci(
            &main_cache_dir,
            "main-write",
            "main-revision",
            Some("success".into()),
        )
        .unwrap();
        assert_eq!(
            CiCache::load_strict(&linked_cache_dir)
                .unwrap()
                .get_ci_state_for_revision("main-write", "main-revision")
                .as_deref(),
            Some("success")
        );
        CiCache::update_branch_pr(&linked_cache_dir, "linked-write", Some("DRAFT".into())).unwrap();
        assert_eq!(
            CiCache::load_strict(&main_cache_dir)
                .unwrap()
                .branches
                .get("linked-write")
                .and_then(|entry| entry.pr_state.as_deref()),
            Some("DRAFT")
        );

        let main_cache_dir = Arc::new(main_cache_dir);
        let linked_cache_dir = Arc::new(linked_cache_dir);
        let barrier = Arc::new(Barrier::new(3));
        let ci_writer = {
            let cache_dir = Arc::clone(&main_cache_dir);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                CiCache::update_branch_ci(
                    &cache_dir,
                    "shared",
                    "shared-revision",
                    Some("success".into()),
                )
                .unwrap();
            })
        };
        let pr_writer = {
            let cache_dir = Arc::clone(&linked_cache_dir);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                CiCache::update_branch_pr(&cache_dir, "shared", Some("OPEN".into())).unwrap();
            })
        };

        barrier.wait();
        ci_writer.join().unwrap();
        pr_writer.join().unwrap();

        let current = CiCache::load_strict(&main_cache_dir).unwrap();
        let shared = current.branches.get("shared").unwrap();
        assert_eq!(shared.ci_state.as_deref(), Some("success"));
        assert_eq!(shared.pr_state.as_deref(), Some("OPEN"));
    }

    #[test]
    fn concurrent_diff_transactions_preserve_different_keys() {
        let temp = TempDir::new().unwrap();
        let root = Arc::new(temp.path().to_path_buf());
        let barrier = Arc::new(Barrier::new(3));
        let writers = [("key-a", "patch-a"), ("key-b", "patch-b")]
            .into_iter()
            .map(|(key, content)| {
                let root = Arc::clone(&root);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    TuiDiffCache::insert_persisted(&root, key.to_string(), disk_diff(content))
                        .unwrap();
                })
            })
            .collect::<Vec<_>>();

        barrier.wait();
        for writer in writers {
            writer.join().unwrap();
        }

        assert_eq!(
            TuiDiffCache::read_persisted(&root, "key-a").unwrap(),
            Some(disk_diff("patch-a"))
        );
        assert_eq!(
            TuiDiffCache::read_persisted(&root, "key-b").unwrap(),
            Some(disk_diff("patch-b"))
        );
    }

    #[test]
    fn malformed_ci_cache_is_not_clobbered_by_transaction() {
        let temp = TempDir::new().unwrap();
        let path = CiCache::cache_path(temp.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let malformed = b"{\"branches\":";
        fs::write(&path, malformed).unwrap();

        let error =
            CiCache::update_branch_ci(temp.path(), "feature", "revision", Some("success".into()))
                .unwrap_err();

        assert!(error.to_string().contains("parse"));
        assert_eq!(fs::read(path).unwrap(), malformed);
    }

    #[test]
    fn legacy_malformed_diff_cache_does_not_block_per_entry_write() {
        let temp = TempDir::new().unwrap();
        let path = TuiDiffCache::cache_path(temp.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let malformed = b"{\"entries\":";
        fs::write(&path, malformed).unwrap();

        TuiDiffCache::insert_persisted(temp.path(), "key".into(), disk_diff("patch")).unwrap();

        assert!(!path.exists());
        assert_eq!(
            TuiDiffCache::read_persisted(temp.path(), "key").unwrap(),
            Some(disk_diff("patch"))
        );
    }

    #[test]
    fn shared_lock_readers_never_observe_partial_ci_json() {
        let temp = TempDir::new().unwrap();
        CiCache::update_branch_ci(temp.path(), "seed", "seed-revision", Some("pending".into()))
            .unwrap();
        let root = Arc::new(temp.path().to_path_buf());
        let barrier = Arc::new(Barrier::new(2));
        let writer_root = Arc::clone(&root);
        let writer_barrier = Arc::clone(&barrier);
        let writer = thread::spawn(move || {
            writer_barrier.wait();
            for index in 0..100 {
                CiCache::update_branch_ci(
                    &writer_root,
                    &format!("branch-{index}"),
                    &format!("revision-{index}"),
                    Some("success".into()),
                )
                .unwrap();
            }
        });

        barrier.wait();
        for _ in 0..100 {
            let cache = CiCache::load_strict(&root).unwrap();
            assert_eq!(
                cache
                    .get_ci_state_for_revision("seed", "seed-revision")
                    .as_deref(),
                Some("pending")
            );
        }
        writer.join().unwrap();
    }

    #[test]
    fn shared_lock_readers_never_observe_partial_diff_json() {
        let temp = TempDir::new().unwrap();
        TuiDiffCache::insert_persisted(temp.path(), "seed".into(), disk_diff("seed")).unwrap();
        let root = Arc::new(temp.path().to_path_buf());
        let barrier = Arc::new(Barrier::new(2));
        let writer_root = Arc::clone(&root);
        let writer_barrier = Arc::clone(&barrier);
        let writer = thread::spawn(move || {
            writer_barrier.wait();
            for index in 0..100 {
                TuiDiffCache::insert_persisted(
                    &writer_root,
                    format!("key-{index}"),
                    disk_diff(&format!("patch-{index}")),
                )
                .unwrap();
            }
        });

        barrier.wait();
        for _ in 0..100 {
            assert_eq!(
                TuiDiffCache::read_persisted(&root, "seed").unwrap(),
                Some(disk_diff("seed"))
            );
        }
        writer.join().unwrap();
    }
}
