use crate::config::Config;
use crate::engine::{BranchMetadata, Stack};
use crate::forge::ForgeClient;
use crate::git::GitRepo;
use crate::github::pr_template::{discover_pr_templates, select_template_interactive};
use crate::remote;
use anyhow::{bail, Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm, Editor, Select};
use regex::Regex;
use serde::Deserialize;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

// ---------------------------------------------------------------------------
// Known models per agent — loaded from models.json at compile time.
// To add/update models edit src/commands/models.json; no Rust changes needed.
// ---------------------------------------------------------------------------

const MODELS_JSON: &str = include_str!("models.json");

#[derive(Deserialize)]
struct ModelsFile {
    claude: Vec<ModelOption>,
    codex: Vec<ModelOption>,
    gemini: Vec<ModelOption>,
    opencode: Vec<ModelOption>,
}

fn models_file() -> &'static ModelsFile {
    static PARSED: std::sync::OnceLock<ModelsFile> = std::sync::OnceLock::new();
    PARSED.get_or_init(|| {
        serde_json::from_str(MODELS_JSON).expect("src/commands/models.json is invalid JSON")
    })
}

const SUPPORTED_AGENTS: &[&str] = &["claude", "codex", "gemini", "opencode"];

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(
    edit: bool,
    no_prompt: bool,
    agent_flag: Option<String>,
    model_flag: Option<String>,
    template_flag: Option<String>,
    no_template: bool,
) -> Result<()> {
    let config = Config::load()?;
    let repo = GitRepo::open()?;
    let workdir = repo.workdir()?.to_path_buf();
    let stack = Stack::load(&repo)?;
    let current_branch = repo.current_branch()?;

    // Ensure current branch is tracked
    let branch_info = stack
        .branches
        .get(&current_branch)
        .context("Current branch is not tracked by stax. Run `stax branch track` first.")?;

    let parent = branch_info
        .parent
        .as_deref()
        .context("Current branch has no parent set")?;

    // Read metadata to find existing PR
    let meta = BranchMetadata::read(repo.inner(), &current_branch)?
        .context("No metadata for current branch")?;

    let pr_number = meta
        .pr_info
        .as_ref()
        .filter(|p| p.number > 0)
        .map(|p| p.number)
        .context("No PR found for current branch. Submit first with `stax submit` or `stax ss`.")?;

    // Resolve AI agent and model (interactive if needed)
    let mut config = config;
    let agent = resolve_agent(agent_flag.as_deref(), &mut config, no_prompt)?;
    let model = resolve_model(model_flag.as_deref(), &config, &agent, "generate")?;

    // Collect context for the prompt
    println!("{}", "Collecting context...".dimmed());
    let diff_stat = get_diff_stat(&workdir, parent, &current_branch);
    let diff = get_full_diff(&workdir, parent, &current_branch);
    let commits = collect_commit_messages(&workdir, parent, &current_branch);

    // Discover and select PR template using the same logic as `submit`
    let discovered_templates = if no_template {
        Vec::new()
    } else {
        discover_pr_templates(&workdir).unwrap_or_default()
    };

    let selected_template = if no_template {
        None
    } else if let Some(ref template_name) = template_flag {
        // --template flag: find by name
        let found = discovered_templates
            .iter()
            .find(|t| t.name == *template_name)
            .cloned();
        if found.is_none() {
            eprintln!(
                "  {} Template '{}' not found, using no template",
                "!".yellow(),
                template_name
            );
        }
        found
    } else if no_prompt {
        // --no-prompt: use first template if exactly one exists
        if discovered_templates.len() == 1 {
            Some(discovered_templates[0].clone())
        } else {
            None
        }
    } else {
        // Interactive selection (handles empty list, single template, and multiple)
        select_template_interactive(&discovered_templates)?
    };

    let template_content = selected_template.as_ref().map(|t| t.content.as_str());

    if diff.trim().is_empty() && commits.is_empty() {
        bail!("No changes found between {} and {}", parent, current_branch);
    }

    // Build the AI prompt
    let prompt = build_ai_prompt(&diff_stat, &diff, &commits, template_content);

    // Invoke AI agent
    print_using_agent(&agent, model.as_deref());

    let generated_body = invoke_ai_agent(&agent, model.as_deref(), &prompt)?;

    if generated_body.trim().is_empty() {
        bail!("AI agent returned an empty response");
    }

    // Let user review/edit the generated body
    let final_body = if edit {
        Editor::new()
            .edit(&generated_body)?
            .unwrap_or(generated_body)
    } else if no_prompt {
        generated_body
    } else {
        // Show preview and confirm
        println!();
        println!("{}", "─── Generated PR Body ───".blue().bold());
        println!("{}", generated_body);
        println!("{}", "──────────────────────────".blue().bold());
        println!();

        let options = vec!["Use as-is", "Edit in $EDITOR", "Cancel"];
        let choice = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("What would you like to do?")
            .items(&options)
            .default(0)
            .interact()?;

        match choice {
            0 => generated_body,
            1 => Editor::new()
                .edit(&generated_body)?
                .unwrap_or(generated_body),
            _ => {
                println!("{}", "Cancelled.".yellow());
                return Ok(());
            }
        }
    };

    // Update the PR body on the forge
    print!("  Updating PR #{} body... ", pr_number.to_string().cyan());
    std::io::stdout().flush().ok();

    let remote_info = remote::RemoteInfo::from_repo(&repo, &config)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let _enter = runtime.enter();
    let client = ForgeClient::new(&remote_info)?;

    runtime.block_on(async { client.update_pr_body(pr_number, &final_body).await })?;

    println!("{}", "done".green());
    println!(
        "  {} PR #{} body updated successfully",
        "✓".green().bold(),
        pr_number
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Agent resolution
// ---------------------------------------------------------------------------

fn resolve_agent(cli_flag: Option<&str>, config: &mut Config, no_prompt: bool) -> Result<String> {
    // 1. CLI flag takes priority
    if let Some(agent) = cli_flag {
        validate_agent_name(agent)?;
        return Ok(agent.to_string());
    }

    // 2. Per-feature config is set — use it (no prompt needed)
    if config.ai.generate.agent.is_some() {
        return Ok(config.ai.agent_for("generate").unwrap().to_string());
    }

    // No per-feature config yet
    if no_prompt {
        // Non-interactive path: use global silently, or auto-detect as last resort
        if let Some(agent) = config.ai.agent_for("generate") {
            return Ok(agent.to_string());
        }
        let agent = auto_detect_agent(&detect_available_agents())?;
        println!(
            "  {} {}",
            "Detected AI agent:".dimmed(),
            agent.cyan().bold()
        );
        return Ok(agent);
    }

    // First-use: prompt and persist to [ai.generate], even if a global default exists
    let (agent, _) = prompt_for_feature_ai(config, "generate")?;
    Ok(agent)
}

pub(crate) fn resolve_agent_non_interactive(
    cli_flag: Option<&str>,
    config: &Config,
    feature: &str,
) -> Result<String> {
    if let Some(agent) = cli_flag {
        validate_agent_name(agent)?;
        return Ok(agent.to_string());
    }

    if let Some(agent) = config.ai.agent_for(feature) {
        return Ok(agent.to_string());
    }

    auto_detect_agent(&detect_available_agents())
}

pub(crate) fn validate_agent_name(agent: &str) -> Result<()> {
    if !SUPPORTED_AGENTS.contains(&agent) {
        bail!(
            "Unsupported AI agent: '{}'. Supported agents: {}",
            agent,
            SUPPORTED_AGENTS.join(", ")
        );
    }
    Ok(())
}

fn detect_available_agents() -> Vec<String> {
    SUPPORTED_AGENTS
        .iter()
        .filter(|&&name| which_exists(name))
        .map(|&name| name.to_string())
        .collect()
}

fn auto_detect_agent(available: &[String]) -> Result<String> {
    available.first().cloned().context(
        "No AI agent found on PATH.\n  \
         Install one of:\n    \
         - claude (https://docs.anthropic.com)\n    \
         - codex  (https://github.com/openai/codex)\n  \
         - gemini (https://github.com/google-gemini/gemini-cli)\n  \
         - opencode (https://opencode.ai)\n  \
         Or set manually in ~/.config/stax/config.toml:\n    \
         [ai]\n    \
         agent = \"claude\"",
    )
}

fn which_exists(command: &str) -> bool {
    Command::new("which")
        .arg(command)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Model resolution
// ---------------------------------------------------------------------------

fn resolve_model(
    cli_flag: Option<&str>,
    config: &Config,
    agent: &str,
    feature: &str,
) -> Result<Option<String>> {
    // 1. CLI flag takes priority
    if let Some(model) = cli_flag {
        validate_model_soft(agent, model);
        return Ok(Some(model.to_string()));
    }

    // 2. Per-feature config, then global config
    if let Some(model) = config.ai.model_for(feature) {
        // If resolved model is a known model for a different agent, ignore it and
        // fall back to the selected agent default.
        if let Some(model_agent) = known_agent_for_model(model) {
            if model_agent != agent {
                eprintln!(
                    "  {} Configured model '{}' is for agent '{}', but current agent is '{}'. Using agent default.",
                    "⚠".yellow(),
                    model.yellow(),
                    model_agent,
                    agent
                );
                return Ok(None);
            }
        }
        validate_model_soft(agent, model);
        return Ok(Some(model.to_string()));
    }

    // 3. No model specified — let agent use its own default
    Ok(None)
}

fn pick_model_interactive(agent: &str) -> Result<Option<String>> {
    let models = available_models_for(agent);
    if models.is_empty() {
        return Ok(None);
    }

    // "Default" is always item 0 — selecting it saves model=None so the agent
    // picks its own default rather than pinning a specific version.
    let mut items = vec!["Default — let the agent decide".to_string()];
    items.extend(
        models
            .iter()
            .map(|m| format!("{} — {}", m.id, m.description)),
    );

    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(format!("Select model for {}", agent))
        .items(&items)
        .default(0)
        .interact()?;

    if selection == 0 {
        Ok(None)
    } else {
        Ok(Some(models[selection - 1].id.clone()))
    }
}

fn validate_model_soft(agent: &str, model: &str) {
    let models = known_models_for(agent);
    if !models.is_empty()
        && !models.iter().any(|m| m.id == model)
        && !model_matches_agent_family(agent, model)
    {
        eprintln!(
            "  {} Unknown model '{}' for agent '{}', proceeding anyway...",
            "⚠".yellow(),
            model.yellow(),
            agent
        );
    }
}

fn known_models_for(agent: &str) -> Vec<ModelOption> {
    let f = models_file();
    match agent {
        "claude" => f.claude.clone(),
        "codex" => f.codex.clone(),
        "gemini" => f.gemini.clone(),
        "opencode" => f.opencode.clone(),
        _ => vec![],
    }
}

fn known_agent_for_model(model: &str) -> Option<&'static str> {
    ["claude", "gemini", "opencode"]
        .into_iter()
        .find(|agent| known_models_for(agent).iter().any(|m| m.id == model))
        .or_else(|| {
            if model_matches_agent_family("codex", model) {
                Some("codex")
            } else {
                None
            }
        })
}

pub(crate) fn prompt_for_agent_and_model(
    config: &mut Config,
    confirm_before_save: bool,
) -> Result<(String, Option<String>)> {
    let available = detect_available_agents();

    match available.len() {
        0 => auto_detect_agent(&available).map(|agent| (agent, None)),
        1 => {
            let agent = available[0].clone();
            println!(
                "  {} {}",
                "Detected AI agent:".dimmed(),
                agent.cyan().bold()
            );
            let model = pick_model_interactive(&agent)?;
            persist_prompt_selection(config, &agent, model.clone(), confirm_before_save)?;
            Ok((agent, model))
        }
        _ => {
            let items: Vec<String> = available
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    if i == 0 {
                        format!("{} (default)", name)
                    } else {
                        name.to_string()
                    }
                })
                .collect();

            let selection = Select::with_theme(&ColorfulTheme::default())
                .with_prompt("Select AI agent")
                .items(&items)
                .default(0)
                .interact()?;

            let agent = available[selection].clone();
            let model = pick_model_interactive(&agent)?;
            persist_prompt_selection(config, &agent, model.clone(), confirm_before_save)?;
            Ok((agent, model))
        }
    }
}

fn persist_prompt_selection(
    config: &mut Config,
    agent: &str,
    model: Option<String>,
    confirm_before_save: bool,
) -> Result<()> {
    let should_save = if confirm_before_save {
        Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Save choices to config?")
            .default(true)
            .interact()?
    } else {
        true
    };

    if should_save {
        config.ai.agent = Some(agent.to_string());
        config.ai.model = model.clone();
        config.save()?;
        let model_display = model.as_deref().unwrap_or("agent default");
        println!(
            "  {} Saved ai.agent = \"{}\", ai.model = \"{}\"",
            "✓".green().bold(),
            agent,
            model_display
        );
    }

    Ok(())
}

/// Interactively pick agent+model for a specific feature and persist to `[ai.<feature>]`.
/// Falls back to persisting to global `[ai]` when `feature` is "global" or unknown.
pub(crate) fn prompt_for_feature_ai(
    config: &mut Config,
    feature: &str,
) -> Result<(String, Option<String>)> {
    let available = detect_available_agents();

    let agent = match available.len() {
        0 => auto_detect_agent(&available)?,
        1 => {
            let agent = available[0].clone();
            println!(
                "  {} {}",
                "Detected AI agent:".dimmed(),
                agent.cyan().bold()
            );
            agent
        }
        _ => {
            let items: Vec<String> = available
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    if i == 0 {
                        format!("{} (default)", name)
                    } else {
                        name.to_string()
                    }
                })
                .collect();

            let selection = Select::with_theme(&ColorfulTheme::default())
                .with_prompt(format!("Select AI agent for {}", feature))
                .items(&items)
                .default(0)
                .interact()?;

            available[selection].clone()
        }
    };

    let model = pick_model_interactive(&agent)?;

    // Persist to feature-specific config, or global if feature is "global"/unknown.
    if let Some(feat_cfg) = config.ai.feature_config_mut(feature) {
        feat_cfg.agent = Some(agent.clone());
        feat_cfg.model = model.clone();
        config.save()?;
        let model_display = model.as_deref().unwrap_or("agent default");
        println!(
            "  {} Saved [ai.{}] agent = \"{}\", model = \"{}\"",
            "✓".green().bold(),
            feature,
            agent,
            model_display
        );
    } else {
        // "global" or unknown feature — write to top-level [ai]
        config.ai.agent = Some(agent.clone());
        config.ai.model = model.clone();
        config.save()?;
        let model_display = model.as_deref().unwrap_or("agent default");
        println!(
            "  {} Saved ai.agent = \"{}\", ai.model = \"{}\"",
            "✓".green().bold(),
            agent,
            model_display
        );
    }

    Ok((agent, model))
}

/// Print a consistent "Using <agent> [with model <model>]" line before an AI call.
/// Uses stderr so it doesn't pollute captured stdout (e.g. `--json` flows).
pub(crate) fn print_using_agent(agent: &str, model: Option<&str>) {
    match model {
        Some(m) => eprintln!(
            "  {} {}",
            format!("Using {} with model", agent).dimmed(),
            m.cyan()
        ),
        None => eprintln!("  {}", format!("Using {}", agent).dimmed()),
    }
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
struct ModelOption {
    id: String,
    description: String,
}

fn available_models_for(agent: &str) -> Vec<ModelOption> {
    if agent == "codex" {
        if let Ok(models) = fetch_openai_codex_models() {
            if !models.is_empty() {
                return models;
            }
        }
    }
    known_models_for(agent)
}

#[derive(Debug, Deserialize)]
struct OpenAiModelsResponse {
    data: Vec<OpenAiModel>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModel {
    id: String,
}

fn fetch_openai_codex_models() -> Result<Vec<ModelOption>> {
    let api_key = std::env::var("OPENAI_API_KEY")
        .context("OPENAI_API_KEY is not set; falling back to local codex model list")?;
    let base_url = std::env::var("STAX_OPENAI_API_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com".to_string());
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));

    let runtime = tokio::runtime::Runtime::new()?;
    let response = runtime.block_on(async {
        reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(3))
            .timeout(std::time::Duration::from_secs(5))
            .build()?
            .get(&url)
            .bearer_auth(api_key)
            .send()
            .await?
            .error_for_status()?
            .json::<OpenAiModelsResponse>()
            .await
    })?;

    Ok(filter_live_codex_models(response.data))
}

fn filter_live_codex_models(models: Vec<OpenAiModel>) -> Vec<ModelOption> {
    let dated_snapshot = Regex::new(r"-\d{4}-\d{2}-\d{2}$").expect("valid regex");
    let mut model_ids: Vec<String> = models
        .into_iter()
        .map(|model| model.id)
        .filter(|id| is_codex_picker_candidate(id))
        .filter(|id| !id.contains("search-api"))
        .filter(|id| !id.ends_with("-chat-latest"))
        .filter(|id| !dated_snapshot.is_match(id))
        .collect();

    model_ids.sort_by(|a, b| compare_model_ids(a, b));
    model_ids.dedup();

    model_ids
        .into_iter()
        .map(|id| ModelOption {
            id,
            description: "live via OpenAI Models API".to_string(),
        })
        .collect()
}

fn compare_model_ids(left: &str, right: &str) -> std::cmp::Ordering {
    model_rank(right)
        .cmp(&model_rank(left))
        .then_with(|| left.cmp(right))
}

fn model_rank(model: &str) -> (i32, i32, i32, u8) {
    let version = parse_model_version(model);
    let family_rank = if model.contains("-nano") {
        0
    } else if model.contains("-mini") {
        1
    } else if model.contains("codex") {
        2
    } else if model.contains("-pro") {
        3
    } else {
        4
    };
    (version.0, version.1, version.2, family_rank)
}

fn parse_model_version(model: &str) -> (i32, i32, i32) {
    let version = model
        .trim_start_matches("gpt-")
        .split('-')
        .next()
        .unwrap_or_default();
    let mut parts = version.split('.');
    let major = parts
        .next()
        .and_then(|part| part.parse().ok())
        .unwrap_or_default();
    let minor = parts
        .next()
        .and_then(|part| part.parse().ok())
        .unwrap_or_default();
    let patch = parts
        .next()
        .and_then(|part| part.parse().ok())
        .unwrap_or_default();
    (major, minor, patch)
}

fn is_codex_picker_candidate(model: &str) -> bool {
    model.starts_with("gpt-5") || model.starts_with("gpt-4.1")
}

fn model_matches_agent_family(agent: &str, model: &str) -> bool {
    match agent {
        "claude" => model.starts_with("claude-"),
        "codex" => is_codex_picker_candidate(model) || model.starts_with("codex"),
        "gemini" => model.starts_with("gemini-"),
        "opencode" => model.starts_with("opencode/"),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Context collection
// ---------------------------------------------------------------------------

pub fn get_diff_stat(workdir: &Path, parent: &str, branch: &str) -> String {
    let output = Command::new("git")
        .args(["diff", "--stat", &format!("{}..{}", parent, branch)])
        .current_dir(workdir)
        .output();

    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => String::new(),
    }
}

pub fn get_full_diff(workdir: &Path, parent: &str, branch: &str) -> String {
    let output = Command::new("git")
        .args(["diff", &format!("{}..{}", parent, branch)])
        .current_dir(workdir)
        .output();

    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => String::new(),
    }
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

// ---------------------------------------------------------------------------
// Prompt construction
// ---------------------------------------------------------------------------

const MAX_DIFF_BYTES: usize = 80_000; // ~80KB limit to stay within context windows

pub fn build_ai_prompt(
    diff_stat: &str,
    diff: &str,
    commits: &[String],
    template: Option<&str>,
) -> String {
    let mut prompt = String::new();

    prompt.push_str("Generate a pull request description for the following changes.\n\n");

    if let Some(tmpl) = template {
        prompt.push_str(
            "Use this PR template as the structure. Fill in each section based on the changes:\n\n",
        );
        prompt.push_str(tmpl);
        prompt.push_str("\n\n");
    } else {
        prompt.push_str("Use a clear markdown format with a Summary section.\n\n");
    }

    if !commits.is_empty() {
        prompt.push_str("Commit messages:\n");
        for msg in commits {
            prompt.push_str(&format!("- {}\n", msg));
        }
        prompt.push('\n');
    }

    if !diff_stat.is_empty() {
        prompt.push_str("Diff stat (file-level summary):\n```\n");
        prompt.push_str(diff_stat);
        prompt.push_str("\n```\n\n");
    }

    if !diff.is_empty() {
        let truncated = if diff.len() > MAX_DIFF_BYTES {
            let safe = &diff[..MAX_DIFF_BYTES];
            // Cut at last newline to avoid splitting a line
            let cut = safe.rfind('\n').unwrap_or(MAX_DIFF_BYTES);
            format!(
                "{}\n\n... (diff truncated, showing first ~80KB of {} total) ...",
                &diff[..cut],
                format_bytes(diff.len())
            )
        } else {
            diff.to_string()
        };

        prompt.push_str("Full diff:\n```diff\n");
        prompt.push_str(&truncated);
        prompt.push_str("\n```\n\n");
    }

    prompt.push_str("Write only the PR body in markdown. Do not include any preamble, explanation, or wrapping code fences.");

    prompt
}

fn format_bytes(bytes: usize) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1}MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{}B", bytes)
    }
}

// ---------------------------------------------------------------------------
// AI agent invocation
// ---------------------------------------------------------------------------

pub fn invoke_ai_agent(agent: &str, model: Option<&str>, prompt: &str) -> Result<String> {
    let mut args: Vec<String> = Vec::new();
    let mut write_prompt_to_stdin = true;

    match agent {
        "claude" => {
            args.extend(["-p".into(), "--output-format".into(), "text".into()]);
            if let Some(m) = model {
                args.extend(["--model".into(), m.into()]);
            }
        }
        "codex" => {
            args.push("exec".into());
            if let Some(m) = model {
                args.extend(["--model".into(), m.into()]);
            }
        }
        "gemini" => {
            if let Some(m) = model {
                args.extend(["-m".into(), m.into()]);
            }
        }
        "opencode" => {
            args.push("run".into());
            if let Some(m) = model {
                args.extend(["--model".into(), m.into()]);
            }
            args.extend(["--format".into(), "default".into()]);
            args.push(prompt.to_string());
            write_prompt_to_stdin = false;
        }
        _ => bail!("Unsupported agent: {}", agent),
    }

    let mut child = Command::new(agent)
        .args(&args)
        .stdin(if write_prompt_to_stdin {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context(format!(
            "Failed to start '{}'. Is it installed and on your PATH?",
            agent
        ))?;

    if write_prompt_to_stdin {
        // Write prompt to stdin
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(prompt.as_bytes())
                .context("Failed to write prompt to AI agent stdin")?;
            // stdin is dropped here, closing the pipe
        }
    }

    let output = child
        .wait_with_output()
        .context("Failed to read AI agent output")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "AI agent '{}' exited with status {}:\n{}",
            agent,
            output.status,
            stderr.trim()
        );
    }

    let body = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_agent_name_accepts_gemini() {
        assert!(validate_agent_name("gemini").is_ok());
    }

    #[test]
    fn validate_agent_name_accepts_opencode() {
        assert!(validate_agent_name("opencode").is_ok());
    }

    #[test]
    fn known_models_include_gemini_defaults() {
        let models = known_models_for("gemini");
        assert!(models.iter().any(|m| m.id == "gemini-2.5-pro"));
        assert!(models.iter().any(|m| m.id == "gemini-2.5-flash"));
    }

    #[test]
    fn known_models_include_opencode_defaults() {
        let models = known_models_for("opencode");
        assert!(models.iter().any(|m| m.id == "opencode/gpt-5.1-codex"));
    }

    #[test]
    fn live_codex_model_filter_keeps_latest_aliases() {
        let filtered = filter_live_codex_models(vec![
            OpenAiModel {
                id: "gpt-5.4-2026-03-05".to_string(),
            },
            OpenAiModel {
                id: "gpt-5.4".to_string(),
            },
            OpenAiModel {
                id: "gpt-5.3-codex".to_string(),
            },
            OpenAiModel {
                id: "gpt-5-search-api".to_string(),
            },
            OpenAiModel {
                id: "gpt-5.2-chat-latest".to_string(),
            },
        ]);

        let ids: Vec<&str> = filtered.iter().map(|model| model.id.as_str()).collect();
        assert_eq!(ids.first().copied(), Some("gpt-5.4"));
        assert!(ids.contains(&"gpt-5.4"));
        assert!(ids.contains(&"gpt-5.3-codex"));
        assert!(!ids.contains(&"gpt-5.4-2026-03-05"));
        assert!(!ids.contains(&"gpt-5-search-api"));
        assert!(!ids.contains(&"gpt-5.2-chat-latest"));
    }

    #[test]
    fn resolve_model_ignores_known_model_from_other_agent() {
        let mut config = Config::default();
        config.ai.model = Some("gpt-5.3-codex".to_string());

        let resolved = resolve_model(None, &config, "gemini", "generate").unwrap();
        assert_eq!(resolved, None);
    }

    #[test]
    fn resolve_model_keeps_unknown_custom_model() {
        let mut config = Config::default();
        config.ai.model = Some("my-custom-model".to_string());

        let resolved = resolve_model(None, &config, "gemini", "generate").unwrap();
        assert_eq!(resolved, Some("my-custom-model".to_string()));
    }

    #[test]
    fn resolve_model_ignores_opencode_model_for_other_agent() {
        let mut config = Config::default();
        config.ai.model = Some("opencode/gpt-5.1-codex".to_string());

        let resolved = resolve_model(None, &config, "claude", "generate").unwrap();
        assert_eq!(resolved, None);
    }

    #[test]
    fn resolve_model_prefers_per_feature_over_global() {
        let mut config = Config::default();
        config.ai.model = Some("global-model".to_string());
        config.ai.generate.model = Some("generate-model".to_string());

        let resolved = resolve_model(None, &config, "claude", "generate").unwrap();
        assert_eq!(resolved, Some("generate-model".to_string()));
    }

    #[test]
    fn resolve_model_falls_back_to_global_when_feature_unset() {
        let mut config = Config::default();
        config.ai.model = Some("global-model".to_string());

        let resolved = resolve_model(None, &config, "claude", "generate").unwrap();
        assert_eq!(resolved, Some("global-model".to_string()));
    }

    #[test]
    fn known_agent_for_model_recognizes_newer_codex_family_models() {
        assert_eq!(known_agent_for_model("gpt-5.4"), Some("codex"));
        assert_eq!(known_agent_for_model("gpt-5.4-pro"), Some("codex"));
    }

    #[test]
    fn auto_detect_agent_uses_first_available_agent() {
        let available = vec!["codex".to_string(), "gemini".to_string()];

        let resolved = auto_detect_agent(&available).unwrap();

        assert_eq!(resolved, "codex");
    }

    #[test]
    fn auto_detect_agent_errors_when_none_available() {
        let err = auto_detect_agent(&[]).unwrap_err();

        assert!(err.to_string().contains("No AI agent found on PATH"));
    }

    // ---------------------------------------------------------------------------
    // resolve_agent — first-use prompt behaviour
    //
    // The interactive branch (no_prompt=false) calls dialoguer and cannot be
    // exercised in unit tests. The no_prompt=true path covers the non-interactive
    // fallback (scripts, pipes) and proves the resolution order is correct.
    // ---------------------------------------------------------------------------

    #[test]
    fn resolve_agent_cli_flag_overrides_all_config() {
        let mut config = Config::default();
        config.ai.agent = Some("codex".to_string());
        config.ai.generate.agent = Some("gemini".to_string());

        let result = resolve_agent(Some("claude"), &mut config, true).unwrap();
        assert_eq!(result, "claude");
    }

    #[test]
    fn resolve_agent_uses_per_feature_when_set_no_prompt() {
        let mut config = Config::default();
        config.ai.agent = Some("codex".to_string()); // global
        config.ai.generate.agent = Some("claude".to_string()); // per-feature

        let result = resolve_agent(None, &mut config, true).unwrap();
        assert_eq!(result, "claude");
    }

    #[test]
    fn resolve_agent_falls_back_to_global_when_no_feature_config_no_prompt() {
        // This is the scenario that was broken: global config exists, no per-feature
        // config. With no_prompt=true (non-interactive), we should use the global
        // agent silently instead of blocking on a prompt or erroring.
        let mut config = Config::default();
        config.ai.agent = Some("codex".to_string()); // global only, no [ai.generate]

        let result = resolve_agent(None, &mut config, true).unwrap();
        assert_eq!(result, "codex");
    }

    #[test]
    fn resolve_agent_per_feature_takes_priority_over_global_no_prompt() {
        // Even in no_prompt mode the per-feature slot wins over global.
        let mut config = Config::default();
        config.ai.agent = Some("gemini".to_string());
        config.ai.generate.agent = Some("opencode".to_string());

        let result = resolve_agent(None, &mut config, true).unwrap();
        assert_eq!(result, "opencode");
    }
}
