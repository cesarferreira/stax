use anyhow::{Context, Result};
use dialoguer::{theme::ColorfulTheme, FuzzySelect};
use std::fs;
use std::path::Path;

/// Represents a discovered PR template
#[derive(Debug, Clone)]
#[allow(dead_code)] // Will be used in future tasks
pub struct PrTemplate {
    /// Display name (e.g., "feature", "bugfix", "Default")
    pub name: String,
    /// Full file path
    pub path: std::path::PathBuf,
    /// Template content
    pub content: String,
}

/// Discover all PR templates in standard GitHub locations
///
/// Priority order:
/// 1. .github/PULL_REQUEST_TEMPLATE/ directory - scan for all .md files
/// 2. .github/PULL_REQUEST_TEMPLATE.md - single template (named "Default")
/// 3. .github/pull_request_template.md - lowercase variant
/// 4. docs/PULL_REQUEST_TEMPLATE.md
/// 5. docs/pull_request_template.md
#[allow(dead_code)] // Will be used in future tasks
pub fn discover_pr_templates(workdir: &Path) -> Result<Vec<PrTemplate>> {
    let mut templates = Vec::new();

    // Check directory first (multiple templates)
    let template_dir = workdir.join(".github/PULL_REQUEST_TEMPLATE");
    if template_dir.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(&template_dir)
            .context("Failed to read PR template directory")?
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

        for entry in entries {
            let path = entry.path();
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("template")
                .to_string();

            let content = fs::read_to_string(&path)
                .context(format!("Failed to read PR template: {}", path.display()))?;

            templates.push(PrTemplate {
                name,
                path,
                content,
            });
        }

        if !templates.is_empty() {
            return Ok(templates);
        }
    }

    // Check single template locations
    let single_template_candidates = [
        ".github/PULL_REQUEST_TEMPLATE.md",
        ".github/pull_request_template.md",
        "docs/PULL_REQUEST_TEMPLATE.md",
        "docs/pull_request_template.md",
    ];

    for candidate in &single_template_candidates {
        let path = workdir.join(candidate);
        if path.is_file() {
            let content = fs::read_to_string(&path)
                .context(format!("Failed to read PR template: {}", path.display()))?;
            templates.push(PrTemplate {
                name: "Default".to_string(),
                path,
                content,
            });
            return Ok(templates);
        }
    }

    Ok(templates)
}

/// Build selection options list: ["No template", ...template names sorted]
#[allow(dead_code)] // Will be used in future tasks
pub fn build_template_options(templates: &[PrTemplate]) -> Vec<String> {
    let mut options = vec!["No template".to_string()];
    let mut names: Vec<_> = templates.iter().map(|t| t.name.clone()).collect();
    names.sort();
    options.extend(names);
    options
}

/// For single templates, return automatically without prompting
#[allow(dead_code)] // Will be used in future tasks
pub fn select_template_auto(templates: &[PrTemplate]) -> Option<PrTemplate> {
    if templates.len() == 1 {
        Some(templates[0].clone())
    } else {
        None
    }
}

/// Show interactive fuzzy-search template picker
/// Returns None if "No template" selected, Some(template) otherwise
#[allow(dead_code)] // Will be used in future tasks
pub fn select_template_interactive(templates: &[PrTemplate]) -> Result<Option<PrTemplate>> {
    if templates.is_empty() {
        return Ok(None);
    }

    // Auto-select if single template
    if let Some(template) = select_template_auto(templates) {
        return Ok(Some(template));
    }

    let options = build_template_options(templates);

    let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Select PR template")
        .items(&options)
        .default(0)
        .interact()?;

    if selection == 0 {
        // "No template" selected
        Ok(None)
    } else {
        // Find template by name (options[selection] is the name)
        let selected_name = &options[selection];
        let template = templates
            .iter()
            .find(|t| &t.name == selected_name)
            .cloned();
        Ok(template)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_discover_single_template() {
        let dir = TempDir::new().unwrap();
        let github_dir = dir.path().join(".github");
        fs::create_dir(&github_dir).unwrap();
        fs::write(
            github_dir.join("PULL_REQUEST_TEMPLATE.md"),
            "# Single template"
        ).unwrap();

        let templates = discover_pr_templates(dir.path()).unwrap();
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].name, "Default");
        assert!(templates[0].content.contains("Single template"));
    }

    #[test]
    fn test_discover_multiple_templates() {
        let dir = TempDir::new().unwrap();
        let template_dir = dir.path().join(".github/PULL_REQUEST_TEMPLATE");
        fs::create_dir_all(&template_dir).unwrap();

        fs::write(template_dir.join("feature.md"), "# Feature").unwrap();
        fs::write(template_dir.join("bugfix.md"), "# Bugfix").unwrap();

        let templates = discover_pr_templates(dir.path()).unwrap();
        assert_eq!(templates.len(), 2);

        let names: Vec<_> = templates.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"bugfix"));
        assert!(names.contains(&"feature"));
    }

    #[test]
    fn test_discover_no_templates() {
        let dir = TempDir::new().unwrap();
        let templates = discover_pr_templates(dir.path()).unwrap();
        assert_eq!(templates.len(), 0);
    }

    #[test]
    fn test_template_selection_options() {
        let dir = TempDir::new().unwrap();
        let template_dir = dir.path().join(".github/PULL_REQUEST_TEMPLATE");
        fs::create_dir_all(&template_dir).unwrap();

        fs::write(template_dir.join("feature.md"), "# Feature PR").unwrap();
        fs::write(template_dir.join("bugfix.md"), "# Bugfix PR").unwrap();

        let templates = discover_pr_templates(dir.path()).unwrap();
        let options = build_template_options(&templates);

        // Should have templates + "No template" option
        assert_eq!(options.len(), 3);
        assert_eq!(options[0], "No template");
        assert_eq!(options[1], "bugfix");
        assert_eq!(options[2], "feature");
    }

    #[test]
    fn test_template_selection_single_returns_directly() {
        let dir = TempDir::new().unwrap();
        let github_dir = dir.path().join(".github");
        fs::create_dir(&github_dir).unwrap();
        fs::write(
            github_dir.join("PULL_REQUEST_TEMPLATE.md"),
            "# Single"
        ).unwrap();

        let templates = discover_pr_templates(dir.path()).unwrap();
        assert_eq!(templates.len(), 1);

        // Single template should be used directly, no selection needed
        let selected = select_template_auto(&templates);
        assert!(selected.is_some());
        assert_eq!(selected.unwrap().name, "Default");
    }
}
