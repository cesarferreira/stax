use super::shared::{
    build_agent_launch_spec, build_tmux_launch_spec, compute_worktree_details, default_create_base,
    default_tmux_session_name, derive_unique_worktree_name, emit_shell_message, emit_shell_payload,
    ensure_gitignore, ensure_managed_worktrees_root, find_worktree, format_create_message,
    format_go_message, list_tmux_sessions, managed_worktrees_dir, resolve_branch_name,
    run_blocking_hook, spawn_background_hook, status_labels, ExistingTmuxSessionBehavior,
    LaunchSpec, TmuxSession,
};
use crate::commands::generate;
use crate::config::Config;
use crate::engine::BranchMetadata;
use crate::git::repo::WorktreeInfo;
use crate::git::GitRepo;
use anyhow::{bail, Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, FuzzySelect, Input};
use std::collections::HashMap;
use std::fs;
use std::io::IsTerminal;
use std::path::Path;

#[derive(Debug, Clone)]
struct AiLaneRequest {
    prompt: Option<String>,
    agent: Option<String>,
    model: Option<String>,
    no_tmux: bool,
    tmux_session: Option<String>,
}

#[derive(Debug, Clone)]
struct PreparedAiLaunch {
    launch: LaunchSpec,
    messages: Vec<String>,
}

enum LaneSelection {
    Existing(String),
    Create {
        name: String,
        prompt: Option<String>,
    },
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    name: Option<String>,
    prompt: Option<String>,
    no_verify: bool,
    shell_output: bool,
    agent: Option<String>,
    model: Option<String>,
    no_tmux: bool,
    tmux_session: Option<String>,
) -> Result<()> {
    if name.is_none() && tmux_session.is_some() {
        bail!("--tmux-session requires an explicit lane name");
    }

    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let request = AiLaneRequest {
        prompt: normalize_prompt(prompt),
        agent,
        model,
        no_tmux,
        tmux_session,
    };

    let (name, prompt) = match name {
        Some(name) => (name, request.prompt.clone()),
        None => match pick_lane_interactively(&repo)? {
            LaneSelection::Existing(name) => (name, None),
            LaneSelection::Create { name, prompt } => (name, prompt),
        },
    };

    let request = AiLaneRequest { prompt, ..request };
    run_named_lane(&repo, &config, name, no_verify, shell_output, &request)
}

fn run_named_lane(
    repo: &GitRepo,
    config: &Config,
    input_name: String,
    no_verify: bool,
    shell_output: bool,
    request: &AiLaneRequest,
) -> Result<()> {
    if let Some(worktree) = find_worktree(repo, &input_name)? {
        return run_existing_lane(config, &worktree, no_verify, shell_output, request);
    }

    let (branch_name, branch_exists) = resolve_branch_name(repo, config, &input_name)?;
    if let Some(worktree) = find_worktree(repo, &branch_name)? {
        return run_existing_lane(config, &worktree, no_verify, shell_output, request);
    }

    let base_branch = if branch_exists {
        None
    } else {
        let base_branch = default_create_base(repo)?;
        repo.branch_commit(&base_branch)
            .with_context(|| format!("Base branch '{}' does not exist", base_branch))?;
        Some(base_branch)
    };

    let worktree_name = derive_unique_worktree_name(repo, &branch_name)?;
    let prepared = prepare_ai_launch(config, &worktree_name, request)?;
    let worktrees_dir = managed_worktrees_dir(repo, config)?;
    let worktree_path = worktrees_dir.join(&worktree_name);
    if worktree_path.exists() {
        bail!(
            "Worktree path '{}' already exists.",
            worktree_path.display()
        );
    }

    fs::create_dir_all(&worktrees_dir)?;
    ensure_managed_worktrees_root(repo, config, &worktrees_dir)?;
    let main_repo_workdir = repo.main_repo_workdir()?;
    ensure_gitignore(&main_repo_workdir, &config.worktree.root_dir)?;

    if branch_exists {
        repo.worktree_create(&branch_name, &worktree_path)?;
    } else {
        let from_branch = base_branch
            .as_deref()
            .expect("base branch is always set for a new lane");
        repo.worktree_create_new_branch(&branch_name, &worktree_path, from_branch)?;
        let parent_rev = repo.branch_commit(from_branch)?;
        let meta = BranchMetadata::new(from_branch, &parent_rev);
        meta.write(repo.inner(), &branch_name)?;
    }

    let copied_files = repo.tracked_file_count_at(&worktree_path).unwrap_or(0);
    let repo_name = main_repo_workdir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".to_string());
    let from_label = if let Some(base_branch) = base_branch.as_deref() {
        if repo.has_remote(base_branch) {
            format!("origin/{}", base_branch)
        } else {
            base_branch.to_string()
        }
    } else {
        branch_name.clone()
    };
    format_create_message(
        &repo_name,
        &worktree_name,
        &branch_name,
        &from_label,
        copied_files,
        branch_exists,
    );

    if !no_verify {
        run_blocking_hook(
            config.worktree.hooks.post_create.as_deref(),
            &worktree_path,
            "post_create",
        )?;
        spawn_background_hook(
            config.worktree.hooks.post_start.as_deref(),
            &worktree_path,
            "post_start",
        )?;
    }

    emit_or_execute_launch(&worktree_path, &prepared, shell_output)
}

fn run_existing_lane(
    config: &Config,
    worktree: &WorktreeInfo,
    no_verify: bool,
    shell_output: bool,
    request: &AiLaneRequest,
) -> Result<()> {
    if !worktree.path.exists() {
        bail!(
            "Worktree path '{}' does not exist. Run `stax worktree prune`.",
            worktree.path.display()
        );
    }

    let prepared = prepare_ai_launch(config, &worktree.name, request)?;
    format_go_message(worktree);

    if !no_verify {
        spawn_background_hook(
            config.worktree.hooks.post_go.as_deref(),
            &worktree.path,
            "post_go",
        )?;
    }

    emit_or_execute_launch(&worktree.path, &prepared, shell_output)
}

fn emit_or_execute_launch(
    path: &Path,
    prepared: &PreparedAiLaunch,
    shell_output: bool,
) -> Result<()> {
    if shell_output {
        for message in &prepared.messages {
            emit_shell_message(message);
        }
        emit_shell_payload(path, Some(&prepared.launch));
        return Ok(());
    }

    for message in &prepared.messages {
        println!("{} {}", "Note:".dimmed(), message);
    }
    prepared.launch.execute_in(path)
}

fn prepare_ai_launch(
    config: &Config,
    default_session_name: &str,
    request: &AiLaneRequest,
) -> Result<PreparedAiLaunch> {
    prepare_ai_launch_with_tmux_probe(config, default_session_name, request, list_tmux_sessions())
}

fn prepare_ai_launch_with_tmux_probe(
    config: &Config,
    default_session_name: &str,
    request: &AiLaneRequest,
    tmux_probe: Result<Vec<TmuxSession>>,
) -> Result<PreparedAiLaunch> {
    let mut messages = Vec::new();
    let prompt_args = request.prompt.clone().into_iter().collect::<Vec<_>>();

    if !request.no_tmux {
        match tmux_probe {
            Ok(sessions) => {
                let session_name = request
                    .tmux_session
                    .as_deref()
                    .unwrap_or(default_session_name);
                let session_exists = sessions.iter().any(|session| session.name == session_name);

                if request.prompt.is_none() && session_exists {
                    let launch = build_tmux_launch_spec(
                        session_name,
                        None,
                        ExistingTmuxSessionBehavior::Attach,
                    )?;
                    return Ok(PreparedAiLaunch { launch, messages });
                }

                let agent =
                    generate::resolve_agent_non_interactive(request.agent.as_deref(), config)?;
                let inner = build_agent_launch_spec(&agent, request.model.clone(), prompt_args)?;
                let behavior = if request.prompt.is_some() {
                    ExistingTmuxSessionBehavior::NewWindow
                } else {
                    ExistingTmuxSessionBehavior::Attach
                };
                let launch = build_tmux_launch_spec(session_name, Some(&inner), behavior)?;
                return Ok(PreparedAiLaunch { launch, messages });
            }
            Err(_) if request.tmux_session.is_some() => {
                bail!("tmux is not available, so --tmux-session cannot be used");
            }
            Err(_) => {
                messages.push("tmux is not available; launching directly in this lane".to_string());
            }
        }
    }

    let agent = generate::resolve_agent_non_interactive(request.agent.as_deref(), config)?;
    let launch = build_agent_launch_spec(&agent, request.model.clone(), prompt_args)?;
    Ok(PreparedAiLaunch { launch, messages })
}

fn pick_lane_interactively(repo: &GitRepo) -> Result<LaneSelection> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        bail!("`st lane` with no name requires an interactive terminal");
    }

    let managed = repo
        .list_worktrees()?
        .into_iter()
        .map(|worktree| compute_worktree_details(repo, worktree))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|details| details.is_managed)
        .collect::<Vec<_>>();

    if managed.is_empty() {
        return prompt_for_new_lane();
    }

    let (tmux_sessions, tmux_available) = match list_tmux_sessions() {
        Ok(sessions) => (
            sessions
                .into_iter()
                .map(|session| (session.name.clone(), session))
                .collect::<HashMap<_, _>>(),
            true,
        ),
        Err(_) => (HashMap::new(), false),
    };

    let name_width = managed
        .iter()
        .map(|details| details.info.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let branch_width = managed
        .iter()
        .map(|details| details.branch_label.len())
        .max()
        .unwrap_or(6)
        .max(6);

    let mut items = vec!["+ Create new lane...".to_string()];
    items.extend(managed.iter().map(|details| {
        let tmux = lane_tmux_label(details, &tmux_sessions, tmux_available);
        let status = lane_status_summary(details);
        format!(
            "{} {:<name_width$}  {:<branch_width$}  {:<13}  {}",
            if details.info.is_current { "*" } else { " " },
            details.info.name,
            details.branch_label,
            tmux,
            status,
            name_width = name_width,
            branch_width = branch_width,
        )
    }));

    let default_idx = managed
        .iter()
        .position(|details| details.info.is_current)
        .map(|idx| idx + 1)
        .unwrap_or(0);

    let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Select lane")
        .items(&items)
        .default(default_idx)
        .interact()?;

    if selection == 0 {
        prompt_for_new_lane()
    } else {
        Ok(LaneSelection::Existing(
            managed[selection - 1].info.name.clone(),
        ))
    }
}

fn prompt_for_new_lane() -> Result<LaneSelection> {
    let name: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Lane name")
        .interact_text()?;
    if name.trim().is_empty() {
        bail!("Lane name cannot be empty");
    }

    let prompt: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Task prompt (Enter to skip)")
        .allow_empty(true)
        .interact_text()?;

    Ok(LaneSelection::Create {
        name: name.trim().to_string(),
        prompt: normalize_prompt(Some(prompt)),
    })
}

fn lane_tmux_label(
    details: &super::shared::WorktreeDetails,
    sessions: &HashMap<String, TmuxSession>,
    tmux_available: bool,
) -> String {
    if !tmux_available {
        return "tmux:off".to_string();
    }

    let session_name =
        default_tmux_session_name(&details.info.name).unwrap_or_else(|_| details.info.name.clone());
    match sessions.get(&session_name) {
        Some(session) if session.attached_clients > 0 => "tmux:attached".to_string(),
        Some(_) => "tmux:ready".to_string(),
        None => "tmux:new".to_string(),
    }
}

fn lane_status_summary(details: &super::shared::WorktreeDetails) -> String {
    let compact = status_labels(details)
        .into_iter()
        .filter(|label| label != "managed" && label != "clean")
        .collect::<Vec<_>>();
    if compact.is_empty() {
        "clean".to_string()
    } else {
        compact.join(",")
    }
}

fn normalize_prompt(prompt: Option<String>) -> Option<String> {
    prompt.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{prepare_ai_launch_with_tmux_probe, AiLaneRequest};
    use crate::commands::worktree::shared::LaunchSpec;
    use crate::config::Config;
    use anyhow::anyhow;

    #[test]
    fn prepare_ai_launch_ignores_configured_model_for_ai_lanes() {
        let mut config = Config::default();
        config.ai.agent = Some("claude".to_string());
        config.ai.model = Some("gpt-5.4".to_string());

        let prepared = prepare_ai_launch_with_tmux_probe(
            &config,
            "review-pass",
            &AiLaneRequest {
                prompt: Some("fix macOS build".to_string()),
                agent: None,
                model: None,
                no_tmux: true,
                tmux_session: None,
            },
            Err(anyhow!("tmux unavailable")),
        )
        .expect("prepare launch");

        match prepared.launch {
            LaunchSpec::Process { program, args, .. } => {
                assert_eq!(program, "claude");
                assert_eq!(args, vec!["fix macOS build".to_string()]);
            }
            LaunchSpec::Shell { .. } => panic!("expected direct process launch"),
        }
    }

    #[test]
    fn prepare_ai_launch_keeps_explicit_model_for_ai_lanes() {
        let mut config = Config::default();
        config.ai.agent = Some("claude".to_string());
        config.ai.model = Some("gpt-5.4".to_string());

        let prepared = prepare_ai_launch_with_tmux_probe(
            &config,
            "review-pass",
            &AiLaneRequest {
                prompt: Some("fix macOS build".to_string()),
                agent: None,
                model: Some("claude-sonnet-4-5-20250929".to_string()),
                no_tmux: true,
                tmux_session: None,
            },
            Err(anyhow!("tmux unavailable")),
        )
        .expect("prepare launch");

        match prepared.launch {
            LaunchSpec::Process { program, args, .. } => {
                assert_eq!(program, "claude");
                assert_eq!(
                    args,
                    vec![
                        "--model".to_string(),
                        "claude-sonnet-4-5-20250929".to_string(),
                        "fix macOS build".to_string(),
                    ]
                );
            }
            LaunchSpec::Shell { .. } => panic!("expected direct process launch"),
        }
    }

    #[test]
    fn prepare_ai_launch_defaults_to_tmux_and_attaches_existing_session_without_prompt() {
        let mut config = Config::default();
        config.ai.agent = Some("claude".to_string());

        let prepared = prepare_ai_launch_with_tmux_probe(
            &config,
            "review-pass",
            &AiLaneRequest {
                prompt: None,
                agent: None,
                model: None,
                no_tmux: false,
                tmux_session: None,
            },
            Ok(vec![crate::commands::worktree::shared::TmuxSession {
                name: "review-pass".to_string(),
                attached_clients: 0,
            }]),
        )
        .expect("prepare launch");

        match prepared.launch {
            LaunchSpec::Shell { command, .. } => {
                assert!(command.contains("tmux attach-session -t review-pass"));
                assert!(!command.contains("tmux new-window -t review-pass"));
            }
            LaunchSpec::Process { .. } => panic!("expected tmux launch"),
        }
    }

    #[test]
    fn prepare_ai_launch_falls_back_to_direct_launch_when_tmux_is_unavailable() {
        let mut config = Config::default();
        config.ai.agent = Some("claude".to_string());

        let prepared = prepare_ai_launch_with_tmux_probe(
            &config,
            "review-pass",
            &AiLaneRequest {
                prompt: None,
                agent: None,
                model: None,
                no_tmux: false,
                tmux_session: None,
            },
            Err(anyhow!("tmux unavailable")),
        )
        .expect("prepare launch");

        assert_eq!(
            prepared.messages,
            vec!["tmux is not available; launching directly in this lane".to_string()]
        );
        match prepared.launch {
            LaunchSpec::Process { program, .. } => assert_eq!(program, "claude"),
            LaunchSpec::Shell { .. } => panic!("expected direct process launch"),
        }
    }
}
