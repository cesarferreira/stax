use crate::commands::open::open_url_in_browser;
use crate::config::{Config, StackLinksMode};
use crate::engine::{BranchMetadata, Stack};
use crate::forge::ForgeClient;
use crate::git::GitRepo;
use crate::github::pr::{
    generate_stack_links_markdown, remove_stack_links_from_body, upsert_stack_links_in_body,
    PrInfoWithHead, StackPrInfo,
};
use crate::github::pr_template::{discover_pr_templates, select_template_interactive};
use crate::ops::receipt::{OpKind, PlanSummary};
use crate::ops::tx::{self, Transaction};
use crate::progress::LiveTimer;
use crate::remote::{self, RemoteInfo};
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Editor, Input, Select};
use futures_util::future::join_all;
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitScope {
    Branch,
    Downstack,
    Upstack,
    Stack,
}

impl SubmitScope {
    fn label(self) -> &'static str {
        match self {
            SubmitScope::Branch => "branch",
            SubmitScope::Downstack => "downstack",
            SubmitScope::Upstack => "upstack",
            SubmitScope::Stack => "stack",
        }
    }
}

#[derive(Debug, Default)]
pub struct SubmitOptions {
    pub draft: bool,
    pub publish: bool,
    pub no_pr: bool,
    pub no_fetch: bool,
    pub no_verify: bool,
    /// Deprecated; kept for CLI compatibility (currently a no-op).
    pub force: bool,
    pub yes: bool,
    pub no_prompt: bool,
    pub reviewers: Vec<String>,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub quiet: bool,
    pub open: bool,
    pub verbose: bool,
    pub template: Option<String>,
    pub no_template: bool,
    pub edit: bool,
    pub ai: bool,
    pub title: bool,
    pub body: bool,
    pub rerequest_review: bool,
    pub squash: bool,
    pub update_title: bool,
}

struct PrPlan {
    branch: String,
    parent: String,
    existing_pr: Option<u64>,
    existing_pr_is_draft: Option<bool>,
    /// Tip commit subject line (for auto-updating PR title)
    tip_commit_subject: Option<String>,
    /// Whether the tip commit subject differs from the existing PR title.
    needs_title_update: bool,
    existing_pr_title: Option<String>,
    // For new PRs, we'll collect these upfront
    title: Option<String>,
    body: Option<String>,
    ai_title_update: Option<String>,
    generated_body_update: Option<String>,
    is_draft: Option<bool>,
    // Track if this is a no-op (already synced)
    needs_push: bool,
    needs_pr_update: bool,
    // Empty branches get pushed but no PR created
    is_empty: bool,
}

struct ExistingPrLookup {
    branch: String,
    existing_pr: Option<PrInfoWithHead>,
    needs_full_scan_fallback: bool,
}

#[derive(Default, Clone)]
struct SubmitPhaseTimings {
    planning: Duration,
    open_pr_discovery: Duration,
    pr_create_update: Duration,
    stack_links: Duration,
}

const PR_TYPE_OPTIONS: [&str; 2] = ["Create as draft", "Publish immediately"];
const PR_TYPE_DEFAULT_INDEX: usize = 0;
const MAX_AI_DIFF_BYTES: usize = 80_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AiPrTargets {
    title: bool,
    body: bool,
    explicit_scope: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AiPrDetails {
    title: Option<String>,
    body: Option<String>,
}

#[derive(Debug, Clone)]
struct AiAgentSelection {
    agent: String,
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawAiPrDetails {
    title: Option<String>,
    body: Option<String>,
}

fn resolve_ai_targets(
    ai: bool,
    title: bool,
    body: bool,
    update_title: bool,
) -> Result<Option<AiPrTargets>> {
    if !ai {
        if title {
            anyhow::bail!("--title requires --ai");
        }
        if body {
            anyhow::bail!("--body requires --ai");
        }
        return Ok(None);
    }

    let explicit_scope = title || body;
    let targets = AiPrTargets {
        title: if explicit_scope { title } else { true },
        body: if explicit_scope { body } else { true },
        explicit_scope,
    };

    if update_title && targets.title {
        anyhow::bail!("--update-title cannot be combined with AI title generation");
    }

    Ok(Some(targets))
}

fn existing_ai_targets_for_auto_accept(targets: AiPrTargets) -> Option<AiPrTargets> {
    targets.explicit_scope.then_some(targets)
}

fn parse_ai_pr_details(raw: &str, targets: AiPrTargets) -> Result<AiPrDetails> {
    let json = extract_ai_json(raw);
    let parsed: RawAiPrDetails = serde_json::from_str(&json)
        .context("AI agent did not return JSON PR details with title/body fields")?;

    let title = parsed.title.and_then(non_empty_trimmed);
    let body = parsed.body.and_then(non_empty_trimmed);

    if targets.title && title.is_none() {
        anyhow::bail!("AI agent did not return a non-empty title");
    }
    if targets.body && body.is_none() {
        anyhow::bail!("AI agent did not return a non-empty body");
    }

    Ok(AiPrDetails {
        title: targets.title.then_some(title).flatten(),
        body: targets.body.then_some(body).flatten(),
    })
}

fn non_empty_trimmed(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
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

fn resolve_is_draft_without_prompt(
    draft_flag_set: bool,
    publish_flag_set: bool,
    draft: bool,
    no_prompt: bool,
) -> Option<bool> {
    if draft_flag_set {
        Some(draft)
    } else if publish_flag_set {
        Some(false)
    } else if no_prompt {
        Some(true)
    } else {
        None
    }
}

pub fn run(scope: SubmitScope, options: SubmitOptions) -> Result<()> {
    let SubmitOptions {
        draft,
        publish,
        no_pr,
        no_fetch,
        no_verify,
        force,
        yes,
        no_prompt,
        reviewers,
        labels,
        assignees,
        quiet,
        open,
        verbose,
        template,
        no_template,
        edit,
        ai,
        title: ai_title,
        body: body_scope,
        rerequest_review,
        squash,
        update_title,
    } = options;

    let ai_targets = resolve_ai_targets(ai, ai_title, body_scope, update_title)?;
    let auto_accept_prompts = yes || no_prompt;

    if force && !quiet {
        eprintln!(
            "  {} --force is deprecated and has no effect (see issue #222)",
            "warning:".yellow()
        );
    }
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let config = Config::load()?;
    let stack_links_mode = config.submit.stack_links;

    // Track if --draft was explicitly passed (we'll ask interactively if not)
    let draft_flag_set = draft;

    if matches!(scope, SubmitScope::Branch) && current == stack.trunk {
        anyhow::bail!(
            "Cannot submit trunk '{}' as a single branch.\n\
             Checkout a tracked branch and run `stax branch submit`, or run `stax submit` for the whole stack.",
            stack.trunk
        );
    }

    let branches_to_submit = resolve_branches_for_scope(&stack, &current, scope);
    if branches_to_submit.is_empty() {
        if !quiet {
            println!("{}", "No tracked branches to submit.".yellow());
        }
        return Ok(());
    }

    // Validation phase
    if !quiet {
        println!("{} {}...", "Submitting".bold(), scope.label().bold());
    }

    // Check for needs restack - show warning but continue (like fp)
    let needs_restack: Vec<_> = branches_to_submit
        .iter()
        .filter(|b| {
            stack
                .branches
                .get(*b)
                .map(|br| br.needs_restack)
                .unwrap_or(false)
        })
        .collect();

    if !needs_restack.is_empty() && !quiet {
        for b in &needs_restack {
            println!("  {} {} needs restack", "!".yellow(), b.cyan());
        }
    }

    // Check for branches with no changes (empty branches)
    let empty_branches: Vec<_> = branches_to_submit
        .iter()
        .filter(|b| {
            if let Some(branch_info) = stack.branches.get(*b) {
                if let Some(parent) = &branch_info.parent {
                    if let Ok(branch_commit) = repo.branch_commit(b) {
                        if let Ok(parent_commit) = repo.branch_commit(parent) {
                            return branch_commit == parent_commit;
                        }
                    }
                }
            }
            false
        })
        .collect();

    // Empty branches will be pushed but won't get PRs created
    let empty_set: HashSet<_> = empty_branches.iter().cloned().collect();

    if !empty_branches.is_empty() && !quiet {
        println!("  {} Empty branches (will push, skip PR):", "!".yellow());
        for b in &empty_branches {
            println!("    {}", b.dimmed());
        }
    }

    let remote_info = RemoteInfo::from_repo(&repo, &config)?;

    // Fetch trunk + branches being submitted + (for narrow scope) parents used in validation.
    // Run `git ls-remote --heads` in parallel for an up-to-date remote branch name set.
    let (fetch_summary, remote_branches) = if no_fetch {
        if !quiet {
            println!(
                "  {} {}",
                "Skipping fetch".yellow(),
                "(--no-fetch)".dimmed()
            );
        }
        let rb = remote::get_remote_branches(repo.workdir()?, &remote_info.name)?
            .into_iter()
            .collect::<HashSet<_>>();
        ("skipped (--no-fetch)".to_string(), rb)
    } else {
        let refs = branches_to_fetch_for_submit(&repo, &stack, scope, &branches_to_submit)?;
        let fetch_timer =
            LiveTimer::maybe_new(!quiet, &format!("Fetching from {}...", remote_info.name));
        let workdir = repo.workdir()?.to_path_buf();
        let remote_name = remote_info.name.clone();
        let refs_clone = refs.clone();

        let wd_fetch = workdir.clone();
        let rn_fetch = remote_name.clone();
        let fetch_handle = std::thread::spawn(move || {
            remote::fetch_remote_refs(&wd_fetch, &rn_fetch, &refs_clone)
        });

        let wd_ls = workdir;
        let rn_ls = remote_name.clone();
        let ls_handle = std::thread::spawn(move || remote::ls_remote_heads(&wd_ls, &rn_ls));

        let fetch_res = fetch_handle
            .join()
            .map_err(|_| anyhow::anyhow!("submit fetch thread panicked"))?;
        let ls_joined = ls_handle
            .join()
            .map_err(|_| anyhow::anyhow!("submit ls-remote thread panicked"))?;

        // We need the remote-branch set (from ls-remote) to know which of the
        // refs we asked git to fetch actually exist on the remote. New branches
        // (about to be pushed for the first time) won't, and `git fetch origin
        // trunk feat-new` legitimately fails with "couldn't find remote ref" —
        // that case is benign and must NOT bail. The dangerous case is when an
        // *existing* remote ref failed to refresh, because then any subsequent
        // `--force-with-lease` push runs against stale refs and is rejected by
        // the remote as `(stale info)`.
        let rb = match ls_joined {
            Ok(set) => set,
            Err(e) => anyhow::bail!(
                "git ls-remote --heads {} failed: {e:#}\n\n\
                 Re-run with --no-fetch to fall back to cached \
                 remote-tracking refs.",
                remote_info.name
            ),
        };

        match fetch_res {
            Ok(()) => LiveTimer::maybe_finish_ok(fetch_timer, "done"),
            Err(initial_err) => {
                // The optimistic parallel fetch may have failed simply because
                // some of the requested branches do not yet exist on the remote
                // (git fetch is all-or-nothing). Retry with only the refs that
                // ls-remote confirmed exist; if THAT still fails the failure is
                // real (auth/network/lock).
                let existing_refs: Vec<String> =
                    refs.iter().filter(|r| rb.contains(*r)).cloned().collect();

                if existing_refs.is_empty() {
                    LiveTimer::maybe_finish_ok(fetch_timer, "skipped (no existing remote refs)");
                } else if existing_refs.len() == refs.len() {
                    // Every requested ref existed on remote, yet fetch still
                    // failed. This is the dangerous case.
                    LiveTimer::maybe_finish_warn(fetch_timer, "FAILED");
                    anyhow::bail!(
                        "git fetch from '{}' failed: {initial_err:#}\n\n\
                         Refusing to continue: existing remote-tracking refs \
                         ({}) could not be refreshed, and \
                         --force-with-lease against stale refs is unsafe — the \
                         remote will reject it as `(stale info)`.\n\n\
                         Try one of:\n  \
                         - Fix the underlying fetch failure (auth, network, \
                         .git/index.lock) and retry\n  \
                         - Re-run with --no-fetch to explicitly accept cached \
                         remote-tracking refs",
                        remote_info.name,
                        existing_refs.join(", "),
                    );
                } else {
                    match remote::fetch_remote_refs(
                        repo.workdir()?,
                        &remote_info.name,
                        &existing_refs,
                    ) {
                        Ok(()) => LiveTimer::maybe_finish_ok(
                            fetch_timer,
                            "done (skipped non-existent remote refs)",
                        ),
                        Err(retry_err) => {
                            LiveTimer::maybe_finish_warn(fetch_timer, "FAILED");
                            anyhow::bail!(
                                "git fetch from '{}' failed: {retry_err:#}\n\n\
                                 Refusing to continue: existing remote-tracking refs \
                                 ({}) could not be refreshed, and \
                                 --force-with-lease against stale refs is unsafe — the \
                                 remote will reject it as `(stale info)`.\n\n\
                                 Try one of:\n  \
                                 - Fix the underlying fetch failure (auth, network, \
                                 .git/index.lock) and retry\n  \
                                 - Re-run with --no-fetch to explicitly accept cached \
                                 remote-tracking refs",
                                remote_info.name,
                                existing_refs.join(", "),
                            );
                        }
                    }
                }
            }
        }

        ("ok".to_string(), rb)
    };

    // Verify trunk exists on remote
    if !remote_branches.contains(&stack.trunk) {
        if no_fetch {
            anyhow::bail!(
                "Base branch '{}' was not found in cached ref '{}/{}'.\n\
                 You used --no-fetch, so stax did not refresh remote refs.\n\n\
                 Try one of:\n  \
                 - Run without --no-fetch\n  \
                 - git fetch {} {}\n",
                stack.trunk,
                remote_info.name,
                stack.trunk,
                remote_info.name,
                stack.trunk
            );
        } else {
            anyhow::bail!(
                "Base branch '{}' does not exist on the remote.\n\n\
                 This can happen if:\n  \
                 - This is a new repository that hasn't been pushed yet\n  \
                 - The default branch has a different name on the remote\n\n\
                 To fix this, push your base branch first:\n  \
                 git push -u {} {}",
                stack.trunk,
                remote_info.name,
                stack.trunk
            );
        }
    }

    if matches!(scope, SubmitScope::Branch | SubmitScope::Upstack) {
        validate_narrow_scope_submit(
            scope,
            &repo,
            &stack,
            &current,
            &remote_info.name,
            &branches_to_submit,
            no_fetch,
        )?;
    }

    // Build plan - determine which PRs need create vs update
    let planning_timer = LiveTimer::maybe_new(!quiet, "Planning PR operations...");
    let planning_started_at = Instant::now();
    let mut timings = SubmitPhaseTimings::default();
    let mut full_scan_fallbacks = 0usize;

    let mut plans: Vec<PrPlan> = Vec::new();
    let mut rt: Option<tokio::runtime::Runtime> = None;
    let client: Option<ForgeClient>;

    if no_pr {
        let runtime = tokio::runtime::Runtime::new().ok();
        let _enter = runtime.as_ref().map(|rt| rt.enter());
        let forge_client = ForgeClient::new(&remote_info).ok();
        client = forge_client.clone();
        let mut open_prs_by_head: Option<HashMap<String, PrInfoWithHead>> = None;

        for branch in &branches_to_submit {
            let mut meta = BranchMetadata::read(repo.inner(), branch)?
                .context(format!("No metadata for branch {}", branch))?;
            let is_empty = empty_set.contains(branch);
            let needs_push = branch_needs_push(repo.workdir()?, &remote_info.name, branch);
            let mut existing_pr = None;
            let had_metadata_pr = meta.pr_info.as_ref().filter(|p| p.number > 0).is_some();

            // Best-effort metadata refresh when no-pr is used.
            if !is_empty {
                if let (Some(runtime), Some(forge_client)) =
                    (runtime.as_ref(), forge_client.as_ref())
                {
                    let mut found_pr: Option<PrInfoWithHead> = None;

                    if let Some(pr_info) = meta.pr_info.as_ref().filter(|p| p.number > 0) {
                        let lookup_started_at = Instant::now();
                        found_pr = runtime
                            .block_on(async { forge_client.get_pr_with_head(pr_info.number).await })
                            .ok();
                        timings.open_pr_discovery += lookup_started_at.elapsed();
                    }

                    if found_pr.is_none() {
                        let lookup_started_at = Instant::now();
                        found_pr = runtime
                            .block_on(async { forge_client.find_open_pr_by_head(branch).await })
                            .ok()
                            .flatten();
                        timings.open_pr_discovery += lookup_started_at.elapsed();
                    }

                    if found_pr.is_none() && (had_metadata_pr || remote_branches.contains(branch)) {
                        full_scan_fallbacks += 1;
                        if verbose && !quiet {
                            println!(
                                "    Falling back to full open PR scan for {} (metadata mismatch)",
                                branch.cyan()
                            );
                        }
                        if open_prs_by_head.is_none() {
                            let lookup_started_at = Instant::now();
                            open_prs_by_head = runtime
                                .block_on(async { forge_client.list_open_prs_by_head().await })
                                .ok();
                            timings.open_pr_discovery += lookup_started_at.elapsed();
                            if verbose && !quiet {
                                if let Some(map) = &open_prs_by_head {
                                    println!("      Cached {} open PRs", map.len());
                                }
                            }
                        }
                        if let Some(map) = &open_prs_by_head {
                            found_pr = map.get(branch).cloned();
                        }
                    }

                    if let Some(pr) = found_pr {
                        existing_pr = Some(pr.info.number);
                        let owner_matches = pr
                            .head_label
                            .as_ref()
                            .and_then(|label| label.split_once(':').map(|(owner, _)| owner))
                            .map(|owner| owner == remote_info.owner())
                            .unwrap_or(false);

                        let needs_meta_update = meta
                            .pr_info
                            .as_ref()
                            .map(|info| {
                                info.number != pr.info.number
                                    || info.state != pr.info.state
                                    || info.is_draft.unwrap_or(false) != pr.info.is_draft
                            })
                            .unwrap_or(true);

                        if needs_meta_update && owner_matches {
                            meta = BranchMetadata {
                                pr_info: Some(crate::engine::metadata::PrInfo {
                                    number: pr.info.number,
                                    state: pr.info.state.clone(),
                                    is_draft: Some(pr.info.is_draft),
                                }),
                                ..meta
                            };
                            meta.write(repo.inner(), branch)?;
                        }
                    }
                }
            }

            plans.push(PrPlan {
                branch: branch.clone(),
                parent: meta.parent_branch_name,
                existing_pr,
                existing_pr_is_draft: None,
                tip_commit_subject: None,
                needs_title_update: false,
                existing_pr_title: None,
                title: None,
                body: None,
                ai_title_update: None,
                generated_body_update: None,
                is_draft: None,
                needs_push,
                needs_pr_update: false,
                is_empty,
            });
        }
    } else {
        let runtime = tokio::runtime::Runtime::new()?;
        let _enter = runtime.enter();
        let forge_client = ForgeClient::new(&remote_info)?;
        let mut lookup_inputs = Vec::new();
        for branch in &branches_to_submit {
            if empty_set.contains(branch) {
                continue;
            }

            let meta = BranchMetadata::read(repo.inner(), branch)?
                .context(format!("No metadata for branch {}", branch))?;
            let metadata_pr_number = meta
                .pr_info
                .as_ref()
                .filter(|p| p.number > 0)
                .map(|p| p.number);
            lookup_inputs.push((
                branch.clone(),
                metadata_pr_number,
                remote_branches.contains(branch),
            ));
        }

        let lookup_started_at = Instant::now();
        let lookup_results = runtime.block_on(async {
            join_all(lookup_inputs.into_iter().map(
                |(branch, metadata_pr_number, has_remote_branch)| {
                    discover_existing_pr(
                        forge_client.clone(),
                        branch,
                        metadata_pr_number,
                        has_remote_branch,
                    )
                },
            ))
            .await
        });
        timings.open_pr_discovery += lookup_started_at.elapsed();

        let mut lookups_by_branch = HashMap::new();
        for lookup in lookup_results {
            let lookup = lookup?;
            lookups_by_branch.insert(lookup.branch.clone(), lookup);
        }

        if lookups_by_branch
            .values()
            .any(|lookup| lookup.needs_full_scan_fallback)
        {
            full_scan_fallbacks += lookups_by_branch
                .values()
                .filter(|lookup| lookup.needs_full_scan_fallback)
                .count();
            if verbose && !quiet {
                println!("    Falling back to full open PR scan for metadata mismatches");
            }

            let lookup_started_at = Instant::now();
            let open_prs_by_head =
                runtime.block_on(async { forge_client.list_open_prs_by_head().await })?;
            timings.open_pr_discovery += lookup_started_at.elapsed();
            if verbose && !quiet {
                println!("      Cached {} open PRs", open_prs_by_head.len());
            }

            for lookup in lookups_by_branch.values_mut() {
                if lookup.needs_full_scan_fallback {
                    lookup.existing_pr = open_prs_by_head.get(&lookup.branch).cloned();
                }
            }
        }

        for branch in &branches_to_submit {
            let meta = BranchMetadata::read(repo.inner(), branch)?
                .context(format!("No metadata for branch {}", branch))?;

            let is_empty = empty_set.contains(branch);

            // Check if PR exists (skip for empty branches)
            let existing_pr = lookups_by_branch
                .get(branch)
                .and_then(|lookup| lookup.existing_pr.clone());
            if !is_empty {
                if verbose && !quiet {
                    println!("    Checking PR for {}", branch.cyan());
                    if let Some(found) = &existing_pr {
                        println!("      Found open PR #{}", found.info.number);
                    } else {
                        println!("      No open PR found");
                    }
                }
            } else if verbose && !quiet {
                println!("    Empty branch {}, skipping PR lookup", branch.cyan());
            }
            let pr_number = existing_pr.as_ref().map(|p| p.info.number);

            if let Some(pr) = &existing_pr {
                let owner_matches = pr
                    .head_label
                    .as_ref()
                    .and_then(|label| label.split_once(':').map(|(owner, _)| owner))
                    .map(|owner| owner == remote_info.owner())
                    .unwrap_or(false);

                let needs_meta_update = meta
                    .pr_info
                    .as_ref()
                    .map(|info| {
                        info.number != pr.info.number
                            || info.state != pr.info.state
                            || info.is_draft.unwrap_or(false) != pr.info.is_draft
                    })
                    .unwrap_or(true);

                if needs_meta_update && owner_matches {
                    let updated_meta = BranchMetadata {
                        pr_info: Some(crate::engine::metadata::PrInfo {
                            number: pr.info.number,
                            state: pr.info.state.clone(),
                            is_draft: Some(pr.info.is_draft),
                        }),
                        ..meta.clone()
                    };
                    updated_meta.write(repo.inner(), branch)?;
                    if verbose && !quiet {
                        println!("      Cached PR #{} in metadata", pr.info.number);
                    }
                } else if needs_meta_update && verbose && !quiet {
                    println!(
                        "      Skipped caching PR #{} (fork or unknown owner)",
                        pr.info.number
                    );
                }
            }

            // Determine the base branch for PR
            let base = meta.parent_branch_name.clone();

            // Check if we actually need to push
            let needs_push = branch_needs_push(repo.workdir()?, &remote_info.name, branch);

            // Check if PR base needs updating (not for empty branches)
            let needs_pr_update = if is_empty {
                false
            } else if let Some(pr) = &existing_pr {
                pr.info.base != base || needs_push
            } else {
                true // New PR always needs creation
            };

            // Capture tip commit subject for auto-updating PR title on existing PRs.
            // Only computed when the user opts in via `--update-title` so default submits
            // do not silently rewrite PR titles from local commit messages.
            let tip_commit_subject = if update_title && pr_number.is_some() && !is_empty {
                tip_commit_subject(repo.workdir()?, branch)
            } else {
                None
            };
            let needs_title_update = update_title
                && existing_pr
                    .as_ref()
                    .zip(tip_commit_subject.as_ref())
                    .map(|(pr, commit_subject)| pr.title != *commit_subject)
                    .unwrap_or(false);

            plans.push(PrPlan {
                branch: branch.clone(),
                parent: base,
                existing_pr: pr_number,
                existing_pr_is_draft: existing_pr.as_ref().map(|pr| pr.info.is_draft),
                tip_commit_subject,
                needs_title_update,
                existing_pr_title: existing_pr.as_ref().map(|pr| pr.title.clone()),
                title: None,
                body: None,
                ai_title_update: None,
                generated_body_update: None,
                is_draft: None,
                needs_push,
                needs_pr_update,
                is_empty,
            });
        }

        rt = Some(runtime);
        client = Some(forge_client);
    }
    timings.planning = planning_started_at.elapsed();
    LiveTimer::maybe_finish_ok(planning_timer, "done");

    // Show plan summary (exclude empty branches from PR counts)
    let creates: Vec<_> = plans
        .iter()
        .filter(|p| p.existing_pr.is_none() && !p.is_empty)
        .collect();
    let updates: Vec<_> = plans
        .iter()
        .filter(|p| p.existing_pr.is_some() && p.needs_pr_update && !p.is_empty)
        .collect();
    let noops: Vec<_> = plans
        .iter()
        .filter(|p| p.existing_pr.is_some() && !p.needs_pr_update && !p.needs_push && !p.is_empty)
        .collect();

    if !quiet {
        if !creates.is_empty() {
            println!(
                "  {} {} {} to create",
                creates.len().to_string().cyan(),
                "▸".dimmed(),
                if creates.len() == 1 { "PR" } else { "PRs" }
            );
        }
        if !updates.is_empty() {
            println!(
                "  {} {} {} to update",
                updates.len().to_string().cyan(),
                "▸".dimmed(),
                if updates.len() == 1 { "PR" } else { "PRs" }
            );
        }
        if !noops.is_empty() {
            println!(
                "  {} {} {} already up to date",
                noops.len().to_string().dimmed(),
                "▸".dimmed(),
                if noops.len() == 1 { "PR" } else { "PRs" }
            );
        }
    }

    // Collect PR details and AI update choices BEFORE pushing (skip empty branches).
    if !no_pr {
        // Discover all available PR templates
        let discovered_templates = if no_template {
            Vec::new()
        } else {
            discover_pr_templates(repo.workdir()?).unwrap_or_default()
        };
        let mut ai_agent_selection: Option<AiAgentSelection> = None;
        let new_prs: Vec<_> = plans
            .iter()
            .filter(|p| p.existing_pr.is_none() && !p.is_empty)
            .collect();
        if !new_prs.is_empty() && !quiet {
            println!();
            println!("{}", "New PR details:".bold());
        }

        for plan in &mut plans {
            if plan.existing_pr.is_some() || plan.is_empty {
                continue;
            }

            // Template selection per branch
            let selected_template = if no_template {
                None
            } else if let Some(ref template_name) = template {
                // --template flag: find by name
                let found = discovered_templates
                    .iter()
                    .find(|t| t.name == *template_name)
                    .cloned();

                if found.is_none() && !quiet {
                    eprintln!(
                        "  {} Template '{}' not found, using no template",
                        "!".yellow(),
                        template_name
                    );
                }
                found
            } else if auto_accept_prompts {
                // Non-interactive/auto-accept: use first template if exactly one exists
                if discovered_templates.len() == 1 {
                    Some(discovered_templates[0].clone())
                } else {
                    None
                }
            } else {
                // Interactive selection (handles empty list, single template, and multiple)
                select_template_interactive(&discovered_templates)?
            };

            let commit_messages =
                collect_commit_messages(repo.workdir()?, &plan.parent, &plan.branch);
            let default_title = default_pr_title(&commit_messages, &plan.branch);

            // Use selected template content if available
            let template_content = selected_template.as_ref().map(|t| t.content.as_str());
            let default_body =
                build_default_pr_body(template_content, &plan.branch, &commit_messages);

            if !quiet {
                println!("  {}", plan.branch.cyan());
            }

            let ai_details = if let Some(targets) = ai_targets {
                match generate_ai_pr_details(
                    repo.workdir()?,
                    &plan.parent,
                    &plan.branch,
                    template_content,
                    targets,
                    &mut ai_agent_selection,
                    auto_accept_prompts,
                    quiet,
                ) {
                    Ok(details) => Some(details),
                    Err(e) => {
                        if !quiet {
                            eprintln!(
                                "    {} AI generation failed: {}. Falling back to defaults.",
                                "⚠".yellow(),
                                e
                            );
                        }
                        finish_default_pr_detail_progress(
                            quiet,
                            &plan.branch,
                            "PR details",
                            "fallback",
                        );
                        None
                    }
                }
            } else {
                None
            };

            if let Some(targets) = ai_targets {
                if !targets.title {
                    finish_default_pr_detail_progress(quiet, &plan.branch, "PR title", "done");
                }
                if !targets.body {
                    finish_default_pr_detail_progress(quiet, &plan.branch, "PR body", "done");
                }
            }

            let suggested_title = ai_details
                .as_ref()
                .and_then(|details| details.title.clone())
                .unwrap_or(default_title);
            let suggested_body = ai_details
                .as_ref()
                .and_then(|details| details.body.clone())
                .unwrap_or(default_body);

            let title = if auto_accept_prompts {
                suggested_title
            } else {
                Input::with_theme(&ColorfulTheme::default())
                    .with_prompt("  Title")
                    .default(suggested_title)
                    .interact_text()?
            };

            let body = if auto_accept_prompts {
                suggested_body
            } else if edit {
                // --edit flag: always open editor
                Editor::new()
                    .edit(&suggested_body)?
                    .unwrap_or(suggested_body)
            } else {
                // Interactive prompt
                let options = if suggested_body.trim().is_empty() {
                    vec!["Edit", "Skip (leave empty)"]
                } else {
                    vec!["Use default", "Edit", "Skip (leave empty)"]
                };

                let choice = Select::with_theme(&ColorfulTheme::default())
                    .with_prompt("  Body")
                    .items(&options)
                    .default(0)
                    .interact()?;

                match options[choice] {
                    "Use default" => suggested_body,
                    "Edit" => Editor::new()
                        .edit(&suggested_body)?
                        .unwrap_or(suggested_body),
                    _ => String::new(),
                }
            };

            // Ask about draft vs publish (only if --draft/--publish wasn't explicitly set)
            let is_draft = if let Some(is_draft) =
                resolve_is_draft_without_prompt(draft_flag_set, publish, draft, auto_accept_prompts)
            {
                is_draft
            } else {
                let choice = Select::with_theme(&ColorfulTheme::default())
                    .with_prompt("  PR type")
                    .items(PR_TYPE_OPTIONS)
                    .default(PR_TYPE_DEFAULT_INDEX)
                    .interact()?;
                choice == PR_TYPE_DEFAULT_INDEX
            };

            plan.title = Some(title);
            plan.body = Some(body);
            plan.is_draft = Some(is_draft);
        }

        if let Some(targets) = ai_targets {
            let should_consider_existing_ai =
                !auto_accept_prompts || existing_ai_targets_for_auto_accept(targets).is_some();
            if should_consider_existing_ai {
                let existing_prs: Vec<_> = plans
                    .iter()
                    .filter(|p| p.existing_pr.is_some() && !p.is_empty)
                    .collect();
                if !existing_prs.is_empty() && !quiet {
                    println!();
                    println!("{}", "Existing PR AI updates:".bold());
                }

                for plan in &mut plans {
                    let Some(pr_number) = plan.existing_pr else {
                        continue;
                    };
                    if plan.is_empty {
                        continue;
                    }

                    let selected_targets = if auto_accept_prompts {
                        existing_ai_targets_for_auto_accept(targets)
                    } else {
                        if !quiet {
                            println!("  {} #{}", plan.branch.cyan(), pr_number);
                        }
                        prompt_existing_ai_targets(targets, &plan.branch, pr_number)?
                    };

                    let Some(selected_targets) = selected_targets else {
                        if !quiet && !auto_accept_prompts {
                            println!("    {}", "Skipping AI content update".dimmed());
                        }
                        continue;
                    };

                    let selected_template = if !selected_targets.body || no_template {
                        None
                    } else if let Some(ref template_name) = template {
                        let found = discovered_templates
                            .iter()
                            .find(|t| t.name == *template_name)
                            .cloned();

                        if found.is_none() && !quiet {
                            eprintln!(
                                "  {} Template '{}' not found, using no template",
                                "!".yellow(),
                                template_name
                            );
                        }
                        found
                    } else if auto_accept_prompts {
                        if discovered_templates.len() == 1 {
                            Some(discovered_templates[0].clone())
                        } else {
                            None
                        }
                    } else {
                        select_template_interactive(&discovered_templates)?
                    };
                    let template_content = selected_template.as_ref().map(|t| t.content.as_str());

                    match generate_ai_pr_details(
                        repo.workdir()?,
                        &plan.parent,
                        &plan.branch,
                        template_content,
                        selected_targets,
                        &mut ai_agent_selection,
                        auto_accept_prompts,
                        quiet,
                    ) {
                        Ok(details) => {
                            if let Some(title) = details.title {
                                if plan.existing_pr_title.as_deref() == Some(title.as_str()) {
                                    if !quiet {
                                        println!(
                                            "    {}",
                                            "AI title matches existing title".dimmed()
                                        );
                                    }
                                } else {
                                    plan.ai_title_update = Some(title);
                                }
                            }
                            if let Some(body) = details.body {
                                plan.generated_body_update = Some(body);
                            }
                        }
                        Err(e) => {
                            if !quiet {
                                eprintln!(
                                    "    {} AI generation failed for existing PR #{}: {}",
                                    "⚠".yellow(),
                                    pr_number,
                                    e
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // Now push branches that need it
    let branches_needing_push: Vec<_> = plans.iter().filter(|p| p.needs_push).collect();

    // Create transaction if we have branches to push
    let mut tx = if !branches_needing_push.is_empty() {
        let mut tx = Transaction::begin(OpKind::Submit, &repo, quiet)?;

        // Plan local branches (for backup)
        let branch_names: Vec<String> = branches_needing_push
            .iter()
            .map(|p| p.branch.clone())
            .collect();
        tx.plan_branches(&repo, &branch_names)?;

        // Plan remote refs (record current remote state before pushing)
        for plan in &branches_needing_push {
            tx.plan_remote_branch(&repo, &remote_info.name, &plan.branch)?;
        }

        let summary = PlanSummary {
            branches_to_rebase: 0,
            branches_to_push: branches_needing_push.len(),
            description: vec![format!(
                "Submit {} {}",
                branches_needing_push.len(),
                if branches_needing_push.len() == 1 {
                    "branch"
                } else {
                    "branches"
                }
            )],
        };
        tx::print_plan(tx.kind(), &summary, quiet);
        tx.set_plan_summary(summary);
        tx.snapshot()?;

        Some(tx)
    } else {
        None
    };

    if !branches_needing_push.is_empty() {
        if !quiet {
            println!();
            println!("{}", "Pushing branches...".bold());
        }

        for plan in &branches_needing_push {
            // Squash all commits on the branch down to one before pushing
            if squash {
                if let Err(e) = squash_branch_commits(repo.workdir()?, &plan.branch, &plan.parent) {
                    if !quiet {
                        println!("  {} squash {}: {}", "⚠".yellow(), plan.branch, e);
                    }
                }
            }

            let push_timer = LiveTimer::maybe_new(!quiet, &format!("Pushing {}...", plan.branch));

            // Get local OID before push (this is what we're pushing)
            let local_oid = repo.branch_commit(&plan.branch).ok();

            match push_branch(repo.workdir()?, &remote_info.name, &plan.branch, no_verify) {
                Ok(()) => {
                    // Record after-OIDs
                    if let Some(ref mut tx) = tx {
                        let _ = tx.record_after(&repo, &plan.branch);
                        if let Some(oid) = &local_oid {
                            tx.record_remote_after(&remote_info.name, &plan.branch, oid);
                        }
                    }
                    LiveTimer::maybe_finish_ok(push_timer, "done");
                }
                Err(e) => {
                    LiveTimer::maybe_finish_err(push_timer, "failed");
                    if let Some(tx) = tx {
                        tx.finish_err(
                            &format!("Push failed: {}", e),
                            Some("push"),
                            Some(&plan.branch),
                        )?;
                    }
                    return Err(e);
                }
            }
        }
    }

    if no_pr {
        // Finish transaction successfully
        if let Some(tx) = tx {
            tx.finish_ok()?;
        }
        if !quiet {
            println!();
            println!("{}", "✓ Branches pushed successfully!".green().bold());
            if verbose {
                print_verbose_network_summary(
                    client.as_ref(),
                    &remote_info.name,
                    &fetch_summary,
                    &timings,
                    full_scan_fallbacks,
                );
            }
        }
        return Ok(());
    }

    // Check if anything needs to be done (exclude empty branches)
    let any_pr_work = plans.iter().any(|p| {
        !p.is_empty
            && (p.existing_pr.is_none()
                || p.needs_pr_update
                || p.ai_title_update.is_some()
                || p.generated_body_update.is_some())
    });

    let any_existing_prs = plans.iter().any(|p| !p.is_empty && p.existing_pr.is_some());

    if !any_pr_work && branches_needing_push.is_empty() && !any_existing_prs {
        if !quiet {
            println!();
            println!("{}", "✓ Stack already up to date!".green().bold());
            if verbose {
                print_verbose_network_summary(
                    client.as_ref(),
                    &remote_info.name,
                    &fetch_summary,
                    &timings,
                    full_scan_fallbacks,
                );
            }
        }
        return Ok(());
    }

    // Create/update PRs
    if any_pr_work && !quiet {
        println!();
        println!("{}", "Processing PRs...".bold());
    }

    let rt = rt.context("Internal error: missing runtime for PR submission")?;
    let client = client.context("Internal error: missing forge client for PR submission")?;

    let (open_pr_url, async_timings, async_full_scan_fallbacks) = rt.block_on(async {
        let mut pr_infos: Vec<StackPrInfo> = Vec::new();
        let mut created_pr_numbers: HashSet<u64> = HashSet::new();
        let mut async_timings = SubmitPhaseTimings::default();
        let async_full_scan_fallbacks = 0usize;

        let create_update_started_at = Instant::now();
        for plan in &plans {
            // Skip empty branches for PR operations
            if plan.is_empty {
                continue;
            }

            let meta = BranchMetadata::read(repo.inner(), &plan.branch)?
                .context(format!("No metadata for branch {}", plan.branch))?;
            let desired_draft_state = if draft {
                Some(true)
            } else if publish {
                Some(false)
            } else {
                None
            };

            if let Some(existing_pr_number) = plan.existing_pr {
                if plan.needs_pr_update {
                    // Update existing PR (only if needed)
                    let update_timer = LiveTimer::maybe_new(
                        !quiet,
                        &format!("Updating {} #{}...", plan.branch, existing_pr_number),
                    );

                    // Update base if needed
                    client
                        .update_pr_base(existing_pr_number, &plan.parent)
                        .await?;

                    // Auto-update PR title from tip commit subject when it has changed
                    if plan.needs_title_update {
                        if let Some(ref commit_subject) = plan.tip_commit_subject {
                            client
                                .update_pr_title(existing_pr_number, commit_subject)
                                .await?;
                        }
                    }

                    apply_ai_pr_content_updates(
                        &client,
                        existing_pr_number,
                        &plan.branch,
                        plan.ai_title_update.as_deref(),
                        plan.generated_body_update.as_deref(),
                        quiet,
                    )
                    .await?;

                    apply_pr_metadata(&client, existing_pr_number, &reviewers, &labels, &assignees)
                        .await?;

                    // Toggle draft status if --draft or --publish was passed.
                    if let Some(is_draft) = desired_draft_state {
                        if plan.existing_pr_is_draft == Some(is_draft) {
                            let reason = if is_draft {
                                "already draft"
                            } else {
                                "already published"
                            };
                            if verbose && !quiet {
                                println!(
                                    "      Skipping draft toggle for #{} ({})",
                                    existing_pr_number, reason
                                );
                            }
                        } else {
                            client.set_pr_draft(existing_pr_number, is_draft).await?;
                        }
                    }

                    // Re-request review from existing reviewers if flag is set
                    if rerequest_review {
                        let existing_reviewers = client
                            .get_requested_reviewers(existing_pr_number)
                            .await
                            .unwrap_or_default();
                        if !existing_reviewers.is_empty() {
                            client
                                .request_reviewers(existing_pr_number, &existing_reviewers)
                                .await?;
                        }
                    }

                    LiveTimer::maybe_finish_ok(update_timer, "done");

                    // Get current PR state
                    let pr = client.get_pr(existing_pr_number).await?;

                    let updated_meta = BranchMetadata {
                        pr_info: Some(crate::engine::metadata::PrInfo {
                            number: pr.number,
                            state: pr.state.clone(),
                            is_draft: Some(pr.is_draft),
                        }),
                        ..meta
                    };
                    updated_meta.write(repo.inner(), &plan.branch)?;

                    pr_infos.push(StackPrInfo {
                        branch: plan.branch.clone(),
                        pr_number: Some(pr.number),
                    });
                } else {
                    // Toggle draft status even when no other update is needed
                    if let Some(is_draft) = desired_draft_state {
                        let draft_timer = LiveTimer::maybe_new(
                            !quiet,
                            &format!(
                                "{} {} #{}...",
                                if is_draft {
                                    "Converting to draft"
                                } else {
                                    "Publishing"
                                },
                                plan.branch,
                                existing_pr_number,
                            ),
                        );
                        if plan.existing_pr_is_draft == Some(is_draft) {
                            LiveTimer::maybe_finish_skipped(
                                draft_timer,
                                if is_draft {
                                    "already draft"
                                } else {
                                    "already published"
                                },
                            );
                        } else {
                            client.set_pr_draft(existing_pr_number, is_draft).await?;
                            LiveTimer::maybe_finish_ok(draft_timer, "done");

                            // Refresh metadata after draft status change
                            let pr = client.get_pr(existing_pr_number).await?;
                            let updated_meta = BranchMetadata {
                                pr_info: Some(crate::engine::metadata::PrInfo {
                                    number: pr.number,
                                    state: pr.state.clone(),
                                    is_draft: Some(pr.is_draft),
                                }),
                                ..meta
                            };
                            updated_meta.write(repo.inner(), &plan.branch)?;
                        }
                    }

                    // Update PR title if opt-in and the tip commit subject drifted
                    if plan.needs_title_update {
                        let title_timer = LiveTimer::maybe_new(
                            !quiet,
                            &format!(
                                "Updating title for {} #{}...",
                                plan.branch, existing_pr_number
                            ),
                        );
                        if let Some(ref commit_subject) = plan.tip_commit_subject {
                            client
                                .update_pr_title(existing_pr_number, commit_subject)
                                .await?;
                        }
                        LiveTimer::maybe_finish_ok(title_timer, "done");
                    }

                    apply_ai_pr_content_updates(
                        &client,
                        existing_pr_number,
                        &plan.branch,
                        plan.ai_title_update.as_deref(),
                        plan.generated_body_update.as_deref(),
                        quiet,
                    )
                    .await?;

                    // No-op - just add to pr_infos for summary
                    pr_infos.push(StackPrInfo {
                        branch: plan.branch.clone(),
                        pr_number: Some(existing_pr_number),
                    });
                }
            } else {
                // Create new PR
                let title = plan.title.as_ref().unwrap();
                let body = plan.body.as_ref().unwrap();
                let is_draft = plan.is_draft.unwrap_or(draft);

                let create_timer =
                    LiveTimer::maybe_new(!quiet, &format!("Creating {}...", plan.branch));

                let pr = client
                    .create_pr(&plan.branch, &plan.parent, title, body, is_draft)
                    .await
                    .context(format!(
                        "Failed to create PR for '{}' with base '{}'\n\
                         This may happen if:\n  \
                         - The base branch '{}' doesn't exist on the remote\n  \
                         - The branch has no commits different from base\n  \
                         - API request timed out (check network/VPN and retry)\n  \
                         Try: git log {}..{} to see the commits",
                        plan.branch, plan.parent, plan.parent, plan.parent, plan.branch
                    ))?;
                created_pr_numbers.insert(pr.number);

                LiveTimer::maybe_finish_ok(
                    create_timer,
                    &format!("created {}", format!("#{}", pr.number).dimmed()),
                );

                // Update metadata with PR info
                let updated_meta = BranchMetadata {
                    pr_info: Some(crate::engine::metadata::PrInfo {
                        number: pr.number,
                        state: pr.state.clone(),
                        is_draft: Some(pr.is_draft),
                    }),
                    ..meta
                };
                updated_meta.write(repo.inner(), &plan.branch)?;

                apply_pr_metadata(&client, pr.number, &reviewers, &labels, &assignees).await?;

                pr_infos.push(StackPrInfo {
                    branch: plan.branch.clone(),
                    pr_number: Some(pr.number),
                });
            }
        }
        async_timings.pr_create_update = create_update_started_at.elapsed();

        // Sync stack links on ALL PRs in the stack, even on otherwise no-op submit runs.
        let prs_with_numbers: Vec<_> = pr_infos
            .iter()
            .filter_map(|p| p.pr_number.map(|num| (num, p.branch.clone())))
            .collect();

        let stack_links_started_at = Instant::now();
        for (pr_number, _branch) in &prs_with_numbers {
            let sync_timer =
                LiveTimer::maybe_new(!quiet, &format!("Syncing stack links on #{}...", pr_number));
            let stack_links =
                generate_stack_links_markdown(&pr_infos, *pr_number, &remote_info, &stack.trunk);

            match stack_links_mode {
                StackLinksMode::Comment | StackLinksMode::Both => {
                    if created_pr_numbers.contains(pr_number) {
                        client
                            .create_stack_comment(*pr_number, &stack_links)
                            .await?;
                    } else {
                        client
                            .update_stack_comment(*pr_number, &stack_links)
                            .await?;
                    }
                }
                StackLinksMode::Body | StackLinksMode::Off => {
                    client.delete_stack_comment(*pr_number).await?;
                }
            }

            let current_body = client.get_pr_body(*pr_number).await?;
            let desired_body = match stack_links_mode {
                StackLinksMode::Body | StackLinksMode::Both => {
                    upsert_stack_links_in_body(&current_body, &stack_links)
                }
                StackLinksMode::Comment | StackLinksMode::Off => {
                    remove_stack_links_from_body(&current_body)
                }
            };

            if desired_body != current_body {
                client.update_pr_body(*pr_number, &desired_body).await?;
            }

            LiveTimer::maybe_finish_ok(sync_timer, "done");
        }
        async_timings.stack_links = stack_links_started_at.elapsed();

        if !quiet {
            println!();
            println!("{}", "✓ Stack submitted!".green().bold());

            // Print PR URLs
            if !pr_infos.is_empty() {
                for pr_info in &pr_infos {
                    if let Some(num) = pr_info.pr_number {
                        println!("  {} {}", "✓".green(), remote_info.pr_url(num));
                    }
                }
            }
        }

        let open_pr_url = if open {
            pr_infos
                .iter()
                .find(|pr_info| pr_info.branch == current)
                .and_then(|pr_info| pr_info.pr_number)
                .map(|num| remote_info.pr_url(num))
        } else {
            None
        };

        Ok::<(Option<String>, SubmitPhaseTimings, usize), anyhow::Error>((
            open_pr_url,
            async_timings,
            async_full_scan_fallbacks,
        ))
    })?;
    timings.open_pr_discovery += async_timings.open_pr_discovery;
    timings.pr_create_update += async_timings.pr_create_update;
    timings.stack_links += async_timings.stack_links;
    full_scan_fallbacks += async_full_scan_fallbacks;

    if let Some(pr_url) = open_pr_url {
        if !quiet {
            println!("Opening {} in browser...", pr_url.cyan());
        }
        open_url_in_browser(&pr_url);
    } else if open && !quiet {
        eprintln!(
            "  {} No PR found for current branch {}; nothing to open.",
            "!".yellow(),
            current.cyan()
        );
    }

    // Finish transaction successfully
    if let Some(tx) = tx {
        tx.finish_ok()?;
    }

    if verbose && !quiet {
        print_verbose_network_summary(
            Some(&client),
            &remote_info.name,
            &fetch_summary,
            &timings,
            full_scan_fallbacks,
        );
    }

    Ok(())
}

/// Squash all commits on a branch down to one commit above the base.
/// Uses `git reset --soft` + `git commit` to preserve the tree while collapsing history.
fn squash_branch_commits(workdir: &Path, branch: &str, base: &str) -> Result<()> {
    // Check how many commits ahead of base
    let output = Command::new("git")
        .args(["rev-list", "--count", &format!("{}..{}", base, branch)])
        .current_dir(workdir)
        .output()
        .context("Failed to count commits")?;

    let count: usize = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(0);

    if count <= 1 {
        return Ok(()); // Already single commit or empty, nothing to squash
    }

    // Get the current branch so we can restore it
    let current_output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(workdir)
        .output()?;
    let current = String::from_utf8_lossy(&current_output.stdout)
        .trim()
        .to_string();

    // Get the first commit message on the branch (for the squashed commit)
    let msg_output = Command::new("git")
        .args([
            "log",
            "--format=%s",
            "--reverse",
            &format!("{}..{}", base, branch),
        ])
        .current_dir(workdir)
        .output()?;
    let first_msg = String::from_utf8_lossy(&msg_output.stdout)
        .lines()
        .next()
        .unwrap_or(branch)
        .to_string();

    // Checkout the branch, soft-reset to base, recommit
    let _ = Command::new("git")
        .args(["checkout", branch])
        .current_dir(workdir)
        .output();

    let reset = Command::new("git")
        .args(["reset", "--soft", base])
        .current_dir(workdir)
        .status()?;

    if !reset.success() {
        // Restore branch state
        let _ = Command::new("git")
            .args(["checkout", &current])
            .current_dir(workdir)
            .output();
        anyhow::bail!("Failed to soft-reset {} to {}", branch, base);
    }

    let commit = Command::new("git")
        .args(["commit", "-m", &first_msg])
        .current_dir(workdir)
        .status()?;

    if !commit.success() {
        let _ = Command::new("git")
            .args(["checkout", &current])
            .current_dir(workdir)
            .output();
        anyhow::bail!("Failed to commit squashed changes on {}", branch);
    }

    // Return to original branch
    if current != branch {
        let _ = Command::new("git")
            .args(["checkout", &current])
            .current_dir(workdir)
            .output();
    }

    Ok(())
}

fn push_branch(
    workdir: &std::path::Path,
    remote: &str,
    branch: &str,
    no_verify: bool,
) -> Result<()> {
    let mut args = vec!["push", "--force-with-lease"];
    if no_verify {
        args.push("--no-verify");
    }
    args.extend(["-u", remote, branch]);

    let status = Command::new("git")
        .args(args)
        .current_dir(workdir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Failed to push branch")?;

    if !status.success() {
        anyhow::bail!("Failed to push branch {}", branch);
    }
    Ok(())
}

fn resolve_branches_for_scope(stack: &Stack, current: &str, scope: SubmitScope) -> Vec<String> {
    let branches = match scope {
        SubmitScope::Stack => stack.current_stack(current),
        SubmitScope::Downstack => {
            let mut ancestors = stack.ancestors(current);
            ancestors.reverse();
            ancestors.push(current.to_string());
            ancestors
        }
        SubmitScope::Upstack => {
            let mut upstack = vec![current.to_string()];
            upstack.extend(stack.descendants(current));
            upstack
        }
        SubmitScope::Branch => vec![current.to_string()],
    };

    branches
        .into_iter()
        .filter(|branch| branch != &stack.trunk)
        .collect()
}

/// Ref names to pass to `git fetch --no-tags <remote> ...` before submit (trunk, submitted branches,
/// and parents required for narrow-scope validation).
fn branches_to_fetch_for_submit(
    repo: &GitRepo,
    stack: &Stack,
    scope: SubmitScope,
    branches_to_submit: &[String],
) -> Result<Vec<String>> {
    let mut names = BTreeSet::new();
    names.insert(stack.trunk.clone());
    for b in branches_to_submit {
        names.insert(b.clone());
    }
    if matches!(scope, SubmitScope::Branch | SubmitScope::Upstack) {
        let submitted: HashSet<&str> = branches_to_submit.iter().map(String::as_str).collect();
        for branch in branches_to_submit {
            let meta = BranchMetadata::read(repo.inner(), branch)?
                .with_context(|| format!("No metadata for branch {}", branch))?;
            let parent = &meta.parent_branch_name;
            if parent == &stack.trunk || submitted.contains(parent.as_str()) {
                continue;
            }
            names.insert(parent.clone());
        }
    }
    Ok(names.into_iter().collect())
}

fn validate_narrow_scope_submit(
    scope: SubmitScope,
    repo: &GitRepo,
    stack: &Stack,
    current: &str,
    remote_name: &str,
    branches_to_submit: &[String],
    no_fetch: bool,
) -> Result<()> {
    if matches!(scope, SubmitScope::Branch) && current == stack.trunk {
        anyhow::bail!(
            "Cannot submit trunk '{}' as a single branch.\n\
             Checkout a tracked branch and run `stax branch submit`, or run `stax submit` for the whole stack.",
            stack.trunk
        );
    }

    let current_meta = BranchMetadata::read(repo.inner(), current)?;
    if current != stack.trunk && current_meta.is_none() {
        anyhow::bail!(
            "Branch '{}' is not tracked by stax.\n\
             Use `stax branch track --parent <branch>` (or `stax branch reparent`) and retry.",
            current
        );
    }

    let submitted: HashSet<&str> = branches_to_submit.iter().map(String::as_str).collect();

    for branch in branches_to_submit {
        let meta = BranchMetadata::read(repo.inner(), branch)?
            .context(format!("No metadata for branch {}", branch))?;
        let parent = meta.parent_branch_name;

        if parent == stack.trunk || submitted.contains(parent.as_str()) {
            continue;
        }

        let needs_restack = stack
            .branches
            .get(branch)
            .map(|b| b.needs_restack)
            .unwrap_or(false);
        if needs_restack {
            anyhow::bail!(
                "Branch '{}' needs restack before scoped submit.\n\
                 Run `stax restack` or submit with ancestor scope: `stax downstack submit` / `stax submit`.",
                branch
            );
        }

        if !branch_matches_remote(repo.workdir()?, remote_name, &parent) {
            if no_fetch {
                anyhow::bail!(
                    "Parent branch '{}' is not in sync with cached '{}/{}'.\n\
                     You used --no-fetch, so cached refs may be stale.\n\
                     Try rerunning without --no-fetch, or run `git fetch {}` first.\n\
                     Narrow scope submit for '{}' is unsafe while parent appears out-of-sync.",
                    parent,
                    remote_name,
                    parent,
                    remote_name,
                    branch
                );
            } else {
                anyhow::bail!(
                    "Parent branch '{}' is not in sync with '{}/{}'.\n\
                     Narrow scope submit for '{}' is unsafe because its parent is excluded.\n\
                     Run `stax downstack submit` or `stax submit` to include ancestors first.",
                    parent,
                    remote_name,
                    parent,
                    branch
                );
            }
        }
    }

    Ok(())
}

async fn discover_existing_pr(
    forge_client: ForgeClient,
    branch: String,
    metadata_pr_number: Option<u64>,
    has_remote_branch: bool,
) -> Result<ExistingPrLookup> {
    let mut existing_pr = None;
    let had_metadata_pr = metadata_pr_number.is_some();

    if let Some(pr_number) = metadata_pr_number {
        if let Ok(pr) = forge_client.get_pr_with_head(pr_number).await {
            let state = pr.info.state.to_ascii_lowercase();
            if pr.head == branch && matches!(state.as_str(), "open" | "opened") {
                existing_pr = Some(pr);
            }
        }
    }

    if existing_pr.is_none() {
        existing_pr = forge_client.find_open_pr_by_head(&branch).await?;
    }

    Ok(ExistingPrLookup {
        branch,
        needs_full_scan_fallback: existing_pr.is_none() && (had_metadata_pr || has_remote_branch),
        existing_pr,
    })
}

/// Check if a branch needs to be pushed (local differs from remote)
fn branch_needs_push(workdir: &Path, remote: &str, branch: &str) -> bool {
    // Get local commit
    let local = Command::new("git")
        .args(["rev-parse", branch])
        .current_dir(workdir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    // Get remote commit
    let remote_ref = format!("{}/{}", remote, branch);
    let remote_commit = Command::new("git")
        .args(["rev-parse", &remote_ref])
        .current_dir(workdir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    match (local, remote_commit) {
        (Some(l), Some(r)) => l != r, // Need push if different
        (Some(_), None) => true,      // Branch not on remote yet
        _ => true,                    // Default to push if unsure
    }
}

fn branch_matches_remote(workdir: &Path, remote: &str, branch: &str) -> bool {
    let local = Command::new("git")
        .args(["rev-parse", branch])
        .current_dir(workdir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    let remote_ref = format!("{}/{}", remote, branch);
    let remote_commit = Command::new("git")
        .args(["rev-parse", &remote_ref])
        .current_dir(workdir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    match (local, remote_commit) {
        (Some(l), Some(r)) => l == r,
        _ => false,
    }
}

/// Get the subject line of the tip commit on a branch.
fn tip_commit_subject(workdir: &Path, branch: &str) -> Option<String> {
    Command::new("git")
        .args(["log", "-1", "--format=%s", branch])
        .current_dir(workdir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        })
}

fn collect_commit_messages(workdir: &Path, parent: &str, branch: &str) -> Vec<String> {
    let output = Command::new("git")
        .args([
            "log",
            "--reverse",
            "--format=%s",
            &format!("{}..{}", parent, branch),
        ])
        .current_dir(workdir)
        .output();

    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn default_pr_title(commit_messages: &[String], branch: &str) -> String {
    if let Some(first) = commit_messages.first() {
        return first.clone();
    }

    branch
        .split('/')
        .next_back()
        .unwrap_or(branch)
        .replace(['-', '_'], " ")
}

fn build_default_pr_body(
    template: Option<&str>,
    branch: &str,
    commit_messages: &[String],
) -> String {
    let commits_text = render_commit_list(commit_messages);

    let mut body = if let Some(template) = template {
        template.to_string()
    } else if commits_text.is_empty() {
        String::new()
    } else {
        format!("## Summary\n\n{}", commits_text)
    };

    if !body.is_empty() {
        body = body.replace("{{BRANCH}}", branch);
        body = body.replace("{{COMMITS}}", &commits_text);
    }

    body
}

fn render_commit_list(commit_messages: &[String]) -> String {
    if commit_messages.is_empty() {
        return String::new();
    }

    commit_messages
        .iter()
        .map(|msg| format!("- {}", msg))
        .collect::<Vec<_>>()
        .join("\n")
}

fn prompt_existing_ai_targets(
    targets: AiPrTargets,
    branch: &str,
    pr_number: u64,
) -> Result<Option<AiPrTargets>> {
    let items = existing_ai_prompt_items(targets);
    let labels: Vec<_> = items.iter().map(|(label, _)| *label).collect();
    let choice = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(format!("  AI update for {} #{}", branch, pr_number))
        .items(&labels)
        .default(0)
        .interact()?;

    Ok(items[choice].1)
}

fn existing_ai_prompt_items(targets: AiPrTargets) -> Vec<(&'static str, Option<AiPrTargets>)> {
    let mut items: Vec<(&str, Option<AiPrTargets>)> = vec![("Skip", None)];

    match (targets.title, targets.body) {
        (true, true) => {
            items.push((
                "Update title",
                Some(AiPrTargets {
                    title: true,
                    body: false,
                    explicit_scope: true,
                }),
            ));
            items.push((
                "Update body",
                Some(AiPrTargets {
                    title: false,
                    body: true,
                    explicit_scope: true,
                }),
            ));
            items.push((
                "Update title and body",
                Some(AiPrTargets {
                    title: true,
                    body: true,
                    explicit_scope: true,
                }),
            ));
        }
        (true, false) => items.push(("Update title", Some(targets))),
        (false, true) => items.push(("Update body", Some(targets))),
        (false, false) => {}
    }

    items
}

fn finish_default_pr_detail_progress(quiet: bool, branch: &str, field: &str, suffix: &str) {
    let timer = LiveTimer::maybe_new(
        !quiet,
        &format!("    Using default {} for {}...", field, branch),
    );
    if suffix == "fallback" {
        LiveTimer::maybe_finish_warn(timer, suffix);
    } else {
        LiveTimer::maybe_finish_ok(timer, suffix);
    }
}

fn resolve_ai_agent_selection(
    cache: &mut Option<AiAgentSelection>,
    non_interactive: bool,
) -> Result<AiAgentSelection> {
    if let Some(selection) = cache {
        return Ok(selection.clone());
    }

    use super::generate;

    let mut config = Config::load()?;
    let (agent, model) = if non_interactive {
        let agent = generate::resolve_agent_non_interactive(None, &config, "generate")?;
        let model = generate::resolve_model(None, &config, &agent, "generate")?;
        (agent, model)
    } else if config.ai.generate.agent.is_some() {
        let agent = config
            .ai
            .agent_for("generate")
            .context("No AI agent configured for PR generation")?
            .to_string();
        let model = generate::resolve_model(None, &config, &agent, "generate")?;
        (agent, model)
    } else {
        generate::prompt_for_feature_ai(&mut config, "generate")?
    };

    let selection = AiAgentSelection { agent, model };
    *cache = Some(selection.clone());
    Ok(selection)
}

#[allow(clippy::too_many_arguments)]
fn generate_ai_pr_details(
    workdir: &Path,
    parent: &str,
    branch: &str,
    template: Option<&str>,
    targets: AiPrTargets,
    selection_cache: &mut Option<AiAgentSelection>,
    non_interactive: bool,
    quiet: bool,
) -> Result<AiPrDetails> {
    use super::generate;

    let selection = resolve_ai_agent_selection(selection_cache, non_interactive)?;
    if !quiet {
        generate::print_using_agent(&selection.agent, selection.model.as_deref());
    }

    let context_timer = LiveTimer::maybe_new(
        !quiet,
        &format!("    Collecting PR context for {}...", branch),
    );
    let diff_stat = generate::get_diff_stat(workdir, parent, branch);
    let diff = generate::get_full_diff(workdir, parent, branch);
    let commits = collect_commit_messages(workdir, parent, branch);
    LiveTimer::maybe_finish_ok(context_timer, "done");

    let prompt = build_ai_pr_details_prompt(&diff_stat, &diff, &commits, template, targets);

    let generation_timer = LiveTimer::maybe_new(
        !quiet,
        &format!("    Generating AI PR details for {}...", branch),
    );
    let raw = match generate::invoke_ai_agent(&selection.agent, selection.model.as_deref(), &prompt)
    {
        Ok(raw) => raw,
        Err(err) => {
            LiveTimer::maybe_finish_warn(generation_timer, "failed");
            return Err(err);
        }
    };

    match parse_ai_pr_details(&raw, targets) {
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

fn build_ai_pr_details_prompt(
    diff_stat: &str,
    diff: &str,
    commits: &[String],
    template: Option<&str>,
    targets: AiPrTargets,
) -> String {
    let mut prompt = String::new();

    match (targets.title, targets.body) {
        (true, true) => {
            prompt.push_str("Generate a pull request title and body for the following changes.\n\n")
        }
        (true, false) => {
            prompt.push_str("Generate a pull request title for the following changes.\n\n")
        }
        (false, true) => {
            prompt.push_str("Generate a pull request body for the following changes.\n\n")
        }
        (false, false) => prompt.push_str("Summarize the following changes.\n\n"),
    }

    prompt.push_str("Return only a compact JSON object with these string fields: ");
    let fields = match (targets.title, targets.body) {
        (true, true) => "\"title\" and \"body\"",
        (true, false) => "\"title\"",
        (false, true) => "\"body\"",
        (false, false) => "",
    };
    prompt.push_str(fields);
    prompt.push_str(". Do not include markdown fences or explanatory text.\n\n");

    if targets.title {
        prompt.push_str("Title requirements:\n- Concise PR title, no trailing period\n- Describe the user-visible change, not the implementation mechanics\n\n");
    }

    if targets.body {
        if let Some(tmpl) = template {
            prompt.push_str("Use this PR template as the body structure. Fill in each section based on the changes:\n\n");
            prompt.push_str(tmpl);
            prompt.push_str("\n\n");
        } else {
            prompt.push_str("Body requirements:\n- Markdown\n- Include a clear Summary section\n- Mention important tests or validation only when supported by the diff or commits\n\n");
        }
    }

    if !commits.is_empty() {
        prompt.push_str("Commit messages:\n");
        for msg in commits {
            prompt.push_str(&format!("- {}\n", msg));
        }
        prompt.push('\n');
    }

    if !diff_stat.is_empty() {
        prompt.push_str("Diff stat:\n```\n");
        prompt.push_str(diff_stat);
        prompt.push_str("\n```\n\n");
    }

    if !diff.is_empty() {
        prompt.push_str("Full diff:\n```diff\n");
        prompt.push_str(&truncate_ai_diff(diff));
        prompt.push_str("\n```\n\n");
    }

    prompt
}

fn truncate_ai_diff(diff: &str) -> String {
    if diff.len() <= MAX_AI_DIFF_BYTES {
        return diff.to_string();
    }

    let safe_end = safe_char_boundary(diff, MAX_AI_DIFF_BYTES);
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

// Deprecated: Use github::pr_template::discover_pr_templates instead
#[allow(dead_code)]
fn load_pr_template(workdir: &Path) -> Option<String> {
    let candidates = [
        ".github/pull_request_template.md",
        ".github/PULL_REQUEST_TEMPLATE.md",
        "PULL_REQUEST_TEMPLATE.md",
        "pull_request_template.md",
    ];

    for candidate in &candidates {
        let path = workdir.join(candidate);
        if path.is_file() {
            if let Ok(content) = fs::read_to_string(path) {
                return Some(content);
            }
        }
    }

    let dir = workdir.join(".github").join("pull_request_template");
    if dir.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .ok()?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .map(|ext| ext == "md")
                    .unwrap_or(false)
            })
            .collect();
        entries.sort_by_key(|entry| entry.path());
        if let Some(entry) = entries.first() {
            if let Ok(content) = fs::read_to_string(entry.path()) {
                return Some(content);
            }
        }
    }

    None
}

async fn apply_pr_metadata(
    client: &ForgeClient,
    pr_number: u64,
    reviewers: &[String],
    labels: &[String],
    assignees: &[String],
) -> Result<()> {
    if !reviewers.is_empty() {
        client.request_reviewers(pr_number, reviewers).await?;
    }

    if !labels.is_empty() {
        client.add_labels(pr_number, labels).await?;
    }

    if !assignees.is_empty() {
        client.add_assignees(pr_number, assignees).await?;
    }

    Ok(())
}

async fn apply_ai_pr_content_updates(
    client: &ForgeClient,
    pr_number: u64,
    branch: &str,
    title: Option<&str>,
    body: Option<&str>,
    quiet: bool,
) -> Result<()> {
    if let Some(title) = title {
        let timer = LiveTimer::maybe_new(
            !quiet,
            &format!("Updating AI title for {} #{}...", branch, pr_number),
        );
        client.update_pr_title(pr_number, title).await?;
        LiveTimer::maybe_finish_ok(timer, "done");
    }

    if let Some(body) = body {
        let timer = LiveTimer::maybe_new(
            !quiet,
            &format!("Updating AI body for {} #{}...", branch, pr_number),
        );
        client.update_pr_body(pr_number, body).await?;
        LiveTimer::maybe_finish_ok(timer, "done");
    }

    Ok(())
}

fn print_verbose_network_summary(
    client: Option<&ForgeClient>,
    remote_name: &str,
    fetch_summary: &str,
    timings: &SubmitPhaseTimings,
    full_scan_fallbacks: usize,
) {
    println!();
    println!("{}", "Verbose network summary:".bold());
    println!(
        "  {:<28} {}",
        format!("git fetch {}", remote_name),
        fetch_summary
    );

    if let Some(stats) = client.and_then(|client| client.api_call_stats()) {
        println!(
            "  {:<28} {}",
            "forge.api.total",
            stats.total_requests.to_string().cyan()
        );
        if stats.by_operation.is_empty() {
            println!("  {}", "No forge API requests recorded".dimmed());
        } else {
            for (operation, count) in stats.by_operation {
                println!("    {:<28} {}", operation, count);
            }
        }
    } else {
        println!("  {:<28} {}", "forge.api.total", "0".cyan());
        println!("  {}", "No API stats available".dimmed());
    }

    println!();
    println!("{}", "Phase timings:".bold());
    println!("  {:<28} {}", "planning", format_duration(timings.planning));
    println!(
        "  {:<28} {}",
        "open PR discovery",
        format_duration(timings.open_pr_discovery)
    );
    println!(
        "  {:<28} {}",
        "create/update PRs",
        format_duration(timings.pr_create_update)
    );
    println!(
        "  {:<28} {}",
        "stack links",
        format_duration(timings.stack_links)
    );
    println!(
        "  {:<28} {}",
        "full-scan fallbacks",
        full_scan_fallbacks.to_string().cyan()
    );
}

fn format_duration(duration: Duration) -> String {
    if duration.as_secs_f64() < 0.001 {
        "0.000s".to_string()
    } else {
        format!("{:.3}s", duration.as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_ai_pr_details_prompt, existing_ai_prompt_items, existing_ai_targets_for_auto_accept,
        parse_ai_pr_details, resolve_ai_targets, resolve_is_draft_without_prompt, truncate_ai_diff,
        AiPrTargets, MAX_AI_DIFF_BYTES, PR_TYPE_DEFAULT_INDEX, PR_TYPE_OPTIONS,
    };

    #[test]
    fn no_prompt_defaults_to_draft() {
        assert_eq!(
            resolve_is_draft_without_prompt(false, false, false, true),
            Some(true)
        );
    }

    #[test]
    fn explicit_draft_flag_still_forces_draft() {
        assert_eq!(
            resolve_is_draft_without_prompt(true, false, true, false),
            Some(true)
        );
    }

    #[test]
    fn explicit_no_draft_flag_still_requires_prompt() {
        assert_eq!(
            resolve_is_draft_without_prompt(false, false, false, false),
            None
        );
    }

    #[test]
    fn publish_flag_forces_non_draft() {
        assert_eq!(
            resolve_is_draft_without_prompt(false, true, false, false),
            Some(false)
        );
    }

    #[test]
    fn publish_flag_overrides_no_prompt_default() {
        assert_eq!(
            resolve_is_draft_without_prompt(false, true, false, true),
            Some(false)
        );
    }

    #[test]
    fn interactive_default_option_is_draft() {
        assert_eq!(PR_TYPE_DEFAULT_INDEX, 0);
        assert_eq!(PR_TYPE_OPTIONS[PR_TYPE_DEFAULT_INDEX], "Create as draft");
    }

    #[test]
    fn ai_without_modifiers_targets_title_and_body() {
        assert_eq!(
            resolve_ai_targets(true, false, false, false).unwrap(),
            Some(AiPrTargets {
                title: true,
                body: true,
                explicit_scope: false,
            })
        );
    }

    #[test]
    fn body_scope_targets_body_only() {
        assert_eq!(
            resolve_ai_targets(true, false, true, false).unwrap(),
            Some(AiPrTargets {
                title: false,
                body: true,
                explicit_scope: true,
            })
        );
    }

    #[test]
    fn title_scope_targets_title_only() {
        assert_eq!(
            resolve_ai_targets(true, true, false, false).unwrap(),
            Some(AiPrTargets {
                title: true,
                body: false,
                explicit_scope: true,
            })
        );
    }

    #[test]
    fn title_and_body_scope_targets_both_explicitly() {
        assert_eq!(
            resolve_ai_targets(true, true, true, false).unwrap(),
            Some(AiPrTargets {
                title: true,
                body: true,
                explicit_scope: true,
            })
        );
    }

    #[test]
    fn title_and_body_modifiers_require_ai() {
        let err = resolve_ai_targets(false, true, false, false).unwrap_err();
        assert!(err.to_string().contains("--title requires --ai"));

        let err = resolve_ai_targets(false, false, true, false).unwrap_err();
        assert!(err.to_string().contains("--body requires --ai"));
    }

    #[test]
    fn update_title_conflicts_when_ai_generates_title() {
        let err = resolve_ai_targets(true, false, false, true).unwrap_err();
        assert!(err
            .to_string()
            .contains("--update-title cannot be combined"));

        let err = resolve_ai_targets(true, true, false, true).unwrap_err();
        assert!(err
            .to_string()
            .contains("--update-title cannot be combined"));

        assert!(resolve_ai_targets(true, false, true, true).is_ok());
    }

    #[test]
    fn auto_accept_plain_ai_skips_existing_pr_content_updates() {
        let targets = resolve_ai_targets(true, false, false, false)
            .unwrap()
            .unwrap();

        assert_eq!(existing_ai_targets_for_auto_accept(targets), None);
    }

    #[test]
    fn auto_accept_explicit_title_updates_existing_pr_title() {
        let targets = resolve_ai_targets(true, true, false, false)
            .unwrap()
            .unwrap();

        assert_eq!(
            existing_ai_targets_for_auto_accept(targets),
            Some(AiPrTargets {
                title: true,
                body: false,
                explicit_scope: true,
            })
        );
    }

    #[test]
    fn auto_accept_explicit_body_updates_existing_pr_body() {
        let targets = resolve_ai_targets(true, false, true, false)
            .unwrap()
            .unwrap();

        assert_eq!(
            existing_ai_targets_for_auto_accept(targets),
            Some(AiPrTargets {
                title: false,
                body: true,
                explicit_scope: true,
            })
        );
    }

    #[test]
    fn auto_accept_explicit_title_and_body_updates_existing_pr_both() {
        let targets = resolve_ai_targets(true, true, true, false)
            .unwrap()
            .unwrap();

        assert_eq!(
            existing_ai_targets_for_auto_accept(targets),
            Some(AiPrTargets {
                title: true,
                body: true,
                explicit_scope: true,
            })
        );
    }

    #[test]
    fn existing_ai_prompt_choices_match_requested_scope() {
        let full = AiPrTargets {
            title: true,
            body: true,
            explicit_scope: false,
        };
        let labels: Vec<_> = existing_ai_prompt_items(full)
            .into_iter()
            .map(|(label, _)| label)
            .collect();
        assert_eq!(
            labels,
            vec![
                "Skip",
                "Update title",
                "Update body",
                "Update title and body"
            ]
        );

        let title_only = AiPrTargets {
            title: true,
            body: false,
            explicit_scope: true,
        };
        let labels: Vec<_> = existing_ai_prompt_items(title_only)
            .into_iter()
            .map(|(label, _)| label)
            .collect();
        assert_eq!(labels, vec!["Skip", "Update title"]);

        let body_only = AiPrTargets {
            title: false,
            body: true,
            explicit_scope: true,
        };
        let labels: Vec<_> = existing_ai_prompt_items(body_only)
            .into_iter()
            .map(|(label, _)| label)
            .collect();
        assert_eq!(labels, vec!["Skip", "Update body"]);
    }

    #[test]
    fn parses_ai_pr_details_json() {
        let targets = AiPrTargets {
            title: true,
            body: true,
            explicit_scope: false,
        };

        let details = parse_ai_pr_details(
            r###"{"title":"Improve submit AI","body":"## Summary\n\nAdds AI PR details."}"###,
            targets,
        )
        .unwrap();

        assert_eq!(details.title.as_deref(), Some("Improve submit AI"));
        assert_eq!(
            details.body.as_deref(),
            Some("## Summary\n\nAdds AI PR details.")
        );
    }

    #[test]
    fn parses_ai_pr_details_from_json_fence() {
        let targets = AiPrTargets {
            title: false,
            body: true,
            explicit_scope: true,
        };

        let details =
            parse_ai_pr_details("```json\n{\"body\":\"Only update the body\"}\n```", targets)
                .unwrap();

        assert_eq!(details.title, None);
        assert_eq!(details.body.as_deref(), Some("Only update the body"));
    }

    #[test]
    fn parse_ai_pr_details_requires_requested_fields() {
        let targets = AiPrTargets {
            title: true,
            body: false,
            explicit_scope: true,
        };
        let err = parse_ai_pr_details(r###"{"body":"Body only"}"###, targets).unwrap_err();
        assert!(err.to_string().contains("non-empty title"));

        let targets = AiPrTargets {
            title: false,
            body: true,
            explicit_scope: true,
        };
        let err = parse_ai_pr_details(r###"{"title":"Title only"}"###, targets).unwrap_err();
        assert!(err.to_string().contains("non-empty body"));
    }

    #[test]
    fn ai_prompt_for_title_only_requests_title_json_without_body_rules() {
        let prompt = build_ai_pr_details_prompt(
            "src/lib.rs | 1 +",
            "diff --git a/src/lib.rs b/src/lib.rs",
            &["Add scoped submit AI".to_string()],
            Some("## Summary\n{{COMMITS}}"),
            AiPrTargets {
                title: true,
                body: false,
                explicit_scope: true,
            },
        );

        assert!(prompt.contains("Generate a pull request title"));
        assert!(prompt.contains("\"title\""));
        assert!(!prompt.contains("\"body\""));
        assert!(prompt.contains("Title requirements:"));
        assert!(!prompt.contains("Use this PR template"));
    }

    #[test]
    fn ai_prompt_for_body_only_uses_template_and_body_json() {
        let prompt = build_ai_pr_details_prompt(
            "",
            "",
            &[],
            Some("## Summary\n{{COMMITS}}"),
            AiPrTargets {
                title: false,
                body: true,
                explicit_scope: true,
            },
        );

        assert!(prompt.contains("Generate a pull request body"));
        assert!(prompt.contains("\"body\""));
        assert!(!prompt.contains("\"title\""));
        assert!(prompt.contains("Use this PR template as the body structure"));
        assert!(prompt.contains("## Summary"));
    }

    #[test]
    fn truncate_ai_diff_does_not_split_utf8() {
        let mut diff = "a".repeat(MAX_AI_DIFF_BYTES - 1);
        diff.push('é');
        diff.push_str("\nrest");

        let truncated = truncate_ai_diff(&diff);
        assert!(truncated.contains("diff truncated"));
        assert!(truncated.is_char_boundary(truncated.len()));
    }
}
