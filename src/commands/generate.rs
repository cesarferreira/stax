use crate::config::Config;
use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::github::pr_template::discover_pr_templates;
use crate::github::GitHubClient;
use crate::remote;
use anyhow::{bail, Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm, Editor, Select};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

// ---------------------------------------------------------------------------
// Known models per agent (for validation and interactive picker)
// ---------------------------------------------------------------------------

const CLAUDE_MODELS: &[(&str, &str)] = &[
    (
        "claude-sonnet-4-5-20250929",
        "Sonnet 4.5 (default, balanced)",
    ),
    ("claude-haiku-4-5-20251001", "Haiku 4.5 (fastest, cheapest)"),
    ("claude-opus-4-6", "Opus 4.6 (most capable)"),
    ("claude-sonnet-4-20250514", "Sonnet 4"),
];

const CODEX_MODELS: &[(&str, &str)] = &[
    ("gpt-5.3-codex", "GPT-5.3 Codex (default)"),
    ("gpt-5.3-codex-spark", "GPT-5.3 Codex Spark (Pro)"),
    ("gpt-4.1-mini", "GPT-4.1 Mini"),
];

const GEMINI_MODELS: &[(&str, &str)] = &[
    ("gemini-2.5-pro", "Gemini 2.5 Pro (default)"),
    ("gemini-2.5-flash", "Gemini 2.5 Flash (faster, cheaper)"),
];

const OPENCODE_MODELS: &[(&str, &str)] = &[(
    "opencode/gpt-5.1-codex",
    "GPT-5.1 Codex via OpenCode (default)",
)];

const SUPPORTED_AGENTS: &[&str] = &["claude", "codex", "gemini", "opencode"];

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(edit: bool, agent_flag: Option<String>, model_flag: Option<String>) -> Result<()> {
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
    let agent = resolve_agent(agent_flag.as_deref(), &mut config)?;
    let model = resolve_model(model_flag.as_deref(), &config, &agent)?;

    // Collect context for the prompt
    println!("{}", "Collecting context...".dimmed());
    let diff_stat = get_diff_stat(&workdir, parent, &current_branch);
    let diff = get_full_diff(&workdir, parent, &current_branch);
    let commits = collect_commit_messages(&workdir, parent, &current_branch);
    let templates = discover_pr_templates(&workdir).unwrap_or_default();
    let template_content = templates.first().map(|t| t.content.as_str());

    if diff.trim().is_empty() && commits.is_empty() {
        bail!("No changes found between {} and {}", parent, current_branch);
    }

    // Build the AI prompt
    let prompt = build_ai_prompt(&diff_stat, &diff, &commits, template_content);

    // Invoke AI agent
    let model_display = model.as_deref().unwrap_or("default");
    println!(
        "  {} {} (model: {})...",
        "Generating PR body with".dimmed(),
        agent.cyan().bold(),
        model_display.dimmed()
    );

    let generated_body = invoke_ai_agent(&agent, model.as_deref(), &prompt)?;

    if generated_body.trim().is_empty() {
        bail!("AI agent returned an empty response");
    }

    // Let user review/edit the generated body
    let final_body = if edit {
        Editor::new()
            .edit(&generated_body)?
            .unwrap_or(generated_body)
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

    // Update the PR body on GitHub
    print!("  Updating PR #{} body... ", pr_number.to_string().cyan());
    std::io::stdout().flush().ok();

    let remote_info = remote::RemoteInfo::from_repo(&repo, &config)?;
    let owner = remote_info.owner().to_string();
    let repo_name = remote_info.repo.clone();

    let runtime = tokio::runtime::Runtime::new()?;
    let client = runtime.block_on(async {
        GitHubClient::new(&owner, &repo_name, remote_info.api_base_url.clone())
    })?;

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

fn resolve_agent(cli_flag: Option<&str>, config: &mut Config) -> Result<String> {
    // 1. CLI flag takes priority
    if let Some(agent) = cli_flag {
        validate_agent_name(agent)?;
        return Ok(agent.to_string());
    }

    // 2. Config value
    if let Some(ref agent) = config.ai.agent {
        if !agent.is_empty() {
            return Ok(agent.clone());
        }
    }

    // 3. Auto-detect from PATH
    let available = detect_available_agents();

    match available.len() {
        0 => {
            bail!(
                "No AI agent found on PATH.\n  \
                 Install one of:\n    \
                 - claude (https://docs.anthropic.com)\n    \
                 - codex  (https://github.com/openai/codex)\n  \
                 - gemini (https://github.com/google-gemini/gemini-cli)\n  \
                 - opencode (https://opencode.ai)\n  \
                 Or set manually in ~/.config/stax/config.toml:\n    \
                 [ai]\n    \
                 agent = \"claude\""
            );
        }
        1 => {
            let agent = available[0].clone();
            println!(
                "  {} {}",
                "Detected AI agent:".dimmed(),
                agent.cyan().bold()
            );

            // Still show model picker, then offer to save
            let model = pick_model_interactive(&agent)?;
            let save = Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt("Save choices to config?")
                .default(true)
                .interact()?;

            if save {
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

            Ok(agent)
        }
        _ => {
            // Multiple agents: show picker
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

            // Show model picker
            let model = pick_model_interactive(&agent)?;

            let save = Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt("Save choices to config?")
                .default(true)
                .interact()?;

            if save {
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

            Ok(agent)
        }
    }
}

fn validate_agent_name(agent: &str) -> Result<()> {
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

fn resolve_model(cli_flag: Option<&str>, config: &Config, agent: &str) -> Result<Option<String>> {
    // 1. CLI flag takes priority
    if let Some(model) = cli_flag {
        validate_model_soft(agent, model);
        return Ok(Some(model.to_string()));
    }

    // 2. Config value
    if let Some(ref model) = config.ai.model {
        if !model.is_empty() {
            // If config model is a known model for a different agent, ignore it and
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
            return Ok(Some(model.clone()));
        }
    }

    // 3. No model specified — let agent use its own default
    Ok(None)
}

fn pick_model_interactive(agent: &str) -> Result<Option<String>> {
    let models = known_models_for(agent);
    if models.is_empty() {
        return Ok(None);
    }

    let items: Vec<String> = models
        .iter()
        .map(|(id, desc)| format!("{} — {}", id, desc))
        .collect();

    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(format!("Select model for {}", agent))
        .items(&items)
        .default(0)
        .interact()?;

    Ok(Some(models[selection].0.to_string()))
}

fn validate_model_soft(agent: &str, model: &str) {
    let models = known_models_for(agent);
    if !models.is_empty() && !models.iter().any(|(id, _)| *id == model) {
        eprintln!(
            "  {} Unknown model '{}' for agent '{}', proceeding anyway...",
            "⚠".yellow(),
            model.yellow(),
            agent
        );
    }
}

fn known_models_for(agent: &str) -> &'static [(&'static str, &'static str)] {
    match agent {
        "claude" => CLAUDE_MODELS,
        "codex" => CODEX_MODELS,
        "gemini" => GEMINI_MODELS,
        "opencode" => OPENCODE_MODELS,
        _ => &[],
    }
}

fn known_agent_for_model(model: &str) -> Option<&'static str> {
    ["claude", "codex", "gemini", "opencode"]
        .into_iter()
        .find(|agent| known_models_for(agent).iter().any(|(id, _)| *id == model))
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
        assert!(models.iter().any(|(id, _)| *id == "gemini-2.5-pro"));
        assert!(models.iter().any(|(id, _)| *id == "gemini-2.5-flash"));
    }

    #[test]
    fn known_models_include_opencode_defaults() {
        let models = known_models_for("opencode");
        assert!(models.iter().any(|(id, _)| *id == "opencode/gpt-5.1-codex"));
    }

    #[test]
    fn resolve_model_ignores_known_model_from_other_agent() {
        let mut config = Config::default();
        config.ai.model = Some("gpt-5.3-codex".to_string());

        let resolved = resolve_model(None, &config, "gemini").unwrap();
        assert_eq!(resolved, None);
    }

    #[test]
    fn resolve_model_keeps_unknown_custom_model() {
        let mut config = Config::default();
        config.ai.model = Some("my-custom-model".to_string());

        let resolved = resolve_model(None, &config, "gemini").unwrap();
        assert_eq!(resolved, Some("my-custom-model".to_string()));
    }

    #[test]
    fn resolve_model_ignores_opencode_model_for_other_agent() {
        let mut config = Config::default();
        config.ai.model = Some("opencode/gpt-5.1-codex".to_string());

        let resolved = resolve_model(None, &config, "claude").unwrap();
        assert_eq!(resolved, None);
    }
}
