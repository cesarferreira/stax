//! `stax create` — create a new branch, optionally with a first commit.
//!
//! Two flows live in this file:
//!
//! - **Commit-first** (`-m "msg"`): `git commit` runs *before* any destination
//!   branch ref is created. When the requested parent is the current branch, the
//!   commit is made on the current branch and then split off. For `--from` and
//!   `--below`, the commit is made on a detached checkout of the requested
//!   parent so the new commit has the right base without advancing that parent.
//!   On hook failure or Ctrl+C no destination branch exists, so retries do not
//!   drift to `mybranch-2`, `mybranch-3`, etc.
//! - **Branch-first** (everything else: name-only `st create`, no-op `-m` with
//!   a clean tree): create the branch and switch to it first. Failures here use
//!   the legacy `rollback_create` cleanup.
//!
//! Both flows share `create_branch_with_banner` for the create → metadata →
//! `--insert`/`--below` placement → checkout → summary sequence.
//!
//! When the user passes `-m` but nothing is staged and `-a` wasn't supplied,
//! `stax create` offers the shared staging menu (see
//! `crate::commands::staging`) — stage all, `--patch`, empty branch, or abort.

use crate::commands::staging::{self, ContinueLabel, StagingAction};
use crate::config::Config;
use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::progress::LiveTimer;
use crate::remote;
use anyhow::{bail, Context, Result};
use colored::Colorize;
use console::Term;
use dialoguer::{theme::ColorfulTheme, Input, Select};
use serde::Deserialize;
use std::io::IsTerminal;
use std::path::Path;
use std::process::{Command, ExitStatus};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StageMode {
    /// No staging (plain `stax create <name>` or wizard empty-branch choice).
    None,
    /// `-m` was passed without `-a`: prompt if nothing is staged, otherwise
    /// commit what's already in the index.
    ExistingOnly,
    /// `-a/--all` was passed: force-stage everything.
    All,
}

struct CreatePlacement {
    parent_branch: String,
    below_current_meta: Option<BranchMetadata>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AiCreateTargets {
    branch: bool,
    message: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AiCreateDetails {
    branch: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawAiCreateDetails {
    branch: Option<String>,
    branch_name: Option<String>,
    message: Option<String>,
    commit_message: Option<String>,
}

const MAX_AI_CREATE_DIFF_BYTES: usize = 80_000;

fn resolve_ai_create_targets(
    ai: bool,
    name: Option<&str>,
    message: Option<&str>,
    all: bool,
    has_staged_changes: bool,
    has_uncommitted_changes: bool,
    can_prompt_for_staging: bool,
) -> Result<Option<AiCreateTargets>> {
    if !ai {
        return Ok(None);
    }

    let targets = AiCreateTargets {
        branch: name.is_none(),
        message: message.is_none()
            && (all || has_staged_changes || (has_uncommitted_changes && can_prompt_for_staging)),
    };

    Ok((targets.branch || targets.message).then_some(targets))
}

fn generate_ai_create_details(
    workdir: &Path,
    config: &mut Config,
    targets: AiCreateTargets,
    stage_all: bool,
    non_interactive: bool,
) -> Result<AiCreateDetails> {
    use crate::commands::generate;

    let (agent, model) = if non_interactive {
        let agent = generate::resolve_agent_non_interactive(None, config, "generate")?;
        let model = generate::resolve_model(None, config, &agent, "generate")?;
        (agent, model)
    } else if config.ai.generate.agent.is_some() {
        let agent = config
            .ai
            .agent_for("generate")
            .context("No AI agent configured for create generation")?
            .to_string();
        let model = generate::resolve_model(None, config, &agent, "generate")?;
        (agent, model)
    } else {
        generate::prompt_for_feature_ai(config, "generate")?
    };

    generate::print_using_agent(&agent, model.as_deref());

    let context_timer = LiveTimer::maybe_new(true, "Collecting branch context...");
    let status = git_stdout(workdir, &["status", "--short", "--untracked-files=all"]);
    let staged_diff = git_stdout(workdir, &["diff", "--cached"]);
    let unstaged_diff = git_stdout(workdir, &["diff"]);
    LiveTimer::maybe_finish_ok(context_timer, "done");

    let prompt =
        build_ai_create_details_prompt(&status, &staged_diff, &unstaged_diff, targets, stage_all);

    let generation_timer = LiveTimer::maybe_new(true, "Generating AI create details...");
    let raw = match generate::invoke_ai_agent(&agent, model.as_deref(), &prompt) {
        Ok(raw) => raw,
        Err(err) => {
            LiveTimer::maybe_finish_warn(generation_timer, "failed");
            return Err(err);
        }
    };

    match parse_ai_create_details(&raw, targets) {
        Ok(details) => {
            LiveTimer::maybe_finish_ok(generation_timer, "done");
            Ok(details)
        }
        Err(err) => {
            LiveTimer::maybe_finish_warn(generation_timer, "failed");
            Err(err)
        }
    }
}

fn resolve_ai_create_details_for_use(
    mut details: AiCreateDetails,
    targets: AiCreateTargets,
    auto_accept: bool,
) -> Result<AiCreateDetails> {
    if auto_accept {
        return Ok(details);
    }

    if targets.branch {
        let suggested = details
            .branch
            .clone()
            .context("AI agent did not return a branch")?;
        let branch: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Branch name")
            .default(suggested)
            .interact_text()?;
        details.branch = Some(require_non_empty(branch, "branch")?);
    }

    if targets.message {
        let suggested = details
            .message
            .clone()
            .context("AI agent did not return a commit message")?;
        let message: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Commit message")
            .default(suggested)
            .interact_text()?;
        details.message = Some(require_non_empty(message, "commit message")?);
    }

    Ok(details)
}

fn parse_ai_create_details(raw: &str, targets: AiCreateTargets) -> Result<AiCreateDetails> {
    let json = extract_ai_json(raw);
    let parsed: RawAiCreateDetails = serde_json::from_str(&json)
        .context("AI agent did not return JSON create details with branch/message fields")?;

    let branch = parsed
        .branch
        .or(parsed.branch_name)
        .and_then(non_empty_trimmed);
    let message = parsed
        .message
        .or(parsed.commit_message)
        .and_then(non_empty_trimmed);

    if targets.branch && branch.is_none() {
        bail!("AI agent did not return a non-empty branch");
    }
    if targets.message && message.is_none() {
        bail!("AI agent did not return a non-empty commit message");
    }

    Ok(AiCreateDetails {
        branch: targets.branch.then_some(branch).flatten(),
        message: targets.message.then_some(message).flatten(),
    })
}

fn build_ai_create_details_prompt(
    status: &str,
    staged_diff: &str,
    unstaged_diff: &str,
    targets: AiCreateTargets,
    stage_all: bool,
) -> String {
    let mut prompt = String::new();

    match (targets.branch, targets.message) {
        (true, true) => prompt
            .push_str("Generate a branch name and first commit message for these changes.\n\n"),
        (true, false) => prompt.push_str("Generate a branch name for these changes.\n\n"),
        (false, true) => prompt.push_str("Generate a first commit message for these changes.\n\n"),
        (false, false) => prompt.push_str("Summarize these changes.\n\n"),
    }

    prompt.push_str("Return only a compact JSON object with these string fields: ");
    match (targets.branch, targets.message) {
        (true, true) => prompt.push_str("\"branch\" and \"message\""),
        (true, false) => prompt.push_str("\"branch\""),
        (false, true) => prompt.push_str("\"message\""),
        (false, false) => prompt.push_str(""),
    }
    prompt.push_str(". Do not include markdown fences or explanatory text.\n\n");

    if targets.branch {
        prompt.push_str(
            "Branch requirements:\n- Short lowercase slug suitable for a Git branch\n- No spaces\n- No prefix like feature/ unless the change clearly needs a nested branch name\n\n",
        );
    }

    if targets.message {
        prompt.push_str(
            "Commit message requirements:\n- Imperative mood\n- Concise subject line\n- No trailing period\n\n",
        );
    }

    if stage_all {
        prompt.push_str("The command will stage all changes before committing.\n\n");
    } else {
        prompt.push_str(
            "Only already-staged changes are guaranteed to be committed; unstaged changes may be used for naming context.\n\n",
        );
    }

    if !status.is_empty() {
        prompt.push_str("Git status:\n```\n");
        prompt.push_str(status);
        prompt.push_str("\n```\n\n");
    }

    if !staged_diff.is_empty() {
        prompt.push_str("Staged diff:\n```diff\n");
        prompt.push_str(&truncate_ai_create_diff(staged_diff));
        prompt.push_str("\n```\n\n");
    }

    if !unstaged_diff.is_empty() {
        prompt.push_str("Unstaged diff:\n```diff\n");
        prompt.push_str(&truncate_ai_create_diff(unstaged_diff));
        prompt.push_str("\n```\n\n");
    }

    prompt
}

fn git_stdout(workdir: &Path, args: &[&str]) -> String {
    Command::new("git")
        .args(args)
        .current_dir(workdir)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default()
}

fn truncate_ai_create_diff(diff: &str) -> String {
    if diff.len() <= MAX_AI_CREATE_DIFF_BYTES {
        return diff.to_string();
    }

    let safe_end = safe_char_boundary(diff, MAX_AI_CREATE_DIFF_BYTES);
    let safe = &diff[..safe_end];
    let cut = safe.rfind('\n').unwrap_or(safe.len());
    format!(
        "{}\n\n... (diff truncated, showing first ~80KB of {} total) ...",
        &safe[..cut],
        format_ai_bytes(diff.len())
    )
}

fn safe_char_boundary(value: &str, max: usize) -> usize {
    let mut end = max.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    end
}

fn format_ai_bytes(bytes: usize) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1}MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{}B", bytes)
    }
}

fn extract_ai_json(raw: &str) -> String {
    let trimmed = raw.trim();
    let unfenced = if trimmed.starts_with("```") {
        let without_opening = trimmed.lines().skip(1).collect::<Vec<_>>().join("\n");
        without_opening
            .trim()
            .strip_suffix("```")
            .unwrap_or(without_opening.trim())
            .trim()
            .to_string()
    } else {
        trimmed.to_string()
    };

    if unfenced.starts_with('{') && unfenced.ends_with('}') {
        return unfenced;
    }

    match (unfenced.find('{'), unfenced.rfind('}')) {
        (Some(start), Some(end)) if start < end => unfenced[start..=end].to_string(),
        _ => unfenced,
    }
}

fn non_empty_trimmed(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn require_non_empty(value: String, field: &str) -> Result<String> {
    non_empty_trimmed(value).with_context(|| format!("{} cannot be empty", field))
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    name: Option<String>,
    message: Option<String>,
    from: Option<String>,
    prefix: Option<String>,
    all: bool,
    insert: bool,
    below: bool,
    no_verify: bool,
    ai: bool,
    yes: bool,
) -> Result<()> {
    let repo = GitRepo::open()?;
    let mut config = Config::load()?;
    let current = repo.current_branch()?;
    let placement = resolve_create_placement(&repo, &current, from, insert, below)?;
    let parent_branch = placement.parent_branch;
    let below_current_meta = placement.below_current_meta;

    if repo.branch_commit(&parent_branch).is_err() {
        anyhow::bail!("Branch '{}' does not exist", parent_branch);
    }

    let workdir = repo.workdir()?;
    let has_staged_changes = !staging::is_staging_area_empty(workdir)?;
    let has_uncommitted_changes = staging::has_uncommitted_changes(workdir);
    let non_interactive =
        yes || !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal();
    let can_prompt_for_staging = !non_interactive;
    let ai_targets = resolve_ai_create_targets(
        ai,
        name.as_deref(),
        message.as_deref(),
        all,
        has_staged_changes,
        has_uncommitted_changes,
        can_prompt_for_staging,
    )?;
    let ai_details = match ai_targets {
        Some(targets) => {
            let details =
                generate_ai_create_details(workdir, &mut config, targets, all, non_interactive)?;
            Some(resolve_ai_create_details_for_use(
                details,
                targets,
                non_interactive,
            )?)
        }
        None => None,
    };
    let ai_branch = ai_details
        .as_ref()
        .and_then(|details| details.branch.clone());
    let ai_message = ai_details
        .as_ref()
        .and_then(|details| details.message.clone());

    // Get the branch name from either name or message
    // When using -m, the message is used for both branch name and commit message.
    // `stax create -m` respects already-staged changes. When nothing is staged
    // it offers an interactive menu (or bails in non-TTY). Use -a/--all to
    // skip the menu and force-stage.
    // When neither name nor message is provided, launch interactive wizard.
    let (input, commit_message, stage_mode, generated_from_message) = if let Some(n) = &name {
        let commit_message = ai_message;
        let stage_mode = if commit_message.is_some() {
            if all {
                StageMode::All
            } else {
                StageMode::ExistingOnly
            }
        } else if all {
            StageMode::All
        } else {
            StageMode::None
        };
        (n.clone(), commit_message, stage_mode, false)
    } else if let Some(generated_branch) = ai_branch {
        let commit_message = message.clone().or(ai_message);
        let stage_mode = if commit_message.is_some() {
            if all {
                StageMode::All
            } else {
                StageMode::ExistingOnly
            }
        } else if all {
            StageMode::All
        } else {
            StageMode::None
        };
        (generated_branch, commit_message, stage_mode, true)
    } else if let Some(m) = &message {
        (
            m.clone(),
            Some(m.clone()),
            if all {
                StageMode::All
            } else {
                StageMode::ExistingOnly
            },
            true,
        )
    } else {
        if !Term::stderr().is_term() {
            bail!("Branch name required. Use: stax create <name> or stax create -m \"message\"");
        }
        // Launch interactive wizard
        let (wizard_name, wizard_msg, wizard_stage) = run_wizard(repo.workdir()?, &parent_branch)?;
        (wizard_name, wizard_msg, wizard_stage, false)
    };

    // Format the branch name according to config
    let branch_name = match prefix.as_deref() {
        Some(_) => config.format_branch_name_with_prefix_override(&input, prefix.as_deref()),
        None => config.format_branch_name(&input),
    };
    let existing_branches = repo.list_branches()?;
    let branch_name =
        resolve_branch_name_conflicts(&branch_name, &existing_branches, generated_from_message)?;

    // Before creating the branch, resolve the staging question. Doing this
    // early means declining ("Abort" / empty `--patch` exit) is a clean no-op
    // — no orphaned branch, no refs touched.
    //
    // Returns (needs_stage_all, skip_commit):
    //   needs_stage_all — run `git add -A` before the commit.
    //   skip_commit     — the user picked "empty branch"; drop the message and
    //                     create an empty branch instead of committing.
    let (needs_stage_all, skip_commit) = if stage_mode == StageMode::ExistingOnly {
        let workdir = repo.workdir()?;
        if staging::is_staging_area_empty(workdir)? && staging::has_uncommitted_changes(workdir) {
            match staging::prompt_action(
                workdir,
                ContinueLabel::EmptyBranch,
                "Stage files with `git add` first, or use `stax create -a -m \"message\"`.",
            )? {
                StagingAction::All => (true, false),
                StagingAction::Patch => {
                    staging::stage_patch(workdir)?;
                    if staging::is_staging_area_empty(workdir)? {
                        staging::print_patch_empty_notice();
                        return Ok(());
                    }
                    (false, false)
                }
                StagingAction::Continue => (false, true),
                StagingAction::Abort => {
                    println!(
                        "{}",
                        "Aborted. Stage files with `git add` first, or use `stax create -a -m \"message\"`."
                            .dimmed()
                    );
                    return Ok(());
                }
            }
        } else {
            (false, false)
        }
    } else {
        (false, false)
    };

    let commit_message = if skip_commit { None } else { commit_message };

    // Commit-first path: when the user supplied -m, run the commit BEFORE
    // creating or switching to the destination branch. If pre-commit hooks fail
    // (or the user hits Ctrl+C) we exit with no destination refs touched: no
    // orphan branch, no name drift to `-2`/`-3`, and the user can retry the same
    // command. Only after a successful commit do we create metadata and move
    // into the requested placement.
    if let Some(msg) = commit_message.as_deref() {
        return run_commit_first(
            &repo,
            &config,
            &current,
            &parent_branch,
            &branch_name,
            msg,
            stage_mode,
            needs_stage_all,
            insert,
            below_current_meta.as_ref(),
            no_verify,
        );
    }

    create_branch_with_banner(
        &repo,
        &config,
        &current,
        &parent_branch,
        &branch_name,
        insert,
        below_current_meta.as_ref(),
    )?;

    // Stage/commit behavior:
    // - StageMode::All / needs_stage_all => run `git add -A`
    // - StageMode::ExistingOnly (files already staged) => keep current index
    // - StageMode::None => no staging/committing
    if stage_mode != StageMode::None {
        let workdir = repo.workdir()?;

        if stage_mode == StageMode::All || needs_stage_all {
            if let Err(e) = staging::stage_all(workdir) {
                rollback_create_and_restore(
                    &repo,
                    &current,
                    &branch_name,
                    below_current_meta.as_ref(),
                );
                return Err(e);
            }
        }

        if stage_mode == StageMode::All {
            println!("{}", "Changes staged".dimmed());
        }
    }

    if config.ui.tips {
        println!(
            "{}",
            "Hint: Run `st ss` to submit, or add changes with `st modify -a -m \"message\"`"
                .dimmed()
        );
    }

    Ok(())
}

/// Best-effort rollback: unstage changes, checkout the original branch,
/// delete the new branch and its metadata.
/// Errors during rollback are intentionally ignored (matching the pattern in split_hunk/app.rs).
fn rollback_create(repo: &GitRepo, original_branch: &str, new_branch: &str) {
    if let Ok(workdir) = repo.workdir() {
        // Reset index first so staged changes from stage_all don't block checkout
        // or leak onto the original branch. This preserves working tree files.
        let _ = Command::new("git")
            .args(["reset"])
            .current_dir(workdir)
            .status();
        let _ = Command::new("git")
            .args(["checkout", original_branch])
            .current_dir(workdir)
            .status();
    }
    let _ = repo.delete_branch(new_branch, true);
    let _ = BranchMetadata::delete(repo.inner(), new_branch);
}

fn rollback_create_and_restore(
    repo: &GitRepo,
    original_branch: &str,
    new_branch: &str,
    restore_original_meta: Option<&BranchMetadata>,
) {
    if let Some(meta) = restore_original_meta {
        let _ = meta.write(repo.inner(), original_branch);
    }
    rollback_create(repo, original_branch, new_branch);
}

/// Graphite-style commit-first flow: commit before creating the destination
/// branch, then split the new commit off to a new branch.
///
/// The key property: nothing observable changes until `git commit` returns
/// successfully. If pre-commit hooks reject the commit, or the user hits
/// Ctrl+C during the commit, no destination branch is created and no metadata
/// is written — so retrying the exact same command is a clean operation that
/// does not drift into `mybranch-2`, `mybranch-3`, etc.
///
/// When `parent_branch != current` (`--from` or `--below`), we check out the
/// parent detached before committing. That makes the resulting commit's parent
/// correct without advancing the requested parent branch.
#[allow(clippy::too_many_arguments)]
fn run_commit_first(
    repo: &GitRepo,
    config: &Config,
    current: &str,
    parent_branch: &str,
    branch_name: &str,
    message: &str,
    stage_mode: StageMode,
    needs_stage_all: bool,
    insert: bool,
    below_current_meta: Option<&BranchMetadata>,
    no_verify: bool,
) -> Result<()> {
    let workdir = repo.workdir()?;
    let committing_on_current = parent_branch == current;
    let parent_sha = repo.branch_commit(parent_branch)?;

    if !committing_on_current {
        checkout_detached_for_commit(workdir, parent_branch)?;
    }

    // Stage (if requested) BEFORE the commit so hooks see the final tree.
    if stage_mode == StageMode::All || needs_stage_all {
        if let Err(e) = staging::stage_all(workdir) {
            restore_after_failed_pre_branch_commit(repo, workdir, current, committing_on_current);
            return Err(e);
        }
    }

    // If nothing is staged by the time we reach here, there is no commit to
    // make. Fall back to creating an empty branch — same shape as the
    // branch-first path without the trailing commit.
    let staging_area_empty = match staging::is_staging_area_empty(workdir) {
        Ok(empty) => empty,
        Err(e) => {
            restore_after_failed_pre_branch_commit(repo, workdir, current, committing_on_current);
            return Err(e);
        }
    };

    if staging_area_empty {
        create_branch_with_banner(
            repo,
            config,
            current,
            parent_branch,
            branch_name,
            insert,
            below_current_meta,
        )?;
        println!("{}", "No changes to commit".dimmed());
        print_tips(config);
        return Ok(());
    }

    // Run the commit before the destination branch exists. `--quiet`
    // suppresses git's "[<branch> <sha>] <msg>" summary that would otherwise
    // mention the temporary commit location; we print our own summary after
    // the commit is split off. Pre-commit hook output is not suppressed by -q.
    let commit = match run_git_commit(workdir, message, true, no_verify) {
        Ok(commit) => commit,
        Err(e) => {
            restore_after_failed_pre_branch_commit(repo, workdir, current, committing_on_current);
            return Err(e);
        }
    };

    if !commit.status.success() {
        restore_after_failed_pre_branch_commit(repo, workdir, current, committing_on_current);
        if commit.interrupted {
            bail!(
                "Commit interrupted. \
                 No branch was created — fix the issue and retry with the same command."
            );
        } else {
            bail!(
                "Commit failed (pre-commit hook or other error). \
                 No branch was created — fix the issue and retry with the same command."
            );
        }
    }

    if commit.interrupted {
        rollback_after_commit(
            workdir,
            current,
            &parent_sha,
            None,
            repo,
            committing_on_current,
            below_current_meta,
        );
        bail!(
            "Commit interrupted. \
             No branch was created — fix the issue and retry with the same command."
        );
    }

    // From here on the commit exists either on the current branch or on a
    // detached HEAD at `parent_branch`. Any failure must preserve the user's
    // staged work and avoid leaving a destination branch behind.
    let new_sha = match repo.rev_parse("HEAD") {
        Ok(sha) => sha,
        Err(e) => {
            rollback_after_commit(
                workdir,
                current,
                &parent_sha,
                None,
                repo,
                committing_on_current,
                below_current_meta,
            );
            return Err(e);
        }
    };

    if let Err(e) = repo.create_branch_at_commit(branch_name, &new_sha) {
        rollback_after_commit(
            workdir,
            current,
            &parent_sha,
            None,
            repo,
            committing_on_current,
            below_current_meta,
        );
        return Err(e);
    }

    let meta = BranchMetadata::new(parent_branch, &parent_sha);
    if let Err(e) = meta.write(repo.inner(), branch_name) {
        rollback_after_commit(
            workdir,
            current,
            &parent_sha,
            Some(branch_name),
            repo,
            committing_on_current,
            below_current_meta,
        );
        return Err(e);
    }

    if insert {
        if let Err(e) = apply_insert_reparenting(repo, parent_branch, branch_name) {
            rollback_after_commit(
                workdir,
                current,
                &parent_sha,
                Some(branch_name),
                repo,
                committing_on_current,
                below_current_meta,
            );
            return Err(e);
        }
    }

    if let Some(current_meta) = below_current_meta {
        if let Err(e) = apply_below_reparenting(repo, current, branch_name, current_meta) {
            rollback_after_commit(
                workdir,
                current,
                &parent_sha,
                Some(branch_name),
                repo,
                committing_on_current,
                below_current_meta,
            );
            return Err(e);
        }
    }

    if committing_on_current {
        // Move the current branch ref back to the pre-commit SHA. The new commit
        // now lives only on `branch_name`.
        let current_ref = format!("refs/heads/{}", current);
        if let Err(e) = repo.update_ref(&current_ref, &parent_sha) {
            rollback_after_commit(
                workdir,
                current,
                &parent_sha,
                Some(branch_name),
                repo,
                committing_on_current,
                below_current_meta,
            );
            return Err(e);
        }
    }

    // Switch to the new branch. In the current-parent case HEAD still points at
    // `current` (now at the old SHA) while the working tree matches the new
    // commit; in the detached-parent case HEAD already points at the new commit.
    // `git checkout` only moves HEAD in both cases.
    repo.checkout(branch_name)?;

    print_remote_parent_warning(repo, config, parent_branch);
    println!(
        "Created and switched to branch '{}' (stacked on {})",
        branch_name.green(),
        parent_branch.blue()
    );
    println!("Committed: {}", message.cyan());
    print_tips(config);

    Ok(())
}

struct GitCommitResult {
    status: ExitStatus,
    interrupted: bool,
}

struct CommitSignalGuard {
    interrupted: Arc<AtomicBool>,
    registrations: Vec<signal_hook::SigId>,
}

impl CommitSignalGuard {
    fn install() -> Result<Self> {
        let interrupted = Arc::new(AtomicBool::new(false));
        let sigint = signal_hook::flag::register(
            signal_hook::consts::signal::SIGINT,
            Arc::clone(&interrupted),
        )
        .context("Failed to install SIGINT handler")?;
        let sigterm = match signal_hook::flag::register(
            signal_hook::consts::signal::SIGTERM,
            Arc::clone(&interrupted),
        ) {
            Ok(registration) => registration,
            Err(e) => {
                let _ = signal_hook::low_level::unregister(sigint);
                return Err(anyhow::Error::from(e).context("Failed to install SIGTERM handler"));
            }
        };

        Ok(Self {
            interrupted,
            registrations: vec![sigint, sigterm],
        })
    }

    fn interrupted(&self) -> bool {
        self.interrupted.load(Ordering::SeqCst)
    }
}

impl Drop for CommitSignalGuard {
    fn drop(&mut self) {
        for registration in self.registrations.drain(..) {
            let _ = signal_hook::low_level::unregister(registration);
        }
    }
}

fn run_git_commit(
    workdir: &Path,
    message: &str,
    quiet: bool,
    no_verify: bool,
) -> Result<GitCommitResult> {
    let guard = CommitSignalGuard::install()?;
    let mut args = vec!["commit"];
    if quiet {
        args.push("--quiet");
    }
    if no_verify {
        args.push("--no-verify");
    }
    args.extend(["-m", message]);

    let status = Command::new("git")
        .args(args)
        .current_dir(workdir)
        .status()
        .context("Failed to run git commit")?;
    let interrupted = guard.interrupted();

    Ok(GitCommitResult {
        status,
        interrupted,
    })
}

fn checkout_detached_for_commit(workdir: &Path, parent_branch: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["checkout", "--detach", parent_branch])
        .current_dir(workdir)
        .output()
        .context("Failed to run git checkout --detach")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!(
            "Failed to prepare commit on '{}': {}",
            parent_branch,
            stderr
        );
    }

    Ok(())
}

fn restore_after_failed_pre_branch_commit(
    repo: &GitRepo,
    workdir: &Path,
    original_branch: &str,
    committing_on_current: bool,
) {
    if committing_on_current {
        return;
    }

    let _ = Command::new("git")
        .args(["reset"])
        .current_dir(workdir)
        .status();
    let _ = repo.checkout(original_branch);
}

/// Undo the partial state left by `run_commit_first` when a step after
/// `git commit` (branch creation, metadata write, ref update) fails.
///
/// `--soft` keeps the working tree and index exactly as git left them after
/// the successful commit, so the user can retry without re-staging.
fn rollback_after_commit(
    workdir: &Path,
    original_branch: &str,
    old_sha: &str,
    new_branch: Option<&str>,
    repo: &GitRepo,
    committing_on_current: bool,
    restore_original_meta: Option<&BranchMetadata>,
) {
    if let Some(meta) = restore_original_meta {
        let _ = meta.write(repo.inner(), original_branch);
    }
    if let Some(name) = new_branch {
        let _ = BranchMetadata::delete(repo.inner(), name);
        let _ = repo.delete_branch(name, true);
    }
    let _ = Command::new("git")
        .args(["reset", "--soft", old_sha])
        .current_dir(workdir)
        .status();

    if !committing_on_current {
        let _ = repo.checkout(original_branch);
    }
}

/// Create `branch_name` stacked on `parent_branch`, write stax metadata,
/// apply `--insert`/`--below` reparenting, check out the new branch, and print the
/// "Created and switched…" banner. On any failure, undo all of it with
/// `rollback_create` so the caller sees a clean failure.
///
/// `original` is the branch the user was on when `st create` was invoked; it's
/// where `rollback_create` checks out on failure. For the commit-first
/// no-changes fallback `original == parent_branch == current`; for the
/// branch-first flow `original == current`, and `parent_branch` may or may not
/// equal it (the `--from` and `--below` cases).
///
/// The caller owns whatever comes next — the trailing "No changes to commit"
/// note, the stage/commit block, or the tips line.
fn create_branch_with_banner(
    repo: &GitRepo,
    config: &Config,
    original: &str,
    parent_branch: &str,
    branch_name: &str,
    insert: bool,
    below_current_meta: Option<&BranchMetadata>,
) -> Result<()> {
    if parent_branch == original {
        repo.create_branch(branch_name)?;
    } else {
        repo.create_branch_at(branch_name, parent_branch)?;
    }

    let parent_rev = repo.branch_commit(parent_branch)?;
    let meta = BranchMetadata::new(parent_branch, &parent_rev);
    if let Err(e) = meta.write(repo.inner(), branch_name) {
        rollback_create(repo, original, branch_name);
        return Err(e);
    }

    if insert {
        if let Err(e) = apply_insert_reparenting(repo, parent_branch, branch_name) {
            rollback_create(repo, original, branch_name);
            return Err(e);
        }
    }

    if let Some(current_meta) = below_current_meta {
        if let Err(e) = apply_below_reparenting(repo, original, branch_name, current_meta) {
            rollback_create_and_restore(repo, original, branch_name, below_current_meta);
            return Err(e);
        }
    }

    if let Err(e) = repo.checkout(branch_name) {
        rollback_create_and_restore(repo, original, branch_name, below_current_meta);
        return Err(e);
    }

    print_remote_parent_warning(repo, config, parent_branch);
    println!(
        "Created and switched to branch '{}' (stacked on {})",
        branch_name.green(),
        parent_branch.blue()
    );

    Ok(())
}

fn resolve_create_placement(
    repo: &GitRepo,
    current: &str,
    from: Option<String>,
    insert: bool,
    below: bool,
) -> Result<CreatePlacement> {
    if insert && below {
        bail!("`--insert` and `--below` cannot be used together");
    }
    if below && from.is_some() {
        bail!("`--below` cannot be used with `--from`");
    }

    if below {
        let meta = resolve_below_current_metadata(repo, current)?;
        return Ok(CreatePlacement {
            parent_branch: meta.parent_branch_name.clone(),
            below_current_meta: Some(meta),
        });
    }

    Ok(CreatePlacement {
        parent_branch: from.unwrap_or_else(|| current.to_string()),
        below_current_meta: None,
    })
}

fn resolve_below_current_metadata(repo: &GitRepo, current: &str) -> Result<BranchMetadata> {
    let trunk = repo.trunk_branch()?;
    if current == trunk {
        bail!("Cannot create a branch below trunk. Checkout a stacked branch first.");
    }

    let meta = BranchMetadata::read(repo.inner(), current)?.ok_or_else(|| {
        anyhow::anyhow!(
            "Branch '{}' is not tracked by stax. Run `st branch track` first.",
            current
        )
    })?;

    if meta.parent_branch_name == current {
        bail!(
            "Cannot create a branch below '{}': branch metadata points to itself as parent.",
            current
        );
    }

    Ok(meta)
}

/// Reparent children of `parent_branch` onto `new_branch` and print the usual
/// `--insert` summary. Extracted from the branch-first path so both flows
/// share the same behaviour.
fn apply_insert_reparenting(repo: &GitRepo, parent_branch: &str, new_branch: &str) -> Result<()> {
    let stack = Stack::load(repo)?;
    let Some(parent_info) = stack.branches.get(parent_branch) else {
        return Ok(());
    };
    let children: Vec<String> = parent_info
        .children
        .iter()
        .filter(|c| c.as_str() != new_branch)
        .cloned()
        .collect();

    if children.is_empty() {
        return Ok(());
    }

    let new_parent_rev = repo.branch_commit(new_branch)?;
    for child in &children {
        if let Some(child_meta) = BranchMetadata::read(repo.inner(), child)? {
            let updated = BranchMetadata {
                parent_branch_name: new_branch.to_string(),
                parent_branch_revision: new_parent_rev.clone(),
                ..child_meta
            };
            updated.write(repo.inner(), child)?;
        }
    }

    println!(
        "Reparented {} child branch(es) to '{}'",
        children.len(),
        new_branch.green()
    );
    for child in &children {
        println!("  {} -> {}", child.cyan(), new_branch.green());
    }
    println!(
        "{}",
        "Run `stax restack --all` to rebase the reparented branches.".yellow()
    );

    Ok(())
}

/// Insert `new_branch` between `current_branch` and its current parent.
/// Descendant metadata stays untouched: moving the subtree root makes all
/// descendants follow it while preserving their relative structure.
fn apply_below_reparenting(
    repo: &GitRepo,
    current_branch: &str,
    new_branch: &str,
    current_meta: &BranchMetadata,
) -> Result<()> {
    let new_parent_rev = repo.branch_commit(new_branch)?;
    let parent_revision = repo
        .merge_base(new_branch, current_branch)
        .unwrap_or(new_parent_rev);
    let updated = BranchMetadata {
        parent_branch_name: new_branch.to_string(),
        parent_branch_revision: parent_revision,
        ..current_meta.clone()
    };
    updated.write(repo.inner(), current_branch)?;

    let descendants = Stack::load(repo)?.descendants(current_branch);

    println!(
        "Reparented '{}' onto '{}'",
        current_branch.green(),
        new_branch.green()
    );
    if !descendants.is_empty() {
        println!(
            "  {} descendant branch(es) moved with it:",
            descendants.len().to_string().cyan()
        );
        for descendant in &descendants {
            println!("    {}", descendant.dimmed());
        }
    }
    println!(
        "{}",
        "Run `stax restack` to rebase the moved branches onto their new parent.".yellow()
    );

    Ok(())
}

fn print_remote_parent_warning(repo: &GitRepo, config: &Config, parent_branch: &str) {
    let Ok(workdir) = repo.workdir() else {
        return;
    };
    if let Ok(remote_branches) = remote::get_remote_branches(workdir, config.remote_name()) {
        if !remote_branches.contains(&parent_branch.to_string()) {
            println!(
                "{}",
                format!(
                    "Warning: parent '{}' is not on remote '{}'.",
                    parent_branch,
                    config.remote_name()
                )
                .yellow()
            );
        }
    }
}

fn print_tips(config: &Config) {
    if config.ui.tips {
        println!(
            "{}",
            "Hint: Run `st ss` to submit, or add changes with `st modify -a -m \"message\"`"
                .dimmed()
        );
    }
}

#[derive(Clone, Copy)]
enum BranchNameConflict<'a> {
    Exact(&'a str),
    ExistingIsAncestor(&'a str),
    ExistingIsDescendant(&'a str),
}

fn resolve_branch_name_conflicts(
    branch_name: &str,
    existing_branches: &[String],
    generated_from_message: bool,
) -> Result<String> {
    match detect_branch_name_conflict(branch_name, existing_branches) {
        None => Ok(branch_name.to_string()),
        Some(BranchNameConflict::Exact(_) | BranchNameConflict::ExistingIsDescendant(_))
            if generated_from_message =>
        {
            for suffix in 2..1000 {
                let candidate = append_branch_suffix(branch_name, suffix);
                if detect_branch_name_conflict(&candidate, existing_branches).is_none() {
                    return Ok(candidate);
                }
            }

            bail!(
                "Cannot create a unique branch name from '{}'. Too many similarly named branches already exist.",
                branch_name
            );
        }
        Some(conflict) => bail!("{}", branch_name_conflict_message(branch_name, conflict)),
    }
}

fn detect_branch_name_conflict<'a>(
    branch_name: &str,
    existing_branches: &'a [String],
) -> Option<BranchNameConflict<'a>> {
    for existing in existing_branches {
        if branch_name == existing {
            return Some(BranchNameConflict::Exact(existing));
        }

        if branch_name.starts_with(&format!("{}/", existing)) {
            return Some(BranchNameConflict::ExistingIsAncestor(existing));
        }

        if existing.starts_with(&format!("{}/", branch_name)) {
            return Some(BranchNameConflict::ExistingIsDescendant(existing));
        }
    }

    None
}

fn branch_name_conflict_message(branch_name: &str, conflict: BranchNameConflict<'_>) -> String {
    match conflict {
        BranchNameConflict::Exact(existing) => format!(
            "Cannot create '{}': branch '{}' already exists.\n\
             Use `st checkout {}` or choose a different name.",
            branch_name, existing, existing
        ),
        BranchNameConflict::ExistingIsAncestor(existing) => format!(
            "Cannot create '{}': branch '{}' already exists.\n\
             Git doesn't allow a branch and its sub-path to coexist.\n\
             Either delete '{}' first, or use a different name like '{}-ui'.",
            branch_name, existing, existing, existing
        ),
        BranchNameConflict::ExistingIsDescendant(existing) => format!(
            "Cannot create '{}': branch '{}' already exists.\n\
             Git doesn't allow a branch and its sub-path to coexist.\n\
             Either delete '{}' first, or use a different name.",
            branch_name, existing, existing
        ),
    }
}

fn append_branch_suffix(branch_name: &str, suffix: usize) -> String {
    match branch_name.rsplit_once('/') {
        Some((prefix, leaf)) => format!("{}/{}-{}", prefix, leaf, suffix),
        None => format!("{}-{}", branch_name, suffix),
    }
}

/// Interactive wizard for branch creation when no arguments provided.
///
/// Returns `(branch_name, commit_message, stage_mode)`. When the wizard
/// routes to the staging menu and the user picks `--patch`, we run
/// `git add -p` inline so the caller sees the "staged manually" shape
/// (`StageMode::ExistingOnly`, no auto-stage-all).
fn run_wizard(workdir: &Path, parent_branch: &str) -> Result<(String, Option<String>, StageMode)> {
    println!();
    println!("╭─ Create Stacked Branch ─────────────────────────────╮");
    println!(
        "│ Parent: {:<43} │",
        format!("{} (current branch)", parent_branch.cyan())
    );
    println!("╰─────────────────────────────────────────────────────╯");
    println!();

    let name: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Branch name")
        .interact_text()?;

    if name.trim().is_empty() {
        bail!("Branch name cannot be empty");
    }

    let has_changes = staging::has_uncommitted_changes(workdir);
    let change_count = staging::count_uncommitted_changes(workdir);

    if !has_changes {
        println!();
        return Ok((name, None, StageMode::None));
    }

    println!();

    let stage_label = if change_count > 0 {
        format!("Stage all changes ({} files modified)", change_count)
    } else {
        "Stage all changes".to_string()
    };
    let options = vec![
        stage_label.as_str(),
        "Select changes to commit (--patch)",
        "Empty branch (no changes)",
    ];

    let choice = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("What to include")
        .items(&options)
        .default(0)
        .interact()?;

    let stage_mode = match choice {
        0 => StageMode::All,
        1 => {
            staging::stage_patch(workdir)?;
            if staging::is_staging_area_empty(workdir)? {
                staging::print_patch_empty_notice();
                // Fall through to creating the branch with no commit —
                // matches the "empty branch" outcome.
                StageMode::None
            } else {
                StageMode::ExistingOnly
            }
        }
        _ => StageMode::None,
    };

    // Commit message only when we have something staged.
    let msg = if stage_mode != StageMode::None {
        println!();
        let m: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Commit message (Enter to skip)")
            .allow_empty(true)
            .interact_text()?;
        if m.is_empty() {
            None
        } else {
            Some(m)
        }
    } else {
        None
    };

    println!();
    Ok((name, msg, stage_mode))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(branch: bool, message: bool) -> AiCreateTargets {
        AiCreateTargets { branch, message }
    }

    #[test]
    fn ai_create_targets_plain_ai_generates_branch_and_message_for_all_changes() {
        let resolved =
            resolve_ai_create_targets(true, None, None, true, false, true, false).unwrap();

        assert_eq!(resolved, Some(targets(true, true)));
    }

    #[test]
    fn ai_create_targets_named_branch_generates_commit_message_only() {
        let resolved =
            resolve_ai_create_targets(true, Some("manual-branch"), None, false, true, true, false)
                .unwrap();

        assert_eq!(resolved, Some(targets(false, true)));
    }

    #[test]
    fn ai_create_targets_manual_message_generates_branch_only() {
        let resolved = resolve_ai_create_targets(
            true,
            None,
            Some("Manual commit message"),
            true,
            false,
            true,
            false,
        )
        .unwrap();

        assert_eq!(resolved, Some(targets(true, false)));
    }

    #[test]
    fn ai_create_targets_skips_ai_when_no_field_needs_generation() {
        let resolved = resolve_ai_create_targets(
            true,
            Some("manual-branch"),
            None,
            false,
            false,
            false,
            false,
        )
        .unwrap();

        assert_eq!(resolved, None);
    }

    #[test]
    fn ai_create_targets_do_not_generate_message_without_commit_path() {
        let resolved =
            resolve_ai_create_targets(true, None, None, false, false, true, false).unwrap();

        assert_eq!(resolved, Some(targets(true, false)));
    }

    #[test]
    fn parse_ai_create_details_accepts_fenced_json() {
        let parsed = parse_ai_create_details(
            "```json\n{\"branch\":\"add-ai-create\",\"message\":\"Add AI create\"}\n```",
            targets(true, true),
        )
        .unwrap();

        assert_eq!(parsed.branch.as_deref(), Some("add-ai-create"));
        assert_eq!(parsed.message.as_deref(), Some("Add AI create"));
    }

    #[test]
    fn parse_ai_create_details_requires_requested_fields() {
        let err = parse_ai_create_details(r#"{"message":"Only a message"}"#, targets(true, false))
            .unwrap_err();
        assert!(err.to_string().contains("non-empty branch"));

        let err = parse_ai_create_details(r#"{"branch":"only-branch"}"#, targets(false, true))
            .unwrap_err();
        assert!(err.to_string().contains("non-empty commit message"));
    }

    #[test]
    fn build_ai_create_prompt_mentions_only_requested_fields() {
        let prompt = build_ai_create_details_prompt(
            " M src/lib.rs",
            "diff --git a/src/lib.rs b/src/lib.rs",
            "",
            targets(false, true),
            true,
        );

        assert!(prompt.contains("\"message\""));
        assert!(!prompt.contains("\"branch\" and \"message\""));
        assert!(prompt.contains("The command will stage all changes before committing."));
        assert!(prompt.contains("diff --git"));
    }
}
