use anyhow::{Context, Result, bail};
use colored::Colorize;
use std::path::PathBuf;

const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");
const REMOTE_URL: &str = "https://raw.githubusercontent.com/cesarferreira/stax/main/skills.md";
/// Skill body shipped with this binary (same source as `skills update` writes into harness files).
const BUNDLED_SKILL_BODY: &str = include_str!("../../skills.md");

/// Known agent skill file locations (relative to `$HOME`).
pub struct SkillLocation {
    /// Stable slug for `--skills` and config (`claude`, `codex`, …).
    pub id: &'static str,
    /// Display name shown in output.
    pub name: &'static str,
    /// Path relative to the user's home directory.
    relative_path: &'static str,
    /// Harness root under `$HOME`; existence means the agent is likely installed.
    detect_relative_path: &'static str,
    /// Whether this file uses YAML frontmatter (SKILL.md format).
    has_frontmatter: bool,
}

const SKILL_LOCATIONS: &[SkillLocation] = &[
    SkillLocation {
        id: "codex",
        name: "Codex",
        relative_path: ".codex/skills/stax/SKILL.md",
        detect_relative_path: ".codex",
        has_frontmatter: true,
    },
    SkillLocation {
        id: "opencode",
        name: "OpenCode",
        relative_path: ".config/opencode/skills/stax/SKILL.md",
        detect_relative_path: ".config/opencode",
        has_frontmatter: true,
    },
    SkillLocation {
        id: "claude",
        name: "Claude Code (global)",
        relative_path: ".claude/skills/stax/SKILL.md",
        detect_relative_path: ".claude",
        has_frontmatter: true,
    },
    SkillLocation {
        id: "cursor",
        name: "Cursor",
        relative_path: ".cursor/skills/stax/SKILL.md",
        detect_relative_path: ".cursor",
        has_frontmatter: true,
    },
    SkillLocation {
        id: "pi",
        name: "pi",
        relative_path: ".pi/agent/skills/stax/SKILL.md",
        detect_relative_path: ".pi",
        has_frontmatter: true,
    },
];

/// Which harnesses receive skill installs/updates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HarnessSelection {
    All,
    Detected,
    /// Detected on disk plus any harness that already has a skill file.
    Auto,
    Only(Vec<String>),
}

/// How the harness set was chosen for a `skills update` run (shown in CLI output).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SkillsUpdateOrigin {
    AllFlag,
    Cli { spec: String },
    Config,
    Auto,
    Setup,
}

pub struct HarnessInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub detected: bool,
}

pub fn harnesses() -> Vec<HarnessInfo> {
    SKILL_LOCATIONS
        .iter()
        .map(|loc| HarnessInfo {
            id: loc.id,
            name: loc.name,
            detected: is_detected(loc),
        })
        .collect()
}

fn is_detected(loc: &SkillLocation) -> bool {
    dirs::home_dir()
        .map(|h| h.join(loc.detect_relative_path).is_dir())
        .unwrap_or(false)
}

pub fn parse_harness_selection(spec: &str) -> Result<HarnessSelection> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        bail!("--skills requires a value (all, detected, auto, none, or comma-separated ids)");
    }

    let lower = trimmed.to_ascii_lowercase();
    match lower.as_str() {
        "all" => return Ok(HarnessSelection::All),
        "detected" => return Ok(HarnessSelection::Detected),
        "auto" => return Ok(HarnessSelection::Auto),
        "none" => return Ok(HarnessSelection::Only(vec![])),
        _ => {}
    }

    let parts: Vec<&str> = trimmed
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    if parts.is_empty() {
        bail!("--skills requires at least one harness id");
    }

    for part in &parts {
        let id = part.to_ascii_lowercase();
        if matches!(id.as_str(), "all" | "detected" | "auto" | "none") {
            bail!("keyword `{part}` cannot be combined with other harness ids");
        }
    }

    let valid_ids: Vec<&str> = SKILL_LOCATIONS.iter().map(|l| l.id).collect();
    let mut chosen = Vec::new();
    for part in parts {
        let id = part.to_ascii_lowercase();
        if !valid_ids.contains(&id.as_str()) {
            bail!(
                "unknown harness `{part}` — valid ids: {}",
                valid_ids.join(", ")
            );
        }
        if !chosen.iter().any(|existing: &String| existing == &id) {
            chosen.push(id);
        }
    }

    // Preserve canonical SKILL_LOCATIONS order.
    let ordered: Vec<String> = SKILL_LOCATIONS
        .iter()
        .filter(|loc| chosen.iter().any(|id| id == loc.id))
        .map(|loc| loc.id.to_string())
        .collect();

    Ok(HarnessSelection::Only(ordered))
}

pub fn resolve_locations(sel: &HarnessSelection) -> Vec<&'static SkillLocation> {
    match sel {
        HarnessSelection::All => SKILL_LOCATIONS.iter().collect(),
        HarnessSelection::Detected => SKILL_LOCATIONS
            .iter()
            .filter(|loc| is_detected(loc))
            .collect(),
        HarnessSelection::Auto => SKILL_LOCATIONS
            .iter()
            .filter(|loc| is_detected(loc) || skill_path(loc).map(|p| p.exists()).unwrap_or(false))
            .collect(),
        HarnessSelection::Only(ids) => SKILL_LOCATIONS
            .iter()
            .filter(|loc| ids.iter().any(|id| id == loc.id))
            .collect(),
    }
}

pub fn resolve_ids(sel: &HarnessSelection) -> Vec<String> {
    resolve_locations(sel)
        .iter()
        .map(|loc| loc.id.to_string())
        .collect()
}

pub fn configured_selection() -> HarnessSelection {
    configured_selection_with_origin().0
}

pub fn configured_selection_with_origin() -> (HarnessSelection, SkillsUpdateOrigin) {
    match crate::config::Config::load() {
        Ok(config) => match config.skills.harnesses {
            Some(ids) => (HarnessSelection::Only(ids), SkillsUpdateOrigin::Config),
            None => (HarnessSelection::Auto, SkillsUpdateOrigin::Auto),
        },
        Err(_) => (HarnessSelection::Auto, SkillsUpdateOrigin::Auto),
    }
}

fn format_update_command_line(dry_run: bool, origin: &SkillsUpdateOrigin) -> String {
    let mut parts = vec!["stax skills update".to_string()];
    if dry_run {
        parts.push("--dry-run".to_string());
    }
    match origin {
        SkillsUpdateOrigin::AllFlag => parts.push("--all".to_string()),
        SkillsUpdateOrigin::Cli { spec } => parts.push(format!("--skills {spec}")),
        SkillsUpdateOrigin::Config | SkillsUpdateOrigin::Auto | SkillsUpdateOrigin::Setup => {}
    }
    parts.join(" ")
}

fn format_harness_selection_summary(ids: &[String], origin: &SkillsUpdateOrigin) -> String {
    let id_list = ids.join(", ");
    let source = match origin {
        SkillsUpdateOrigin::AllFlag => "all known harnesses (--all)".to_string(),
        SkillsUpdateOrigin::Cli { spec } => format!("--skills {spec}"),
        SkillsUpdateOrigin::Config => "config [skills] harnesses".to_string(),
        SkillsUpdateOrigin::Auto => {
            "auto: detected on disk or existing skill file (set [skills] harnesses or run st setup to narrow)"
                .to_string()
        }
        SkillsUpdateOrigin::Setup => "st setup selection".to_string(),
    };
    format!("Harnesses ({source}): {id_list}")
}

/// Parse `<!-- stax-skills-version: X.Y.Z -->` or `stax_version: "X.Y.Z"` from the
/// first 40 lines of a skill file's content.
pub fn extract_skills_version(content: &str) -> Option<String> {
    for line in content.lines().take(40) {
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix("<!-- stax-skills-version:") {
            let v = rest.trim_end_matches("-->").trim().to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }

        if let Some(rest) = trimmed.strip_prefix("stax_version:") {
            let v = rest.trim().trim_matches('"').trim_matches('\'').to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

/// Build the full path for a skill location from `$HOME`.
fn skill_path(loc: &SkillLocation) -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(loc.relative_path))
}

/// Generate the content to write for a given location given the remote body.
///
/// For `has_frontmatter = true` files we prepend a minimal YAML front-matter so
/// agent skill runners can load them.  For plain markdown files we write the body
/// as-is (it already contains the `<!-- stax-skills-version: … -->` marker).
/// Full agent skill document for the canonical SKILL.md format (YAML frontmatter + body).
pub fn bundled_agent_skill_markdown() -> String {
    build_content(BUNDLED_SKILL_BODY, &SKILL_LOCATIONS[0])
}

/// Print the bundled agent skill to stdout (for `st --skill`).
pub fn run_print_skill() -> Result<()> {
    print!("{}", bundled_agent_skill_markdown());
    Ok(())
}

fn build_content(body: &str, loc: &SkillLocation) -> String {
    if loc.has_frontmatter {
        format!(
            "---\nname: stax\ndescription: Use when the user wants to create, submit, sync, restack, navigate, or merge stacked Git branches or PRs, or asks about stax commands, flags, or workflows. Covers all stax commands and best practices for AI coding agents.\nstax_version: \"{PKG_VERSION}\"\nmetadata:\n  short-description: Stax stacked-branch and PR management commands\n---\n\n{body}",
        )
    } else {
        body.to_string()
    }
}

/// Download the latest `skills.md` from GitHub and return its content.
fn fetch_remote_skills() -> Result<String> {
    let remote_url = std::env::var("STAX_SKILLS_URL").unwrap_or_else(|_| REMOTE_URL.to_string());
    let runtime = tokio::runtime::Runtime::new()?;
    let body = runtime
        .block_on(async {
            reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(5))
                .timeout(std::time::Duration::from_secs(10))
                .build()?
                .get(remote_url)
                .send()
                .await?
                .error_for_status()?
                .text()
                .await
        })
        .context("Failed to download skills from GitHub")?;
    Ok(body)
}

fn location_selected(loc: &SkillLocation, sel: &HarnessSelection) -> bool {
    resolve_locations(sel)
        .iter()
        .any(|selected| selected.id == loc.id)
}

pub fn run_list() -> Result<()> {
    println!("{}", "stax skills".bold());
    println!();

    let selection = configured_selection();
    let mut any_found = false;

    for loc in SKILL_LOCATIONS {
        let Some(path) = skill_path(loc) else {
            continue;
        };

        let selected = location_selected(loc, &selection);
        let not_selected_suffix = if selected { "" } else { "  not selected" };

        match std::fs::read_to_string(&path) {
            Ok(content) => {
                any_found = true;
                let installed = extract_skills_version(&content);
                let label = loc.name.cyan();
                let path_str = path.display().to_string().dimmed();

                match &installed {
                    Some(v) if v == PKG_VERSION => {
                        println!(
                            "{}  {} {}  {}{}",
                            "✓".green(),
                            label,
                            format!("(v{v})").dimmed(),
                            path_str,
                            not_selected_suffix.dimmed(),
                        );
                    }
                    Some(v) => {
                        println!(
                            "{}  {} {}  {}{}",
                            "⚠".yellow(),
                            label,
                            format!("(v{v} → v{PKG_VERSION} available)").yellow(),
                            path_str,
                            not_selected_suffix.dimmed(),
                        );
                    }
                    None => {
                        println!(
                            "{}  {} {}  {}{}",
                            "⚠".yellow(),
                            label,
                            "(no version marker — may be out of date)".yellow(),
                            path_str,
                            not_selected_suffix.dimmed(),
                        );
                    }
                }
            }
            Err(_) => {
                let path_str = path.display().to_string().dimmed();
                let detected = if is_detected(loc) {
                    " (detected)".dimmed().to_string()
                } else {
                    String::new()
                };
                println!(
                    "{}  {}{}  {}{}",
                    "–".dimmed(),
                    loc.name.dimmed(),
                    detected,
                    path_str,
                    not_selected_suffix.dimmed(),
                );
            }
        }
    }

    println!();
    if !any_found {
        println!(
            "{}",
            "No skill files found. Run `stax skills update` to install them.".yellow()
        );
    } else {
        println!(
            "Run {} to bring selected skill files up to date.",
            "`stax skills update`".cyan()
        );
    }

    Ok(())
}

pub fn run_update(dry_run: bool) -> Result<()> {
    let (sel, origin) = configured_selection_with_origin();
    run_update_with(dry_run, &sel, origin)
}

pub fn run_update_with(
    dry_run: bool,
    sel: &HarnessSelection,
    origin: SkillsUpdateOrigin,
) -> Result<()> {
    let locations = resolve_locations(sel);
    if locations.is_empty() {
        println!(
            "{}",
            "No agent harnesses selected — nothing to update. Run `stax skills update --all` or re-run `stax setup`.".dimmed()
        );
        return Ok(());
    }

    let harness_ids: Vec<String> = locations.iter().map(|loc| loc.id.to_string()).collect();

    println!("{}", format_update_command_line(dry_run, &origin).bold());
    println!(
        "{}",
        format_harness_selection_summary(&harness_ids, &origin).dimmed()
    );
    println!();

    println!("Fetching latest skills from GitHub…");
    let remote_body = fetch_remote_skills()?;

    let remote_body_version = extract_skills_version(&remote_body);

    println!("Target version: {}", format!("v{PKG_VERSION}").green());
    if let Some(v) = remote_body_version.as_deref().filter(|v| *v != PKG_VERSION) {
        println!(
            "{}",
            format!(
                "(remote skills.md marker is v{v} — informational only, does not affect updates)"
            )
            .dimmed(),
        );
    }
    println!();

    let mut updated = 0usize;
    let mut skipped = 0usize;

    for loc in locations {
        let Some(path) = skill_path(loc) else {
            continue;
        };

        let installed_version = std::fs::read_to_string(&path)
            .ok()
            .and_then(|c| extract_skills_version(&c));

        let needs_update = installed_version
            .as_deref()
            .map(|v| v != PKG_VERSION)
            .unwrap_or(true);

        let file_exists = path.exists();

        if !needs_update && file_exists {
            println!(
                "{}  {} {}",
                "✓".green(),
                loc.name.cyan(),
                "already up to date".dimmed(),
            );
            skipped += 1;
            continue;
        }

        let action = if file_exists { "update" } else { "install" };
        let content = build_content(&remote_body, loc);

        if dry_run {
            println!(
                "{}  {} {}",
                "→".cyan(),
                loc.name.cyan(),
                format!("would {action}: {}", path.display()).dimmed(),
            );
        } else {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create directory {}", parent.display()))?;
            }
            std::fs::write(&path, &content)
                .with_context(|| format!("Failed to write {}", path.display()))?;

            println!(
                "{}  {} {}",
                "✓".green(),
                loc.name.cyan(),
                format!("{action}d: {}", path.display()).dimmed(),
            );
            updated += 1;
        }
    }

    println!();
    if dry_run {
        println!("{}", "Dry run complete — no files were written.".dimmed());
    } else if updated == 0 {
        println!("{}", "All skill files are already up to date.".green());
    } else {
        println!(
            "{}",
            format!(
                "Updated {} skill file(s){}.",
                updated,
                if skipped > 0 {
                    format!(", {skipped} already current")
                } else {
                    String::new()
                }
            )
            .green()
        );
    }

    Ok(())
}

/// Check installed skill files and return a list of (name, installed_version) pairs
/// that are out of date relative to PKG_VERSION.  Used by `stax doctor`.
pub fn stale_skill_files() -> Vec<(String, Option<String>)> {
    let mut stale = Vec::new();
    let selection = configured_selection();

    for loc in resolve_locations(&selection) {
        let Some(path) = skill_path(loc) else {
            continue;
        };
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };

        let installed = extract_skills_version(&content);
        let is_current = installed.as_deref() == Some(PKG_VERSION);

        if !is_current {
            stale.push((loc.name.to_string(), installed));
        }
    }

    stale
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_html_comment_version() {
        let content = "<!-- stax-skills-version: 1.2.3 -->\n# Stax Skills\n";
        assert_eq!(extract_skills_version(content), Some("1.2.3".to_string()));
    }

    #[test]
    fn test_extract_yaml_frontmatter_version() {
        let content = "---\nname: stax\nstax_version: \"0.50.2\"\n---\n# Stax\n";
        assert_eq!(extract_skills_version(content), Some("0.50.2".to_string()));
    }

    #[test]
    fn test_extract_yaml_single_quotes() {
        let content = "---\nstax_version: '1.0.0'\n---\n";
        assert_eq!(extract_skills_version(content), Some("1.0.0".to_string()));
    }

    #[test]
    fn test_extract_missing_returns_none() {
        let content = "# Stax Skills\nNo version here.\n";
        assert_eq!(extract_skills_version(content), None);
    }

    #[test]
    fn test_bundled_agent_skill_has_frontmatter_and_pkg_version() {
        let content = bundled_agent_skill_markdown();
        assert!(content.starts_with("---\n"));
        assert!(content.contains("name: stax"));
        assert!(content.contains(&format!("stax_version: \"{PKG_VERSION}\"")));
        assert!(content.contains("# Stax Skills"));
    }

    #[test]
    fn test_build_content_with_frontmatter() {
        let loc = &SKILL_LOCATIONS[0]; // Codex — has_frontmatter = true
        let body = "<!-- stax-skills-version: 0.50.2 -->\n# Skills\n";
        let content = build_content(body, loc);
        assert!(content.starts_with("---\n"));
        assert!(content.contains("stax_version:"));
        assert!(content.contains("# Skills"));
    }

    /// Frontmatter `stax_version` (always written as `PKG_VERSION`) must take
    /// precedence over an older `<!-- stax-skills-version: ... -->` marker that
    /// may be stuck in the upstream skills.md body. This is what makes
    /// `skills update` and `skills list` agree on freshness.
    #[test]
    fn test_extracted_version_after_build_is_pkg_version() {
        let loc = &SKILL_LOCATIONS[0]; // Codex — has_frontmatter = true
        let body = "<!-- stax-skills-version: 0.50.2 -->\n# Skills\n";
        let written = build_content(body, loc);
        assert_eq!(
            extract_skills_version(&written).as_deref(),
            Some(PKG_VERSION),
            "frontmatter PKG_VERSION should win over a stale body marker",
        );
    }

    #[test]
    fn test_skill_locations_include_pi() {
        let pi = SKILL_LOCATIONS
            .iter()
            .find(|loc| loc.name == "pi")
            .expect("pi skill location should be registered");
        assert_eq!(pi.relative_path, ".pi/agent/skills/stax/SKILL.md");
        assert!(pi.has_frontmatter);
    }

    #[test]
    fn test_stale_files_skips_missing() {
        let _ = stale_skill_files();
    }

    #[test]
    fn test_skill_location_ids_unique_and_detect_paths() {
        let mut seen = std::collections::HashSet::new();
        for loc in SKILL_LOCATIONS {
            assert!(!loc.id.is_empty());
            assert!(seen.insert(loc.id), "duplicate id: {}", loc.id);
            assert!(
                loc.relative_path.starts_with(loc.detect_relative_path),
                "{} detect path should prefix relative_path",
                loc.id
            );
        }
    }

    #[test]
    fn test_parse_harness_selection_keywords() {
        assert_eq!(
            parse_harness_selection("all").unwrap(),
            HarnessSelection::All
        );
        assert_eq!(
            parse_harness_selection("detected").unwrap(),
            HarnessSelection::Detected
        );
        assert_eq!(
            parse_harness_selection("auto").unwrap(),
            HarnessSelection::Auto
        );
        assert_eq!(
            parse_harness_selection("none").unwrap(),
            HarnessSelection::Only(vec![])
        );
    }

    #[test]
    fn test_parse_harness_selection_ids() {
        assert_eq!(
            parse_harness_selection("Codex, cursor").unwrap(),
            HarnessSelection::Only(vec!["codex".into(), "cursor".into()])
        );
    }

    #[test]
    fn test_parse_harness_selection_unknown_id() {
        let err = parse_harness_selection("bogus").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("bogus"));
        assert!(msg.contains("codex"));
    }

    #[test]
    fn test_parse_harness_selection_keyword_mix_error() {
        assert!(parse_harness_selection("all,codex").is_err());
    }

    #[test]
    fn test_parse_harness_selection_dedupes() {
        assert_eq!(
            parse_harness_selection("codex,codex,claude").unwrap(),
            HarnessSelection::Only(vec!["codex".into(), "claude".into()])
        );
    }

    #[test]
    fn parse_harness_selection_rejects_empty_spec() {
        let err = parse_harness_selection("   ").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("--skills requires a value"));
    }

    #[test]
    fn parse_harness_selection_rejects_comma_only_spec() {
        let err = parse_harness_selection(",,,").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("--skills requires at least one harness id"));
    }

    #[test]
    fn test_format_update_command_line_shows_flags() {
        assert_eq!(
            format_update_command_line(false, &SkillsUpdateOrigin::AllFlag),
            "stax skills update --all"
        );
        assert_eq!(
            format_update_command_line(
                true,
                &SkillsUpdateOrigin::Cli {
                    spec: "codex".into()
                }
            ),
            "stax skills update --dry-run --skills codex"
        );
    }

    #[test]
    fn test_format_harness_selection_summary_auto() {
        let line = format_harness_selection_summary(
            &["codex".into(), "cursor".into()],
            &SkillsUpdateOrigin::Auto,
        );
        assert!(line.contains("auto:"));
        assert!(line.contains("codex, cursor"));
    }
}
