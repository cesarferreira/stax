use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::ops::receipt::OpKind;
use crate::ops::tx::Transaction;
use anyhow::{Result, bail};
use colored::Colorize;
use console::Term;
use dialoguer::Select;
use dialoguer::theme::ColorfulTheme;
use std::io::Write;
use std::path::Path;
use std::process::Command;

/// Commit info parsed from `git log`.
struct CommitInfo {
    sha: String,
    message: String,
}

impl CommitInfo {
    fn short_sha(&self) -> &str {
        &self.sha[..7.min(self.sha.len())]
    }
}

/// Actions the user can choose per commit (maps to git rebase -i verbs).
#[derive(Clone, Copy, PartialEq, Eq)]
enum EditAction {
    Pick,
    Reword,
    Squash,
    Fixup,
    Drop,
}

impl EditAction {
    fn label(self) -> &'static str {
        match self {
            Self::Pick => "pick",
            Self::Reword => "reword",
            Self::Squash => "squash",
            Self::Fixup => "fixup",
            Self::Drop => "drop",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Pick => "pick   - keep commit as-is",
            Self::Reword => "reword - change commit message",
            Self::Squash => "squash - combine with previous (keep both messages)",
            Self::Fixup => "fixup  - combine with previous (discard this message)",
            Self::Drop => "drop   - remove commit",
        }
    }
}

pub fn run(yes: bool, no_verify: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let stack = Stack::load(&repo)?;
    let current = repo.current_branch()?;

    if current == stack.trunk {
        bail!("Cannot edit commits on trunk. Checkout a stacked branch first.");
    }

    if repo.is_dirty()? {
        bail!("Working tree has uncommitted changes. Commit or stash them first.");
    }

    // Find the parent branch boundary
    let meta = BranchMetadata::read(repo.inner(), &current)?
        .ok_or_else(|| anyhow::anyhow!("Branch '{}' is not tracked by stax", current))?;
    let parent = meta.parent_branch_name.clone();
    let child_branches = stack.descendants(&current);

    // Get commits between parent and HEAD (oldest first)
    let workdir = repo.workdir()?;
    let output = Command::new("git")
        .args([
            "log",
            "--reverse",
            "--format=%H %s",
            &format!("{}..HEAD", parent),
        ])
        .current_dir(workdir)
        .output()?;

    if !output.status.success() {
        bail!(
            "Failed to list commits: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let commits: Vec<CommitInfo> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let (sha, message) = line.split_once(' ').unwrap_or((line, ""));
            CommitInfo {
                sha: sha.to_string(),
                message: message.to_string(),
            }
        })
        .collect();

    if commits.is_empty() {
        println!(
            "{}",
            format!("No commits on '{}' ahead of '{}'.", current, parent).yellow()
        );
        return Ok(());
    }

    let interactive_terminal = Term::stderr().is_term();
    if !interactive_terminal {
        if yes {
            bail!(
                "Interactive terminal required for `stax edit` to choose per-commit actions. `--yes` only skips the final confirmation."
            );
        }
        bail!("Interactive terminal required for `stax edit`.");
    }

    if commits.len() == 1 {
        println!(
            "{}",
            format!(
                "Only 1 commit on '{}'. Edit actions: reword, drop.",
                current
            )
            .dimmed()
        );
    }

    // Display commits
    println!(
        "{}",
        format!(
            "Commits on '{}' (oldest first, {} total):",
            current,
            commits.len()
        )
        .bold()
    );
    for (i, c) in commits.iter().enumerate() {
        println!("  {}. {} {}", i + 1, c.short_sha().yellow(), c.message);
    }
    println!();

    if !child_branches.is_empty() {
        println!(
            "{}",
            format!(
                "Warning: editing '{}' will require restacking {} child branch(es): {}",
                current,
                child_branches.len(),
                child_branches.join(", ")
            )
            .yellow()
        );
        println!(
            "{}",
            "Run `stax restack --all` after editing to repair the stack.".dimmed()
        );
        println!();
    }

    // Collect actions for each commit
    let mut actions: Vec<EditAction> = vec![EditAction::Pick; commits.len()];

    for (i, commit) in commits.iter().enumerate() {
        let prompt = format!(
            "{} {} {}",
            format!("[{}/{}]", i + 1, commits.len()).dimmed(),
            commit.short_sha().yellow(),
            commit.message
        );
        println!("{}", prompt);

        // Squash/fixup not available for the first commit (nothing to combine with)
        let available: Vec<EditAction> = if i == 0 {
            vec![EditAction::Pick, EditAction::Reword, EditAction::Drop]
        } else {
            vec![
                EditAction::Pick,
                EditAction::Reword,
                EditAction::Squash,
                EditAction::Fixup,
                EditAction::Drop,
            ]
        };

        let items: Vec<&str> = available.iter().map(|a| a.description()).collect();

        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Action")
            .items(&items)
            .default(0)
            .interact_opt()?;

        let Some(idx) = selection else {
            println!("Cancelled.");
            return Ok(());
        };

        actions[i] = available[idx];
    }

    // Check if anything changed from default (all pick)
    if actions.iter().all(|a| *a == EditAction::Pick) {
        println!("{}", "No changes selected. Nothing to do.".yellow());
        return Ok(());
    }

    // Show summary
    println!();
    println!("{}", "Edit plan:".bold());
    let has_reword = actions.contains(&EditAction::Reword);
    for (i, (commit, action)) in commits.iter().zip(actions.iter()).enumerate() {
        let action_str = match action {
            EditAction::Pick => action.label().dimmed().to_string(),
            EditAction::Drop => action.label().red().to_string(),
            _ => action.label().cyan().to_string(),
        };
        println!(
            "  {}. {} {} {}",
            i + 1,
            action_str,
            commit.short_sha().yellow(),
            commit.message
        );
    }
    if has_reword {
        println!(
            "{}",
            "  Note: reword will open your editor for each reworded commit.".dimmed()
        );
    }
    println!();

    // Confirm
    if !yes {
        let proceed = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Apply this edit plan?")
            .items(["Yes, apply", "Cancel"])
            .default(0)
            .interact_opt()?;

        match proceed {
            Some(0) => {}
            _ => {
                println!("Cancelled.");
                return Ok(());
            }
        }
    }

    apply_edit_plan(
        &repo,
        &current,
        meta,
        &commits,
        &actions,
        no_verify,
        &child_branches,
    )
}

/// Apply an edit plan: rebase `current`'s commits per `actions`, then refresh the
/// branch's stax metadata inside an undo transaction.
///
/// Split out of `run()` so the non-interactive half is directly testable
/// without a pty -- `run()`'s per-commit `dialoguer::Select` loop requires a
/// real terminal, which the unit tests in this module's `mod tests` bypass
/// entirely by calling this function directly. A real end-to-end CLI test
/// exists too (`edit_drop_via_real_interactive_session_updates_metadata_and_supports_undo`
/// in `tests/edit_tests.rs`, driven through a pty).
fn apply_edit_plan(
    repo: &GitRepo,
    current: &str,
    meta: BranchMetadata,
    commits: &[CommitInfo],
    actions: &[EditAction],
    no_verify: bool,
    child_branches: &[String],
) -> Result<()> {
    let workdir = repo.workdir()?;
    let parent = meta.parent_branch_name.clone();

    // Create undo snapshot. A successful edit rewrites BOTH the branch head and
    // the branch's `refs/branch-metadata/<branch>` blob (parentBranchRevision),
    // so the metadata ref MUST be planned and recorded too -- otherwise `stax undo`
    // restores the commits but leaves the post-edit boundary behind (issue #835 family).
    let mut tx = Transaction::begin(OpKind::Edit, repo, false)?;
    tx.plan_branch(repo, current)?;
    tx.plan_metadata_ref(repo, current)?;
    tx.snapshot()?;

    // Build the rebase todo list
    let todo: String = commits
        .iter()
        .zip(actions.iter())
        .map(|(c, a)| format!("{} {} {}", a.label(), c.sha, c.message))
        .collect::<Vec<_>>()
        .join("\n");

    // Write todo to a temp file
    let mut tmp = tempfile::NamedTempFile::new()?;
    writeln!(tmp, "{}", todo)?;
    tmp.flush()?;
    let todo_path = tmp.path().to_string_lossy().to_string();

    // Run git rebase -i with GIT_SEQUENCE_EDITOR that replaces the todo.
    // Quote the source path to handle paths with spaces. No explicit destination
    // arg: git's editor launcher always appends `"$@"` (the todo-file path) to
    // whatever command is configured, so an explicit `"$1"` here would make `cp`
    // see the destination twice (`cp src dest dest`) and fail with
    // "target ... Not a directory".
    let editor_cmd = format!("cp '{}'", todo_path.replace('\'', "'\\''"));

    let mut rebase_args = vec!["rebase", "-i"];
    if no_verify {
        rebase_args.push("--no-verify");
    }
    rebase_args.push(parent.as_str());

    let rebase_status = Command::new("git")
        .args(&rebase_args)
        .env("GIT_SEQUENCE_EDITOR", &editor_cmd)
        .current_dir(workdir)
        .status()?;

    if rebase_status.success() {
        // Update metadata to reflect new parent boundary
        let parent_rev = repo.branch_commit(&parent)?;
        let updated = BranchMetadata {
            parent_branch_revision: parent_rev,
            ..meta
        };
        updated.write(repo.inner(), current)?;

        tx.record_after(repo, current)?;
        // Resolved via subprocess, not `record_metadata_ref_after`: the write
        // above goes through `git update-ref`, which libgit2's cached refdb in
        // `repo` may not observe within this process.
        tx.record_known_metadata_after(
            current,
            ref_oid(workdir, &format!("refs/branch-metadata/{}", current)).as_deref(),
        );
        tx.finish_ok()?;

        println!("{}", "Edit applied successfully.".green());
        if child_branches.is_empty() {
            println!(
                "{}",
                "Run `stax restack --all` to rebase child branches if needed.".yellow()
            );
        } else {
            println!(
                "{}",
                format!(
                    "Run `stax restack --all` to rebase {} child branch(es): {}",
                    child_branches.len(),
                    child_branches.join(", ")
                )
                .yellow()
            );
        }
    } else if repo.rebase_in_progress()? {
        // Transaction stays open -- stax continue will handle completion
        println!(
            "{}",
            "Rebase paused due to conflicts. Resolve them, then run `stax continue` or `stax abort`."
                .yellow()
        );
    } else {
        tx.finish_err("rebase failed", Some("edit"), Some(current))?;
        bail!("Rebase failed. Run `stax undo` to restore the previous state.");
    }

    Ok(())
}

/// Resolve a ref to its OID via a git subprocess; `None` when absent.
/// Branch metadata refs are written with `git update-ref` (see
/// `git::refs::write_metadata`), which libgit2's cached refdb may not observe
/// within the same process -- mirrors `detach.rs`/`split.rs`'s `ref_oid`.
fn ref_oid(workdir: &Path, refname: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", refname])
        .current_dir(workdir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{NoopOperationReporter, RepositorySession};
    use crate::ops::receipt::OpReceipt;

    fn git_hermetic(cwd: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env(
                "GIT_CONFIG_SYSTEM",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Build `main` (base -> M1) with `feature` (F1, F2) forked at `base`, and
    /// record feature's metadata boundary as `base`. `main` is deliberately
    /// advanced past the recorded boundary so that a successful edit rewrites
    /// `parentBranchRevision` to a genuinely different value -- otherwise the
    /// metadata blob would be rewritten byte-identically and its ref OID would
    /// not move, making the assertions vacuous.
    ///
    /// Returns `(base_oid, main_tip_oid)`. Leaves `feature` checked out.
    fn setup(dir: &Path) -> (String, String) {
        git_hermetic(dir, &["init", "-b", "main"]);
        git_hermetic(dir, &["config", "user.name", "Test"]);
        git_hermetic(dir, &["config", "user.email", "test@example.com"]);
        // Local config wins over the developer's global config; the `git rebase`
        // subprocess inside apply_edit_plan does not get GIT_CONFIG_GLOBAL.
        git_hermetic(dir, &["config", "commit.gpgsign", "false"]);
        git_hermetic(
            dir,
            &["config", "core.hooksPath", "/nonexistent-stax-hooks"],
        );

        std::fs::write(dir.join("base.txt"), "base\n").unwrap();
        git_hermetic(dir, &["add", "-A"]);
        git_hermetic(dir, &["commit", "-m", "base"]);
        let base_oid = git_hermetic(dir, &["rev-parse", "HEAD"]);

        GitRepo::open_from_path(dir)
            .unwrap()
            .set_trunk("main")
            .unwrap();

        git_hermetic(dir, &["checkout", "-b", "feature"]);
        std::fs::write(dir.join("f1.txt"), "f1\n").unwrap();
        git_hermetic(dir, &["add", "-A"]);
        git_hermetic(dir, &["commit", "-m", "F1"]);
        std::fs::write(dir.join("f2.txt"), "f2\n").unwrap();
        git_hermetic(dir, &["add", "-A"]);
        git_hermetic(dir, &["commit", "-m", "F2"]);

        git_hermetic(dir, &["checkout", "main"]);
        std::fs::write(dir.join("m1.txt"), "m1\n").unwrap();
        git_hermetic(dir, &["add", "-A"]);
        git_hermetic(dir, &["commit", "-m", "M1"]);
        let main_tip = git_hermetic(dir, &["rev-parse", "HEAD"]);

        git_hermetic(dir, &["checkout", "feature"]);
        let repo = GitRepo::open_from_path(dir).unwrap();
        BranchMetadata::new("main", &base_oid)
            .write(repo.inner(), "feature")
            .unwrap();

        (base_oid, main_tip)
    }

    /// Same `git log` shape `run()` parses into its commit list.
    fn collect_commits(dir: &Path) -> Vec<CommitInfo> {
        git_hermetic(dir, &["log", "--reverse", "--format=%H %s", "main..HEAD"])
            .lines()
            .filter(|l| !l.is_empty())
            .map(|line| {
                let (sha, message) = line.split_once(' ').unwrap_or((line, ""));
                CommitInfo {
                    sha: sha.to_string(),
                    message: message.to_string(),
                }
            })
            .collect()
    }

    /// A successful edit rewrites `parentBranchRevision`; that write must be
    /// tracked in the op receipt or `stax undo` cannot reverse it.
    #[test]
    fn edit_records_the_branch_metadata_ref_in_the_op_receipt() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        let (base_oid, main_tip) = setup(dir);

        let repo = GitRepo::open_from_path(dir).unwrap();
        let meta = BranchMetadata::read(repo.inner(), "feature")
            .unwrap()
            .unwrap();
        let commits = collect_commits(dir);
        assert_eq!(commits.len(), 2);
        let meta_oid_before = git_hermetic(dir, &["rev-parse", "refs/branch-metadata/feature"]);

        // Drop the second commit -- `drop` is always legal on a non-first commit.
        let actions = [EditAction::Pick, EditAction::Drop];
        apply_edit_plan(&repo, "feature", meta, &commits, &actions, true, &[]).unwrap();

        let meta_oid_after = git_hermetic(dir, &["rev-parse", "refs/branch-metadata/feature"]);
        assert_ne!(
            meta_oid_before, meta_oid_after,
            "the edit must have rewritten the metadata blob"
        );

        // Re-open: the metadata blob was written by a `git update-ref`
        // subprocess, which the original handle's cached refdb may not see.
        let reopened = GitRepo::open_from_path(dir).unwrap();
        let updated = BranchMetadata::read(reopened.inner(), "feature")
            .unwrap()
            .unwrap();
        assert_eq!(updated.parent_branch_revision, main_tip);
        assert_ne!(updated.parent_branch_revision, base_oid);

        let receipt = OpReceipt::load_latest(reopened.git_dir().unwrap())
            .unwrap()
            .expect("edit should have written a receipt");
        let entry = receipt
            .local_refs
            .iter()
            .find(|e| e.branch == "feature@meta")
            .expect("feature@meta must be tracked so `stax undo` can restore parentBranchRevision");
        assert_eq!(entry.oid_before.as_deref(), Some(meta_oid_before.as_str()));
        assert_eq!(entry.oid_after.as_deref(), Some(meta_oid_after.as_str()));
        assert!(entry.after_recorded);
    }

    /// End-to-end: `stax undo` after `stax edit` must restore the branch head
    /// *and* the pre-edit `parentBranchRevision`.
    #[test]
    fn undo_after_edit_restores_the_pre_edit_parent_branch_revision() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        let (base_oid, main_tip) = setup(dir);

        let repo = GitRepo::open_from_path(dir).unwrap();
        let meta = BranchMetadata::read(repo.inner(), "feature")
            .unwrap()
            .unwrap();
        assert_eq!(meta.parent_branch_revision, base_oid);
        let commits = collect_commits(dir);
        let head_before = git_hermetic(dir, &["rev-parse", "feature"]);

        let actions = [EditAction::Pick, EditAction::Drop];
        apply_edit_plan(&repo, "feature", meta, &commits, &actions, true, &[]).unwrap();

        let after = GitRepo::open_from_path(dir).unwrap();
        let edited = BranchMetadata::read(after.inner(), "feature")
            .unwrap()
            .unwrap();
        assert_eq!(edited.parent_branch_revision, main_tip);
        assert_ne!(git_hermetic(dir, &["rev-parse", "feature"]), head_before);
        assert_eq!(
            git_hermetic(dir, &["log", "--format=%s", "main..feature"])
                .lines()
                .count(),
            1,
            "the drop should have removed one commit"
        );

        // Same code path `stax undo` uses (see commands/undo.rs).
        RepositorySession::open(dir)
            .unwrap()
            .undo_transaction(None, false, &mut NoopOperationReporter)
            .unwrap();

        let restored_repo = GitRepo::open_from_path(dir).unwrap();
        let restored = BranchMetadata::read(restored_repo.inner(), "feature")
            .unwrap()
            .unwrap();
        assert_eq!(
            restored.parent_branch_revision, base_oid,
            "undo must restore the pre-edit parentBranchRevision, not leave the post-edit one"
        );
        assert_eq!(git_hermetic(dir, &["rev-parse", "feature"]), head_before);
    }
}
