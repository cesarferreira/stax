//! Warm-slot pool for recycling worktrees.
//!
//! When a disposable worktree is removed it is "parked" as an idle slot rather
//! than deleted, keeping its built (gitignored) dependencies on disk. A later
//! create/lane can "adopt" an idle slot instead of a cold `git worktree add`.
//!
//! The pool is a JSON manifest (`.stax-pool.json`) in the managed worktrees
//! root, guarded by an advisory file lock (`.stax-pool.lock`) so concurrent
//! `st lane` invocations don't race on the same slot.

use anyhow::{Context, Result};
use fs4::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const POOL_MANIFEST: &str = ".stax-pool.json";
pub const POOL_LOCK: &str = ".stax-pool.lock";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SlotState {
    Idle,
    Leased,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slot {
    pub path: PathBuf,
    pub state: SlotState,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub lease_owner_pid: Option<u32>,
    #[serde(default)]
    pub last_used: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Pool {
    #[serde(default)]
    pub slots: Vec<Slot>,
}

impl Pool {
    /// Number of currently idle slots.
    pub fn idle_count(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.state == SlotState::Idle)
            .count()
    }

    /// Add or update a slot for `path`, marking it Idle.
    pub fn mark_idle(&mut self, path: &Path, branch: Option<String>) {
        let now = now_secs();
        if let Some(slot) = self.slots.iter_mut().find(|slot| slot.path == path) {
            slot.state = SlotState::Idle;
            slot.branch = branch;
            slot.lease_owner_pid = None;
            slot.last_used = now;
        } else {
            self.slots.push(Slot {
                path: path.to_path_buf(),
                state: SlotState::Idle,
                branch,
                lease_owner_pid: None,
                last_used: now,
            });
        }
    }

    /// Drop any slot entry whose path matches `path`.
    pub fn remove_path(&mut self, path: &Path) {
        self.slots.retain(|slot| slot.path != path);
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn manifest_path(dir: &Path) -> PathBuf {
    dir.join(POOL_MANIFEST)
}

fn lock_path(dir: &Path) -> PathBuf {
    dir.join(POOL_LOCK)
}

/// Load the pool manifest, returning an empty pool when it is absent.
pub fn load(dir: &Path) -> Result<Pool> {
    let path = manifest_path(dir);
    if !path.exists() {
        return Ok(Pool::default());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read pool manifest {}", path.display()))?;
    if content.trim().is_empty() {
        return Ok(Pool::default());
    }
    let pool: Pool = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse pool manifest {}", path.display()))?;
    Ok(pool)
}

/// Persist the pool manifest.
pub fn save(dir: &Path, pool: &Pool) -> Result<()> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("Failed to create pool directory {}", dir.display()))?;
    let path = manifest_path(dir);
    let content = serde_json::to_string_pretty(pool)?;
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write pool manifest {}", path.display()))?;
    Ok(())
}

/// Run `f` under an exclusive advisory lock over the pool lock file, passing the
/// current (loaded) manifest for read-modify-write. The manifest is saved after
/// `f` returns `Ok`.
pub fn with_lock<T>(dir: &Path, f: impl FnOnce(&mut Pool) -> Result<T>) -> Result<T> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("Failed to create pool directory {}", dir.display()))?;
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(lock_path(dir))
        .with_context(|| format!("Failed to open pool lock in {}", dir.display()))?;
    <std::fs::File as FileExt>::lock(&lock_file).context("Failed to acquire pool lock")?;

    let result = (|| {
        let mut pool = load(dir)?;
        let value = f(&mut pool)?;
        save(dir, &pool)?;
        Ok(value)
    })();

    let _ = <std::fs::File as FileExt>::unlock(&lock_file);
    result
}

/// Acquire an idle (or dead-pid leased) slot, marking it Leased with the current
/// pid and persisting the change. Returns the leased slot, or `None` when no
/// reusable slot exists.
pub fn acquire_idle_slot(dir: &Path) -> Result<Option<Slot>> {
    with_lock(dir, |pool| {
        let pid = std::process::id();
        let now = now_secs();
        let selected = pool.slots.iter_mut().find(|slot| match slot.state {
            SlotState::Idle => true,
            SlotState::Leased => slot
                .lease_owner_pid
                .map(|owner| !pid_alive(owner))
                .unwrap_or(true),
        });

        let Some(slot) = selected else {
            return Ok(None);
        };

        slot.state = SlotState::Leased;
        slot.lease_owner_pid = Some(pid);
        slot.last_used = now;
        Ok(Some(slot.clone()))
    })
}

/// Return true when a process with `pid` appears to be alive.
///
/// On Unix this uses `kill -0`, which reports whether the process exists without
/// sending a real signal. On other platforms we conservatively assume the
/// process is alive so we never steal a slot that might still be in use.
#[cfg(unix)]
pub fn pid_alive(pid: u32) -> bool {
    use std::process::{Command, Stdio};
    matches!(
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status(),
        Ok(status) if status.success()
    )
}

#[cfg(not(unix))]
pub fn pid_alive(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn serde_round_trip() {
        let mut pool = Pool::default();
        pool.slots.push(Slot {
            path: PathBuf::from("/tmp/slot-a"),
            state: SlotState::Idle,
            branch: Some("feature-a".to_string()),
            lease_owner_pid: None,
            last_used: 42,
        });
        let json = serde_json::to_string(&pool).unwrap();
        let parsed: Pool = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.slots.len(), 1);
        assert_eq!(parsed.slots[0].path, PathBuf::from("/tmp/slot-a"));
        assert_eq!(parsed.slots[0].state, SlotState::Idle);
        assert_eq!(parsed.slots[0].branch.as_deref(), Some("feature-a"));
    }

    #[test]
    fn acquire_prefers_idle_slot() {
        let dir = tempdir().unwrap();
        with_lock(dir.path(), |pool| {
            pool.mark_idle(Path::new("/tmp/slot-a"), Some("a".to_string()));
            Ok(())
        })
        .unwrap();

        let slot = acquire_idle_slot(dir.path()).unwrap().expect("idle slot");
        assert_eq!(slot.path, PathBuf::from("/tmp/slot-a"));
        assert_eq!(slot.state, SlotState::Leased);
        assert_eq!(slot.lease_owner_pid, Some(std::process::id()));

        // A second acquire finds nothing (only slot is leased by this live pid).
        assert!(acquire_idle_slot(dir.path()).unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn acquire_reclaims_dead_pid_lease() {
        let dir = tempdir().unwrap();
        // A pid that is almost certainly not alive.
        let dead_pid = 999_999_999u32;
        with_lock(dir.path(), |pool| {
            pool.slots.push(Slot {
                path: PathBuf::from("/tmp/slot-dead"),
                state: SlotState::Leased,
                branch: None,
                lease_owner_pid: Some(dead_pid),
                last_used: 1,
            });
            Ok(())
        })
        .unwrap();

        let slot = acquire_idle_slot(dir.path())
            .unwrap()
            .expect("dead-pid slot reclaimed");
        assert_eq!(slot.path, PathBuf::from("/tmp/slot-dead"));
        assert_eq!(slot.lease_owner_pid, Some(std::process::id()));
    }

    #[cfg(not(unix))]
    #[test]
    fn leased_slot_is_not_reclaimed_without_pid_liveness() {
        let dir = tempdir().unwrap();
        let unknown_pid = 999_999_999u32;
        with_lock(dir.path(), |pool| {
            pool.slots.push(Slot {
                path: PathBuf::from("/tmp/slot-unknown"),
                state: SlotState::Leased,
                branch: None,
                lease_owner_pid: Some(unknown_pid),
                last_used: 1,
            });
            Ok(())
        })
        .unwrap();

        assert!(acquire_idle_slot(dir.path()).unwrap().is_none());
    }

    #[test]
    fn live_pid_lease_is_not_reclaimed() {
        let dir = tempdir().unwrap();
        let live_pid = std::process::id();
        with_lock(dir.path(), |pool| {
            pool.slots.push(Slot {
                path: PathBuf::from("/tmp/slot-live"),
                state: SlotState::Leased,
                branch: None,
                lease_owner_pid: Some(live_pid),
                last_used: 1,
            });
            Ok(())
        })
        .unwrap();

        assert!(acquire_idle_slot(dir.path()).unwrap().is_none());
    }

    #[test]
    fn idle_count_and_remove_path() {
        let mut pool = Pool::default();
        pool.mark_idle(Path::new("/tmp/a"), None);
        pool.mark_idle(Path::new("/tmp/b"), None);
        assert_eq!(pool.idle_count(), 2);
        pool.remove_path(Path::new("/tmp/a"));
        assert_eq!(pool.idle_count(), 1);
        assert_eq!(pool.slots[0].path, PathBuf::from("/tmp/b"));
    }
}
