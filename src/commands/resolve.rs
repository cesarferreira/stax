use crate::commands::{continue_cmd, generate};
use crate::config::Config;
use crate::git::{GitRepo, RebaseResult};
use anyhow::{bail, Context, Result};
use colored::Colorize;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;

#[derive(Debug, Deserialize)]
struct ResolveResponse {
    resolutions: Vec<FileResolution>,
}

#[derive(Debug, Deserialize)]
struct FileResolution {
    path: String,
    content: String,
}

pub fn run(
    agent_flag: Option<String>,
    model_flag: Option<String>,
    max_rounds: usize,
) -> Result<()> {
    if max_rounds == 0 {
        bail!("--max-rounds must be at least 1");
    }

    let repo = GitRepo::open()?;
    if !repo.rebase_in_progress()? {
        println!("{}", "No rebase in progress.".yellow());
        return Ok(());
    }

    let (agent, model) = resolve_agent_and_model(agent_flag, model_flag)?;
    println!(
        "Resolving rebase conflicts with {} (max rounds: {})",
        agent.cyan().bold(),
        max_rounds.to_string().cyan()
    );

    for round in 1..=max_rounds {
        let conflicted_files = repo.conflicted_files()?;
        if conflicted_files.is_empty() {
            bail!(
                "Rebase is in progress but no conflicted files were found. \
Run `stax continue` or inspect `git status`."
            );
        }

        println!(
            "  Round {}/{}: resolving {} conflicted file(s)",
            round.to_string().cyan(),
            max_rounds,
            conflicted_files.len().to_string().cyan()
        );

        let baseline_changes: HashSet<String> = repo.changed_files()?.into_iter().collect();
        let conflicted_contents = read_conflicted_files(&repo, &conflicted_files)?;
        let prompt = build_resolve_prompt(&conflicted_contents);
        let raw_response = generate::invoke_ai_agent(&agent, model.as_deref(), &prompt)?;
        let parsed = parse_agent_response(&raw_response)?;
        let resolutions = validate_resolutions(&conflicted_files, parsed.resolutions)?;
        apply_resolutions(&repo, &resolutions)?;
        enforce_conflicted_only_changes(&repo, &conflicted_files, &baseline_changes)?;
        repo.add_files(&conflicted_files)?;

        match continue_cmd::continue_rebase_and_update_metadata(&repo)? {
            RebaseResult::Success => {
                println!("{}", "✓ Rebase completed successfully!".green());
                return Ok(());
            }
            RebaseResult::Conflict => {
                if round < max_rounds {
                    println!("  {}", "More conflicts detected, continuing...".yellow());
                }
            }
        }
    }

    bail!(
        "Reached max rounds ({}) with unresolved conflicts. \
Resolve manually and run `stax continue`, or rerun `stax resolve --max-rounds <n>`.",
        max_rounds
    );
}

fn resolve_agent_and_model(
    agent_flag: Option<String>,
    model_flag: Option<String>,
) -> Result<(String, Option<String>)> {
    let config = Config::load()?;
    let agent = agent_flag
        .or(config.ai.agent)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .context(
            "No AI agent configured. Add [ai] agent = \"claude\" (or \"codex\" / \"gemini\" / \"opencode\") \
to ~/.config/stax/config.toml, or pass --agent <name>.",
        )?;
    generate::validate_agent_name(&agent)?;

    let model = model_flag
        .or(config.ai.model)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    Ok((agent, model))
}

fn read_conflicted_files(repo: &GitRepo, paths: &[String]) -> Result<Vec<(String, String)>> {
    let workdir = repo.workdir()?;
    let mut files = Vec::with_capacity(paths.len());

    for path in paths {
        let full_path = workdir.join(path);
        let bytes = fs::read(&full_path)
            .with_context(|| format!("Failed to read conflicted file '{}'", full_path.display()))?;
        let content = String::from_utf8(bytes).with_context(|| {
            format!(
                "Conflicted file '{}' is not UTF-8 text. \
This command currently supports text-file conflicts only.",
                path
            )
        })?;
        files.push((path.clone(), content));
    }

    Ok(files)
}

fn build_resolve_prompt(conflicts: &[(String, String)]) -> String {
    let mut prompt = String::new();
    prompt.push_str("Resolve the following Git rebase conflicts.\n");
    prompt.push_str("Return only a JSON object with this exact schema:\n");
    prompt.push_str("{\"resolutions\":[{\"path\":\"<path>\",\"content\":\"<full resolved file content>\"}]}\n\n");
    prompt.push_str("Rules:\n");
    prompt.push_str("- Include every conflicted file exactly once.\n");
    prompt.push_str("- Do not include any file that is not conflicted.\n");
    prompt
        .push_str("- `content` must be the complete final file text after conflict resolution.\n");
    prompt.push_str("- Output JSON only, with no markdown and no code fences.\n\n");
    prompt.push_str("Conflicted files:\n");
    for (path, content) in conflicts {
        prompt.push_str(&format!("\nFILE: {}\n", path));
        prompt.push_str("----- BEGIN CONTENT -----\n");
        prompt.push_str(content);
        if !content.ends_with('\n') {
            prompt.push('\n');
        }
        prompt.push_str("----- END CONTENT -----\n");
    }
    prompt
}

fn parse_agent_response(raw: &str) -> Result<ResolveResponse> {
    let cleaned = strip_markdown_fences(raw);
    serde_json::from_str(&cleaned).with_context(|| {
        format!(
            "AI response is not valid JSON in the expected schema.\nResponse:\n{}",
            raw.trim()
        )
    })
}

fn strip_markdown_fences(raw: &str) -> String {
    let trimmed = raw.trim();
    if !trimmed.starts_with("```") {
        return trimmed.to_string();
    }

    let after_open = trimmed.trim_start_matches('`');
    let Some(newline_idx) = after_open.find('\n') else {
        return trimmed.to_string();
    };
    let body = &after_open[newline_idx + 1..];
    let Some(end_idx) = body.rfind("```") else {
        return trimmed.to_string();
    };

    body[..end_idx].trim().to_string()
}

fn validate_resolutions(
    conflicted_files: &[String],
    resolutions: Vec<FileResolution>,
) -> Result<HashMap<String, String>> {
    let expected: HashSet<String> = conflicted_files.iter().cloned().collect();
    let mut seen: HashSet<String> = HashSet::new();
    let mut resolved: HashMap<String, String> = HashMap::new();

    for entry in resolutions {
        let path = entry.path.trim().to_string();
        if path.is_empty() {
            bail!("AI response contained an empty `path` field.");
        }
        if !expected.contains(&path) {
            bail!(
                "AI response included non-conflicted file '{}'. \
Only currently conflicted files are allowed.",
                path
            );
        }
        if !seen.insert(path.clone()) {
            bail!("AI response contains duplicate resolution for '{}'.", path);
        }
        resolved.insert(path, entry.content);
    }

    if resolved.is_empty() {
        bail!("AI response did not include any file resolutions.");
    }

    let mut missing: Vec<String> = expected
        .iter()
        .filter(|path| !resolved.contains_key(*path))
        .cloned()
        .collect();
    missing.sort();
    if !missing.is_empty() {
        bail!(
            "AI response is missing conflicted file(s): {}",
            missing.join(", ")
        );
    }

    Ok(resolved)
}

fn apply_resolutions(repo: &GitRepo, resolutions: &HashMap<String, String>) -> Result<()> {
    let workdir = repo.workdir()?;
    for (path, content) in resolutions {
        let full_path = workdir.join(path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Failed to create parent directory for '{}'",
                    full_path.display()
                )
            })?;
        }
        fs::write(&full_path, content)
            .with_context(|| format!("Failed to write resolved file '{}'", full_path.display()))?;
    }
    Ok(())
}

fn enforce_conflicted_only_changes(
    repo: &GitRepo,
    conflicted_files: &[String],
    baseline_changes: &HashSet<String>,
) -> Result<()> {
    let allowed: HashSet<String> = conflicted_files.iter().cloned().collect();
    let mut unexpected: Vec<String> = repo
        .changed_files()?
        .into_iter()
        .filter(|path| !baseline_changes.contains(path))
        .filter(|path| !allowed.contains(path))
        .collect();

    unexpected.sort();
    if !unexpected.is_empty() {
        bail!(
            "Detected edits outside conflicted files: {}. \
Only conflicted files may be changed during `stax resolve`.",
            unexpected.join(", ")
        );
    }

    Ok(())
}
