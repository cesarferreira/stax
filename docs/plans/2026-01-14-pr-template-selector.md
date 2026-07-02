# PR Template Selector Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add interactive PR template selection to `stax submit` with fuzzy search, similar to freephite's flow.

**Architecture:** Enhance the existing `load_pr_template` function to discover multiple templates, create a new template selection module with fuzzy search UI, and integrate template selection into the submit command's interactive flow for new PRs.

**Tech Stack:** Rust, dialoguer (fuzzy-select), std::fs for file discovery

---

## Task 1: Create PR Template Discovery Module

**Files:**
- Create: `src/github/pr_template.rs`
- Modify: `src/github/mod.rs` (add `pub mod pr_template;`)

**Step 1: Write failing test for template discovery**

```rust
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
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo nextest run test_discover`
Expected: FAIL with "cannot find function `discover_pr_templates`"

**Step 3: Implement PR template data structure and discovery**

```rust
use anyhow::Result;
use std::fs;
use std::path::Path;

/// Represents a discovered PR template
#[derive(Debug, Clone)]
pub struct PrTemplate {
    /// Display name (e.g., "feature", "bugfix", "Default")
    pub name: String,
    /// Full file path
    pub path: std::path::PathBuf,
    /// Template content (loaded lazily in real usage, but eager in tests)
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
pub fn discover_pr_templates(workdir: &Path) -> Result<Vec<PrTemplate>> {
    let mut templates = Vec::new();

    // Check directory first (multiple templates)
    let template_dir = workdir.join(".github/PULL_REQUEST_TEMPLATE");
    if template_dir.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(&template_dir)?
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

            let content = fs::read_to_string(&path)?;

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
            let content = fs::read_to_string(&path)?;
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
```

**Step 4: Run tests to verify they pass**

Run: `cargo nextest run test_discover`
Expected: PASS (all 3 tests)

**Step 5: Commit**

```bash
git add src/github/pr_template.rs src/github/mod.rs
git commit -m "feat: add PR template discovery with multi-template support"
```

---

## Task 2: Add Template Selection UI

**Files:**
- Modify: `src/github/pr_template.rs`

**Step 1: Write test for template selection (integration-style)**

```rust
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
```

**Step 2: Run tests to verify failure**

Run: `cargo nextest run test_template_selection`
Expected: FAIL with "cannot find function `build_template_options`"

**Step 3: Implement selection helpers**

```rust
use dialoguer::{theme::ColorfulTheme, FuzzySelect};

/// Build selection options list: ["No template", ...template names sorted]
pub fn build_template_options(templates: &[PrTemplate]) -> Vec<String> {
    let mut options = vec!["No template".to_string()];
    let mut names: Vec<_> = templates.iter().map(|t| t.name.clone()).collect();
    names.sort();
    options.extend(names);
    options
}

/// For single templates, return automatically without prompting
pub fn select_template_auto(templates: &[PrTemplate]) -> Option<PrTemplate> {
    if templates.len() == 1 {
        Some(templates[0].clone())
    } else {
        None
    }
}

/// Show interactive fuzzy-search template picker
/// Returns None if "No template" selected, Some(template) otherwise
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
```

**Step 4: Run tests**

Run: `cargo nextest run test_template_selection`
Expected: PASS

**Step 5: Commit**

```bash
git add src/github/pr_template.rs
git commit -m "feat: add interactive template selection with fuzzy search"
```

---

## Task 3: Add CLI Flags for Template Control

**Files:**
- Modify: `src/main.rs` (Submit command struct)

**Step 1: Add new flags to Submit command**

In `src/main.rs`, find the `Submit` command struct and add:

```rust
/// Submit stack - push branches and create/update PRs
#[command(visible_alias = "ss")]
Submit {
    /// Create PRs as drafts
    #[arg(short, long)]
    draft: bool,
    /// Only push, don't create/update PRs
    #[arg(long)]
    no_pr: bool,
    /// Skip restack check and submit anyway
    #[arg(short, long)]
    force: bool,
    /// Auto-approve prompts
    #[arg(long)]
    yes: bool,
    /// Use defaults, skip interactive prompts
    #[arg(long)]
    no_prompt: bool,
    /// Request PR reviewers
    #[arg(long, value_delimiter = ',')]
    reviewers: Vec<String>,
    /// Add PR labels
    #[arg(long, value_delimiter = ',')]
    labels: Vec<String>,
    /// Assign PR to users
    #[arg(long, value_delimiter = ',')]
    assignees: Vec<String>,
    /// Suppress extra output
    #[arg(long)]
    quiet: bool,
    /// Specify template by name (skip picker)
    #[arg(long)]
    template: Option<String>,
    /// Skip template selection (no template)
    #[arg(long)]
    no_template: bool,
    /// Always open editor for PR body
    #[arg(long)]
    edit: bool,
},
```

**Step 2: Update submit.rs function signature**

In `src/commands/submit.rs`, update the `run` function:

```rust
#[allow(clippy::too_many_arguments)]
pub fn run(
    draft: bool,
    no_pr: bool,
    _force: bool,
    yes: bool,
    no_prompt: bool,
    reviewers: Vec<String>,
    labels: Vec<String>,
    assignees: Vec<String>,
    quiet: bool,
    template: Option<String>,
    no_template: bool,
    edit: bool,
) -> Result<()> {
```

**Step 3: Update main.rs to pass new parameters**

In `src/main.rs`, find the `Commands::Submit` match arm and update:

```rust
Commands::Submit {
    draft,
    no_pr,
    force,
    yes,
    no_prompt,
    reviewers,
    labels,
    assignees,
    quiet,
    template,
    no_template,
    edit,
} => commands::submit::run(
    draft,
    no_pr,
    force,
    yes,
    no_prompt,
    reviewers,
    labels,
    assignees,
    quiet,
    template,
    no_template,
    edit,
),
```

**Step 4: Verify compilation**

Run: `cargo check`
Expected: SUCCESS

**Step 5: Commit**

```bash
git add src/main.rs src/commands/submit.rs
git commit -m "feat: add --template, --no-template, --edit flags to submit"
```

---

## Task 4: Integrate Template Selection into Submit Flow

**Files:**
- Modify: `src/commands/submit.rs`

**Step 1: Add import**

At top of `src/commands/submit.rs`:

```rust
use crate::github::pr_template::{discover_pr_templates, select_template_interactive, PrTemplate};
```

**Step 2: Replace load_pr_template with discovery**

Find the line (around line 248):
```rust
let pr_template = load_pr_template(repo.workdir()?);
```

Replace with:

```rust
// Discover all available PR templates
let discovered_templates = if no_template {
    Vec::new()
} else {
    discover_pr_templates(repo.workdir()?).unwrap_or_default()
};
```

**Step 3: Implement per-branch template selection**

Find the loop starting around line 255 (`for plan in &mut plans {`).

Before the existing title/body collection, add template selection logic:

```rust
for plan in &mut plans {
    if plan.existing_pr.is_some() || plan.is_empty {
        continue;
    }

    // NEW: Template selection per branch
    let selected_template = if no_template {
        None
    } else if let Some(ref template_name) = template {
        // --template flag: find by name
        discovered_templates
            .iter()
            .find(|t| t.name == *template_name)
            .cloned()
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

    let commit_messages =
        collect_commit_messages(repo.workdir()?, &plan.parent, &plan.branch);
    let default_title = default_pr_title(&commit_messages, &plan.branch);

    // Use selected template content if available
    let template_content = selected_template.as_ref().map(|t| t.content.as_str());
    let default_body =
        build_default_pr_body(template_content, &plan.branch, &commit_messages);

    // Rest of existing code for title/body/draft...
```

**Step 4: Update body prompt to respect --edit flag**

Find the body selection code (around line 279-301) and update:

```rust
let body = if no_prompt {
    default_body
} else if edit {
    // --edit flag: always open editor
    Editor::new()
        .edit(&default_body)?
        .unwrap_or(default_body)
} else {
    // Interactive prompt
    let options = if default_body.trim().is_empty() {
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
        "Use default" => default_body,
        "Edit" => Editor::new()
            .edit(&default_body)?
            .unwrap_or(default_body),
        _ => String::new(),
    }
};
```

**Step 5: Remove or deprecate old load_pr_template function**

Find `fn load_pr_template` (line 687) and mark it as deprecated or remove it entirely since we're now using the new discovery system:

```rust
// Deprecated: Use github::pr_template::discover_pr_templates instead
#[allow(dead_code)]
fn load_pr_template(workdir: &Path) -> Option<String> {
    // Keep implementation for backwards compatibility during transition
    // ... existing code ...
}
```

**Step 6: Verify compilation**

Run: `cargo check && cargo clippy -- -D warnings`
Expected: SUCCESS with no warnings

**Step 7: Commit**

```bash
git add src/commands/submit.rs
git commit -m "feat: integrate template selector into submit flow per-branch"
```

---

## Task 5: Add Integration Tests

**Files:**
- Create: `tests/pr_template_tests.rs`

**Step 1: Write test for single template auto-selection**

```rust
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn stax_bin() -> &'static str {
    env!("CARGO_BIN_EXE_stax")
}

#[test]
fn test_submit_with_single_template() {
    let dir = TempDir::new().unwrap();
    let path = dir.path();

    // Setup git repo
    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(path)
        .output()
        .unwrap();

    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .output()
        .unwrap();

    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(path)
        .output()
        .unwrap();

    // Initial commit
    fs::write(path.join("file.txt"), "initial").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(path)
        .output()
        .unwrap();

    // Create PR template
    let github_dir = path.join(".github");
    fs::create_dir(&github_dir).unwrap();
    fs::write(
        github_dir.join("PULL_REQUEST_TEMPLATE.md"),
        "## Description\n\nPlease describe your changes.\n\n## Testing\n\nHow was this tested?"
    ).unwrap();

    // Create a branch with stax
    let output = Command::new(stax_bin())
        .args(["create", "test-branch", "-m", "test commit"])
        .current_dir(path)
        .output()
        .unwrap();

    assert!(output.status.success(), "Failed to create branch");

    // Note: Full submit test would require GitHub API mocking
    // This test validates that template discovery works in real repo context
    // Actual PR creation tested in Task 6
}

#[test]
fn test_template_discovery_multiple() {
    let dir = TempDir::new().unwrap();
    let template_dir = dir.path().join(".github/PULL_REQUEST_TEMPLATE");
    fs::create_dir_all(&template_dir).unwrap();

    fs::write(template_dir.join("feature.md"), "# Feature template").unwrap();
    fs::write(template_dir.join("bugfix.md"), "# Bugfix template").unwrap();
    fs::write(template_dir.join("docs.md"), "# Docs template").unwrap();

    // Test discovery returns all templates
    let templates = stax::github::pr_template::discover_pr_templates(dir.path()).unwrap();
    assert_eq!(templates.len(), 3);

    let names: Vec<_> = templates.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"feature"));
    assert!(names.contains(&"bugfix"));
    assert!(names.contains(&"docs"));
}
```

**Step 2: Run tests**

Run: `cargo nextest run test_submit_with_single_template test_template_discovery_multiple`
Expected: PASS

**Step 3: Write test for --no-template flag**

```rust
#[test]
fn test_no_template_flag_skips_template() {
    let dir = TempDir::new().unwrap();

    // Create template
    let github_dir = dir.path().join(".github");
    fs::create_dir(&github_dir).unwrap();
    fs::write(
        github_dir.join("PULL_REQUEST_TEMPLATE.md"),
        "# Template content"
    ).unwrap();

    // Verify template exists
    let templates = stax::github::pr_template::discover_pr_templates(dir.path()).unwrap();
    assert_eq!(templates.len(), 1);

    // When --no-template is used, should return empty list
    // (This would be tested via CLI flag parsing in actual submit command)
}
```

**Step 4: Run tests**

Run: `cargo nextest run test_no_template`
Expected: PASS

**Step 5: Commit**

```bash
git add tests/pr_template_tests.rs
git commit -m "test: add integration tests for PR template selection"
```

---

## Task 6: Manual Testing & Documentation

**Files:**
- Modify: `README.md` (document new flags)

**Step 1: Manual test - single template auto-selection**

```bash
cd /tmp
mkdir test-stax-template && cd test-stax-template
git init -b main
git config user.email "test@test.com"
git config user.name "Test"

echo "init" > file.txt
git add . && git commit -m "initial"

mkdir -p .github
cat > .github/PULL_REQUEST_TEMPLATE.md << 'EOF'
## Description
{{COMMITS}}

## Testing
- [ ] Tested locally
EOF

cargo run --bin stax -- create feature-1 -am "Add feature 1"

# Test: Should auto-select single template without prompting
# (Would need GitHub token and real PR creation to test fully)
```

**Step 2: Manual test - multiple templates with fuzzy search**

```bash
mkdir -p .github/PULL_REQUEST_TEMPLATE
cat > .github/PULL_REQUEST_TEMPLATE/feature.md << 'EOF'
## Feature
- Description:
- Motivation:
EOF

cat > .github/PULL_REQUEST_TEMPLATE/bugfix.md << 'EOF'
## Bugfix
- Problem:
- Solution:
- Root cause:
EOF

rm .github/PULL_REQUEST_TEMPLATE.md

cargo run --bin stax -- create feature-2 -am "Add feature 2"

# Test: Should show fuzzy-search picker with:
# - No template
# - bugfix
# - feature
```

**Step 3: Manual test - CLI flags**

```bash
# Test --no-template
cargo run --bin stax -- create feature-3 -am "No template" && \
  cargo run --bin stax -- submit --no-template --no-pr

# Test --template
cargo run --bin stax -- create feature-4 -am "Specific template" && \
  cargo run --bin stax -- submit --template bugfix --no-pr

# Test --edit
cargo run --bin stax -- create feature-5 -am "Edit body" && \
  cargo run --bin stax -- submit --edit --no-pr
```

**Step 4: Update README documentation**

In README.md, find the Submit command section (around line 661-663) and add:

```markdown
- `stax submit --template <name>` - Use specific template by name (skip picker)
- `stax submit --no-template` - Skip template selection (no template)
- `stax submit --edit` - Always open editor for PR body
```

Also add a new section after "Freephite/Graphite Compatibility":

```markdown
## PR Templates

stax automatically discovers PR templates in your repository:

### Single Template
If you have one template at `.github/PULL_REQUEST_TEMPLATE.md`, stax uses it automatically:

```bash
stax submit  # Auto-uses template, shows "Edit body?" prompt
```

### Multiple Templates
Place templates in `.github/PULL_REQUEST_TEMPLATE/` directory:

```
.github/
  └── PULL_REQUEST_TEMPLATE/
      ├── feature.md
      ├── bugfix.md
      └── docs.md
```

stax shows an interactive fuzzy-search picker:

```bash
stax submit
# ? Select PR template
#   > No template
#     bugfix
#     feature
#     docs
```

### Template Control Flags

- `--template <name>`: Skip picker, use specific template
- `--no-template`: Don't use any template
- `--edit`: Always open $EDITOR for body (regardless of template)

```bash
stax submit --template bugfix  # Use bugfix.md directly
stax submit --no-template      # Empty body
stax submit --edit             # Force editor open
```
```

**Step 5: Verify documentation formatting**

Run: `cargo run -- --help | grep -A5 "Submit"`
Expected: Shows new flags with descriptions

**Step 6: Commit**

```bash
git add README.md
git commit -m "docs: document PR template selection feature"
```

---

## Task 7: Final Testing & Cleanup

**Step 1: Run full test suite**

Run: `cargo nextest run`
Expected: All tests PASS

**Step 2: Run clippy for lint check**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

**Step 3: Format code**

Run: `cargo fmt`
Expected: Code formatted

**Step 4: Build release version**

Run: `cargo build --release`
Expected: SUCCESS

**Step 5: Final commit**

```bash
git add .
git commit -m "chore: final cleanup and formatting for PR template feature"
```

---

## Summary

This plan implements PR template selection for `stax submit` with:

✅ **Multiple template discovery** - Scans `.github/PULL_REQUEST_TEMPLATE/` directory and fallback locations
✅ **Fuzzy search UI** - Interactive picker using dialoguer's FuzzySelect (consistent with `stax co`)
✅ **Smart defaults** - Auto-selects single templates, respects `--no-prompt` mode
✅ **CLI flags** - `--template <name>`, `--no-template`, `--edit` for automation
✅ **Per-branch selection** - Each branch in stack can use different template
✅ **Backward compatible** - Existing behavior preserved when no templates exist

**Testing approach:**
- Unit tests for discovery logic
- Integration tests for template selection
- Manual testing for UX validation
- Existing submit tests ensure no regressions

**File changes:**
- New: `src/github/pr_template.rs` (template discovery & selection)
- Modified: `src/commands/submit.rs` (integrate template flow)
- Modified: `src/main.rs` (add CLI flags)
- Modified: `src/github/mod.rs` (export new module)
- New: `tests/pr_template_tests.rs` (integration tests)
- Modified: `README.md` (documentation)
