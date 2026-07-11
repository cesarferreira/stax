# Stax GUI Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver a macOS GPUI application that opens one stax repository and provides a responsive, read-only three-pane stack, patch, branch, PR, and CI cockpit.

**Architecture:** Keep the existing root package as the default workspace member and add a separate `stax-gui` crate pinned to GPUI 0.2.2. Extract repository snapshots, branch details, diffs, and CI summaries into a presentation-neutral `stax::application` module used by both the TUI and GUI. Keep GPUI entities limited to selection, loading, error, and pane state; run Git and network reads on GPUI's background executor and reject stale results with generation tokens.

**Tech Stack:** Rust 1.96, GPUI 0.2.2 with the macOS `font-kit` feature, git2, Tokio, serde/serde_json, existing stax caches and forge clients, cargo-nextest.

**Stacking:** Base this phase branch on `docs/gpui-gui-design` / PR #607. This phase is the first implementation PR; phases 2–4 receive their own plans and branches after this API is verified.

---

## File map

### Root library

- Modify `Cargo.toml` — workspace metadata and shared package fields.
- Modify `Cargo.lock` — lock the GUI package and GPUI dependency graph.
- Modify `.github/workflows/rust-tests.yml` — compile and lint the GUI on macOS.
- Modify `src/lib.rs` — expose the new application module.
- Create `src/application/mod.rs` — public module boundary and re-exports.
- Create `src/application/model.rs` — UI-neutral snapshots, details, diff, CI, and request tokens.
- Create `src/application/repository.rs` — path validation, local snapshots, branch details, and diff loading/cache.
- Create `src/application/ci.rs` — provider-neutral CI loading and summary calculation.
- Modify `src/tui/app.rs` — consume the shared model/loaders instead of owning duplicate read logic.
- Create `tests/application_session_tests.rs` — public API integration coverage.
- Modify `tests/all_tests.rs` — register the consolidated integration-test module.

### GPUI application

- Create `crates/stax-gui/Cargo.toml` — isolated GUI dependencies.
- Create `crates/stax-gui/src/main.rs` — argument parsing and process entry.
- Create `crates/stax-gui/src/lib.rs` — application bootstrap.
- Create `crates/stax-gui/src/state.rs` — pure workspace state and stale-result guards.
- Create `crates/stax-gui/src/preferences.rs` — recent repositories with deterministic test path injection.
- Create `crates/stax-gui/src/theme.rs` — native graphite light/dark tokens.
- Create `crates/stax-gui/src/views/mod.rs` — view exports.
- Create `crates/stax-gui/src/views/welcome.rs` — recent repositories and folder picker.
- Create `crates/stax-gui/src/views/workspace.rs` — root cockpit and async coordination.
- Create `crates/stax-gui/src/views/stack_pane.rs` — virtualized branch tree.
- Create `crates/stax-gui/src/views/changes_pane.rs` — virtualized diffstat and patch.
- Create `crates/stax-gui/src/views/inspector_pane.rs` — selected branch, commits, PR, and CI.

## 1. Workspace and GPUI bootstrap

### Task 1: Add the isolated GUI package

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `.github/workflows/rust-tests.yml`
- Create: `crates/stax-gui/Cargo.toml`
- Create: `crates/stax-gui/src/main.rs`
- Create: `crates/stax-gui/src/lib.rs`

- [ ] **Step 1: Verify the package does not exist**

Run:

```bash
cargo check -p stax-gui
```

Expected: failure containing `package ID specification 'stax-gui' did not match any packages`.

- [ ] **Step 2: Convert the root manifest into a workspace**

Add above `[package]` and move shared package values to workspace metadata:

```toml
[workspace]
members = [".", "crates/stax-gui"]
default-members = ["."]
resolver = "2"

[workspace.package]
version = "0.94.0"
edition = "2024"
rust-version = "1.96"
license = "MIT"
authors = ["Cesar Ferreira"]
repository = "https://github.com/cesarferreira/stax"
```

Change the matching root package keys to:

```toml
[package]
name = "stax"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true
```

Keep `default-run`, `description`, binary declarations, profiles, and existing dependencies unchanged.

- [ ] **Step 3: Add the GUI manifest**

Create `crates/stax-gui/Cargo.toml`:

```toml
[package]
name = "stax-gui"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true
description = "Native macOS desktop app for stax"
publish = false

[[bin]]
name = "stax-gui"
path = "src/main.rs"

[dependencies]
anyhow = "1"
dirs = "6"
gpui = { version = "=0.2.2", default-features = false, features = ["font-kit"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
stax = { path = "../.." }

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 4: Add the smallest launchable application**

Create `crates/stax-gui/src/main.rs`:

```rust
use std::path::PathBuf;

fn main() {
    let repository = std::env::args_os().nth(1).map(PathBuf::from);
    stax_gui::run(repository);
}
```

Create `crates/stax-gui/src/lib.rs`:

```rust
use gpui::{App, Application};
use std::path::PathBuf;

pub fn run(repository: Option<PathBuf>) {
    Application::new().run(move |cx: &mut App| {
        crate::views::open_initial_window(repository.clone(), cx);
        cx.activate(true);
    });
}
```

Temporarily add `mod views` with this launch surface. Task 6 replaces the
`Placeholder` body while preserving the function:

```rust
use gpui::{
    App, Bounds, Context, Render, Window, WindowBounds, WindowOptions, div, prelude::*, px, size,
};
use std::path::PathBuf;

struct Placeholder {
    repository: Option<PathBuf>,
}

impl Render for Placeholder {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                self.repository
                    .as_ref()
                    .map(|path| format!("Stax · {}", path.display()))
                    .unwrap_or_else(|| "Stax".to_string()),
            )
    }
}

pub fn open_initial_window(repository: Option<PathBuf>, cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(1100.0), px(720.0)), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        move |_, cx| cx.new(|_| Placeholder { repository }),
    )
    .expect("open Stax window");
}
```

- [ ] **Step 5: Refresh the tracked workspace lockfile**

Run:

```bash
cargo generate-lockfile
```

Expected: the tracked root `Cargo.lock` contains `stax-gui`, GPUI 0.2.2, and
the GUI's transitive dependency graph.

- [ ] **Step 6: Add a macOS GUI compile gate**

Add a separate job to `.github/workflows/rust-tests.yml` using the repository's
existing checkout, Rust 1.96.1, and `Swatinem/rust-cache` conventions. Override
the Linux-only mold linker flags for this job. Because Clippy also lints the
local `stax` path dependency, mirror the reviewed legacy allowances from
`scripts/lint.sh` while keeping every other warning fatal:

```yaml
  gui-quality:
    name: GPUI Compile and Clippy
    runs-on: macos-latest
    env:
      RUSTFLAGS: ""
    steps:
      - name: Check out repository
        uses: actions/checkout@v7
      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@master
        with:
          toolchain: 1.96.1
          components: clippy
      - name: Cache cargo build
        uses: Swatinem/rust-cache@v2
        with:
          key: macos-latest-gui
      - name: Check GPUI package
        run: cargo check -p stax-gui --locked
      - name: Lint GPUI package
        # Clippy also lints the local stax path dependency. Mirror the reviewed
        # legacy allowances from scripts/lint.sh while keeping other warnings fatal.
        run: |
          cargo clippy -p stax-gui --all-targets --locked -- \
            -D warnings \
            -A clippy::assertions_on_constants \
            -A clippy::bool_assert_comparison \
            -A clippy::clone_on_copy \
            -A clippy::collapsible_if \
            -A clippy::collapsible_match \
            -A clippy::double_comparisons \
            -A clippy::if_same_then_else \
            -A clippy::items_after_test_module \
            -A clippy::len_zero \
            -A clippy::let_and_return \
            -A clippy::manual_checked_ops \
            -A clippy::needless_borrow \
            -A clippy::needless_lifetimes \
            -A clippy::too_many_arguments \
            -A clippy::to_string_in_format_args \
            -A clippy::type_complexity \
            -A clippy::unnecessary_map_or \
            -A clippy::unnecessary_sort_by \
            -A clippy::useless_format \
            -A clippy::useless_vec
```

Keep the existing Linux quality and test jobs unchanged.

- [ ] **Step 7: Verify workspace isolation**

Run:

```bash
lock_before="$(git hash-object Cargo.lock)"
cargo metadata --no-deps --format-version 1 --locked
cargo check --locked
cargo check -p stax-gui --locked
cargo clippy -p stax-gui --all-targets --locked -- \
  -D warnings \
  -A clippy::assertions_on_constants \
  -A clippy::bool_assert_comparison \
  -A clippy::clone_on_copy \
  -A clippy::collapsible_if \
  -A clippy::collapsible_match \
  -A clippy::double_comparisons \
  -A clippy::if_same_then_else \
  -A clippy::items_after_test_module \
  -A clippy::len_zero \
  -A clippy::let_and_return \
  -A clippy::manual_checked_ops \
  -A clippy::needless_borrow \
  -A clippy::needless_lifetimes \
  -A clippy::too_many_arguments \
  -A clippy::to_string_in_format_args \
  -A clippy::type_complexity \
  -A clippy::unnecessary_map_or \
  -A clippy::unnecessary_sort_by \
  -A clippy::useless_format \
  -A clippy::useless_vec
test "$lock_before" = "$(git hash-object Cargo.lock)"
```

Expected: metadata lists `stax` and `stax-gui`; default `cargo check` checks the
root package; explicit GUI check and clippy pass; none of the commands change
the tracked lockfile.

- [ ] **Step 8: Commit the scaffold**

```bash
git add Cargo.toml Cargo.lock .github/workflows/rust-tests.yml crates/stax-gui
git commit -m "feat: scaffold the GPUI desktop app"
```

## 2. Shared read model

### Task 2: Define presentation-neutral repository types

**Files:**
- Create: `src/application/mod.rs`
- Create: `src/application/model.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write model tests**

Add unit tests in `src/application/model.rs` proving:

```rust
fn check(name: &str, status: &str, conclusion: Option<&str>) -> CheckRunInfo {
    CheckRunInfo {
        name: name.into(),
        status: status.into(),
        conclusion: conclusion.map(str::to_string),
        url: None,
        started_at: None,
        completed_at: None,
        elapsed_secs: None,
        average_secs: None,
        completion_percent: None,
    }
}

#[test]
fn request_tokens_match_only_the_same_repository_branch_and_generation() {
    let token = DetailRequestToken::new("/repo", "feature", 7);
    assert!(token.matches("/repo", "feature", 7));
    assert!(!token.matches("/repo", "feature", 8));
    assert!(!token.matches("/repo", "other", 7));
}

#[test]
fn ci_summary_counts_terminal_and_active_checks() {
    let summary = CiSummary::from_checks(
        Some("pending".into()),
        &[
            check("build", "completed", Some("success")),
            check("lint", "completed", Some("failure")),
            check("test", "in_progress", None),
        ],
        Some(120),
    );
    assert_eq!(summary.total, 3);
    assert_eq!(summary.passed, 1);
    assert_eq!(summary.failed, 1);
    assert_eq!(summary.running, 1);
    assert!(summary.is_active());
}
```

- [ ] **Step 2: Run the tests and confirm the module is missing**

Run:

```bash
cargo nextest run --lib application::model::
```

Expected: compilation fails because `application` and its types do not exist.

- [ ] **Step 3: Add the shared types**

Define and re-export these exact public shapes:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositorySnapshot {
    pub repository_root: PathBuf,
    pub current_branch: String,
    pub trunk: String,
    pub branches: Vec<BranchSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSummary {
    pub name: String,
    pub parent: Option<String>,
    pub column: usize,
    pub is_current: bool,
    pub is_trunk: bool,
    pub needs_restack: bool,
    pub pr_number: Option<u64>,
    pub pr_state: Option<String>,
    pub ci_state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchDetails {
    pub ahead: usize,
    pub behind: usize,
    pub has_remote: bool,
    pub unpushed: usize,
    pub unpulled: usize,
    pub commits: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchDiff {
    pub stat: Vec<DiffStatLine>,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffStatLine {
    pub file: String,
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub content: String,
    pub kind: DiffLineKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Header,
    Addition,
    Deletion,
    Context,
    Hunk,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailRequestToken {
    pub repository: PathBuf,
    pub branch: String,
    pub generation: u64,
}
```

Move the existing `BranchCiSummary` behavior from `src/tui/app.rs` into a public `CiSummary` in this module, preserving count, elapsed, percentage, ETA, and active/complete semantics.

Add `pub mod application;` to `src/lib.rs`. `src/application/mod.rs` re-exports model types while keeping loaders in focused submodules.

- [ ] **Step 4: Run model tests**

Run:

```bash
cargo nextest run --lib application::model::
```

Expected: all model tests pass.

- [ ] **Step 5: Commit the model**

```bash
git add src/lib.rs src/application
git commit -m "refactor: add a shared desktop read model"
```

### Task 3: Load repository snapshots through a public session

**Files:**
- Create: `src/application/repository.rs`
- Create: `tests/application_session_tests.rs`
- Modify: `tests/all_tests.rs`
- Modify: `src/application/mod.rs`

- [ ] **Step 1: Register failing integration tests**

Register `application_session_tests.rs` in `tests/all_tests.rs`, then add:

```rust
use crate::common::TestRepo;
use stax::application::RepositorySession;

#[test]
fn snapshot_orders_tracked_stacks_before_trunk() {
    let repo = TestRepo::new();
    repo.create_stack(&["first", "second"]);

    let session = RepositorySession::open(repo.path()).unwrap();
    let snapshot = session.snapshot().unwrap();

    assert_eq!(snapshot.current_branch, "second");
    assert_eq!(snapshot.trunk, "main");
    assert_eq!(
        snapshot.branches.iter().map(|branch| branch.name.as_str()).collect::<Vec<_>>(),
        vec!["second", "first", "main"],
    );
}

#[test]
fn opening_a_non_repository_reports_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let error = RepositorySession::open(dir.path()).unwrap_err().to_string();
    assert!(error.contains(&dir.path().display().to_string()));
    assert!(error.contains("git repository"));
}
```

- [ ] **Step 2: Run the integration tests and verify failure**

Run:

```bash
cargo nextest run application_session_tests::
```

Expected: compilation fails because `RepositorySession` does not exist.

- [ ] **Step 3: Implement `RepositorySession` and snapshot ordering**

Use:

```rust
#[derive(Debug)]
pub struct RepositorySession {
    repository_root: PathBuf,
    git_dir: PathBuf,
    common_git_dir: PathBuf,
}

impl RepositorySession {
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self>;
    pub fn repository_root(&self) -> &Path;
    pub fn snapshot(&self) -> anyhow::Result<RepositorySnapshot>;
    pub fn branch_details(&self, branch: &BranchSummary) -> anyhow::Result<BranchDetails>;
    pub fn diff(&self, branch: &str, parent: &str) -> anyhow::Result<BranchDiff>;
}
```

`open` uses `GitRepo::open_from_path`, canonicalizes `workdir`, and stores the Git and common-Git directories. `snapshot` uses `StackSnapshot::load`, `CiCache`, and the TUI's existing iterative post-order traversal so children appear above parents and trunk remains last. Sort sibling names for deterministic output and guard cycles with `visiting` and `emitted` sets.

`branch_details` preserves the existing calculations: ahead/behind, remote presence, remote divergence, and at most ten commits.

`diff` first checks `TuiDiffCache` using parent, branch, merge-base OIDs; on a miss it calculates diffstat and patch, classifies each line, writes the same cache file, and returns the typed result.

- [ ] **Step 4: Add details and diff integration coverage**

Add tests that:

1. Create a two-commit feature branch and assert ahead count and commit messages.
2. Modify one file and assert diffstat additions/deletions and `DiffLineKind::Addition`.
3. Call `diff` twice and assert the second value equals the first, proving cache serialization round-trips.
4. Request details for trunk and assert an empty commit list without panic.

- [ ] **Step 5: Run focused tests**

Run:

```bash
cargo nextest run application_session_tests::
```

Expected: all application session tests pass.

- [ ] **Step 6: Commit repository reads**

```bash
git add src/application tests/application_session_tests.rs tests/all_tests.rs
git commit -m "feat: expose repository snapshots for desktop clients"
```

### Task 4: Share CI and diff loading with the TUI

**Files:**
- Create: `src/application/ci.rs`
- Modify: `src/application/repository.rs`
- Modify: `src/application/mod.rs`
- Modify: `src/tui/app.rs`

- [ ] **Step 1: Add failing CI loader tests**

Add unit tests in `src/application/ci.rs`:

```rust
fn check(name: &str, status: &str, conclusion: Option<&str>) -> CheckRunInfo {
    CheckRunInfo {
        name: name.into(),
        status: status.into(),
        conclusion: conclusion.map(str::to_string),
        url: None,
        started_at: None,
        completed_at: None,
        elapsed_secs: None,
        average_secs: None,
        completion_percent: None,
    }
}

fn test_session_without_remote() -> (tempfile::TempDir, RepositorySession) {
    let dir = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    let session = RepositorySession::open(dir.path()).unwrap();
    (dir, session)
}

#[test]
fn branch_without_remote_returns_an_actionable_unavailable_state() {
    let (_dir, session) = test_session_without_remote();
    let result = session.load_ci("main");
    let error = result.unwrap_err().to_string();
    assert!(error.contains("configure a git remote"));
}

#[test]
fn completed_checks_report_one_hundred_percent() {
    let summary = CiSummary::from_checks(
        Some("success".into()),
        &[check("build", "completed", Some("success"))],
        Some(60),
    );
    assert_eq!(summary.progress_percent(Utc::now()), Some(100));
    assert_eq!(summary.eta_secs(Utc::now()), Some(0));
}
```

- [ ] **Step 2: Run and verify failure**

Run:

```bash
cargo nextest run --lib application::ci::
```

Expected: failure because `RepositorySession::load_ci` is absent.

- [ ] **Step 3: Implement provider-neutral CI loading**

Add:

```rust
impl RepositorySession {
    pub fn load_ci(&self, branch: &str) -> anyhow::Result<CiSummary> {
        let repo = self.open_repo()?;
        let config = Config::load()?;
        let remote = RemoteInfo::from_repo(&repo, &config)?;
        let sha = repo.branch_commit(branch)?;
        let runtime = tokio::runtime::Runtime::new()?;
        let (overall, checks) = runtime.block_on(async {
            ForgeClient::new(&remote)?.fetch_checks(&repo, &sha).await
        })?;
        let average = history::estimate_run_average(&repo, &checks)
            .or_else(|| checks.iter().filter_map(|check| check.average_secs).max());
        Ok(CiSummary::from_checks(overall, &checks, average))
    }
}
```

Map missing remote/config/auth/network failures with context that can be shown directly in the inspector.

- [ ] **Step 4: Remove duplicate read logic from the TUI**

In `src/tui/app.rs`:

- Import shared `BranchDetails`, `BranchDiff`, `CiSummary`, `DiffLine`, `DiffLineKind`, and `DiffStatLine`.
- Replace local branch-details, diff, cache-conversion, and CI-summary implementations with calls to `RepositorySession`.
- Keep TUI-only mode, selection, pane, receiver, and refresh scheduling state in `App`.
- Preserve existing cache file names and refresh intervals.
- Convert `BranchSummary` into the TUI's richer `BranchDisplay` only where TUI-only mutable hydration fields are needed.

No TUI command behavior or key binding changes in this task.

- [ ] **Step 5: Run shared and TUI tests**

Run:

```bash
cargo nextest run --lib application:: tui::app::
cargo nextest run application_session_tests:: tui_commands_tests::
```

Expected: all focused tests pass.

- [ ] **Step 6: Commit the shared loaders**

```bash
git add src/application src/tui/app.rs
git commit -m "refactor: share desktop data loaders with the TUI"
```

## 3. Testable GUI state

### Task 5: Add selection, generations, and recent repositories

**Files:**
- Create: `crates/stax-gui/src/state.rs`
- Create: `crates/stax-gui/src/preferences.rs`
- Modify: `crates/stax-gui/src/lib.rs`

- [ ] **Step 1: Write pure state tests**

Add:

```rust
fn snapshot() -> RepositorySnapshot {
    RepositorySnapshot {
        repository_root: PathBuf::from("/repo"),
        current_branch: "first".into(),
        trunk: "main".into(),
        branches: vec![
            BranchSummary {
                name: "first".into(),
                parent: Some("main".into()),
                column: 0,
                is_current: true,
                is_trunk: false,
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                ci_state: None,
            },
            BranchSummary {
                name: "second".into(),
                parent: Some("first".into()),
                column: 0,
                is_current: false,
                is_trunk: false,
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                ci_state: None,
            },
        ],
    }
}

fn diff(content: &str) -> BranchDiff {
    BranchDiff {
        stat: Vec::new(),
        lines: vec![DiffLine {
            content: content.into(),
            kind: DiffLineKind::Context,
        }],
    }
}

#[test]
fn selecting_a_branch_invalidates_older_detail_results() {
    let mut state = WorkspaceState::new(snapshot());
    let first = state.select_branch("first").unwrap();
    let second = state.select_branch("second").unwrap();

    assert!(!state.apply_diff(first, Ok(diff("old"))));
    assert!(state.apply_diff(second, Ok(diff("new"))));
    assert_eq!(state.diff.ready().unwrap().lines[0].content, "new");
}

#[test]
fn recent_repositories_are_canonical_deduplicated_and_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let store = RecentRepositories::at(temp.path().join("recent.json"));
    for index in 0..12 {
        let repository = temp.path().join(format!("repo-{index}"));
        std::fs::create_dir(&repository).unwrap();
        store.record(repository).unwrap();
    }
    let recent = store.load().unwrap();
    assert_eq!(recent.len(), 10);
    assert_eq!(recent[0], temp.path().join("repo-11"));
}
```

- [ ] **Step 2: Run GUI tests and verify failure**

Run:

```bash
cargo nextest run -p stax-gui
```

Expected: compilation fails because state and preference types are absent.

- [ ] **Step 3: Implement deterministic workspace state**

Use:

```rust
pub enum LoadState<T> {
    Idle,
    Loading,
    Ready(T),
    Failed(String),
}

impl<T> LoadState<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Self::Ready(value) => Some(value),
            Self::Idle | Self::Loading | Self::Failed(_) => None,
        }
    }
}

pub struct WorkspaceState {
    pub snapshot: RepositorySnapshot,
    pub selected_branch: String,
    pub details: LoadState<BranchDetails>,
    pub diff: LoadState<BranchDiff>,
    pub ci: LoadState<CiSummary>,
    generation: u64,
}
```

Implement these methods:

```rust
impl WorkspaceState {
    pub fn select_branch(&mut self, name: &str) -> Option<DetailRequestToken>;
    pub fn begin_hydration(&mut self) -> Option<(DetailRequestToken, BranchSummary)>;
    pub fn apply_details(
        &mut self,
        token: DetailRequestToken,
        result: Result<BranchDetails, String>,
    ) -> bool;
    pub fn apply_diff(
        &mut self,
        token: DetailRequestToken,
        result: Result<BranchDiff, String>,
    ) -> bool;
    pub fn apply_ci(
        &mut self,
        token: DetailRequestToken,
        result: Result<CiSummary, String>,
    ) -> bool;
}
```

`select_branch` validates the branch, increments `generation`, resets the three
load states, and returns a token. `begin_hydration` marks the three fields
loading and returns the current token plus a cloned branch. Each `apply_*`
method returns `false` without mutation unless repository, branch, and
generation all match; a matching `Err` becomes `LoadState::Failed`.

- [ ] **Step 4: Implement recent repositories**

`RecentRepositories::default_path` uses:

```rust
dirs::data_dir()
    .unwrap_or_else(std::env::temp_dir)
    .join("stax")
    .join("gui")
    .join("recent-repositories.json")
```

Persist a JSON array using write-to-temporary-file then rename. Canonicalize existing paths, remove duplicates, put the newest first, cap at ten, and ignore missing paths when loading.

- [ ] **Step 5: Run GUI state tests**

Run:

```bash
cargo nextest run -p stax-gui state:: preferences::
```

Expected: all tests pass.

- [ ] **Step 6: Commit state and preferences**

```bash
git add crates/stax-gui/src
git commit -m "feat: add deterministic GUI workspace state"
```

## 4. Native graphite cockpit

### Task 6: Build the welcome window and three panes

**Files:**
- Create: `crates/stax-gui/src/theme.rs`
- Create: `crates/stax-gui/src/views/mod.rs`
- Create: `crates/stax-gui/src/views/welcome.rs`
- Create: `crates/stax-gui/src/views/workspace.rs`
- Create: `crates/stax-gui/src/views/stack_pane.rs`
- Create: `crates/stax-gui/src/views/changes_pane.rs`
- Create: `crates/stax-gui/src/views/inspector_pane.rs`
- Modify: `crates/stax-gui/src/lib.rs`

- [ ] **Step 1: Add GPUI render smoke tests**

Use GPUI's test context:

```rust
#[gpui::test]
fn welcome_window_renders(mut cx: &mut gpui::TestAppContext) {
    cx.add_window(|window, cx| WelcomeView::new(window, cx));
    cx.run_until_parked();
}

#[gpui::test]
fn workspace_window_renders_all_three_panes(mut cx: &mut gpui::TestAppContext) {
    cx.add_window(|window, cx| WorkspaceView::from_snapshot(snapshot(), window, cx));
    cx.run_until_parked();
}
```

Also expose test-only pane markers from `WorkspaceView` and assert stack, changes, and inspector are all present.

- [ ] **Step 2: Run and verify failure**

Run:

```bash
cargo nextest run -p stax-gui views::
```

Expected: compilation fails because the views do not exist.

- [ ] **Step 3: Add the graphite theme**

Define `Theme` with semantic tokens rather than colors in views:

```rust
pub struct Theme {
    pub window: Hsla,
    pub surface: Hsla,
    pub surface_selected: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub accent: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub danger: Hsla,
    pub diff_addition: Hsla,
    pub diff_deletion: Hsla,
}
```

Provide `Theme::light()` and `Theme::dark()`. Views consume only semantic fields. Use SF system UI for controls and the platform monospace font for branch metadata and patches.

- [ ] **Step 4: Implement initial window routing**

`open_initial_window` opens:

- `WorkspaceView` when a repository argument successfully loads.
- `WelcomeView` when no path is supplied.
- `WelcomeView` with a visible actionable error when path validation fails.

Use `WindowOptions` with 1100×720 centered bounds and a minimum size of 820×520.

- [ ] **Step 5: Implement the welcome view**

Render the Stax name, one Open Repository button, and recent repository rows. The button calls:

```rust
let receiver = cx.prompt_for_paths(PathPromptOptions {
    files: false,
    directories: true,
    multiple: false,
    prompt: Some("Open Repository".into()),
});
```

Await the receiver in a detached GPUI task, record a successful selection, and replace the welcome window root with `WorkspaceView`. Cancellation leaves the welcome view unchanged; picker errors render inline.

- [ ] **Step 6: Implement the cockpit**

Render a toolbar plus a horizontal flex row with fixed initial proportions:

```rust
div()
    .size_full()
    .flex()
    .flex_col()
    .child(self.render_toolbar(cx))
    .child(
        div()
            .flex()
            .flex_1()
            .min_h_0()
            .child(stack_pane::render(&self.state, &self.theme, cx).w(relative(0.29)))
            .child(changes_pane::render(&self.state, &self.theme, cx).w(relative(0.43)))
            .child(inspector_pane::render(&self.state, &self.theme, cx).w(relative(0.28))),
    )
```

Use `uniform_list` for branches and patch lines. The stack pane shows topology indentation plus current, restack, PR, and CI status. The changes pane shows diffstat and line-kind colors. The inspector shows parent, remote divergence, commits, PR, CI counts, loading/error states, and read-only disabled action buttons labelled for phase 2.

Clicking a branch selects it without checkout.

- [ ] **Step 7: Run GPUI tests and compile the app**

Run:

```bash
cargo nextest run -p stax-gui views::
cargo check -p stax-gui
```

Expected: tests and check pass.

- [ ] **Step 8: Commit the visual shell**

```bash
git add crates/stax-gui/src
git commit -m "feat: render the native stack cockpit"
```

### Task 7: Hydrate branch details, diffs, and CI asynchronously

**Files:**
- Modify: `crates/stax-gui/src/views/workspace.rs`
- Modify: `crates/stax-gui/src/state.rs`

- [ ] **Step 1: Add stale async-result tests**

Create a deterministic loader test that starts branch A, switches to branch B before A completes, then resolves A and B. Assert only B populates details, diff, and CI. Add a failure test asserting an error populates `LoadState::Failed` without clearing the snapshot.

- [ ] **Step 2: Run and verify failure**

Run:

```bash
cargo nextest run -p stax-gui stale_ async_
```

Expected: failure because `WorkspaceView::hydrate_selection` does not exist.

- [ ] **Step 3: Implement one generation-scoped hydration path**

Use the GPUI background executor for blocking Git/network work and apply results on the entity:

```rust
fn hydrate_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let Some((token, branch)) = self.state.begin_hydration() else {
        return;
    };
    let path = token.repository.clone();
    let parent = branch.parent.clone();
    let background = cx.background_executor().clone();

    cx.spawn_in(window, async move |this, cx| {
        let result = background
            .spawn(async move {
                let session = RepositorySession::open(&path)?;
                let details = session.branch_details(&branch)?;
                let diff = match parent {
                    Some(parent) => Some(session.diff(&branch.name, &parent)?),
                    None => None,
                };
                let ci = if details.has_remote {
                    session.load_ci(&branch.name).map_err(|error| error.to_string())
                } else {
                    Err("Push branch to see remote checks".to_string())
                };
                anyhow::Ok((details, diff, ci))
            })
            .await;

        this.update_in(cx, |view, _window, cx| {
            match result {
                Ok((details, diff, ci)) => {
                    view.state
                        .apply_details(token.clone(), Ok(details));
                    view.state.apply_diff(
                        token.clone(),
                        Ok(diff.unwrap_or(BranchDiff {
                            stat: Vec::new(),
                            lines: Vec::new(),
                        })),
                    );
                    view.state.apply_ci(token, ci);
                }
                Err(error) => {
                    let message = error.to_string();
                    view.state
                        .apply_details(token.clone(), Err(message.clone()));
                    view.state
                        .apply_diff(token.clone(), Err(message.clone()));
                    view.state.apply_ci(token, Err(message));
                }
            }
            cx.notify();
        })?;
        anyhow::Ok(())
    })
    .detach();
}
```

Adapt captures to the final shared types so no `GitRepo` or GPUI context crosses threads. Call hydration after initial snapshot load and every selection change. Preserve cached diff content while refreshing.

- [ ] **Step 4: Run GUI state and view tests**

Run:

```bash
cargo nextest run -p stax-gui
cargo check -p stax-gui
```

Expected: all GUI tests pass and no stale result mutates current selection.

- [ ] **Step 5: Commit async hydration**

```bash
git add crates/stax-gui/src
git commit -m "feat: hydrate desktop branch details asynchronously"
```

## 5. Phase verification and first implementation PR

### Task 8: Validate behavior and open the stacked PR

**Files:**
- Modify only files required by formatting, lints, or failures discovered below.

- [ ] **Step 1: Format and lint**

Run:

```bash
cargo fmt --all
cargo fmt --all -- --check
make lint
cargo clippy -p stax-gui --all-targets --locked -- \
  -D warnings \
  -A clippy::assertions_on_constants \
  -A clippy::bool_assert_comparison \
  -A clippy::clone_on_copy \
  -A clippy::collapsible_if \
  -A clippy::collapsible_match \
  -A clippy::double_comparisons \
  -A clippy::if_same_then_else \
  -A clippy::items_after_test_module \
  -A clippy::len_zero \
  -A clippy::let_and_return \
  -A clippy::manual_checked_ops \
  -A clippy::needless_borrow \
  -A clippy::needless_lifetimes \
  -A clippy::too_many_arguments \
  -A clippy::to_string_in_format_args \
  -A clippy::type_complexity \
  -A clippy::unnecessary_map_or \
  -A clippy::unnecessary_sort_by \
  -A clippy::useless_format \
  -A clippy::useless_vec
```

Expected: all commands pass.

- [ ] **Step 2: Run focused tests**

Run:

```bash
cargo nextest run --lib application:: tui::app::
cargo nextest run application_session_tests:: tui_commands_tests::
cargo nextest run -p stax-gui
```

Expected: all focused tests pass.

- [ ] **Step 3: Run the repository-standard full suite**

Ensure Docker Desktop is running, then run:

```bash
make test
```

Expected: the complete suite passes through the Docker path. If Docker reports that its API socket is unavailable, stop and ask for Docker Desktop to be launched; do not fall back to native full-suite execution.

- [ ] **Step 4: Perform a manual macOS smoke test**

Run:

```bash
cargo run -p stax-gui -- "$(pwd)"
```

Verify:

1. The window opens at the expected size.
2. The stack pane selects without checking out.
3. The patch and inspector hydrate without freezing the window.
4. Rapidly selecting branches never shows details for an older selection.
5. Closing and reopening through the welcome screen preserves recent repositories.

- [ ] **Step 5: Commit verification-only fixes**

If formatting or verification changed tracked files, commit only those fixes:

```bash
git add Cargo.toml Cargo.lock .github/workflows/rust-tests.yml \
  src/application src/tui/app.rs tests crates/stax-gui
git commit -m "fix: harden the GPUI read-only cockpit"
```

Skip this commit when verification produced no file changes.

- [ ] **Step 6: Push and open the stacked PR**

Push the phase branch and open it with base `docs/gpui-gui-design`. The PR summary must call out:

- Shared read APIs now power TUI and GUI.
- GPUI remains excluded from default CLI builds.
- The GUI is intentionally read-only until phase 2.
- No user installation docs change because no packaged app or `st gui` launcher ships in phase 1.

Return the PR URL and stop at the phase boundary before writing the phase 2 plan.
