use crate::commands::generate;
use crate::config::Config;
use crate::engine::BranchMetadata;
use crate::git::repo::WorktreeInfo;
use crate::git::GitRepo;
use anyhow::{bail, Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, FuzzySelect};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_WORKTREE_ROOT_MARKER: &str = ".stax-repo-root";

/// Build a [`Command`] that runs a shell snippet on the current platform.
/// Uses `sh -c` on Unix and `cmd /C` on Windows.
pub(crate) fn platform_shell(command: &str) -> Command {
    if cfg!(target_os = "windows") {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", command]);
        cmd
    } else {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", command]);
        cmd
    }
}

const ADJECTIVES: &[&str] = &[
    "beaming", "bouncy", "brisk", "cheeky", "chirpy", "curious", "dapper", "fizzy", "fluffy",
    "giddy", "jolly", "lively", "loopy", "merry", "nimble", "peppy", "perky", "plucky", "puffy",
    "quirky", "snappy", "sparkly", "spicy", "spry", "sunny", "toasty", "wacky", "wiggly", "zesty",
    "zippy",
];

const NOUNS: &[&str] = &[
    "badger",
    "bagel",
    "banjo",
    "biscuit",
    "buffalo",
    "burrito",
    "capybara",
    "croissant",
    "dumpling",
    "falcon",
    "ferret",
    "gecko",
    "gherkin",
    "goblin",
    "kiwi",
    "lemur",
    "mango",
    "meerkat",
    "muffin",
    "narwhal",
    "otter",
    "pancake",
    "penguin",
    "pickle",
    "puffin",
    "raccoon",
    "scooter",
    "taco",
    "walrus",
    "waffle",
];

#[derive(Debug, Clone)]
pub struct WorktreeDetails {
    pub info: WorktreeInfo,
    pub branch_label: String,
    pub is_managed: bool,
    pub stack_parent: Option<String>,
    pub dirty: bool,
    pub rebase_in_progress: bool,
    pub merge_in_progress: bool,
    pub has_conflicts: bool,
    pub marker: Option<String>,
    pub ahead: Option<usize>,
    pub behind: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxSession {
    pub name: String,
    pub attached_clients: usize,
}

#[derive(Debug, Clone, Default)]
pub struct LaunchOptions {
    pub agent: Option<String>,
    pub model: Option<String>,
    pub run: Option<String>,
    pub tmux: bool,
    pub tmux_session: Option<String>,
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum LaunchSpec {
    Process {
        program: String,
        args: Vec<String>,
        display: String,
    },
    Shell {
        command: String,
        display: String,
    },
}

impl LaunchSpec {
    pub fn display(&self) -> &str {
        match self {
            Self::Process { display, .. } | Self::Shell { display, .. } => display,
        }
    }

    pub fn shell_command(&self) -> String {
        match self {
            Self::Process { program, args, .. } => {
                let mut parts = vec![shell_escape(program)];
                parts.extend(args.iter().map(|arg| shell_escape(arg)));
                parts.join(" ")
            }
            Self::Shell { command, .. } => command.clone(),
        }
    }

    pub fn execute_in(&self, cwd: &Path) -> Result<()> {
        match self {
            Self::Process { program, args, .. } => {
                let status = Command::new(program)
                    .args(args)
                    .current_dir(cwd)
                    .stdin(Stdio::inherit())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .status()
                    .with_context(|| format!("Failed to launch '{}'", program))?;
                if !status.success() {
                    bail!("'{}' exited with status {}", program, status);
                }
            }
            Self::Shell { command, .. } => {
                let status = platform_shell(command)
                    .current_dir(cwd)
                    .stdin(Stdio::inherit())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .status()
                    .with_context(|| format!("Failed to launch '{}'", command))?;
                if !status.success() {
                    bail!("Command exited with status {}", status);
                }
            }
        }
        Ok(())
    }
}

pub fn managed_worktrees_dir(repo: &GitRepo, config: &Config) -> Result<PathBuf> {
    let configured = config.worktree.root_dir.trim();
    if configured.is_empty() {
        return default_managed_worktrees_dir(repo);
    }

    let expanded = expand_home_path(configured)?;
    if expanded.is_absolute() {
        Ok(expanded)
    } else {
        Ok(repo.main_repo_workdir()?.join(expanded))
    }
}

pub fn ensure_managed_worktrees_root(
    repo: &GitRepo,
    config: &Config,
    worktrees_dir: &Path,
) -> Result<()> {
    if !config.worktree.root_dir.trim().is_empty() {
        return Ok(());
    }

    let repo_id = canonical_repo_identity(repo)?;
    let marker_path = worktrees_dir.join(DEFAULT_WORKTREE_ROOT_MARKER);
    fs::write(marker_path, format!("{}\n", repo_id))?;
    Ok(())
}

pub fn ensure_gitignore(repo_root: &Path, worktrees_dir: &str) -> Result<()> {
    if worktrees_dir.trim().is_empty() {
        return Ok(());
    }

    let expanded = expand_home_path(worktrees_dir)?;
    if expanded.is_absolute() {
        return Ok(());
    }

    let gitignore = repo_root.join(".gitignore");
    let entry = format!("{}/", worktrees_dir.trim_end_matches('/'));

    if gitignore.exists() {
        let content = fs::read_to_string(&gitignore)?;
        if content
            .lines()
            .any(|line| line.trim() == entry.trim_end_matches('/'))
            || content.lines().any(|line| line.trim() == entry)
        {
            return Ok(());
        }

        let updated = if content.ends_with('\n') {
            format!("{}{}\n", content, entry)
        } else {
            format!("{}\n{}\n", content, entry)
        };
        fs::write(&gitignore, updated)?;
    } else {
        fs::write(&gitignore, format!("{}\n", entry))?;
    }

    Ok(())
}

fn default_managed_worktrees_dir(repo: &GitRepo) -> Result<PathBuf> {
    let home =
        dirs::home_dir().context("Could not determine home directory for default worktree root")?;
    let base_dir = home.join(".stax").join("worktrees");
    let repo_root = repo.main_repo_workdir()?;
    let repo_name = repo_root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "repo".to_string());
    let repo_id = canonical_repo_identity(repo)?;

    let primary = base_dir.join(&repo_name);
    if repo_dir_available_for(&primary, &repo_id)? {
        return Ok(primary);
    }

    let suffix = short_stable_hash(&repo_id);
    let hashed = base_dir.join(format!("{}-{}", repo_name, suffix));
    if repo_dir_available_for(&hashed, &repo_id)? {
        return Ok(hashed);
    }

    for attempt in 2..=100 {
        let candidate = base_dir.join(format!("{}-{}-{}", repo_name, suffix, attempt));
        if repo_dir_available_for(&candidate, &repo_id)? {
            return Ok(candidate);
        }
    }

    bail!(
        "Could not derive a unique default worktree directory under '{}'",
        base_dir.display()
    )
}

fn canonical_repo_identity(repo: &GitRepo) -> Result<String> {
    let repo_root = repo.main_repo_workdir()?;
    let canonical = fs::canonicalize(&repo_root).unwrap_or_else(|_| repo_root.to_path_buf());
    Ok(canonical.to_string_lossy().into_owned())
}

fn repo_dir_available_for(path: &Path, repo_id: &str) -> Result<bool> {
    if !path.exists() {
        return Ok(true);
    }

    let marker_path = path.join(DEFAULT_WORKTREE_ROOT_MARKER);
    if marker_path.exists() {
        let existing = fs::read_to_string(marker_path)?;
        return Ok(existing.trim() == repo_id);
    }

    let mut entries = fs::read_dir(path)?;
    Ok(entries.next().is_none())
}

fn short_stable_hash(input: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:04x}", hash & 0xffff)
}

fn expand_home_path(path: &str) -> Result<PathBuf> {
    if path == "~" {
        return dirs::home_dir().context("Could not determine home directory for '~' expansion");
    }

    if let Some(suffix) = path.strip_prefix("~/") {
        return Ok(dirs::home_dir()
            .context("Could not determine home directory for '~' expansion")?
            .join(suffix));
    }

    Ok(PathBuf::from(path))
}

pub fn default_create_base(repo: &GitRepo) -> Result<String> {
    if let Ok(current) = repo.current_branch() {
        if BranchMetadata::read(repo.inner(), &current)?.is_some() {
            return Ok(current);
        }
    }

    repo.trunk_branch()
}

pub fn pick_branch_interactively(repo: &GitRepo) -> Result<String> {
    let branches = repo.list_branches()?;
    if branches.is_empty() {
        bail!("No local branches found.");
    }

    let current = repo.current_branch().unwrap_or_default();
    let default_idx = branches.iter().position(|b| b == &current).unwrap_or(0);
    let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Select branch for worktree")
        .items(&branches)
        .default(default_idx)
        .interact()?;

    Ok(branches[selection].clone())
}

pub fn pick_worktree_interactively(repo: &GitRepo) -> Result<WorktreeInfo> {
    let worktrees = repo.list_worktrees()?;
    if worktrees.is_empty() {
        bail!("No worktrees found.");
    }

    let items: Vec<String> = worktrees
        .iter()
        .map(|wt| {
            format!(
                "{} {:<18} {:<28} {}",
                if wt.is_current { "*" } else { " " },
                wt.name,
                wt.branch
                    .clone()
                    .unwrap_or_else(|| "(detached)".to_string()),
                wt.path.display()
            )
        })
        .collect();
    let default_idx = worktrees.iter().position(|wt| wt.is_current).unwrap_or(0);
    let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Select worktree")
        .items(&items)
        .default(default_idx)
        .interact()?;

    Ok(worktrees[selection].clone())
}

pub fn find_worktree(repo: &GitRepo, name: &str) -> Result<Option<WorktreeInfo>> {
    let matches: Vec<WorktreeInfo> = repo
        .list_worktrees()?
        .into_iter()
        .filter(|wt| worktree_matches(wt, name))
        .collect();

    match matches.len() {
        0 => Ok(None),
        1 => Ok(matches.into_iter().next()),
        _ => {
            let labels = matches
                .iter()
                .map(|wt| {
                    wt.branch
                        .clone()
                        .unwrap_or_else(|| wt.path.display().to_string())
                })
                .collect::<Vec<_>>()
                .join(", ");
            bail!("Multiple worktrees match '{}': {}", name, labels);
        }
    }
}

pub fn find_current_worktree(repo: &GitRepo) -> Result<WorktreeInfo> {
    repo.list_worktrees()?
        .into_iter()
        .find(|wt| wt.is_current)
        .context("Could not determine the current worktree")
}

pub fn worktree_matches(worktree: &WorktreeInfo, name: &str) -> bool {
    let target_path = Path::new(name);
    let path_match = if target_path.is_absolute() {
        std::fs::canonicalize(target_path)
            .map(|path| path == worktree.path)
            .unwrap_or(false)
    } else {
        false
    };

    path_match
        || worktree.name == name
        || worktree.branch.as_deref() == Some(name)
        || worktree
            .branch
            .as_deref()
            .map(|branch: &str| branch.ends_with(&format!("/{}", name)))
            .unwrap_or(false)
}

pub fn resolve_branch_name(repo: &GitRepo, config: &Config, input: &str) -> Result<(String, bool)> {
    let branches = repo.list_branches()?;

    if let Some(branch) = branches.iter().find(|branch| branch.as_str() == input) {
        return Ok((branch.clone(), true));
    }

    let suffix_matches: Vec<String> = branches
        .iter()
        .filter(|branch| branch.ends_with(&format!("/{}", input)))
        .cloned()
        .collect();
    if suffix_matches.len() == 1 {
        return Ok((suffix_matches[0].clone(), true));
    }
    if suffix_matches.len() > 1 {
        bail!(
            "Multiple branches match '{}': {}",
            input,
            suffix_matches.join(", ")
        );
    }

    let formatted = config.format_branch_name(input);
    let exists = branches.iter().any(|branch| branch == &formatted);
    Ok((formatted, exists))
}

pub fn derive_unique_worktree_name(repo: &GitRepo, branch: &str) -> Result<String> {
    let existing_names: HashSet<String> = repo
        .list_worktrees()?
        .into_iter()
        .map(|wt| wt.name)
        .collect();

    let segments: Vec<&str> = branch.split('/').collect();
    for start in (0..segments.len()).rev() {
        let candidate = segments[start..].join("-");
        if !existing_names.contains(&candidate) {
            return Ok(candidate);
        }
    }

    let full = branch.replace('/', "-");
    if !existing_names.contains(&full) {
        return Ok(full);
    }

    for i in 2..=99_u32 {
        let candidate = format!("{}-{}", full, i);
        if !existing_names.contains(&candidate) {
            return Ok(candidate);
        }
    }

    bail!("Could not derive a unique worktree name for '{}'", branch)
}

pub fn generate_random_lane_slug(repo: &GitRepo, config: &Config) -> Result<String> {
    let existing_names: HashSet<String> = repo
        .list_worktrees()?
        .into_iter()
        .map(|wt| wt.name)
        .collect();
    let existing_branches: HashSet<String> = repo.list_branches()?.into_iter().collect();

    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
        ^ u64::from(std::process::id());

    for attempt in 0..256_u64 {
        let adjective = ADJECTIVES[((seed.wrapping_add(attempt * 17)) as usize) % ADJECTIVES.len()];
        let noun =
            NOUNS[((seed.wrapping_mul(3).wrapping_add(attempt * 31)) as usize) % NOUNS.len()];
        let slug = format!("{}-{}", adjective, noun);
        let branch_name = config.format_branch_name(&slug);
        if !existing_names.contains(&slug) && !existing_branches.contains(&branch_name) {
            return Ok(slug);
        }
    }

    for suffix in 2..=999_u32 {
        let slug = format!("{}-{}-{}", ADJECTIVES[0], NOUNS[0], suffix);
        let branch_name = config.format_branch_name(&slug);
        if !existing_names.contains(&slug) && !existing_branches.contains(&branch_name) {
            return Ok(slug);
        }
    }

    bail!("Could not generate a unique random worktree name");
}

pub fn compute_worktree_details(repo: &GitRepo, worktree: WorktreeInfo) -> Result<WorktreeDetails> {
    let branch_label = worktree
        .branch
        .clone()
        .unwrap_or_else(|| "(detached)".to_string());

    let (is_managed, stack_parent, marker, ahead, behind) =
        if let Some(branch) = worktree.branch.as_deref() {
            let meta = BranchMetadata::read(repo.inner(), branch)?;
            let parent = meta.as_ref().map(|m| m.parent_branch_name.clone());
            let marker = repo.worktree_marker(branch);
            let fallback_trunk = repo.trunk_branch().ok();
            let diff_base = parent.as_deref().or(fallback_trunk.as_deref());
            let diff_pair = diff_base.and_then(|base| repo.commits_ahead_behind(base, branch).ok());

            (
                meta.is_some(),
                parent,
                marker,
                diff_pair.map(|(ahead, _)| ahead),
                diff_pair.map(|(_, behind)| behind),
            )
        } else {
            (false, None, None, None, None)
        };

    let path_available = worktree.path.exists() && !worktree.is_prunable;
    let dirty = if path_available {
        repo.is_dirty_at(&worktree.path)?
    } else {
        false
    };
    let rebase_in_progress = if path_available {
        repo.rebase_in_progress_in(&worktree.path)?
    } else {
        false
    };
    let merge_in_progress = if path_available {
        repo.merge_in_progress_in(&worktree.path)?
    } else {
        false
    };
    let has_conflicts = if path_available {
        repo.has_conflicts_in(&worktree.path)?
    } else {
        false
    };

    Ok(WorktreeDetails {
        dirty,
        rebase_in_progress,
        merge_in_progress,
        has_conflicts,
        info: worktree,
        branch_label,
        is_managed,
        stack_parent,
        marker,
        ahead,
        behind,
    })
}

pub fn build_launch_spec(
    config: &Config,
    options: &LaunchOptions,
    default_tmux_session: &str,
) -> Result<Option<LaunchSpec>> {
    if options.agent.is_some() && options.run.is_some() {
        bail!("--agent and --run cannot be used together");
    }
    if options.model.is_some() && options.agent.is_none() {
        bail!("--model requires --agent");
    }
    if options.tmux_session.is_some() && !options.tmux {
        bail!("--tmux-session requires --tmux");
    }

    let base_launch = if let Some(agent) = options.agent.as_deref() {
        generate::validate_agent_name(agent)?;
        let model = options
            .model
            .clone()
            .or_else(|| config.ai.model.clone())
            .filter(|value| !value.trim().is_empty());

        let mut args = Vec::new();
        match agent {
            "claude" => {
                if let Some(ref model) = model {
                    args.extend(["--model".to_string(), model.clone()]);
                }
            }
            "codex" => {
                if let Some(ref model) = model {
                    args.extend(["--model".to_string(), model.clone()]);
                }
            }
            "gemini" => {
                if let Some(ref model) = model {
                    args.extend(["-m".to_string(), model.clone()]);
                }
            }
            "opencode" => {
                if let Some(ref model) = model {
                    args.extend(["--model".to_string(), model.clone()]);
                }
            }
            _ => bail!("Unsupported AI agent: {}", agent),
        }
        args.extend(options.args.clone());

        let display = if let Some(model) = model {
            format!("{} ({})", agent, model)
        } else {
            agent.to_string()
        };

        Some(LaunchSpec::Process {
            program: agent.to_string(),
            args,
            display,
        })
    } else if let Some(command) = options.run.as_deref() {
        let full_command = if options.args.is_empty() {
            command.to_string()
        } else {
            let args = options
                .args
                .iter()
                .map(|arg| shell_escape(arg))
                .collect::<Vec<_>>()
                .join(" ");
            format!("{} {}", command, args)
        };
        Some(LaunchSpec::Shell {
            command: full_command.clone(),
            display: full_command,
        })
    } else {
        None
    };

    if options.tmux {
        ensure_tmux_available()?;
        let session = options
            .tmux_session
            .as_deref()
            .unwrap_or(default_tmux_session);
        return Ok(Some(build_tmux_launch_spec(session, base_launch.as_ref())?));
    }

    Ok(base_launch)
}

pub fn default_tmux_session_name(value: &str) -> Result<String> {
    let session = sanitize_tmux_session_name(value);
    if session.is_empty() {
        bail!("Could not derive a valid tmux session name");
    }
    Ok(session)
}

pub fn list_tmux_sessions() -> Result<Vec<TmuxSession>> {
    ensure_tmux_available()?;

    let output = Command::new("tmux")
        .args([
            "list-sessions",
            "-F",
            "#{session_name}\t#{session_attached}",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("Failed to list tmux sessions")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("no server running") || stderr.contains("failed to connect to server") {
            return Ok(Vec::new());
        }
        bail!("tmux list-sessions failed: {}", stderr.trim());
    }

    parse_tmux_sessions_output(&String::from_utf8_lossy(&output.stdout))
}

/// Encode the shell navigation payload as a single JSON line.
///
/// JSON encoding ensures that paths or launch commands containing spaces,
/// backslashes, or other shell-sensitive characters round-trip safely through
/// command-substitution in both POSIX and Fish shells.
pub fn format_shell_payload(path: &Path, launch: Option<&LaunchSpec>) -> String {
    let mut obj = serde_json::Map::new();
    obj.insert(
        "path".to_string(),
        Value::String(path.display().to_string()),
    );
    if let Some(l) = launch {
        obj.insert("launch".to_string(), Value::String(l.shell_command()));
    }
    Value::Object(obj).to_string()
}

pub fn emit_shell_payload(path: &Path, launch: Option<&LaunchSpec>) {
    println!("{}", format_shell_payload(path, launch));
}

pub fn status_labels(details: &WorktreeDetails) -> Vec<String> {
    let mut labels = Vec::new();

    if details.info.branch.is_none() {
        labels.push("detached".to_string());
    }
    if details.is_managed {
        labels.push("managed".to_string());
    }
    if details.dirty {
        labels.push("dirty".to_string());
    }
    if details.rebase_in_progress {
        labels.push("rebase".to_string());
    }
    if details.merge_in_progress {
        labels.push("merge".to_string());
    }
    if details.has_conflicts {
        labels.push("conflicts".to_string());
    }
    if details.info.is_locked {
        labels.push("locked".to_string());
    }
    if details.info.is_prunable {
        labels.push("prunable".to_string());
    }
    if let Some(marker) = &details.marker {
        labels.push(format!("marker:{}", marker));
    }

    if labels.is_empty() {
        labels.push("clean".to_string());
    }

    labels
}

pub fn worktree_to_json(details: &WorktreeDetails) -> Value {
    json!({
        "name": details.info.name,
        "branch": details.info.branch,
        "branch_label": details.branch_label,
        "path": details.info.path,
        "is_current": details.info.is_current,
        "is_main": details.info.is_main,
        "is_detached": details.info.branch.is_none(),
        "is_managed": details.is_managed,
        "dirty": details.dirty,
        "rebase_in_progress": details.rebase_in_progress,
        "merge_in_progress": details.merge_in_progress,
        "has_conflicts": details.has_conflicts,
        "is_locked": details.info.is_locked,
        "lock_reason": details.info.lock_reason,
        "is_prunable": details.info.is_prunable,
        "prunable_reason": details.info.prunable_reason,
        "marker": details.marker,
        "stack_parent": details.stack_parent,
        "ahead": details.ahead,
        "behind": details.behind,
        "status": status_labels(details),
    })
}

pub fn run_blocking_hook(command: Option<&str>, cwd: &Path, label: &str) -> Result<()> {
    let Some(command) = command.filter(|cmd| !cmd.trim().is_empty()) else {
        return Ok(());
    };

    let status = platform_shell(command)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("Failed to run {} hook", label))?;

    if !status.success() {
        bail!("{} hook failed with status {}", label, status);
    }

    Ok(())
}

pub fn spawn_background_hook(command: Option<&str>, cwd: &Path, label: &str) -> Result<()> {
    let Some(command) = command.filter(|cmd| !cmd.trim().is_empty()) else {
        return Ok(());
    };

    platform_shell(command)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("Failed to start {} hook", label))?;

    Ok(())
}

pub fn format_create_message(
    repo_name: &str,
    worktree_name: &str,
    branch_name: &str,
    from: &str,
    copied_files: usize,
    existing_branch: bool,
) {
    eprintln!(
        "{} {} {} {}",
        "You're in a new copy of".bold(),
        repo_name.cyan().bold(),
        "called".bold(),
        worktree_name.yellow().bold()
    );
    if existing_branch {
        eprintln!(
            "  {} {}",
            "Checked out existing branch".dimmed(),
            branch_name.blue()
        );
    } else {
        eprintln!(
            "  {} {} {} {}",
            "Branched".dimmed(),
            branch_name.blue(),
            "from".dimmed(),
            from.dimmed()
        );
    }
    eprintln!(
        "  {} {} {} {}",
        "Created".dimmed(),
        worktree_name.yellow(),
        "and copied".dimmed(),
        format!(
            "{} {}",
            copied_files,
            if copied_files == 1 { "file" } else { "files" }
        )
        .dimmed()
    );
}

pub fn format_go_message(worktree: &WorktreeInfo) {
    eprintln!(
        "{}  worktree '{}' ({})",
        "Opening".green().bold(),
        worktree.name.cyan(),
        worktree.branch.as_deref().unwrap_or("(detached)").dimmed()
    );
    eprintln!("  Path:   {}", worktree.path.display().to_string().dimmed());
}

pub fn shell_escape(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }

    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | ':' | '='))
    {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn ensure_tmux_available() -> Result<()> {
    let status = Command::new("tmux")
        .arg("-V")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    match status {
        Ok(status) if status.success() => Ok(()),
        _ => bail!("tmux is not installed or not available on PATH"),
    }
}

fn build_tmux_launch_spec(session_name: &str, inner: Option<&LaunchSpec>) -> Result<LaunchSpec> {
    let session = default_tmux_session_name(session_name)?;

    let session_escaped = shell_escape(&session);
    let new_session_cmd = if let Some(inner) = inner {
        format!(
            "tmux new-session -s {session} sh -c {command}",
            session = session_escaped,
            command = shell_escape(&inner.shell_command())
        )
    } else {
        format!("tmux new-session -s {}", session_escaped)
    };
    let new_session_detached_cmd = if let Some(inner) = inner {
        format!(
            "tmux new-session -d -s {session} sh -c {command}",
            session = session_escaped,
            command = shell_escape(&inner.shell_command())
        )
    } else {
        format!("tmux new-session -d -s {}", session_escaped)
    };

    let command = format!(
        "if tmux has-session -t {session} 2>/dev/null; then \
            if [ -n \"${{TMUX:-}}\" ]; then \
                exec tmux switch-client -t {session}; \
            else \
                exec tmux attach-session -t {session}; \
            fi; \
        else \
            if [ -n \"${{TMUX:-}}\" ]; then \
                {new_detached} || exit $?; \
                exec tmux switch-client -t {session}; \
            else \
                exec {new_attached}; \
            fi; \
        fi",
        session = session_escaped,
        new_detached = new_session_detached_cmd,
        new_attached = new_session_cmd,
    );

    let display = if let Some(inner) = inner {
        format!("tmux:{} -> {}", session, inner.display())
    } else {
        format!("tmux:{}", session)
    };

    Ok(LaunchSpec::Shell { command, display })
}

fn sanitize_tmux_session_name(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ':') {
                ch
            } else {
                '-'
            }
        })
        .collect();

    sanitized
        .trim_matches('-')
        .trim_matches(':')
        .trim_matches('_')
        .to_string()
}

fn parse_tmux_sessions_output(output: &str) -> Result<Vec<TmuxSession>> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (name, attached) = line
                .split_once('\t')
                .context("Unexpected tmux list-sessions output")?;
            Ok(TmuxSession {
                name: name.to_string(),
                attached_clients: attached.parse::<usize>().unwrap_or(0),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{default_tmux_session_name, format_shell_payload, parse_tmux_sessions_output};
    use crate::commands::worktree::shared::LaunchSpec;
    use std::path::Path;

    #[test]
    fn default_tmux_session_name_sanitizes_invalid_chars() {
        let session = default_tmux_session_name("review pass/main").expect("session name");
        assert_eq!(session, "review-pass-main");
    }

    #[test]
    fn parse_tmux_sessions_output_parses_attached_counts() {
        let sessions = parse_tmux_sessions_output("lane-a\t0\nlane-b\t2\n").expect("sessions");
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].name, "lane-a");
        assert_eq!(sessions[0].attached_clients, 0);
        assert_eq!(sessions[1].name, "lane-b");
        assert_eq!(sessions[1].attached_clients, 2);
    }

    #[test]
    fn format_shell_payload_path_only() {
        let path = Path::new("/home/user/my projects/feature");
        let json = format_shell_payload(path, None);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["path"], "/home/user/my projects/feature");
        assert!(parsed.get("launch").is_none());
    }

    #[test]
    fn format_shell_payload_with_launch_containing_spaces() {
        let path = Path::new("/repo/worktrees/feat");
        let launch = LaunchSpec::Shell {
            command: "codex --model gpt-5 --some-flag".to_string(),
            display: "codex".to_string(),
        };
        let json = format_shell_payload(path, Some(&launch));
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["path"], "/repo/worktrees/feat");
        assert_eq!(parsed["launch"], "codex --model gpt-5 --some-flag");
    }

    #[test]
    fn format_shell_payload_path_with_spaces_and_quotes() {
        let path = Path::new("/home/user/my projects/branch with \"quotes\"");
        let json = format_shell_payload(path, None);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(
            parsed["path"],
            "/home/user/my projects/branch with \"quotes\""
        );
    }
}
