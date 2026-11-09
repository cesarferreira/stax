//! Retention pruning of old undo receipts and their backup refs.
//!
//! This permanently deletes recovery data, so the rules are deliberately narrow:
//! - the newest `keep_last` receipts are always kept (minimum 1);
//! - receipts younger than `keep_days` are always kept;
//! - only receipts that parse and have status `success` are ever pruned;
//! - only backup refs attached to a pruned receipt found in *this* git dir are
//!   deleted. Backup refs are shared between linked worktrees but receipts are
//!   per worktree, so a ref without a local receipt may belong to another
//!   worktree and is never swept;
//! - refs are deleted first, in one atomic `update-ref --stdin`, and receipts
//!   only afterwards. A ref failure leaves every receipt intact.

use super::receipt::{OpReceipt, OpStatus};
use super::{backup_ref_prefix, list_op_ids};
use crate::cache::{OpsPruneStamp, ops_prune_due};
use crate::config::UndoConfig;
use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Prune at most once per day per git dir.
pub const PRUNE_INTERVAL_SECS: u64 = 86_400;

const SECS_PER_DAY: u64 = 86_400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrunePolicy {
    pub keep_last: usize,
    pub keep_days: u64,
}

impl From<&UndoConfig> for PrunePolicy {
    fn from(cfg: &UndoConfig) -> Self {
        Self {
            keep_last: cfg.keep_last.max(1),
            keep_days: cfg.keep_days,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PruneReport {
    pub pruned: usize,
    pub refs_deleted: usize,
}

/// Unix timestamp encoded in the first 16 chars of an op id (`20251229T120500Z`).
fn op_id_timestamp(op_id: &str) -> Option<i64> {
    let prefix = op_id.get(..16)?;
    chrono::NaiveDateTime::parse_from_str(prefix, "%Y%m%dT%H%M%SZ")
        .ok()
        .map(|dt| dt.and_utc().timestamp())
}

/// Pick the op ids that may be pruned. `ids_newest_first` must be sorted newest first.
///
/// `status_of` is only called for ops that already passed the count and age checks.
pub(crate) fn select_prunable(
    ids_newest_first: &[String],
    now_secs: i64,
    policy: &PrunePolicy,
    status_of: impl Fn(&str) -> Option<OpStatus>,
) -> Vec<String> {
    let keep_last = policy.keep_last.max(1);
    let keep_secs =
        i64::try_from(policy.keep_days.saturating_mul(SECS_PER_DAY)).unwrap_or(i64::MAX);

    ids_newest_first
        .iter()
        .enumerate()
        .filter(|(idx, id)| {
            if *idx < keep_last {
                return false;
            }
            let Some(ts) = op_id_timestamp(id) else {
                return false;
            };
            if now_secs.saturating_sub(ts) < keep_secs {
                return false;
            }
            matches!(status_of(id), Some(OpStatus::Success))
        })
        .map(|(_, id)| id.clone())
        .collect()
}

/// Delete `(oid, refname)` pairs in one atomic transaction. The expected old OID
/// guards against a concurrent writer moving a ref after it was listed.
fn delete_refs_atomically(workdir: &Path, refs: &[(String, String)]) -> Result<()> {
    if refs.is_empty() {
        return Ok(());
    }
    let mut input = String::new();
    for (oid, name) in refs {
        input.push_str(&format!("delete {name} {oid}\n"));
    }

    let mut child = Command::new("git")
        .args(["update-ref", "--stdin"])
        .current_dir(workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to run git update-ref --stdin")?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(input.as_bytes())
            .context("Failed to write ref deletions")?;
    }
    drop(child.stdin.take());
    let output = child
        .wait_with_output()
        .context("Failed to wait for git update-ref")?;
    if !output.status.success() {
        bail!(
            "Failed to delete backup refs: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn list_backup_refs(workdir: &Path) -> Result<Vec<(String, String)>> {
    let output = Command::new("git")
        .args([
            "for-each-ref",
            "--format=%(objectname) %(refname)",
            "refs/stax/backups/",
        ])
        .current_dir(workdir)
        .output()
        .context("Failed to list backup refs")?;
    if !output.status.success() {
        bail!(
            "Failed to list backup refs: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_once(' '))
        .map(|(oid, name)| (oid.to_string(), name.to_string()))
        .collect())
}

/// Delete old successful receipts and their backup refs, per `policy`.
pub fn prune_ops(
    git_dir: &Path,
    workdir: &Path,
    policy: &PrunePolicy,
    now_secs: i64,
) -> Result<PruneReport> {
    let ids = list_op_ids(git_dir)?;
    if ids.is_empty() {
        return Ok(PruneReport::default());
    }

    let victims = select_prunable(&ids, now_secs, policy, |id| {
        OpReceipt::load(git_dir, id).ok().map(|r| r.status)
    });
    if victims.is_empty() {
        return Ok(PruneReport::default());
    }

    let prefixes: HashSet<String> = victims.iter().map(|id| backup_ref_prefix(id)).collect();
    let refs: Vec<(String, String)> = list_backup_refs(workdir)?
        .into_iter()
        .filter(|(_, name)| {
            name.strip_prefix("refs/stax/backups/")
                .and_then(|rest| rest.split_once('/'))
                .is_some_and(|(id, _)| prefixes.contains(&format!("refs/stax/backups/{id}/")))
        })
        .collect();

    delete_refs_atomically(workdir, &refs)?;

    let mut pruned = 0;
    for id in &victims {
        match std::fs::remove_file(OpReceipt::file_path(git_dir, id)) {
            Ok(()) => pruned += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {}
        }
    }

    Ok(PruneReport {
        pruned,
        refs_deleted: refs.len(),
    })
}

/// Run `prune_ops` when the once-a-day stamp says it is due.
///
/// Returns `Ok(None)` after a single small file read when not due. The stamp is
/// recorded even when pruning fails, so a persistent failure costs one attempt a
/// day instead of one per sync.
pub fn maybe_prune(
    git_dir: &Path,
    workdir: &Path,
    policy: &PrunePolicy,
    now_secs: u64,
) -> Result<Option<PruneReport>> {
    if !ops_prune_due(OpsPruneStamp::load(git_dir), now_secs, PRUNE_INTERVAL_SECS) {
        return Ok(None);
    }
    let now = i64::try_from(now_secs).unwrap_or(i64::MAX);
    let result = prune_ops(git_dir, workdir, policy, now);
    let _ = OpsPruneStamp::record(git_dir, now_secs);
    result.map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::receipt::OpKind;
    use std::collections::HashMap;
    use tempfile::TempDir;

    const NOW: i64 = 1_800_000_000; // 2027-01-15
    const DAY: i64 = 86_400;

    fn id_at(ts: i64, suffix: &str) -> String {
        let dt = chrono::DateTime::from_timestamp(ts, 0).unwrap();
        format!("{}-{}", dt.format("%Y%m%dT%H%M%SZ"), suffix)
    }

    fn policy(keep_last: usize, keep_days: u64) -> PrunePolicy {
        PrunePolicy {
            keep_last,
            keep_days,
        }
    }

    fn all_success(_: &str) -> Option<OpStatus> {
        Some(OpStatus::Success)
    }

    #[test]
    fn select_prunable_keeps_newest_keep_last() {
        let ids: Vec<String> = (1..=5)
            .map(|d| id_at(NOW - 100 * DAY - d, "aaaaaa"))
            .collect();
        let victims = select_prunable(&ids, NOW, &policy(2, 14), all_success);
        assert_eq!(victims, ids[2..].to_vec());
    }

    #[test]
    fn select_prunable_keeps_young_ops() {
        let ids = vec![
            id_at(NOW - DAY, "aaaaaa"),
            id_at(NOW - 13 * DAY, "bbbbbb"),
            id_at(NOW - 15 * DAY, "cccccc"),
        ];
        let victims = select_prunable(&ids, NOW, &policy(1, 14), all_success);
        assert_eq!(victims, vec![ids[2].clone()]);
    }

    #[test]
    fn select_prunable_only_prunes_success() {
        let ids = vec![
            id_at(NOW - 100 * DAY, "aaaaaa"),
            id_at(NOW - 101 * DAY, "bbbbbb"),
            id_at(NOW - 102 * DAY, "cccccc"),
            id_at(NOW - 103 * DAY, "dddddd"),
        ];
        let statuses: HashMap<String, OpStatus> = HashMap::from([
            (ids[0].clone(), OpStatus::Success),
            (ids[1].clone(), OpStatus::Failed),
            (ids[2].clone(), OpStatus::InProgress),
            (ids[3].clone(), OpStatus::Success),
        ]);
        let victims = select_prunable(&ids, NOW, &policy(1, 14), |id| statuses.get(id).cloned());
        assert_eq!(victims, vec![ids[3].clone()]);
    }

    #[test]
    fn select_prunable_keeps_unparsable_ids_and_unreadable_status() {
        let ids = vec![
            id_at(NOW - 100 * DAY, "aaaaaa"),
            "op-1".to_string(),
            "20209999T000000Z-bad".to_string(),
            id_at(NOW - 200 * DAY, "bbbbbb"),
        ];
        let victims = select_prunable(&ids, NOW, &policy(1, 14), |id| {
            (id != ids[3]).then_some(OpStatus::Success)
        });
        assert!(victims.is_empty(), "{victims:?}");
    }

    #[test]
    fn select_prunable_keeps_future_dated_ops() {
        let ids = vec![id_at(NOW, "aaaaaa"), id_at(NOW + 10 * DAY, "bbbbbb")];
        let victims = select_prunable(&ids, NOW, &policy(1, 0), all_success);
        assert!(victims.is_empty(), "{victims:?}");
    }

    #[test]
    fn select_prunable_handles_empty_input() {
        assert!(select_prunable(&[], NOW, &policy(1, 0), all_success).is_empty());
    }

    #[test]
    fn select_prunable_only_queries_candidates() {
        let ids = vec![
            id_at(NOW - 100 * DAY, "aaaaaa"),
            id_at(NOW - 101 * DAY, "bbbbbb"),
        ];
        let queried = std::cell::RefCell::new(Vec::new());
        select_prunable(&ids, NOW, &policy(1, 14), |id| {
            queried.borrow_mut().push(id.to_string());
            Some(OpStatus::Success)
        });
        assert_eq!(*queried.borrow(), vec![ids[1].clone()]);
    }

    #[test]
    fn policy_from_config_clamps_keep_last_to_one() {
        let cfg = UndoConfig {
            auto_prune: true,
            keep_last: 0,
            keep_days: 7,
        };
        let p = PrunePolicy::from(&cfg);
        assert_eq!(p, policy(1, 7));
        let ids = vec![id_at(NOW - 100 * DAY, "aaaaaa")];
        assert!(select_prunable(&ids, NOW, &p, all_success).is_empty());
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    struct Fixture {
        _dir: TempDir,
        workdir: std::path::PathBuf,
        git_dir: std::path::PathBuf,
        head: String,
    }

    fn fixture() -> Fixture {
        let dir = TempDir::new().unwrap();
        let workdir = dir.path().to_path_buf();
        git(&workdir, &["init", "-b", "main"]);
        git(
            &workdir,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--allow-empty",
                "-m",
                "init",
            ],
        );
        let head = git(&workdir, &["rev-parse", "HEAD"]);
        let git_dir = workdir.join(".git");
        Fixture {
            _dir: dir,
            workdir,
            git_dir,
            head,
        }
    }

    fn write_receipt(f: &Fixture, id: &str, status: OpStatus) {
        let mut receipt = OpReceipt::new(
            id.to_string(),
            OpKind::Sync,
            f.workdir.to_string_lossy().to_string(),
            "main".to_string(),
            "main".to_string(),
        );
        receipt.status = status;
        receipt.save(&f.git_dir).unwrap();
    }

    fn make_ref(f: &Fixture, id: &str) {
        git(
            &f.workdir,
            &[
                "update-ref",
                &format!("refs/stax/backups/{id}/main"),
                &f.head,
            ],
        );
    }

    fn ref_exists(f: &Fixture, id: &str) -> bool {
        Command::new("git")
            .args(["rev-parse", "--verify", "-q"])
            .arg(format!("refs/stax/backups/{id}/main"))
            .current_dir(&f.workdir)
            .output()
            .unwrap()
            .status
            .success()
    }

    #[test]
    fn prune_ops_deletes_refs_then_receipts_for_old_success_only() {
        let f = fixture();
        let old_ok = "20200101T000000Z-aaaaa1";
        let old_failed = "20200101T000000Z-aaaaa3";
        let newest = id_at(NOW, "newest");
        let orphan = "20200101T000000Z-ffffff";
        write_receipt(&f, old_ok, OpStatus::Success);
        write_receipt(&f, old_failed, OpStatus::Failed);
        write_receipt(&f, &newest, OpStatus::Success);
        for id in [old_ok, old_failed, orphan, newest.as_str()] {
            make_ref(&f, id);
        }

        let report = prune_ops(&f.git_dir, &f.workdir, &policy(1, 14), NOW).unwrap();
        assert_eq!(
            report,
            PruneReport {
                pruned: 1,
                refs_deleted: 1
            }
        );
        assert!(!OpReceipt::file_path(&f.git_dir, old_ok).exists());
        assert!(!ref_exists(&f, old_ok));
        assert!(OpReceipt::file_path(&f.git_dir, old_failed).exists());
        assert!(ref_exists(&f, old_failed));
        assert!(OpReceipt::file_path(&f.git_dir, &newest).exists());
        assert!(ref_exists(&f, &newest));
        assert!(ref_exists(&f, orphan), "orphan refs are never swept");
    }

    #[test]
    fn prune_ops_does_not_match_ids_sharing_a_prefix() {
        let f = fixture();
        let victim = "20200101T000000Z-aaaaa1";
        let lookalike = "20200101T000000Z-aaaaa10";
        write_receipt(&f, victim, OpStatus::Success);
        write_receipt(&f, &id_at(NOW, "newest"), OpStatus::Success);
        make_ref(&f, victim);
        make_ref(&f, lookalike);

        prune_ops(&f.git_dir, &f.workdir, &policy(1, 14), NOW).unwrap();
        assert!(!ref_exists(&f, victim));
        assert!(ref_exists(&f, lookalike));
    }

    #[test]
    fn prune_ops_keeps_every_receipt_when_ref_deletion_fails() {
        let f = fixture();
        let a = "20200101T000000Z-aaaaa1";
        let b = "20200101T000000Z-aaaaa2";
        write_receipt(&f, a, OpStatus::Success);
        write_receipt(&f, b, OpStatus::Success);
        write_receipt(&f, &id_at(NOW, "newest"), OpStatus::Success);
        make_ref(&f, a);
        make_ref(&f, b);
        // A held lock on one ref makes the whole atomic transaction fail.
        let lock = f.git_dir.join(format!("refs/stax/backups/{b}/main.lock"));
        std::fs::write(&lock, "").unwrap();

        let err = prune_ops(&f.git_dir, &f.workdir, &policy(1, 14), NOW);
        assert!(err.is_err());
        assert!(OpReceipt::file_path(&f.git_dir, a).exists());
        assert!(OpReceipt::file_path(&f.git_dir, b).exists());
        assert!(ref_exists(&f, a), "atomic delete must not partially apply");
        assert!(ref_exists(&f, b));
    }

    #[test]
    fn prune_ops_keeps_corrupt_receipts_without_panicking() {
        let f = fixture();
        let corrupt = "20200101T000000Z-aaaaa2";
        write_receipt(&f, "20200101T000000Z-aaaaa1", OpStatus::Success);
        std::fs::write(OpReceipt::file_path(&f.git_dir, corrupt), "not json").unwrap();
        write_receipt(&f, &id_at(NOW, "newest"), OpStatus::Success);

        let report = prune_ops(&f.git_dir, &f.workdir, &policy(1, 14), NOW).unwrap();
        assert_eq!(report.pruned, 1);
        assert!(OpReceipt::file_path(&f.git_dir, corrupt).exists());
    }

    #[test]
    fn prune_ops_without_ops_dir_is_a_no_op() {
        let f = fixture();
        let report = prune_ops(&f.git_dir, &f.workdir, &policy(1, 14), NOW).unwrap();
        assert_eq!(report, PruneReport::default());
    }

    #[test]
    fn maybe_prune_is_throttled_and_records_the_stamp() {
        let f = fixture();
        let old = "20200101T000000Z-aaaaa1";
        write_receipt(&f, old, OpStatus::Success);
        write_receipt(&f, &id_at(NOW, "newest"), OpStatus::Success);
        let now = NOW as u64;

        let first = maybe_prune(&f.git_dir, &f.workdir, &policy(1, 14), now).unwrap();
        assert_eq!(first.map(|r| r.pruned), Some(1));
        assert_eq!(OpsPruneStamp::load(&f.git_dir), Some(now));

        write_receipt(&f, old, OpStatus::Success);
        let second = maybe_prune(&f.git_dir, &f.workdir, &policy(1, 14), now + 60).unwrap();
        assert!(second.is_none());
        assert!(OpReceipt::file_path(&f.git_dir, old).exists());

        let third = maybe_prune(
            &f.git_dir,
            &f.workdir,
            &policy(1, 14),
            now + PRUNE_INTERVAL_SECS,
        )
        .unwrap();
        assert_eq!(third.map(|r| r.pruned), Some(1));
    }

    #[test]
    fn maybe_prune_records_the_stamp_even_when_pruning_fails() {
        let f = fixture();
        let a = "20200101T000000Z-aaaaa1";
        write_receipt(&f, a, OpStatus::Success);
        write_receipt(&f, &id_at(NOW, "newest"), OpStatus::Success);
        make_ref(&f, a);
        std::fs::write(
            f.git_dir.join(format!("refs/stax/backups/{a}/main.lock")),
            "",
        )
        .unwrap();

        let result = maybe_prune(&f.git_dir, &f.workdir, &policy(1, 14), NOW as u64);
        assert!(result.is_err());
        assert_eq!(OpsPruneStamp::load(&f.git_dir), Some(NOW as u64));
        assert!(OpReceipt::file_path(&f.git_dir, a).exists());
    }
}
