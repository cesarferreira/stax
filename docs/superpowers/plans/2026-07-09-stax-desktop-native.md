# Stax Desktop with Native SDK Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a self-contained Apple Silicon `Stax.app` that renders the approved three-pane Workshop interface with Native SDK and delegates stack inspection and four allow-listed actions to a bundled Rust `st` engine.

**Architecture:** Add a hidden, versioned `st desktop` protocol to the existing Rust binary for snapshot, diff, and action requests. Build a Zig/Native SDK app in `desktop/` that invokes that binary through typed subprocess effects, owns only presentation state, and stages the release `st` binary inside the macOS bundle.

**Tech Stack:** Rust 2024, serde/serde_json, existing stax Git/stack/forge modules, Zig 0.16, Native SDK v0.4.1, declarative `.native` markup, npm only as the pinned Native SDK CLI launcher.

## Global Constraints

- Work on the stax-managed branch `codex/cesar/native-desktop`; keep each task as its own focused commit.
- Target Apple Silicon macOS 11 or newer. Linux, Windows, Intel macOS, notarization, DMG creation, and automatic updates are outside this plan.
- Pin `@native-sdk/cli` to exactly `0.4.1`; use no WebView and no browser runtime.
- Package the Rust engine at `Stax.app/Contents/Resources/bin/st`; never fall back to an ambient `st` on `PATH`.
- Never invoke a shell for desktop engine operations. Pass the selected repository through `--repo <path>` because Native SDK subprocess effects do not expose a child working-directory option.
- Native SDK collect-mode output is capped at 512 KiB. Cap structured patch text at 448 KiB, set `truncated: true`, and render a visible truncation notice; never accept silent truncation.
- Expose exactly four desktop actions: `checkout`, `restack`, `submit-stack`, and `open-pr`. Restack and submit require UI confirmation; no arbitrary command or argument surface is allowed.
- Preserve credentials in existing stax configuration only. The desktop app must not read or persist tokens.
- Register `tests/desktop_tests.rs` in the single `tests/all_tests.rs` target.
- Use targeted `cargo nextest run desktop_tests::` while iterating and `make test` for the full Rust suite. Do not run the full suite with native `cargo test`.
- Update `README.md`, `docs/interface/desktop.md`, `mkdocs.yml`, and `skills.md` for the user-visible desktop behavior.
- Submit the branch with stax as a draft PR after the plan commit, update it after every implementation commit with `st ss --draft --no-prompt --yes`, and verify the final PR with `st ll` and `gh pr view`.

---

## Planned File Structure

### Rust engine

- `src/desktop/mod.rs` — request dispatch, schema validation, terminal JSON emission, and stable error mapping.
- `src/desktop/protocol.rs` — serializable request/response types shared by snapshot, diff, and actions.
- `src/desktop/snapshot.rs` — repository snapshot construction and deterministic stack ordering.
- `src/desktop/diff.rs` — structured patch construction with an explicit size cap.
- `src/desktop/action.rs` — allow-listed action plans, captured child execution, progress events, and PR opening.
- `tests/desktop_tests.rs` — subprocess-level contract tests against real temporary repositories.

### Native application

- `desktop/package.json` / `desktop/package-lock.json` — exact Native SDK CLI pin and reproducible scripts.
- `desktop/app.zon` — application identity, shell window, shortcuts, and macOS packaging metadata.
- `desktop/assets/icon.svg` — Workshop amber `st` app icon.
- `desktop/src/protocol.zig` — JSON protocol mirrors and parsers.
- `desktop/src/engine_bridge.zig` — safe argument-array construction for Native SDK effects.
- `desktop/src/model.zig` — application state, update loop, persistence, request generations, and action confirmations.
- `desktop/src/app.native` — three-pane Workshop UI and dialogs.
- `desktop/src/main.zig` — Native SDK wiring, app-event host wrapper, folder picker, lifecycle refresh, tokens, and bundle engine-path resolution.
- `desktop/src/tests.zig` — parser, model/effect, widget, layout, and host tests.
- `desktop/scripts/package-macos.sh` — Rust/Native builds, sidecar staging, and ad-hoc signing.
- `desktop/scripts/smoke-macos.sh` — bundle layout, protocol, signature, and packaged launch smoke checks.

---

### Task 1: Hidden Desktop Command and Versioned Protocol Envelope

**Files:**
- Create: `src/desktop/mod.rs`
- Create: `src/desktop/protocol.rs`
- Create: `tests/desktop_tests.rs`
- Modify: `src/lib.rs:6-21`
- Modify: `src/cli/args.rs:220-1307`
- Modify: `src/cli/mod.rs:27-68`
- Modify: `tests/all_tests.rs:15-123`

**Interfaces:**
- Produces: `desktop::run_snapshot(repo: PathBuf, schema_version: u32, request_id: String) -> anyhow::Result<()>`
- Produces: `desktop::run_diff(repo: PathBuf, schema_version: u32, request_id: String, branch: String) -> anyhow::Result<()>`
- Produces: `desktop::run_action(repo: PathBuf, schema_version: u32, request_id: String, action: DesktopAction, branch: Option<String>) -> anyhow::Result<()>`
- Produces: `protocol::TerminalEvent<T>`, `protocol::ProgressEvent`, `protocol::DesktopError`, and `protocol::DesktopAction`
- Consumes: no new interfaces

- [ ] **Step 1: Register a failing integration-test module and unsupported-schema test**

Add this module entry after `demo_tests` in `tests/all_tests.rs`:

```rust
#[path = "desktop_tests.rs"]
mod desktop_tests;
```

Create `tests/desktop_tests.rs` with:

```rust
use crate::common::TestRepo;
use serde_json::Value;

fn terminal_json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "desktop stdout was not one JSON object: {error}\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        )
    })
}

#[test]
fn rejects_unsupported_desktop_schema_with_machine_error() {
    let repo = TestRepo::new();
    let repo_path = repo.path().to_string_lossy().into_owned();
    let output = repo.run_stax(&[
        "desktop",
        "snapshot",
        "--repo",
        &repo_path,
        "--schema-version",
        "9",
        "--request-id",
        "req-schema",
    ]);

    assert!(!output.status.success());
    let event = terminal_json(&output);
    assert_eq!(event["schema_version"], 1);
    assert_eq!(event["request_id"], "req-schema");
    assert_eq!(event["type"], "result");
    assert_eq!(event["ok"], false);
    assert_eq!(event["error"]["code"], "unsupported_schema");
    assert_eq!(event["error"]["recovery"], "reinstall_app");
}
```

- [ ] **Step 2: Run the test and verify the desktop command is absent**

Run: `cargo nextest run desktop_tests::rejects_unsupported_desktop_schema_with_machine_error`

Expected: FAIL because clap reports an unrecognized `desktop` subcommand and stdout is not the required JSON object.

- [ ] **Step 3: Define the exact protocol types**

Create `src/desktop/protocol.rs`:

```rust
use serde::Serialize;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopAction {
    Checkout,
    Restack,
    SubmitStack,
    OpenPr,
}

#[derive(Debug, Serialize)]
pub struct ProgressEvent<'a> {
    pub schema_version: u32,
    pub request_id: &'a str,
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub phase: &'a str,
    pub message: &'a str,
}

#[derive(Debug, Serialize)]
pub struct TerminalEvent<'a, T: Serialize> {
    pub schema_version: u32,
    pub request_id: &'a str,
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<DesktopError>,
}

#[derive(Debug, Serialize)]
pub struct DesktopError {
    pub code: &'static str,
    pub message: String,
    pub details: String,
    pub recovery: &'static str,
}

impl DesktopError {
    pub fn unsupported_schema(received: u32) -> Self {
        Self {
            code: "unsupported_schema",
            message: format!("Desktop schema {received} is not supported."),
            details: format!("This engine supports schema {SCHEMA_VERSION}."),
            recovery: "reinstall_app",
        }
    }

    pub fn operation(code: &'static str, message: impl Into<String>, details: impl Into<String>, recovery: &'static str) -> Self {
        Self { code, message: message.into(), details: details.into(), recovery }
    }
}

impl<'a, T: Serialize> TerminalEvent<'a, T> {
    pub fn success(request_id: &'a str, data: T) -> Self {
        Self { schema_version: SCHEMA_VERSION, request_id, event_type: "result", ok: true, data: Some(data), error: None }
    }

    pub fn failure(request_id: &'a str, error: DesktopError) -> Self {
        Self { schema_version: SCHEMA_VERSION, request_id, event_type: "result", ok: false, data: None, error: Some(error) }
    }
}
```

- [ ] **Step 4: Add the hidden clap surface and early dispatch**

Add these variants and argument structs to `src/cli/args.rs`:

```rust
#[derive(Args, Clone)]
pub(crate) struct DesktopRequest {
    #[arg(long)]
    pub(crate) repo: PathBuf,
    #[arg(long, default_value_t = 1)]
    pub(crate) schema_version: u32,
    #[arg(long)]
    pub(crate) request_id: String,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum DesktopActionArg {
    Checkout,
    Restack,
    SubmitStack,
    OpenPr,
}

#[derive(Clone, Subcommand)]
pub(crate) enum DesktopCommands {
    Snapshot {
        #[command(flatten)]
        request: DesktopRequest,
    },
    Diff {
        #[command(flatten)]
        request: DesktopRequest,
        #[arg(long)]
        branch: String,
    },
    Action {
        #[command(flatten)]
        request: DesktopRequest,
        #[arg(long, value_enum)]
        action: DesktopActionArg,
        #[arg(long)]
        branch: Option<String>,
    },
}
```

Add this hidden `Commands` variant before the hidden shortcuts:

```rust
#[command(hide = true)]
Desktop {
    #[command(subcommand)]
    command: DesktopCommands,
},
```

Add `mod desktop;` to `src/lib.rs`. In `src/cli/mod.rs`, immediately after `let cli = Cli::parse();`, dispatch `Commands::Desktop` before config initialization:

```rust
if let Some(Commands::Desktop { command }) = &cli.command {
    let result = match command.clone() {
        DesktopCommands::Snapshot { request } => crate::desktop::run_snapshot(
            request.repo,
            request.schema_version,
            request.request_id,
        ),
        DesktopCommands::Diff { request, branch } => crate::desktop::run_diff(
            request.repo,
            request.schema_version,
            request.request_id,
            branch,
        ),
        DesktopCommands::Action { request, action, branch } => crate::desktop::run_action(
            request.repo,
            request.schema_version,
            request.request_id,
            action.into(),
            branch,
        ),
    };
    return result;
}
```

Implement `From<DesktopActionArg> for crate::desktop::protocol::DesktopAction` in `src/cli/args.rs`, mapping all four variants explicitly.

- [ ] **Step 5: Implement schema validation and JSON-only failure output**

Create `src/desktop/mod.rs` with module declarations, `validate_schema`, `emit_terminal`, and three public dispatch functions. Until later tasks fill the successful paths, valid snapshot/diff/action requests return stable `not_implemented` terminal errors. Unsupported schemas must emit `TerminalEvent::<serde_json::Value>::failure(...)`, flush stdout, and then return `anyhow::bail!("desktop request failed: unsupported_schema")` so the process exits non-zero.

Use this helper exactly:

```rust
fn emit_terminal<T: serde::Serialize>(event: &protocol::TerminalEvent<'_, T>) -> anyhow::Result<()> {
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, event)?;
    writeln!(stdout)?;
    stdout.flush()?;
    Ok(())
}
```

- [ ] **Step 6: Run targeted tests and CLI help checks**

Run: `cargo fmt --all`

Run: `cargo nextest run desktop_tests::rejects_unsupported_desktop_schema_with_machine_error`

Expected: PASS.

Run: `cargo run -- --help | rg '^  desktop'`

Expected: exit 1 from `rg`, proving the command remains hidden.

- [ ] **Step 7: Commit and update the draft PR**

```bash
git add src/desktop src/lib.rs src/cli/args.rs src/cli/mod.rs tests/all_tests.rs tests/desktop_tests.rs
git commit -m "feat: add desktop engine protocol"
st ss --draft --no-prompt --yes
```

Expected: one focused commit and a created or updated draft PR for `codex/cesar/native-desktop`.

---

### Task 2: Repository Snapshot Contract

**Files:**
- Modify: `src/desktop/protocol.rs`
- Create: `src/desktop/snapshot.rs`
- Modify: `src/desktop/mod.rs`
- Modify: `tests/desktop_tests.rs`

**Interfaces:**
- Consumes: `TerminalEvent<T>`, `DesktopError`, `GitRepo::open_from_path`, `Stack::load`, `CiCache::load`
- Produces: `snapshot::build(repo_path: &Path) -> Result<RepositorySnapshot, DesktopError>`
- Produces: `RepositorySnapshot`, `BranchSnapshot`, `RepositoryState`, `RecommendedAction`, and `PullRequestSnapshot`

- [ ] **Step 1: Add failing snapshot happy-path and invalid-repository tests**

Append tests that create `feature/base` and `feature/ui` with `st create`, invoke `desktop snapshot`, and assert:

```rust
assert!(output.status.success());
let event = terminal_json(&output);
assert_eq!(event["ok"], true);
assert_eq!(event["data"]["trunk"], "main");
assert_eq!(event["data"]["current_branch"], "feature/ui");
assert_eq!(event["data"]["repository_state"], "normal");
let names = event["data"]["branches"].as_array().unwrap().iter()
    .map(|branch| branch["name"].as_str().unwrap())
    .collect::<Vec<_>>();
assert_eq!(names, vec!["feature/ui", "feature/base", "main"]);
assert_eq!(event["data"]["branches"][0]["parent"], "feature/base");
assert_eq!(event["data"]["branches"][0]["recommended_action"], "submit_stack");
```

The invalid-repository test uses `tempfile::TempDir`, expects a non-zero exit, and asserts `error.code == "invalid_repository"` and `error.recovery == "choose_repository"`.

- [ ] **Step 2: Run both tests and verify the valid request still reports `not_implemented`**

Run: `cargo nextest run desktop_tests::snapshot`

Expected: FAIL for the happy path and invalid-repository assertions because Task 1 has only the protocol shell.

- [ ] **Step 3: Add snapshot protocol types**

Add these serializable types to `src/desktop/protocol.rs`:

```rust
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryState { Normal, RebaseInProgress, ConflictInProgress }

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecommendedAction { None, Checkout, Restack, SubmitStack, OpenPr }

#[derive(Debug, Serialize)]
pub struct PullRequestSnapshot {
    pub number: u64,
    pub state: String,
    pub is_draft: bool,
    pub url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BranchSnapshot {
    pub name: String,
    pub parent: Option<String>,
    pub column: usize,
    pub is_current: bool,
    pub is_trunk: bool,
    pub ahead: usize,
    pub behind: usize,
    pub needs_restack: bool,
    pub has_remote: bool,
    pub unpushed: usize,
    pub unpulled: usize,
    pub pull_request: Option<PullRequestSnapshot>,
    pub ci_state: Option<String>,
    pub recommended_action: RecommendedAction,
}

#[derive(Debug, Serialize)]
pub struct RepositorySnapshot {
    pub generation: String,
    pub repository_path: String,
    pub repository_name: String,
    pub trunk: String,
    pub current_branch: String,
    pub repository_state: RepositoryState,
    pub dirty: bool,
    pub branches: Vec<BranchSnapshot>,
}
```

- [ ] **Step 4: Implement deterministic display order and generation**

In `src/desktop/snapshot.rs`, implement an iterative post-order traversal matching `status` and the TUI: sort trunk children alphabetically, give siblings `base_column + sibling_index`, emit children before parents, then append trunk. Use `visiting` and `emitted` sets to terminate on malformed cyclic metadata.

Compute generation with `DefaultHasher` over the canonical workdir, current branch, ordered branch names, and each branch tip returned by `repo.branch_commit(name)`; format it as 16 lowercase hex digits.

- [ ] **Step 5: Implement snapshot construction**

`build` must:

1. Open only the passed path with `GitRepo::open_from_path`.
2. Load `Stack`, current branch, dirty state, conflict files, and rebase state.
3. Load cached CI via `CiCache::load(repo.git_dir()?)` without a network request.
4. Compute ahead/behind against each tracked parent, remote divergence with `commits_vs_remote`, and `has_remote` with existing helpers.
5. Resolve PR URLs only when `RemoteInfo::from_repo` succeeds; local data must remain available when forge configuration is absent.
6. Choose the recommendation in this priority: trunk → none; non-current → checkout; needs restack → restack; no PR and ahead > 0 → submit stack; PR present → open PR; otherwise none.

Map `GitRepo::open_from_path` failures to:

```rust
DesktopError::operation(
    "invalid_repository",
    "The selected folder is not a Git repository.",
    error.to_string(),
    "choose_repository",
)
```

- [ ] **Step 6: Wire snapshot success and failure into `src/desktop/mod.rs`**

Validate schema first, then call `snapshot::build`. Emit `TerminalEvent::success` on success. On `DesktopError`, emit `TerminalEvent::<RepositorySnapshot>::failure`, flush it, and return a non-zero `anyhow` error without writing any additional stdout.

- [ ] **Step 7: Run targeted tests and lint**

Run: `cargo fmt --all`

Run: `cargo nextest run desktop_tests::snapshot`

Expected: both snapshot tests PASS.

Run: `cargo clippy --lib --bins --tests -- -D warnings`

Expected: PASS with no warnings.

- [ ] **Step 8: Commit and update the draft PR**

```bash
git add src/desktop tests/desktop_tests.rs
git commit -m "feat: expose desktop repository snapshots"
st ss --draft --no-prompt --yes
```

---

### Task 3: Structured, Bounded Diff Contract

**Files:**
- Modify: `src/desktop/protocol.rs`
- Create: `src/desktop/diff.rs`
- Modify: `src/desktop/mod.rs`
- Modify: `tests/desktop_tests.rs`

**Interfaces:**
- Consumes: `GitRepo::diff_stat`, `GitRepo::diff_against_parent`, snapshot branch-parent semantics
- Produces: `diff::build(repo_path: &Path, branch: &str) -> Result<DiffSnapshot, DesktopError>`
- Produces: `DiffSnapshot`, `DiffFileSnapshot`, `DiffLineSnapshot`, and `DiffLineKind`

- [ ] **Step 1: Add failing diff tests for normal, empty, missing, and oversized patches**

Add four tests:

- A committed text change returns file stats and `addition`/`deletion` lines.
- An empty tracked branch returns `files == []`, `lines == []`, and `truncated == false`.
- An unknown branch returns `branch_not_found` with recovery `refresh`.
- A generated committed file larger than 448 KiB returns `truncated == true`, stdout stays below 512 KiB, and the process succeeds.

Use this core assertion for the normal case:

```rust
assert_eq!(event["data"]["branch"], "feature/diff");
assert_eq!(event["data"]["parent"], "main");
assert_eq!(event["data"]["files"][0]["path"], "src/example.txt");
assert!(event["data"]["lines"].as_array().unwrap().iter().any(|line| line["kind"] == "addition"));
assert_eq!(event["data"]["truncated"], false);
```

- [ ] **Step 2: Run the tests and verify they fail with `not_implemented`**

Run: `cargo nextest run desktop_tests::diff`

Expected: FAIL for all four new tests.

- [ ] **Step 3: Add exact diff protocol types and bounds**

Add to `src/desktop/protocol.rs`:

```rust
pub const MAX_DIFF_TEXT_BYTES: usize = 448 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineKind { File, Hunk, Context, Addition, Deletion, Metadata }

#[derive(Debug, Serialize)]
pub struct DiffFileSnapshot { pub path: String, pub additions: usize, pub deletions: usize }

#[derive(Debug, Serialize)]
pub struct DiffLineSnapshot { pub kind: DiffLineKind, pub text: String }

#[derive(Debug, Serialize)]
pub struct DiffSnapshot {
    pub generation: String,
    pub branch: String,
    pub parent: String,
    pub additions: usize,
    pub deletions: usize,
    pub files: Vec<DiffFileSnapshot>,
    pub lines: Vec<DiffLineSnapshot>,
    pub truncated: bool,
}
```

- [ ] **Step 4: Implement line classification and bounded collection**

Use this classifier in `src/desktop/diff.rs`:

```rust
fn classify(line: &str) -> DiffLineKind {
    if line.starts_with("diff --git ") { DiffLineKind::File }
    else if line.starts_with("@@") { DiffLineKind::Hunk }
    else if line.starts_with("+++") || line.starts_with("---") || line.starts_with("index ") { DiffLineKind::Metadata }
    else if line.starts_with('+') { DiffLineKind::Addition }
    else if line.starts_with('-') { DiffLineKind::Deletion }
    else { DiffLineKind::Context }
}
```

Before pushing each line, calculate `text_bytes + line.len() + 1`. If it would exceed `MAX_DIFF_TEXT_BYTES`, set `truncated = true` and stop. Do not cut a UTF-8 line. Sum additions/deletions from `diff_stat`, and reuse the snapshot generation helper so stale responses can be rejected by the app.

- [ ] **Step 5: Wire diff success/error output and verify the transport bound**

Replace the Task 1 diff stub in `src/desktop/mod.rs`. Serialize the terminal event to a `Vec<u8>` before stdout; if it exceeds `native_sdk`'s 512 KiB transport limit, return `bridge_payload_too_large` rather than printing a partial document. Keep the 448 KiB text cap low enough that the tested envelope remains below the transport limit.

- [ ] **Step 6: Run tests and commit**

Run: `cargo fmt --all`

Run: `cargo nextest run desktop_tests::diff`

Expected: all four diff tests PASS.

```bash
git add src/desktop tests/desktop_tests.rs
git commit -m "feat: expose structured desktop diffs"
st ss --draft --no-prompt --yes
```

---

### Task 4: Allow-listed Desktop Actions and Progress Events

**Files:**
- Modify: `src/desktop/protocol.rs`
- Create: `src/desktop/action.rs`
- Modify: `src/desktop/mod.rs`
- Modify: `tests/desktop_tests.rs`

**Interfaces:**
- Consumes: `DesktopAction`, existing CLI checkout/restack/submit behavior, `resolve_pr_number`, `RemoteInfo`
- Produces: `action::run(repo_path: &Path, action: DesktopAction, branch: Option<&str>, request_id: &str) -> Result<ActionResult, DesktopError>`
- Produces: newline-delimited `ProgressEvent` values followed by one terminal `ActionResult`

- [ ] **Step 1: Add failing action tests**

Cover:

- Checkout changes the real temporary repository's current branch and returns success.
- Restack rejects a dirty repository before starting with `dirty_repository`.
- Unknown branch returns `branch_not_found`.
- Open PR without metadata/remote returns `no_pull_request` without changing branches.
- Submit stack without a remote produces one terminal JSON error and never leaks child human output into stdout.
- Unit-test the exact command plan for restack and submit: checkout selected branch when needed, then `restack --quiet` or `submit --no-prompt --yes --quiet`.

- [ ] **Step 2: Run action tests and verify the stub fails**

Run: `cargo nextest run desktop_tests::action`

Expected: FAIL because actions are not implemented.

- [ ] **Step 3: Define action output and progress helpers**

Add:

```rust
#[derive(Debug, Serialize)]
pub struct ActionResult {
    pub action: &'static str,
    pub branch: Option<String>,
    pub summary: String,
}
```

Implement `emit_progress(request_id, phase, message)` with `serde_json::to_writer`, one newline, and an immediate flush. This is the only non-terminal stdout allowed for action requests.

- [ ] **Step 4: Implement captured child execution without a shell**

Use:

```rust
fn run_stax_child(repo_path: &Path, args: &[&str]) -> Result<std::process::Output, DesktopError> {
    let executable = std::env::current_exe().map_err(|error| DesktopError::operation(
        "engine_unavailable", "The bundled stax engine could not locate itself.", error.to_string(), "reinstall_app"
    ))?;
    std::process::Command::new(executable)
        .args(args)
        .current_dir(repo_path)
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| DesktopError::operation(
            "operation_failed", "The stax operation could not start.", error.to_string(), "retry"
        ))
}
```

Treat non-zero child status as a desktop error whose details include captured stdout and stderr. Classify preconditions before spawning: missing branch, dirty repository for restack/submit, rebase in progress, and conflict files.

- [ ] **Step 5: Implement the four exact action plans**

- `checkout`: run `checkout <branch>`.
- `restack`: if needed run `checkout <branch>`, then `restack --quiet`.
- `submit-stack`: if needed run `checkout <branch>`, then `submit --no-prompt --yes --quiet`.
- `open-pr`: resolve the selected branch's PR without checkout, derive the URL with `RemoteInfo`, and call `/usr/bin/open <url>` directly. When `STAX_DESKTOP_NO_OPEN=1`, return the URL without launching so integration tests remain hermetic.

Emit phases `validating`, `checking_out`, `restacking`, `submitting`, or `opening_pr` before the corresponding work. Captured child output must never be replayed to stdout.

- [ ] **Step 6: Run action tests, CLI tests, and commit**

Run: `cargo fmt --all`

Run: `cargo nextest run desktop_tests::action cli::tests`

Expected: PASS.

```bash
git add src/desktop tests/desktop_tests.rs
git commit -m "feat: add desktop stack actions"
st ss --draft --no-prompt --yes
```

---

### Task 5: Native SDK Scaffold and Protocol Parser

**Files:**
- Create: `desktop/package.json`
- Create: `desktop/package-lock.json`
- Create: `desktop/app.zon`
- Create: `desktop/assets/icon.svg`
- Create: `desktop/src/protocol.zig`
- Create: `desktop/src/tests.zig`
- Modify: `.gitignore`

**Interfaces:**
- Consumes: Rust schema version 1 JSON from Tasks 1-4
- Produces: Zig mirrors `TerminalEnvelope(T)`, `ProgressEvent`, `RepositorySnapshot`, `DiffSnapshot`, `ActionResult`, and `ProtocolError`
- Produces: `parseTerminal(T, allocator, bytes) !TerminalEnvelope(T)` and `parseProgress(allocator, bytes) !ProgressEvent`

- [ ] **Step 1: Generate the Native SDK baseline and pin the CLI**

Run: `npx -y @native-sdk/cli@0.4.1 init desktop`

Generated files are the explicit TDD exception. Replace the generated package metadata with:

```json
{
  "private": true,
  "devDependencies": { "@native-sdk/cli": "0.4.1" },
  "scripts": {
    "check": "native check",
    "test": "native test",
    "build": "native build -Dautomation=true",
    "dev": "native dev"
  }
}
```

Run: `npm install --prefix desktop --package-lock-only`

Add `/desktop/node_modules`, `/desktop/zig-cache`, `/desktop/zig-out`, and `/desktop/dist` to `.gitignore`.

- [ ] **Step 2: Add failing Zig protocol tests before parser implementation**

Replace generated counter tests with tests that parse a complete snapshot, a truncated diff, an error terminal event, and a progress event. The snapshot fixture must include two branches and verify enum strings, optional PR data, and generation. Also assert malformed JSON returns an error and schema 2 is rejected by `expectSchema`.

Use this parser-facing API in the tests:

```zig
var arena_state = std.heap.ArenaAllocator.init(testing.allocator);
defer arena_state.deinit();
const envelope = try protocol.parseTerminal(protocol.RepositorySnapshot, arena_state.allocator(), snapshot_json);
try testing.expect(envelope.ok);
try testing.expectEqualStrings("feature/ui", envelope.data.?.branches[0].name);
try testing.expectEqual(protocol.RecommendedAction.submit_stack, envelope.data.?.branches[0].recommended_action);
```

- [ ] **Step 3: Run Native tests and verify missing parser symbols**

Run: `npm run --prefix desktop test`

Expected: FAIL because `protocol.zig` and its parser functions do not exist.

- [ ] **Step 4: Implement exact Zig protocol mirrors**

Define enums with Rust's snake-case spellings and these envelope factories:

```zig
pub const schema_version: u32 = 1;

pub fn TerminalEnvelope(comptime T: type) type {
    return struct {
        schema_version: u32,
        request_id: []const u8,
        @"type": []const u8,
        ok: bool,
        data: ?T = null,
        @"error": ?ProtocolError = null,
    };
}

pub fn parseTerminal(comptime T: type, allocator: std.mem.Allocator, bytes: []const u8) !TerminalEnvelope(T) {
    const value = try std.json.parseFromSliceLeaky(TerminalEnvelope(T), allocator, bytes, .{ .ignore_unknown_fields = false });
    try expectSchema(value.schema_version);
    if (!std.mem.eql(u8, value.@"type", "result")) return error.UnexpectedEventType;
    if (value.ok == (value.data == null)) return error.InvalidTerminalEnvelope;
    return value;
}
```

Mirror every Rust field from `RepositorySnapshot`, `BranchSnapshot`, `PullRequestSnapshot`, `DiffSnapshot`, `DiffFileSnapshot`, `DiffLineSnapshot`, `ActionResult`, `ProtocolError`, and `ProgressEvent` exactly. Keep optional fields optional; do not tolerate unknown fields so schema drift fails loudly.

- [ ] **Step 5: Replace the generated app manifest and icon**

Replace `desktop/app.zon` with this manifest:

```zig
.{
    .id = "dev.cesarferreira.stax.desktop",
    .name = "Stax",
    .display_name = "Stax",
    .description = "A native desktop workspace for stacked Git branches and pull requests.",
    .version = "0.1.0",
    .icons = .{"assets/icon.svg"},
    .platforms = .{"macos"},
    .permissions = .{ "view", "command", "dialog", "filesystem", "clipboard" },
    .capabilities = .{ "native_views", "gpu_surfaces", "shortcuts" },
    .shortcuts = .{
        .{ .id = "stax.refresh", .key = "r", .modifiers = .{"primary"} },
        .{ .id = "stax.search", .key = "f", .modifiers = .{"primary"} },
        .{ .id = "stax.restack", .key = "r", .modifiers = .{ "primary", "shift" } },
        .{ .id = "stax.submit", .key = "s", .modifiers = .{ "primary", "shift" } },
        .{ .id = "stax.open-pr", .key = "o", .modifiers = .{"primary"} },
        .{ .id = "stax.dismiss", .key = "escape" },
    },
    .shell = .{ .windows = .{.{
        .label = "main",
        .title = "Stax",
        .width = 1180,
        .height = 760,
        .min_width = 880,
        .min_height = 560,
        .restore_state = true,
        .restore_policy = "center_on_primary",
        .titlebar = "hidden_inset_tall",
        .views = .{.{
            .label = "stax-canvas",
            .kind = "gpu_surface",
            .fill = true,
            .role = "Stax workspace",
            .accessibility_label = "Stax",
            .gpu_backend = "metal",
            .gpu_pixel_format = "bgra8_unorm",
            .gpu_present_mode = "timer",
            .gpu_alpha_mode = "opaque",
            .gpu_color_space = "srgb",
            .gpu_vsync = true,
        }},
    }}},
    .security = .{ .navigation = .{
        .allowed_origins = .{ "zero://app", "zero://inline" },
        .external_links = .{ .action = "deny" },
    } },
    .web_engine = "system",
    .cef = .{ .dir = "third_party/cef/macos", .auto_install = false },
}
```

Create `desktop/assets/icon.svg` with:

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">
  <rect width="1024" height="1024" rx="224" fill="#111315"/>
  <rect x="112" y="112" width="800" height="800" rx="176" fill="#E8A93B"/>
  <path fill="#111315" d="M286 386h95v-92h94v92h116v82H475v166c0 41 18 59 58 59 23 0 44-6 62-16v84c-25 14-57 21-96 21-78 0-118-43-118-129V468h-95v-82Zm331 0h95v52c28-42 67-63 118-63 15 0 29 2 42 7v94c-17-7-35-10-55-10-64 0-105 39-105 111v196h-95V386Z"/>
</svg>
```

- [ ] **Step 6: Run Native checks and commit**

Run: `npm run --prefix desktop check`

Run: `npm run --prefix desktop test`

Expected: both PASS.

```bash
git add .gitignore desktop
git commit -m "feat: scaffold Native SDK desktop app"
st ss --draft --no-prompt --yes
```

---

### Task 6: Native Model, Engine Bridge, and Deterministic Effects

**Files:**
- Create: `desktop/src/engine_bridge.zig`
- Create: `desktop/src/model.zig`
- Modify: `desktop/src/tests.zig`

**Interfaces:**
- Consumes: Task 5 protocol types and Native SDK `Effects(Msg)`
- Produces: `Model`, `Msg`, `update(model: *Model, msg: Msg, fx: *Effects)`, `boot(model: *Model, fx: *Effects)`
- Produces: `engine_bridge.requestSnapshot`, `requestDiff`, and `requestAction`

- [ ] **Step 1: Add failing model/effect tests**

Using `Effects.executor = .fake`, test:

- Boot requests persisted recent repositories.
- Selecting a repository creates exact argv: bundled engine, `desktop`, `snapshot`, `--repo`, path, schema 1, request ID.
- Snapshot success selects the current branch and requests its diff.
- Changing selection increments generation and late diff responses are ignored.
- A second mutation is refused while one action is active.
- Restack/submit enter confirmation state before any action spawn.
- Action completion clears mutation state and requests a fresh snapshot.
- Malformed, truncated, spawn-failed, and schema-mismatched effect exits create actionable bridge errors.

- [ ] **Step 2: Run tests and verify missing model/bridge symbols**

Run: `npm run --prefix desktop test`

Expected: FAIL because `model.zig` and `engine_bridge.zig` do not exist.

- [ ] **Step 3: Implement safe argument-array builders**

Use fixed effect keys `1` snapshot, `2` diff, `3` action, `4` recent-file read, and `5` recent-file write. Each engine request must pass the resolved engine path as argv[0], never `st`.

Snapshot and diff use `.collect` with terminal exit messages. Action uses `.lines`, `max_line_bytes = 256 * 1024`, an `on_line` progress/result message, and an exit message. Before a new diff, call `fx.cancel(diff_key)` so old process work is reaped; still check request IDs because a completion may already be queued.

- [ ] **Step 4: Implement model ownership and parser arenas**

`Model` owns separate `ArenaAllocator` values for snapshots and diffs, fixed buffers for engine/store/repository paths and error/status text, a `TextBuffer(96)` filter, recent repository buffers, selection, pane splits, monotonic request ID, active snapshot generation, confirmation enum, and optional active action.

Expose `init(allocator)`, `deinit()`, `setEnginePath`, `setStorePath`, `repositoryPath`, `selectedBranch`, `branchRows(arena)`, `diffRows(arena)`, `statusLine(arena)`, and boolean/query methods used by markup.

When parsing an exit payload, reject `output_truncated`, reset only the destination arena, parse with Task 5 helpers, copy any error text into fixed model storage, and retain the other arena's data.

- [ ] **Step 5: Implement update transitions and persistence format**

Use newline-separated canonical paths for recent repositories. De-duplicate, move the selected path to the front, remove missing paths on load, and cap at ten. `boot` reads the recent file; the first valid path requests a snapshot, otherwise the model requests the folder picker.

Restack and submit set `confirmation = .restack` or `.submit_stack`; `confirm_action` starts the action. Checkout and open PR start immediately. An action result always requests a new snapshot; a failed action preserves the last valid snapshot and exposes retry/copy-diagnostic text.

- [ ] **Step 6: Run model tests, formatting, and commit**

Run: `npm run --prefix desktop test`

Expected: all protocol and model/effect tests PASS.

Run: `zig fmt desktop/src/*.zig`

```bash
git add desktop/src
git commit -m "feat: connect desktop model to stax engine"
st ss --draft --no-prompt --yes
```

---

### Task 7: Three-pane Workshop UI, Folder Picker, and Lifecycle Refresh

**Files:**
- Create: `desktop/src/app.native`
- Create: `desktop/src/main.zig`
- Modify: `desktop/src/model.zig`
- Modify: `desktop/src/tests.zig`

**Interfaces:**
- Consumes: Task 6 `Model`, `Msg`, `update`, `boot`
- Produces: compiled `app.native` view, `AppHost` runtime wrapper, `workshopTokens`, folder selection, and app-activation refresh

- [ ] **Step 1: Add failing widget, layout, keyboard, and host tests**

Test the real compiled/interpreted view for:

- Stack, Branch, and Patch pane labels.
- Current/selected branch row and PR/CI badges.
- Clicking a branch selects without checkout and requests a diff.
- Return dispatches checkout; Command-R refreshes; Command-F focuses filter.
- Restack and submit open confirmation dialogs; cancel spawns nothing; confirm spawns exactly one action.
- Patch additions/deletions receive their semantic styles and a truncated patch shows its notice.
- Empty, loading, invalid-repository, auth/network stale, conflict, bridge-failure, and schema-mismatch states render actionable copy.
- Layout at 1180×760 and 880×560 has no overflow and keeps all three panes visible.
- Accessibility labels exist for every action and pane.
- NullPlatform folder picker dispatches the selected path, and lifecycle `.activate` requests refresh only when no mutation is active.

- [ ] **Step 2: Run Native tests and verify the generated counter UI fails expectations**

Run: `npm run --prefix desktop test`

Expected: FAIL on the first missing `Stack` pane assertion.

- [ ] **Step 3: Implement the complete declarative view**

Build `app.native` as a root `<stack>` containing:

- Hidden-inset titlebar row with traffic-light spacer, repository menu button, canonical path, refresh button, and keyboard hints.
- Nested `<split>` panes with model-owned fractions.
- Left `<tree>`/`<for each="branchRows">` list items, filtering, selection, branch relationship glyph, and badges.
- Center inspector with selected branch, parent/divergence, PR/CI pills, recommended action, and the four visible action buttons.
- Right scroll/virtual list over `diffRows`, file/diffstat header, semantic line colors, loading/empty/binary/truncated states.
- Modal restack and submit confirmation dialogs with cancel/confirm controls.
- Modal error details with Retry, Choose Repository, and Copy Diagnostics controls selected from the fixed recovery enum.
- Bottom status bar with operation phase or keyboard help.

Use only Native SDK components and bindings; do not embed HTML or a WebView.

- [ ] **Step 4: Implement Workshop tokens and compiled view wiring**

Use charcoal `#111315` background, `#17191C` surface, `#23272B` selected surface, amber `#E8A93B` accent, green `#65CE91`, red `#EC7373`, and subdued borders `#30343A`. System font remains primary; mono typography is used for branch names, badges, and diff text.

Use `UiAppWithFeatures(Model, Msg, .{ .runtime_markup = builtin.mode == .Debug })`, a compiled `CompiledMarkupView` for release, and markup watch only in Debug.

- [ ] **Step 5: Implement `AppHost` around Native SDK's `App`**

Store the inner `native_sdk.App` and `*StaxApp`. Delegate scene/start/event/stop/replay to the inner app. After delegated events:

- If `model.choose_repository_requested`, call `runtime.showOpenDialog` with `allow_directories = true`, `allow_multiple = false`, then synchronously dispatch `.repository_selected` with the returned path.
- On `LifecycleEvent.activate`, dispatch `.app_activated`; the model refreshes only when a repository exists and no mutation is active.

This wrapper is required because Native SDK v0.4.1 exposes folder dialogs through `Runtime`, not through `Effects`; do not replace it with `osascript` or a shell.

- [ ] **Step 6: Resolve dev/package paths and application-support storage**

Use `init.environ_map.get("STAX_DESKTOP_ENGINE")` in development. Without the override, derive the executable directory with `std.process.executableDirPath(init.io, ...)` and join `../Resources/bin/st`. Resolve the recent-repository store through `native_sdk.app_dirs` under app name `stax-desktop`.

- [ ] **Step 7: Run Native checks/build and commit**

Run: `npm run --prefix desktop check`

Run: `npm run --prefix desktop test`

Run: `npm run --prefix desktop build`

Expected: all three PASS and `desktop/zig-out/bin/Stax` exists.

```bash
git add desktop/src desktop/app.zon desktop/assets
git commit -m "feat: build Native SDK stack workspace"
st ss --draft --no-prompt --yes
```

---

### Task 8: Packaging, Documentation, Full Verification, and PR Handoff

**Files:**
- Create: `desktop/scripts/package-macos.sh`
- Create: `desktop/scripts/smoke-macos.sh`
- Modify: `Makefile`
- Modify: `README.md`
- Create: `docs/interface/desktop.md`
- Modify: `mkdocs.yml`
- Modify: `skills.md`
- Modify: `docs/superpowers/specs/2026-07-09-stax-desktop-native-design.md`

**Interfaces:**
- Consumes: release Rust binary, Native SDK build, completed desktop tests
- Produces: `desktop/dist/Stax.app`, `make desktop-*` targets, user documentation, final draft PR

- [ ] **Step 1: Add a failing package smoke script first**

Create `desktop/scripts/smoke-macos.sh` that exits non-zero unless all of these hold:

```bash
test -x desktop/dist/Stax.app/Contents/MacOS/Stax
test -x desktop/dist/Stax.app/Contents/Resources/bin/st
codesign --verify --deep --strict desktop/dist/Stax.app
STAX_DISABLE_UPDATE_CHECK=1 desktop/dist/Stax.app/Contents/Resources/bin/st desktop snapshot \
  --repo "$FIXTURE_REPO" --schema-version 1 --request-id smoke | jq -e \
  '.ok == true and .schema_version == 1 and .data.trunk != null'
```

The script creates and initializes its own temporary fixture repository, traps cleanup, and additionally launches the app with Native SDK automation enabled long enough to wait for a snapshot containing the `Stack`, `Branch`, and `Patch` accessibility labels. It prepends an empty temporary directory to `PATH` before launch so success proves the bundled engine path is used.

- [ ] **Step 2: Run the smoke script and verify the bundle is absent**

Run: `bash desktop/scripts/smoke-macos.sh`

Expected: FAIL at the first missing executable assertion.

- [ ] **Step 3: Implement deterministic package assembly**

Create `desktop/scripts/package-macos.sh` with `set -euo pipefail` and these exact phases:

1. `cargo build --release --bin st` from repository root.
2. `npm ci --prefix desktop`.
3. `npm run --prefix desktop check`, `test`, and `build`.
4. Remove only `desktop/dist/Stax.app`.
5. Run `(cd desktop && npm exec -- native package --target macos --output dist/Stax.app --signing none)`.
6. Create `Contents/Resources/bin`, copy `target/release/st`, and `chmod 755` it.
7. Ad-hoc sign after staging: `codesign --force --deep --sign - desktop/dist/Stax.app`.
8. Run `desktop/scripts/smoke-macos.sh`.

Add Make targets `desktop-check`, `desktop-test`, `desktop-build`, `desktop-dev`, `desktop-package`, and `desktop-smoke`; each delegates to the pinned npm scripts or packaging scripts.

- [ ] **Step 4: Document the shipped desktop surface**

Update README Highlights and build-from-source instructions. Create `docs/interface/desktop.md` covering installation, repository picker, three panes, four actions, confirmation behavior, shortcuts, refresh policy, error recovery, Apple Silicon limitation, and CLI-only workflows. Add it beside the TUI in `mkdocs.yml`.

Update `skills.md` with a concise “Desktop app” entry stating that `Stax.app` supports inspection, checkout, restack, submit stack, and open PR; all other workflows remain CLI/TUI-only.

Amend the design spec's transport section to record the discovered Native SDK v0.4.1 constraints: repository path is passed through `--repo`, snapshot/diff use collect mode, action uses line mode, and patches explicitly truncate at 448 KiB.

- [ ] **Step 5: Run targeted verification**

Run: `cargo nextest run desktop_tests::`

Expected: all desktop Rust tests PASS.

Run: `make desktop-check desktop-test desktop-build desktop-package`

Expected: Native validation/tests/build/package/smoke all PASS.

Run: `cargo fmt --all -- --check`

Run: `cargo clippy --all-targets --all-features -- -D warnings`

Expected: both PASS.

- [ ] **Step 6: Run the repository full suite through the required path**

Ensure Docker Desktop is running, then run: `make test`

Expected: exit 0 with no failing tests. If the only failure is inability to connect to the Docker socket, run `open -a Docker`, wait for Docker Desktop, and retry `make test`; do not fall back to the full native suite.

- [ ] **Step 7: Commit the final slice**

```bash
git add desktop/scripts Makefile README.md docs/interface/desktop.md mkdocs.yml skills.md docs/superpowers/specs/2026-07-09-stax-desktop-native-design.md
git commit -m "docs: package and document Stax desktop"
```

- [ ] **Step 8: Update and verify the stax draft PR**

```bash
st validate
st ls
st ss --draft --no-prompt --yes
st ll
gh pr view --json number,title,url,isDraft,headRefName,baseRefName,statusCheckRollup
```

Expected:

- Stack metadata validates.
- `codex/cesar/native-desktop` is the current tracked branch above `main`.
- The draft PR targets `main`, includes every task commit, and has a non-empty URL.
- No PR title, body, commit, or code contains agent attribution.

---

## Plan Self-check Matrix

| Approved requirement | Implemented by |
| --- | --- |
| Self-contained Native SDK macOS app | Tasks 5-8 |
| Bundled Rust engine, no `PATH` fallback | Tasks 6-8 |
| Versioned JSON protocol | Tasks 1-4 |
| Snapshot, lazy diff, four actions | Tasks 2-4, 6-7 |
| Three-pane Workshop UI | Task 7 |
| Native repository picker and recent repositories | Tasks 6-7 |
| No shell/arbitrary command execution | Tasks 4, 6 |
| Mutation confirmation and serialization | Tasks 4, 6-7 |
| Actionable error states and stale-response protection | Tasks 1-7 |
| Explicit 448 KiB patch truncation | Tasks 3, 7-8 |
| Rust, Native, package, and full-suite verification | Tasks 1-8 |
| README/docs/skills policy | Task 8 |
| Focused commits and stax draft PR | Every task, finalized in Task 8 |
