# Stax GUI Phase 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the native Stax GUI operational with typed repository-scoped checkout, branch creation, restack, submit, and pull-request workflows shared with CLI/TUI adapters, plus an unsigned developer Stax.app that `st gui [path]` can launch.

**Architecture:** `stax::application` owns presentation-neutral operation contracts and explicit-path business logic; `ops` remains application-agnostic and exposes only crate-visible receipt facts. Every mutation uses a private common-repository lease and shared rebase preflight, network operations retain trusted-remote credential boundaries, and GPUI applies structured events through generation-aware state that refreshes after success or side-effecting failure. CLI/TUI modules are presentation adapters, while GUI normal operations never invoke `st`, parse terminal output, or change process CWD.

**Tech Stack:** Rust 1.96, GPUI 0.2.2, Tokio, async-channel, unicode-segmentation, url, git2, Clap, existing stax transactions/receipts and forge clients, cargo-nextest, macOS LaunchServices, shell bundle assembly.

**Stacking:** Implement on `cesar/gpui-gui-phase-2`, based on Phase 1 PR #611 / `cesar/gpui-gui-phase-1-ready`. The Phase 2 pull request targets `cesar/gpui-gui-phase-1-ready`, not `main`.

**Phase boundary:** Phase 2 produces an unsigned local developer-preview `Stax.app` with provisional bundle identifier `dev.stax.Stax`. Final icon work, release metadata, signing, notarization, universal binaries, packaging, and distribution remain Phase 4.

---

## File map

### Shared operation contract and repository safety

- Create `src/application/operation.rs` — requests, events, receipts, warnings, error categories/details, side-effect classification, receipt conversion, and event framing.
- Modify `src/application/mod.rs` — export only the public operation contract and public repository operation entry points.
- Modify `src/application/repository.rs` — private common-Git-directory mutation registry, private mutation guard, shared rebase preflight, and runtime guard.
- Modify `src/ops/receipt.rs` — application-agnostic crate-visible accessors; no import or return type from `application`.
- Modify `src/ops/tx.rs` — receipt-returning transaction finalizers that preserve truthful in-memory success/failure receipts when persistence fails.
- Create `scripts/application-boundary-lint.sh` — enforce the application/terminal boundary.
- Create `scripts/application-boundary-lint-tests.sh` — prove forbidden dependencies/output are rejected.
- Modify `scripts/lint.sh` — run the architecture lint.
- Create `tests/application_operation_tests.rs` and modify `tests/all_tests.rs` — one unified integration-test module for public operation behavior.

### Extracted application operations

- Create `src/application/checkout.rs` — explicit-repository checkout with linked-worktree outcome data.
- Create `src/application/pull_request.rs` — read-only explicit-branch PR URL resolution.
- Create `src/application/branch_name.rs` — pure branch-name formatting/validation that returns warnings as data.
- Create `src/application/create.rs` — explicit-name empty child branch creation.
- Create `src/application/restack.rs` — presentation-neutral extraction of the complete `commands::restack::run_impl` mutation pipeline, deterministic scopes, and one failed-receipt finalizer.
- Create `src/application/submit.rs` — application-owned crate-visible submit preparation API plus presentation-neutral extraction of the complete `commands::submit::run` pipeline, including fetch freshness, publish refs/worktrees, discovery, force-with-lease pushes, PR updates, cleanup-before-unlock, and typed outcomes.
- Modify `src/engine/restack_preflight.rs` — return a pure boundary decision; application maps advisories to warning data.
- Modify `src/config/mod.rs`, `src/remote.rs`, `src/forge/{mod,gitlab,gitea}.rs`, `src/github/client.rs`, and `src/application/ci.rs` — preserve repository-local `remote.name`, keep provider/API/auth trust global-only, and enforce origin-bound credential/redirect construction for all noninteractive providers.

### CLI and TUI adapters

- Modify `src/commands/checkout.rs`, `src/commands/branch/create.rs`, `src/commands/restack.rs`, `src/commands/submit.rs`, `src/commands/resolve_pr.rs`, `src/commands/pr.rs`, and `src/commands/open.rs` — keep prompts/output/advanced modes, delegate matching business paths to application methods.
- Modify `src/tui/app.rs` and `src/tui/mod.rs` — represent safe migrated actions as typed requests; retain explicit legacy subprocess fallback only for unmigrated actions.
- Modify `tests/common/mod.rs` — child-process environment helper for trusted custom-endpoint tests; never mutate `STAX_CONFIG_DIR`, token variables, HOME, or CWD in the test process.

### Developer app and launcher

- Create `crates/stax-gui/resources/Info.plist.in` — minimal developer app template.
- Create `scripts/build-gui-app.sh` and `scripts/gui-app-tests.sh` — assemble, validate, and optionally install/register an unsigned local bundle.
- Modify `Makefile` — `gui-app`, `gui-app-test`, and `install-gui-app` targets.
- Modify `.github/workflows/rust-tests.yml` — retain the macOS GUI gate and add bundle validation.
- Modify `src/cli/args.rs` — define `GuiArgs` and `Commands::Gui`.
- Modify `src/cli/mod.rs` — dispatch `Commands::Gui` before initialization; do not define the variant here.
- Create `src/commands/gui.rs` and modify `src/commands/mod.rs` — injectable `open -n -b dev.stax.Stax --args <canonical-path>` launcher.
- Create `tests/gui_command_tests.rs` and register it in `tests/all_tests.rs`.

### GPUI operation experience

- Modify `crates/stax-gui/Cargo.toml` and `Cargo.lock` — add async-channel, url, and unicode-segmentation.
- Create `crates/stax-gui/src/operation.rs` — native/fake operation and browser services.
- Modify `crates/stax-gui/src/state.rs` — operation tokens, overlays, side-effect-aware refresh, banners, receipts, and interaction availability.
- Create `crates/stax-gui/src/views/text_input.rs` — minimal complete GPUI text input with UTF-16 and IME support.
- Create `crates/stax-gui/src/views/operation_overlay.rs` — create, restack, stash-and-restack, and submit confirmations.
- Modify `crates/stax-gui/src/lib.rs`, `crates/stax-gui/src/theme.rs`, and `crates/stax-gui/src/views/{mod,app,workspace,inspector_pane,tests}.rs`.
- Create `crates/stax-gui/src/views/operation_tests.rs`.

### User documentation

- Modify `README.md`, `docs/commands/core.md`, `docs/commands/reference.md`, `mkdocs.yml`, and `skills.md`.
- Create `docs/interface/gui.md`.

## Public contract and non-negotiable invariants

Create this exact public shape in `src/application/operation.rs`:

```rust
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationRequest {
    Checkout { branch: String },
    CreateBranch { name: String, parent: String },
    Restack { scope: RestackScope, auto_stash: bool },
    SubmitStack { new_pull_requests: PullRequestMode },
    ResolvePullRequestUrl { branch: String },
}

impl OperationRequest {
    pub fn is_mutating(&self) -> bool {
        !matches!(self, Self::ResolvePullRequestUrl { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestackScope {
    Branch(String),
    StackContaining(String),
    ThroughBranch(String),
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestMode {
    Draft,
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationStage {
    Validating,
    Preparing,
    CheckingOut,
    CreatingBranch,
    Restacking,
    Pushing,
    UpdatingPullRequests,
    ResolvingPullRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationProgress {
    pub stage: OperationStage,
    pub completed: usize,
    pub total: Option<usize>,
    pub branch: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationEvent {
    Started(OperationRequest),
    Progress(OperationProgress),
    Completed(OperationReceipt),
    Failed(OperationError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationSideEffects {
    None,
    RepositoryChanged,
    RemoteMayHaveChanged,
}

impl OperationSideEffects {
    pub fn requires_refresh(self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationReceipt {
    pub request: OperationRequest,
    pub summary: String,
    pub affected_branches: Vec<String>,
    pub outcome: OperationOutcome,
    pub transaction: Option<TransactionSummary>,
    pub warnings: Vec<OperationWarning>,
    pub side_effects: OperationSideEffects,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationOutcome {
    Checkout(CheckoutOutcome),
    BranchCreated { branch: String, parent: String },
    Restacked {
        branches: Vec<String>,
        skipped_frozen: Vec<String>,
    },
    Submitted { pull_requests: Vec<PullRequestReceipt> },
    PullRequestResolved { branch: String, url: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckoutOutcome {
    CheckedOut { branch: String },
    AlreadyCurrent { branch: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestChange {
    Created,
    Updated,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestReceipt {
    pub branch: String,
    pub number: u64,
    pub url: String,
    pub change: PullRequestChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionStatus {
    InProgress,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionSummary {
    pub id: String,
    pub kind: String,
    pub status: TransactionStatus,
    pub branches: Vec<String>,
    pub can_undo: bool,
    pub changed_remote_refs: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationWarning {
    BranchNameNormalized { original: String, normalized: String },
    RestackBoundaryAdjusted { branch: String, reason: String },
    StashRestoreFailed { worktree: PathBuf, diagnostic: String },
    SubmitReviewersUnsupported { provider: String, reviewers: Vec<String> },
    SubmitNativeStackAdvisory {
        reason: NativeStackAdvisory,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeStackAdvisory {
    GhUnavailable,
    ExtensionMissing,
    ExtensionOutdated,
    ForkedStack,
    AuthenticationUnsupported,
    FeatureDisabled,
    LinkRejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationErrorKind {
    RepositoryUnavailable,
    InitializationRequired,
    Authentication,
    Authorization,
    DirtyWorktree,
    PreconditionFailed,
    RebaseInProgress,
    RebaseConflict,
    LocalGit,
    Network,
    PartialRemoteUpdate,
    UnsupportedCapability,
    Busy,
    InvalidInput,
    Runtime,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationErrorDetails {
    None,
    Branch { branch: String },
    PullRequest { branch: String },
    AlreadyCheckedOutElsewhere { branch: String, path: PathBuf },
    Rebase { branch: Option<String>, worktree: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationError {
    pub request: OperationRequest,
    pub kind: OperationErrorKind,
    pub details: OperationErrorDetails,
    pub primary: String,
    pub action: String,
    pub diagnostic_chain: String,
    pub receipt: Option<OperationReceipt>,
    pub side_effects: OperationSideEffects,
}

pub trait OperationReporter {
    fn report(&mut self, event: OperationEvent);
}

impl<F> OperationReporter for F
where
    F: FnMut(OperationEvent),
{
    fn report(&mut self, event: OperationEvent) {
        self(event);
    }
}

pub struct NoopOperationReporter;

impl OperationReporter for NoopOperationReporter {
    fn report(&mut self, _event: OperationEvent) {}
}

pub type OperationResult = Result<OperationReceipt, OperationError>;
```

Maintain all of these invariants:

1. `src/ops` never imports `crate::application`; `TransactionSummary` conversion lives in `src/application/operation.rs`.
2. Mutation leases and acquisition methods are private or `pub(crate)` and are never re-exported.
3. Every public operation emits `Started` first and exactly one `Completed` or `Failed` last, including repository-open failures in `execute_repository_operation`. GPUI treats these terminal events as diagnostics only; the retained background result is the sole state-completion authority and calls `finish_operation` exactly once.
4. Checkout, create, restack, and submit acquire the private common-repository lease and run shared rebase-in-progress preflight over every affected current or linked worktree before any side effect. Rebase errors carry the canonical worktree path.
5. PR resolution is read-only: forge fallback never writes branch metadata.
6. Blocking network paths return `OperationErrorKind::Runtime` when entered directly from a Tokio runtime; they never construct/block a nested runtime.
7. Trusted-network resolution validates Git remote, global provider/API configuration, and credential host before lookup. The selected repository may supply only `remote.name`; provider/API/auth trust remains global-only. Repository submit preferences are loaded separately and cannot change trusted endpoints. Unknown custom hosts require explicit global trust.
8. Application modules import no GPUI, Ratatui, Crossterm, Dialoguer, command module, terminal progress, or color/output formatting and contain no `println!`/`eprintln!`.
9. A restack conflict preserves rebase state and failed receipt. A partial submit preserves pushed refs and failed receipt. Neither claims rollback.
10. GUI refresh is required after success and after errors whose `side_effects.requires_refresh()` is true; error and receipt remain visible across that refresh.
11. No active mutation exposes cancellation. Cancel/Escape exists only before confirmation/start.
12. A stale event/result/hydration value cannot update progress, receipt, error, selection, or snapshot.
13. Submit and restack move their existing mature pipelines intact; Phase 2 does not introduce a second planner/executor with reduced semantics.
14. Submit temporary refs/worktrees/resources are destroyed while the common-repository mutation lease is still held; the lease is the final `PreparedSubmit` field and therefore the final field dropped.
15. A success-receipt persistence failure never discards observed work: the typed error carries the successful in-memory receipt and exact local/remote side effects.

## 1. Operation contract, receipt conversion, and architecture lint

### Task 1: Define application-owned operation types and enforce the boundary

**Files:**
- Create: `src/application/operation.rs`
- Modify: `src/application/mod.rs`
- Modify: `src/ops/receipt.rs`
- Modify: `src/ops/tx.rs`
- Create: `scripts/application-boundary-lint.sh`
- Create: `scripts/application-boundary-lint-tests.sh`
- Modify: `scripts/lint.sh`
- Create: `tests/application_operation_tests.rs`
- Modify: `tests/all_tests.rs`
- Include in commit: `docs/superpowers/plans/2026-07-12-stax-gui-phase-2.md`

- [ ] **Step 1: Register the unified test module and write failing contract tests**

Add to `tests/all_tests.rs`:

```rust
#[path = "application_operation_tests.rs"]
mod application_operation_tests;
```

Create `tests/application_operation_tests.rs` with:

```rust
use stax::application::{
    OperationErrorKind, OperationRequest, OperationSideEffects, PullRequestMode,
};

#[test]
fn operation_requests_classify_mutations_and_refresh_effects() {
    assert!(OperationRequest::Checkout { branch: "feature".into() }.is_mutating());
    assert!(OperationRequest::SubmitStack {
        new_pull_requests: PullRequestMode::Draft,
    }
    .is_mutating());
    assert!(!OperationRequest::ResolvePullRequestUrl {
        branch: "feature".into(),
    }
    .is_mutating());
    assert!(!OperationSideEffects::None.requires_refresh());
    assert!(OperationSideEffects::RepositoryChanged.requires_refresh());
    assert!(OperationSideEffects::RemoteMayHaveChanged.requires_refresh());
}

#[test]
fn error_categories_are_copyable_and_cover_runtime_and_security_boundaries() {
    fn copy_kind(kind: OperationErrorKind) -> OperationErrorKind {
        kind
    }
    assert_eq!(
        copy_kind(OperationErrorKind::Authentication),
        OperationErrorKind::Authentication
    );
    assert_eq!(
        copy_kind(OperationErrorKind::Authorization),
        OperationErrorKind::Authorization
    );
    assert_eq!(copy_kind(OperationErrorKind::Runtime), OperationErrorKind::Runtime);
}
```

- [ ] **Step 2: Run the public contract test and verify the red state**

Run:

```bash
cargo nextest run application_operation_tests::operation_requests_classify_mutations_and_refresh_effects
```

Expected: compile failure for unresolved `OperationRequest`, `OperationErrorKind`, and `OperationSideEffects`.

- [ ] **Step 3: Implement the public contract and event framing**

Add the exact types from **Public contract and non-negotiable invariants**. Add:

```rust
pub(crate) fn report_operation(
    request: OperationRequest,
    reporter: &mut dyn OperationReporter,
    run: impl FnOnce(&mut dyn OperationReporter) -> OperationResult,
) -> OperationResult {
    reporter.report(OperationEvent::Started(request));
    match run(reporter) {
        Ok(receipt) => {
            reporter.report(OperationEvent::Completed(receipt.clone()));
            Ok(receipt)
        }
        Err(error) => {
            reporter.report(OperationEvent::Failed(error.clone()));
            Err(error)
        }
    }
}

impl std::fmt::Display for OperationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.primary)
    }
}

impl std::error::Error for OperationError {}
```

`primary` and `action` are safe user-facing strings. `diagnostic_chain` is `format!("{source:#}")` from the underlying error chain, is never included in `Display`, and is exposed to the GUI only through an explicit Copy Diagnostics control. Do not include credentials in any source error.

Add a crate-visible `OperationError::from_source(request, kind, details, primary, action, source, receipt, side_effects)` constructor that performs this separation. Unit-test a two-level `anyhow` source: `Display` equals only `primary`, `action` remains separate, and `diagnostic_chain` contains both source levels.

- [ ] **Step 4: Write failing receipt-conversion unit tests**

In `src/application/operation.rs` add unit tests constructing successful and failed `OpReceipt` fixtures:

```rust
#[test]
fn transaction_summary_uses_canonical_can_undo_for_success() {
    let receipt = receipt_with_status_and_local_ref(OpStatus::Success, Some("before"), Some("after"));
    let summary = TransactionSummary::from(&receipt);
    assert_eq!(summary.status, TransactionStatus::Succeeded);
    assert_eq!(summary.can_undo, receipt.can_undo());
    assert!(summary.can_undo);
}

#[test]
fn transaction_summary_maps_failure_without_changing_undo_semantics() {
    let receipt = receipt_with_status_and_local_ref(OpStatus::Failed, None, Some("after"));
    let summary = TransactionSummary::from(&receipt);
    assert_eq!(summary.status, TransactionStatus::Failed);
    assert_eq!(summary.can_undo, receipt.can_undo());
    assert!(!summary.can_undo);
}

#[test]
fn successful_finalization_preserves_receipt_when_persistence_fails() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let ops_dir = super::super::ops_dir(temp.path());
    std::fs::create_dir_all(&ops_dir).unwrap();
    let original_mode = std::fs::metadata(&ops_dir).unwrap().permissions().mode();
    std::fs::set_permissions(&ops_dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    let mut guard = PermissionGuard {
        temp,
        path: ops_dir,
        original_mode,
    };
    let transaction = Transaction {
        receipt: OpReceipt::new(
            "success-save-failure".into(),
            OpKind::Restack,
            guard.temp.path().display().to_string(),
            "main".into(),
            "feature".into(),
        ),
        git_dir: guard.temp.path().to_path_buf(),
        workdir: guard.temp.path().to_path_buf(),
        snapshotted: true,
        finished: false,
        quiet: true,
    };
    let finalized = transaction.finish_ok_preserving_receipt();
    assert_eq!(finalized.receipt.summary_status(), &OpStatus::Success);
    assert!(finalized.persistence_error.is_some());
    guard.restore();
}
```

In the same private Unix-only `src/ops/tx.rs` test module define `struct PermissionGuard { temp: tempfile::TempDir, path: PathBuf, original_mode: u32 }`; `restore(&mut self)` resets permissions with `PermissionsExt::from_mode`, and `Drop` calls `restore`. The test constructs private `Transaction` fields directly because it is a descendant test module. No repository fixture, unnamed helper, process-global mutation, or failpoint is introduced.

Run:

```bash
cargo nextest run --lib application::operation::tests::transaction_summary_
cargo nextest run --lib ops::tx::tests::successful_finalization_preserves_
```

Expected: compile failure because `From<&OpReceipt>`, the crate-visible receipt accessors, and `finish_ok_preserving_receipt` do not exist.

- [ ] **Step 5: Add application-agnostic receipt accessors and application-owned conversion**

In `src/ops/receipt.rs`, add only these crate-visible facts:

```rust
impl OpReceipt {
    pub(crate) fn summary_id(&self) -> &str {
        &self.op_id
    }

    pub(crate) fn summary_kind(&self) -> &'static str {
        self.kind.display_name()
    }

    pub(crate) fn summary_status(&self) -> &OpStatus {
        &self.status
    }

    pub(crate) fn summary_branch_names(&self) -> Vec<String> {
        let mut branches = Vec::new();
        for entry in &self.local_refs {
            let branch = entry
                .branch
                .strip_suffix(super::tx::METADATA_REF_LABEL_SUFFIX)
                .unwrap_or(&entry.branch);
            if !branches.iter().any(|existing| existing == branch) {
                branches.push(branch.to_string());
            }
        }
        branches
    }

    pub(crate) fn changed_remote_refs(&self) -> bool {
        self.remote_refs.iter().any(|entry| entry.oid_after.is_some())
    }
}
```

In `src/application/operation.rs`:

```rust
impl From<&crate::ops::receipt::OpReceipt> for TransactionSummary {
    fn from(receipt: &crate::ops::receipt::OpReceipt) -> Self {
        use crate::ops::receipt::OpStatus;
        Self {
            id: receipt.summary_id().to_string(),
            kind: receipt.summary_kind().to_string(),
            status: match receipt.summary_status() {
                OpStatus::InProgress => TransactionStatus::InProgress,
                OpStatus::Success => TransactionStatus::Succeeded,
                OpStatus::Failed => TransactionStatus::Failed,
            },
            branches: receipt.summary_branch_names(),
            can_undo: receipt.can_undo(),
            changed_remote_refs: receipt.changed_remote_refs(),
        }
    }
}
```

Do not add any `application` import, application return type, or transaction-summary method to `src/ops`.

- [ ] **Step 6: Preserve both successful and failed in-memory receipts**

In `src/ops/tx.rs`:

```rust
pub(crate) struct ReceiptFinalization {
    pub receipt: OpReceipt,
    pub persistence_error: Option<anyhow::Error>,
}

pub fn finish_ok(self) -> Result<()> {
    self.finish_ok_with_receipt().map(drop)
}

pub(crate) fn finish_ok_preserving_receipt(mut self) -> ReceiptFinalization {
    self.receipt.mark_success();
    let persistence_error = self.receipt.save(&self.git_dir).err();
    self.finished = true;
    ReceiptFinalization {
        receipt: self.receipt.clone(),
        persistence_error,
    }
}

pub(crate) fn finish_ok_with_receipt(self) -> Result<OpReceipt> {
    let finalized = self.finish_ok_preserving_receipt();
    match finalized.persistence_error {
        Some(error) => Err(error),
        None => Ok(finalized.receipt),
    }
}

pub fn finish_err(
    self,
    message: &str,
    failed_step: Option<&str>,
    failed_branch: Option<&str>,
) -> Result<()> {
    self.finish_err_with_receipt(message, failed_step, failed_branch)
        .map(drop)
}

pub(crate) fn finish_err_with_receipt(
    mut self,
    message: &str,
    failed_step: Option<&str>,
    failed_branch: Option<&str>,
) -> Result<OpReceipt> {
    self.receipt.mark_failed(message, failed_step, failed_branch);
    self.receipt.save(&self.git_dir)?;
    self.finished = true;
    if !self.quiet {
        self.print_recovery_hint();
    }
    Ok(self.receipt.clone())
}
```

Task 6 reuses this same `ReceiptFinalization` for failed receipts instead of introducing a second finalization struct. Application success paths call `finish_ok_preserving_receipt`: when persistence succeeds they return normally; when it fails they convert the successful in-memory `OpReceipt` into `TransactionSummary` and attach it to `OperationError::receipt`. Local mutations return `LocalGit` plus `RepositoryChanged`; submit after any observed push/PR change returns `PartialRemoteUpdate` plus `RemoteMayHaveChanged`. The persistence error is diagnostic context and cannot replace the actual operation outcome. Existing non-application callers keep `finish_ok_with_receipt`, while existing JSON, `OpReceipt::can_undo()`, `st undo`, and `st redo` semantics remain unchanged.

- [ ] **Step 7: Write and run the architecture-lint red test**

Create `scripts/application-boundary-lint-tests.sh` with a temporary fixture and this assertion helper:

```bash
#!/usr/bin/env bash
set -euo pipefail
root="$(mktemp -d)"
trap 'rm -rf "$root"' EXIT
mkdir -p "$root/src/application"
git -C "$root" init -q

assert_rejected() {
  local source="$1"
  local expected="$2"
  printf '%s\n' "$source" > "$root/src/application/checkout.rs"
  if bash scripts/application-boundary-lint.sh "$root" >"$root/output" 2>&1; then
    echo "expected boundary lint rejection for: $source" >&2
    exit 1
  fi
  rg -F "$expected" "$root/output" >/dev/null
}

printf '%s\n' 'pub fn clean() {}' > "$root/src/application/checkout.rs"
bash scripts/application-boundary-lint.sh "$root"
assert_rejected 'use crate::commands::submit;' 'command or TUI modules'
assert_rejected 'use crate::{commands::submit, git::GitRepo};' 'command or TUI modules'
assert_rejected 'use crate::commands as cli_commands;' 'command or TUI modules'
assert_rejected 'fn bad() { crate::commands::submit::run(); }' 'command or TUI modules'
assert_rejected 'use crate::{tui, git};' 'command or TUI modules'
assert_rejected 'use gpui::App;' 'presentation frameworks'
assert_rejected 'use {ratatui as terminal_ui, git2};' 'presentation frameworks'
assert_rejected 'fn bad() { ::crossterm::execute!(); }' 'presentation frameworks'
assert_rejected 'use crate::progress::LiveTimer;' 'terminal progress'
assert_rejected 'use crate::{progress as terminal_progress, git};' 'terminal progress'
assert_rejected 'use std::io::{stdout, IsTerminal};' 'terminal I/O'
assert_rejected 'println!("hidden terminal output");' 'terminal output macros'
assert_rejected 'std::eprintln!("hidden terminal error");' 'terminal output macros'

mkdir -p "$root/src/application/nested/future"
printf '%s\n' 'use dialoguer as prompt;' > "$root/src/application/nested/future/module.rs"
if bash scripts/application-boundary-lint.sh "$root" >"$root/output" 2>&1; then
  echo "expected recursive boundary lint rejection" >&2
  exit 1
fi
rg -F 'presentation frameworks' "$root/output" >/dev/null
```

Run:

```bash
bash scripts/application-boundary-lint-tests.sh
```

Expected: failure because `scripts/application-boundary-lint.sh` does not exist.

- [ ] **Step 8: Implement and wire the architecture lint**

Create `scripts/application-boundary-lint.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
root="${1:-.}"
files=()
while IFS= read -r file; do
  test -z "$file" || files+=("$root/$file")
done < <(
  cd "$root"
  git ls-files --cached --others --exclude-standard \
    'src/application/*.rs' 'src/application/**/*.rs' |
    LC_ALL=C sort -u
)

check() {
  local label="$1"
  local pattern="$2"
  if ((${#files[@]})) && rg -n "$pattern" "${files[@]}"; then
    echo "application boundary violation: $label" >&2
    exit 1
  fi
}

check "command or TUI modules" '(^|[^[:alnum:]_])((crate|super|self)::)?(\{[^}]*[,{][[:space:]]*)?(commands|tui)(::|[[:space:]};,]|$)'
check "presentation frameworks" '(^|[^[:alnum:]_])(gpui|ratatui|crossterm|dialoguer|colored|console)(::|[[:space:]};,]|$)'
check "terminal progress" '(^|[^[:alnum:]_])((crate|super|self)::)?(\{[^}]*[,{][[:space:]]*)?progress(::|[[:space:]};,]|$)'
check "terminal I/O" 'std::io::(\{[^}]*(stdin|stdout|stderr|IsTerminal)|stdin|stdout|stderr|IsTerminal)'
check "terminal output macros" '(^|[^[:alnum:]_])(std::)?(print|println|eprint|eprintln|dbg)![[:space:]]*\('
```

The test root is initialized with `git init` before its first lint invocation so `git ls-files --others --exclude-standard` is deterministic. The Bash 3.2-compatible production script scans every current or future Rust file recursively, including untracked files under development, and catches direct, grouped, aliased, and fully-qualified references. It does not maintain a hard-coded module list.

Add this invocation to `scripts/lint.sh` before Rust linting:

```bash
bash scripts/application-boundary-lint.sh
```

- [ ] **Step 9: Run green contract, receipt, lint, and ops regressions**

Run:

```bash
cargo nextest run application_operation_tests::operation_requests_classify_mutations_and_refresh_effects
cargo nextest run application_operation_tests::error_categories_are_copyable
cargo nextest run --lib application::operation::tests::transaction_summary_
cargo nextest run --lib ops::
bash scripts/application-boundary-lint-tests.sh
bash scripts/application-boundary-lint.sh
```

Expected: all selected tests pass; the lint fixture rejects every forbidden example and accepts the clean fixture.

- [ ] **Step 10: Commit the operation contract**

```bash
git add docs/superpowers/plans/2026-07-12-stax-gui-phase-2.md scripts/application-boundary-lint.sh scripts/application-boundary-lint-tests.sh scripts/lint.sh src/application/mod.rs src/application/operation.rs src/ops/receipt.rs src/ops/tx.rs tests/all_tests.rs tests/application_operation_tests.rs
git commit -m "feat(application): define operation contract"
```

## 2. Private mutation guard, rebase preflight, and runtime safety

### Task 2: Enforce repository serialization and preconditions internally

**Files:**
- Modify: `src/application/repository.rs`
- Modify: `src/application/operation.rs`
- Test: `src/application/repository.rs`

- [ ] **Step 1: Write failing private-registry unit tests**

Inside `src/application/repository.rs`, where private methods are visible, add these complete local helpers; do not introduce a `RepositoryFixture` abstraction:

```rust
fn git(cwd: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", if cfg!(windows) { "NUL" } else { "/dev/null" })
        .env("GIT_CONFIG_SYSTEM", if cfg!(windows) { "NUL" } else { "/dev/null" })
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn initialized_repository_with_linked_worktree() -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-b", "main"]);
    git(root.path(), &["config", "user.name", "Test User"]);
    git(root.path(), &["config", "user.email", "test@example.com"]);
    std::fs::write(root.path().join("README.md"), "initial\n").unwrap();
    git(root.path(), &["add", "README.md"]);
    git(root.path(), &["commit", "-m", "initial"]);
    let linked = root.path().with_file_name(format!(
        "{}-linked",
        root.path().file_name().unwrap().to_string_lossy()
    ));
    git(
        root.path(),
        &["worktree", "add", "-b", "linked", linked.to_str().unwrap()],
    );
    (root, linked)
}

#[test]
fn sessions_for_main_and_linked_worktrees_share_a_private_gate() {
    let (root, linked_path) = initialized_repository_with_linked_worktree();
    let main = RepositorySession::open(root.path()).unwrap();
    let linked = RepositorySession::open(&linked_path).unwrap();
    let request = OperationRequest::Checkout { branch: "feature".into() };

    let lease = main.try_begin_mutation(&request).unwrap();
    let error = linked.try_begin_mutation(&request).unwrap_err();
    assert_eq!(error.kind, OperationErrorKind::Busy);
    drop(lease);
    assert!(linked.try_begin_mutation(&request).is_ok());
}

#[test]
fn dead_gate_entries_are_pruned_when_another_session_opens() {
    let (root, _linked) = initialized_repository_with_linked_worktree();
    let key = RepositorySession::open(root.path())
        .unwrap()
        .common_git_dir()
        .to_path_buf();
    {
        let session = RepositorySession::open(root.path()).unwrap();
        assert!(registry_contains_live_key(&key));
        drop(session);
    }
    let (other, _other_linked) = initialized_repository_with_linked_worktree();
    let _session = RepositorySession::open(other.path()).unwrap();
    assert!(!registry_contains_key(&key));
}
```

`registry_contains_live_key` and `registry_contains_key` are `#[cfg(test)]` functions in this same module that lock `MUTATION_GATES` and inspect the requested key. No integration test imports `MutationLease` or calls lease acquisition.

- [ ] **Step 2: Write failing shared rebase-preflight and runtime-guard tests**

Add:

```rust
#[test]
fn mutation_preflight_reports_linked_rebase_path_before_running_work() {
    let (root, linked_path) = initialized_repository_with_linked_worktree();
    let linked_git_dir = PathBuf::from(git(&linked_path, &["rev-parse", "--git-dir"]));
    let linked_git_dir = if linked_git_dir.is_absolute() {
        linked_git_dir
    } else {
        linked_path.join(linked_git_dir)
    };
    std::fs::create_dir_all(linked_git_dir.join("rebase-merge")).unwrap();
    let session = RepositorySession::open(root.path()).unwrap();
    let request = OperationRequest::CreateBranch {
        name: "child".into(),
        parent: "main".into(),
    };
    let ran = Cell::new(false);

    let error = session
        .with_mutation(
            &request,
            MutationTargets::branches(["linked"]),
            || {
            ran.set(true);
            unreachable!()
            },
        )
        .unwrap_err();

    assert_eq!(error.kind, OperationErrorKind::RebaseInProgress);
    assert_eq!(
        error.details,
        OperationErrorDetails::Rebase {
            branch: Some("linked".into()),
            worktree: linked_path.canonicalize().unwrap(),
        }
    );
    assert!(!ran.get());
}

#[tokio::test]
async fn blocking_network_guard_returns_runtime_error_without_panicking() {
    let request = OperationRequest::SubmitStack {
        new_pull_requests: PullRequestMode::Draft,
    };
    let error = require_blocking_network_context(&request).unwrap_err();
    assert_eq!(error.kind, OperationErrorKind::Runtime);
    assert_eq!(error.side_effects, OperationSideEffects::None);
}
```

- [ ] **Step 3: Run the private safety tests and verify the red state**

Run:

```bash
cargo nextest run --lib application::repository::tests::sessions_for_main_
cargo nextest run --lib application::repository::tests::mutation_preflight_
cargo nextest run --lib application::repository::tests::blocking_network_guard_
```

Expected: compile failure because the private gate, `MutationTargets`, `with_mutation`, and runtime guard do not exist.

- [ ] **Step 4: Implement the private common-repository registry**

In `src/application/repository.rs`:

```rust
static MUTATION_GATES: OnceLock<Mutex<HashMap<PathBuf, Weak<Mutex<bool>>>>> = OnceLock::new();

#[derive(Debug)]
pub(super) struct MutationLease {
    gate: Arc<Mutex<bool>>,
}

impl Drop for MutationLease {
    fn drop(&mut self) {
        if let Ok(mut active) = self.gate.lock() {
            *active = false;
        }
    }
}
```

Add `mutation_gate: Arc<Mutex<bool>>` to `RepositorySession`. Key it by the canonical common Git directory in `open`, retain only `Weak` values in the static map, and prune entries whose `strong_count() == 0` while holding the registry lock.

Add the private target set and methods:

```rust
#[derive(Debug, Clone)]
pub(super) struct MutationTargets {
    include_current: bool,
    branches: HashSet<String>,
}

impl MutationTargets {
    pub(super) fn current() -> Self {
        Self { include_current: true, branches: HashSet::new() }
    }

    pub(super) fn branches(names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            include_current: true,
            branches: names.into_iter().map(Into::into).collect(),
        }
    }
}

pub(super) fn try_begin_mutation(
    &self,
    request: &OperationRequest,
) -> Result<MutationLease, OperationError>;

pub(super) fn with_mutation<T>(
    &self,
    request: &OperationRequest,
    targets: MutationTargets,
    run: impl FnOnce() -> Result<T, OperationError>,
) -> Result<T, OperationError>;
```

`MutationTargets` is `pub(super)` only so sibling operation modules can describe affected branches; it is not exported by `application::mod`. `with_mutation` acquires the lease, returns `InitializationRequired` when `GitRepo::is_initialized()` is false, calls `GitRepo::list_worktrees()`, canonicalizes each path belonging to the session common Git directory, selects the current worktree plus entries whose checked-out branch is in `targets.branches`, preflights all selected paths, then calls `run`.

- [ ] **Step 5: Implement typed rebase and runtime guards**

For each affected worktree, open its worktree-specific Git directory and detect both rebase-merge and rebase-apply state. If active, return:

```rust
OperationError {
    request: request.clone(),
    kind: OperationErrorKind::RebaseInProgress,
    details: OperationErrorDetails::Rebase {
        branch: worktree.branch.clone(),
        worktree: worktree.path.clone(),
    },
    primary: format!("A rebase is already in progress in {}", worktree.path.display()),
    action: "Resolve conflicts and run `st continue`, or run `st abort`, then retry".into(),
    diagnostic_chain: "repository contains rebase-merge or rebase-apply state".into(),
    receipt: None,
    side_effects: OperationSideEffects::None,
}
```

Add:

```rust
pub(super) fn require_blocking_network_context(
    request: &OperationRequest,
) -> Result<(), OperationError> {
    if tokio::runtime::Handle::try_current().is_ok() {
        return Err(OperationError {
            request: request.clone(),
            kind: OperationErrorKind::Runtime,
            details: OperationErrorDetails::None,
            primary: "This blocking network operation cannot run on a Tokio runtime thread".into(),
            action: "Run it on a blocking/background executor and retry".into(),
            diagnostic_chain: "tokio::runtime::Handle::try_current returned an active handle".into(),
            receipt: None,
            side_effects: OperationSideEffects::None,
        });
    }
    Ok(())
}
```

The GUI invokes blocking operations on `cx.background_executor()`. Direct Tokio callers receive the typed error; no operation calls `Runtime::new().block_on` while a Tokio handle is active. Task 6 tests restack and Task 8 tests submit with an active rebase in a linked target worktree; both must return this exact canonical path before stash, fetch, temporary refs, transaction creation, or push.

- [ ] **Step 6: Run safety tests and prove the lease is not public**

Run:

```bash
cargo nextest run --lib application::repository::tests::
cargo check --lib
```

Expected: all repository unit tests pass. `src/application/mod.rs` exports no `MutationLease`, `try_begin_mutation`, or `with_mutation`.

- [ ] **Step 7: Commit repository safety**

```bash
git add src/application/operation.rs src/application/repository.rs
git commit -m "feat(application): guard repository mutations"
```

## 3. Read-only PR resolution and explicit checkout

### Task 3: Extract checkout and trusted read-only PR resolution

**Files:**
- Create: `src/application/checkout.rs`
- Create: `src/application/pull_request.rs`
- Modify: `src/application/mod.rs`
- Modify: `src/config/mod.rs`
- Modify: `src/remote.rs`
- Modify: `src/forge/mod.rs`
- Modify: `src/github/client.rs`
- Modify: `src/application/ci.rs`
- Modify: `tests/application_operation_tests.rs`
- Modify: `tests/common/mod.rs`
- Test: `src/application/pull_request.rs`

- [ ] **Step 1: Write failing checkout tests, including every mutation preflight**

Add:

```rust
#[test]
fn checkout_changes_only_the_explicit_repository() {
    let repo = TestRepo::new();
    repo.set_trunk("main");
    let feature = repo.create_stack(&["feature"]).remove(0);
    repo.git(&["checkout", "main"]).assert_success();
    let unrelated = TestRepo::new();
    let session = RepositorySession::open(repo.path()).unwrap();
    let receipt = session.checkout("feature", &mut NoopOperationReporter).unwrap();

    assert_eq!(repo.current_branch(), feature);
    assert_eq!(unrelated.current_branch(), "main");
    assert_eq!(receipt.side_effects, OperationSideEffects::RepositoryChanged);
}

#[test]
fn checkout_reports_a_linked_worktree_without_changing_cwd() {
    let repo = TestRepo::new();
    repo.set_trunk("main");
    let feature = repo.create_stack(&["feature"]).remove(0);
    repo.git(&["checkout", "main"]).assert_success();
    let linked_parent = tempfile::tempdir().unwrap();
    let linked = linked_parent.path().join("linked");
    repo.git(&["worktree", "add", linked.to_str().unwrap(), &feature])
        .assert_success();
    let cwd = std::env::current_dir().unwrap();
    let error = RepositorySession::open(repo.path())
        .unwrap()
        .checkout(&feature, &mut NoopOperationReporter)
        .unwrap_err();

    assert_eq!(error.kind, OperationErrorKind::PreconditionFailed);
    assert_eq!(
        error.details,
        OperationErrorDetails::AlreadyCheckedOutElsewhere {
            branch: feature,
            path: linked.canonicalize().unwrap(),
        }
    );
    assert_eq!(std::env::current_dir().unwrap(), cwd);
}

#[test]
fn checkout_rejects_an_existing_rebase_before_changing_head() {
    let repo = TestRepo::new();
    repo.set_trunk("main");
    let feature = repo.create_stack(&["feature"]).remove(0);
    repo.git(&["checkout", "main"]).assert_success();
    std::fs::create_dir_all(repo.path().join(".git/rebase-merge")).unwrap();
    let before = repo.current_branch();
    let error = RepositorySession::open(repo.path())
        .unwrap()
        .checkout(&feature, &mut NoopOperationReporter)
        .unwrap_err();
    assert_eq!(error.kind, OperationErrorKind::RebaseInProgress);
    assert_eq!(repo.current_branch(), before);
}
```

- [ ] **Step 2: Write failing PR read-only and runtime tests**

In the private `src/application/pull_request.rs` test module, build a temporary git2 repository and write `BranchMetadata` inline; capture metadata with:

```rust
fn metadata_bytes(repo: &GitRepo, branch: &str) -> Vec<u8> {
    let reference = crate::git::refs::metadata_refname(branch);
    let object = repo.inner().revparse_single(&reference).unwrap();
    repo.inner().find_blob(object.id()).unwrap().content().to_vec()
}
```

Do not introduce a second metadata namespace: production and tests use the existing `crate::git::refs::metadata_refname`, whose canonical prefix is `refs/branch-metadata/`.

Add `pull_request_resolution_from_metadata_is_read_only`, asserting URL `/pull/42`, unchanged bytes, and `OperationSideEffects::None`.

For fallback, add a private production seam:

```rust
trait PullRequestLookup {
    fn find_open_by_head<'a>(
        &'a self,
        branch: &'a str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Option<PrInfoWithHead>>> + Send + 'a>>;
}
```

Implement it for `ForgeClient`. The test defines `FakePullRequestLookup { result: Option<PrInfoWithHead> }` inline and calls the single generic `resolve_with_lookup`; no mock client fixture or process config is required. Add `pull_request_fallback_does_not_persist_metadata`, capture bytes before/after, and assert URL `/pull/42`.

Add `pull_request_network_fallback_returns_runtime_error_inside_tokio` using inline `TestRepo`: create/track `feature`, omit PR metadata, invoke the public method inside `#[tokio::test]`, and assert `Runtime` before lookup.

Add `config::tests::trusted_network_config_preserves_repository_remote_name_only`: global config trusts the forge/provider/API/auth settings, repository `stax.toml` selects `remote.name = "upstream"` while also attempting different `base_url`, `api_base_url`, `forge`, and `auth`; the repository has only `upstream`. Assert `TrustedRemoteInfo` resolves `upstream`, retains every global endpoint/provider/auth value, and succeeds. Also assert a missing `upstream` fails instead of falling back to `origin`.

- [ ] **Step 3: Run checkout and PR tests and verify absent methods**

Run:

```bash
cargo nextest run application_operation_tests::checkout_
cargo nextest run application_operation_tests::pull_request_
cargo nextest run --lib config::tests::trusted_network_config_preserves_repository_remote_name_only
```

Expected: compile failure because `RepositorySession::checkout`, `resolve_pull_request_url`, and the exact trusted-network loader do not exist.

- [ ] **Step 4: Implement checkout through the private mutation guard**

Expose:

```rust
impl RepositorySession {
    pub fn checkout(
        &self,
        branch: &str,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        let request = OperationRequest::Checkout { branch: branch.to_owned() };
        report_operation(request.clone(), reporter, |reporter| {
            self.checkout_unframed(&request, branch, reporter)
        })
    }

    pub(super) fn checkout_unframed(
        &self,
        request: &OperationRequest,
        branch: &str,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        self.with_mutation(
            request,
            MutationTargets::branches([branch]),
            || checkout_explicit(self, request, branch, reporter),
        )
    }
}
```

`checkout_explicit` validates nonempty input, resolves a local branch, returns `AlreadyCurrent` without Git mutation, checks all worktree porcelain entries, and returns typed linked-worktree details. It emits `CheckingOut`, calls the existing safe `GitRepo` checkout on `self.open_repo()`, and returns no transaction because checkout is not currently an `ops::Transaction`.

- [ ] **Step 5: Generalize trusted-network names without weakening policy**

Rename:

```text
Config::load_for_automatic_network        -> Config::load_for_trusted_network
validate_automatic_network_remote         -> validate_trusted_network_remote
ForgeClient::new_for_automatic            -> ForgeClient::new_for_trusted_remote
GitHubClient::new_for_automatic            -> GitHubClient::new_for_trusted_remote
```

Retain one exact loader:

```rust
#[derive(Debug, Deserialize, Default)]
struct TrustedNetworkRepoConfig {
    #[serde(default)]
    remote: TrustedNetworkRepoRemoteConfig,
}

#[derive(Debug, Deserialize, Default)]
struct TrustedNetworkRepoRemoteConfig {
    #[serde(default)]
    name: Option<String>,
}

pub(crate) fn load_for_trusted_network(root: &Path) -> Result<Config> {
    let path = Self::path()?;
    let mut config = Self::load_path_or_default(&path)?;
    let repo_path = root.join("stax.toml");
    if !repo_path.exists() {
        return Ok(config);
    }
    let content = fs::read_to_string(&repo_path)
        .with_context(|| format!("Failed to read repo config {}", repo_path.display()))?;
    let local: TrustedNetworkRepoConfig = toml::from_str(&content)
        .with_context(|| format!("Failed to parse repo config {}", repo_path.display()))?;
    if let Some(name) = local
        .remote
        .name
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
    {
        config.remote.name = name;
    }
    Ok(config)
}
```

Rename private `AutomaticRepoConfig`/`AutomaticRepoRemoteConfig` to `TrustedNetworkRepoConfig`/`TrustedNetworkRepoRemoteConfig`; the latter still contains exactly `name: Option<String>` and no other deserializable field. This helper always reads the explicit repository root’s safe selector even when `STAX_CONFIG_DIR` isolates the global file; it never invokes the general repository overlay merger. `src/application/ci.rs`, PR resolution, and submit all call this helper. Replace user-facing “Automatic CI hydration” wording with “Noninteractive repository network access”. Keep all of these checks:

1. provider base host matches Git remote host;
2. official hosts reject forge mismatch;
3. custom hosts require matching global `remote.base_url` and explicit global `remote.forge`;
4. custom API hosts require explicit global `remote.api_base_url`;
5. `auth.gh_hostname` matches the validated remote host;
6. redirects never forward Authorization or Private-Token to another authority.

Define in `tests/common/mod.rs`:

```rust
pub struct IsolatedProcessEnv {
    _temp: tempfile::TempDir,
    home_dir: PathBuf,
    config_dir: PathBuf,
    gh_config_dir: PathBuf,
}

impl IsolatedProcessEnv {
    pub fn with_config(config_toml: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home_dir = temp.path().join("home");
        let config_dir = temp.path().join("config");
        let gh_config_dir = temp.path().join("gh");
        std::fs::create_dir_all(&home_dir).unwrap();
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&gh_config_dir).unwrap();
        std::fs::write(config_dir.join("config.toml"), config_toml).unwrap();
        Self {
            home_dir,
            gh_config_dir,
            _temp: temp,
            config_dir,
        }
    }

    pub fn command(&self, repository: &Path) -> Command {
        let mut command = Command::new(stax_bin());
        let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
        command
            .current_dir(repository)
            .env("HOME", &self.home_dir)
            .env("STAX_CONFIG_DIR", &self.config_dir)
            .env("GH_CONFIG_DIR", &self.gh_config_dir)
            .env("GIT_CONFIG_GLOBAL", null)
            .env("GIT_CONFIG_SYSTEM", null)
            .env_remove("STAX_GITHUB_TOKEN")
            .env_remove("STAX_GITLAB_TOKEN")
            .env_remove("STAX_GITEA_TOKEN")
            .env_remove("STAX_FORGE_TOKEN")
            .env_remove("GITHUB_TOKEN")
            .env_remove("GH_TOKEN")
            .env_remove("GITLAB_TOKEN")
            .env_remove("GITEA_TOKEN")
            .env("STAX_DISABLE_UPDATE_CHECK", "1");
        command
    }
}
```

`with_config` writes `config.toml` under the owned temp config directory. Its caller includes `[auth] use_gh_cli = false` unless the test intentionally exercises host-bound `gh` lookup. Individual tests add fixture tokens only to the returned child `Command`. HOME and GH config are always isolated. Never call `std::env::set_var`, `remove_var`, or `set_current_dir` in the test process.

- [ ] **Step 6: Implement read-only explicit-branch resolution**

`resolve_pull_request_url` frames events but does not call `with_mutation`. It opens the repository and returns `InitializationRequired` before local/remote lookup when `GitRepo::is_initialized()` is false. Resolution order:

1. validate the local branch;
2. return stored PR number plus canonical URL when metadata exists;
3. before fallback, call `require_blocking_network_context`;
4. load `Config::load_for_trusted_network(self.repository_root())`;
5. build `TrustedRemoteInfo` and `ForgeClient::new_for_trusted_remote`;
6. pass that `ForgeClient` to `resolve_with_lookup`;
7. return URL data without any `BranchMetadata::write`, ref update, or cache write.

Keep the network seam private and explicit for unit tests:

```rust
fn resolve_with_lookup<L: PullRequestLookup + ?Sized>(
    session: &RepositorySession,
    request: &OperationRequest,
    branch: &str,
    lookup: &L,
    reporter: &mut dyn OperationReporter,
) -> OperationResult;

impl PullRequestLookup for ForgeClient {
    fn find_open_by_head<'a>(
        &'a self,
        branch: &'a str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Option<PrInfoWithHead>>> + Send + 'a>> {
        Box::pin(async move { self.find_open_pr_by_head(branch).await })
    }
}
```

There is no `resolve_with_client` sibling. `impl PullRequestLookup for ForgeClient` delegates to `ForgeClient::find_open_pr_by_head`; the public path and fake path therefore share the same seam.

Map missing credentials to `Authentication`, explicit trust/host mismatch and HTTP 403 to `Authorization`, HTTP 401 to `Authentication`, transport/timeouts/5xx to `Network`, missing branch/input to `InvalidInput`, and no PR to `PreconditionFailed` with `OperationErrorDetails::PullRequest`.

- [ ] **Step 7: Run green read-only, trust, runtime, and checkout tests**

Run:

```bash
cargo nextest run application_operation_tests::checkout_
cargo nextest run application_operation_tests::pull_request_
cargo nextest run --lib application::pull_request::tests::pull_request_fallback_does_not_persist_metadata
cargo nextest run --lib remote::tests::
cargo nextest run --lib config::tests::
cargo nextest run --lib github::client::tests::
cargo nextest run navigation_tests::
```

Expected: all selected tests pass; fallback metadata bytes remain unchanged, direct Tokio fallback returns `Runtime`, and unknown custom hosts fail until the child’s isolated global config explicitly trusts them.

- [ ] **Step 8: Commit checkout, read-only resolution, and trusted-network naming**

```bash
git add src/application/checkout.rs src/application/pull_request.rs src/application/mod.rs src/application/ci.rs src/config/mod.rs src/forge/mod.rs src/github/client.rs src/remote.rs tests/application_operation_tests.rs tests/common/mod.rs
git commit -m "refactor(application): share checkout and PR resolution"
```

## 4. Pure branch naming and empty branch creation

### Task 4: Extract a data-returning branch-name seam and narrow create operation

**Files:**
- Create: `src/application/branch_name.rs`
- Create: `src/application/create.rs`
- Modify: `src/application/mod.rs`
- Modify: `src/commands/branch/create.rs`
- Modify: `tests/application_operation_tests.rs`

- [ ] **Step 1: Write failing pure naming tests**

In `src/application/branch_name.rs`:

```rust
#[test]
fn format_branch_name_is_pure_and_returns_normalization_warning() {
    let context = BranchNameContext {
        format: Some("{user}/{message}".into()),
        prefix: None,
        legacy_date: false,
        date_format: "%Y-%m-%d".into(),
        replacement: "-".into(),
        user: Some("César Ferreira".into()),
        date: chrono::NaiveDate::from_ymd_opt(2026, 7, 12).unwrap(),
    };
    let result = format_branch_name("  Fix GUI!  ", &context).unwrap();
    assert_eq!(result.name, "César-Ferreira/Fix-GUI");
    assert_eq!(
        result.warnings,
        vec![OperationWarning::BranchNameNormalized {
            original: "  Fix GUI!  ".into(),
            normalized: "César-Ferreira/Fix-GUI".into(),
        }]
    );
}

#[test]
fn format_branch_name_rejects_an_empty_normalized_ref() {
    let context = BranchNameContext::literal();
    let error = format_branch_name("!!!", &context).unwrap_err();
    assert_eq!(error, BranchNameError::Empty);
}

#[test]
fn format_branch_name_rejects_an_invalid_git_ref() {
    let error = format_branch_name("///", &BranchNameContext::literal()).unwrap_err();
    assert_eq!(error, BranchNameError::InvalidRef { candidate: "///".into() });
}

#[test]
fn format_branch_name_preserves_unicode_alphanumeric_characters() {
    let result = format_branch_name("ação-日本語", &BranchNameContext::literal()).unwrap();
    assert_eq!(result.name, "ação-日本語");
}
```

Define the types in this task:

```rust
pub(crate) struct BranchNameContext {
    pub format: Option<String>,
    pub prefix: Option<String>,
    pub legacy_date: bool,
    pub date_format: String,
    pub replacement: String,
    pub user: Option<String>,
    pub date: chrono::NaiveDate,
}

impl BranchNameContext {
    pub(crate) fn literal() -> Self {
        Self {
            format: None,
            prefix: None,
            legacy_date: false,
            date_format: "%Y-%m-%d".into(),
            replacement: "-".into(),
            user: None,
            date: chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap(),
        }
    }
}

pub(crate) struct BranchNameResult {
    pub name: String,
    pub warnings: Vec<OperationWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BranchNameError {
    Empty,
    MissingMessagePlaceholder { format: String },
    InvalidRef { candidate: String },
}

pub(crate) fn format_branch_name(
    input: &str,
    context: &BranchNameContext,
) -> Result<BranchNameResult, BranchNameError>;
```

- [ ] **Step 2: Write failing create happy/error/preflight tests**

Add:

```rust
#[test]
fn create_empty_branch_uses_explicit_parent_without_creating_a_commit() {
    let repo = TestRepo::new();
    repo.set_trunk("main");
    let feature = repo.create_stack(&["feature"]).remove(0);
    let parent_oid = repo.get_commit_sha(&feature);
    repo.git(&["checkout", "main"]).assert_success();
    let receipt = RepositorySession::open(repo.path())
        .unwrap()
        .create_empty_branch("child", &feature, &mut NoopOperationReporter)
        .unwrap();
    assert_eq!(repo.get_commit_sha("child"), parent_oid);
    assert_eq!(repo.get_current_parent().as_deref(), Some(feature.as_str()));
    assert_eq!(repo.current_branch(), "child");
    assert_eq!(receipt.side_effects, OperationSideEffects::RepositoryChanged);
}

#[test]
fn create_rejects_rebase_in_progress_before_creating_a_ref() {
    let repo = TestRepo::new();
    repo.set_trunk("main");
    std::fs::create_dir_all(repo.path().join(".git/rebase-merge")).unwrap();
    let error = RepositorySession::open(repo.path())
        .unwrap()
        .create_empty_branch("child", "main", &mut NoopOperationReporter)
        .unwrap_err();
    assert_eq!(error.kind, OperationErrorKind::RebaseInProgress);
    assert!(!repo.list_branches().contains(&"child".to_string()));
}
```

Add `create_rolls_back_its_new_ref_when_metadata_write_fails` to existing `tests/create_rollback_tests.rs`, using that module’s existing repository setup and metadata-namespace failure mechanism. Assert `LocalGit`, absent `child` in `TestRepo::list_branches()`, and `OperationSideEffects::None`; do not create another fixture abstraction.

- [ ] **Step 3: Run naming/create tests and verify the red state**

Run:

```bash
cargo nextest run --lib application::branch_name::tests::
cargo nextest run application_operation_tests::create_
```

Expected: compile failure because `branch_name`, `BranchNameContext`, and `RepositorySession::create_empty_branch` do not exist.

- [ ] **Step 4: Implement pure naming without terminal output**

Move the exact character mapping from `Config::sanitize_branch_segment` and exact template/prefix/date behavior from `Config::{format_branch_name_with_prefix_override,apply_format_template}` into `format_branch_name`. Preserve current semantics: `char::is_alphanumeric` keeps Unicode letters and digits and case is unchanged; whitespace/punctuation becomes the first replacement character; repeated replacement characters collapse; there is no transliteration dependency. Validate with `git2::Reference::is_valid_name("refs/heads/<candidate>")`, which performs no process or repository access.

`BranchNameContext::literal()` means no format, prefix, user, or date behavior and uses `-` replacement. A format missing `{message}` returns `BranchNameError::MissingMessagePlaceholder`; empty and invalid refs return the exact pure variants above. At the create operation boundary, `map_branch_name_error(request, error) -> OperationError` maps all three to `InvalidInput`, safe primary/action text, and a diagnostic containing the variant. Pure naming never constructs an `OperationError`, reads config/user/date, or prints.

The CLI’s AI/staging/insert/below paths call this same pure formatter after collecting their CLI-only inputs. CLI adapters render returned warnings.

- [ ] **Step 5: Implement narrow explicit-name creation**

`RepositorySession::create_empty_branch` constructs `OperationRequest::CreateBranch`, calls `report_operation`, and acquires `with_mutation` internally. It:

1. validates the GUI/application supplied exact name with `BranchNameContext::literal()` and maps `BranchNameError` at the operation boundary;
2. resolves the explicit parent without checking it out;
3. uses that validated literal name exactly once;
4. rejects exact and namespace ref conflicts;
5. creates the branch at the parent OID with no commit;
6. writes parent metadata;
7. safely checks out the new branch;
8. removes only the newly created ref/metadata on failure;
9. returns naming warnings as receipt data and no transaction summary.

`OperationSideEffects` is `RepositoryChanged` on success. A fully rolled-back failure is `None`; a rollback failure is `RepositoryChanged` and its diagnostic chain contains both original and rollback errors.

- [ ] **Step 6: Keep advanced create paths as CLI adapters around shared seams**

In `src/commands/branch/create.rs`, delegate only when an explicit name is supplied and AI, commit/staging, insert, below, custom prefix, and interactive naming options are inactive. Resolve CLI branch config/user/date into `BranchNameContext`, call pure `format_branch_name`, resolve `--from` or current branch as `parent`, then call the same exact-name empty-create core and render naming/receipt warnings. GUI calls that core with `literal()`; both reach one ref/metadata/checkout/rollback implementation.

AI/staging/insert/below remain CLI behavior, but use `format_branch_name` and existing lower-level Git helpers; do not call application modules from commands in reverse and do not add those options to `OperationRequest`.

- [ ] **Step 7: Run create and advanced-mode regressions**

Run:

```bash
cargo nextest run --lib application::branch_name::tests::
cargo nextest run application_operation_tests::create_
cargo nextest run create_rollback_tests::
cargo nextest run create_ai_tests::
cargo nextest run create_below_tests::
cargo nextest run create_insert_tests::
bash scripts/application-boundary-lint.sh
```

Expected: all tests pass; pure naming returns warnings as data, explicit creation creates no commit, rebase preflight has no side effect, and advanced CLI behavior remains.

- [ ] **Step 8: Commit branch naming and creation**

```bash
git add src/application/branch_name.rs src/application/create.rs src/application/mod.rs src/commands/branch/create.rs tests/application_operation_tests.rs
git commit -m "refactor(application): share empty branch creation"
```

## 5. Fork-aware restack scope and intact pipeline extraction

### Task 5: Move the mature restack core without a parallel planner

**Files:**
- Create: `src/application/restack.rs`
- Modify: `src/application/mod.rs`
- Modify: `src/commands/restack.rs`
- Test: `src/application/restack.rs`
- Test: existing `tests/restack_provenance_tests.rs`

- [ ] **Step 1: Write the failing pure scope tests**

Construct the `Stack` directly in `src/application/restack.rs` tests with `StackBranch` values for this graph; do not add a fixture type or fixture constructor:

```text
main
└── base
    ├── selected
    │   ├── child-b
    │   └── child-a
    │       └── grandchild
    └── unrelated-sibling
```

The test creates the `HashMap<String, StackBranch>` inline, then asserts:

```rust
assert_eq!(
    branches_for_scope(&stack, &RestackScope::StackContaining("selected".into())).unwrap(),
    vec!["base", "selected", "child-a", "grandchild", "child-b"],
);
assert_eq!(
    branches_for_scope(&stack, &RestackScope::ThroughBranch("selected".into())).unwrap(),
    vec!["base", "selected"],
);
assert_eq!(
    branches_for_scope(&stack, &RestackScope::All).unwrap(),
    vec![
        "base",
        "selected",
        "child-a",
        "grandchild",
        "child-b",
        "unrelated-sibling",
    ],
);
```

Also assert `Branch("main")` and `Branch("missing")` return `InvalidInput`. Ordering is deterministic depth-first preorder with lexical sibling order. `StackContaining(selected)` is the ancestor chain above trunk through selected plus the complete selected descendant subtree; it excludes every unrelated sibling subtree.

- [ ] **Step 2: Run scope tests and verify the red state**

Run:

```bash
cargo nextest run --lib application::restack::tests::branches_for_scope_
```

Expected: compile failure because `application::restack::branches_for_scope` does not exist.

- [ ] **Step 3: Define the narrow application input**

Add:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RestackExecutionOptions {
    pub scope: RestackScope,
    pub auto_stash: bool,
    pub restore_branch: Option<String>,
    pub completed_from_receipt: HashSet<String>,
}
```

This is the only new execution input. It deliberately has no `quiet`, terminal confirmation, dry-run rendering, submit-after behavior, color, or progress-timer option. Do not add `RestackPlan` or `RestackBranchPlan`; the mature loop already recalculates `needs_restack` from its in-memory `live_stack` after each successful rebase and that behavior must remain intact.

- [ ] **Step 4: Move the complete normal mutation path by symbol**

Move, rather than rewrite, the following current code from `src/commands/restack.rs` into `src/application/restack.rs`:

1. the normal mutation body of `run_impl` from current branch/stack loading through scope selection, frozen filtering, `branches_needing_restack`, linked-target stashing, `choose_rebase_upstream`, per-branch provenance-aware rebase choice, metadata updates, `live_stack` child invalidation, transaction recording, checkout restoration, stash restoration, and receipt finalization;
2. `normalized_workdir`, `restore_stashed_worktrees`, and `branches_needing_restack`;
3. the existing `RestackConflictContext` facts needed to build typed conflict details, but not its terminal renderer.

Preserve every branch of the current loop. Replace only presentation seams:

```text
LiveTimer/println/eprintln       -> OperationProgress / OperationWarning
dialoguer dirty-tree prompt     -> RestackExecutionOptions::auto_stash
print_restack_conflict          -> OperationErrorDetails + failed receipt
tx::print_plan                  -> Preparing progress
should_submit_after_restack     -> command adapter after success
```

`src/commands/restack.rs` keeps `--continue`, `--dry-run`, terminal confirmation, conflict instructions, and submit-after orchestration. For a confirmed normal mutation it builds `RestackExecutionOptions` and calls the application operation. No function in `application::restack` calls a command module.

- [ ] **Step 5: Prove the move preserves dynamic restack behavior**

Extend `tests/restack_provenance_tests.rs` using the existing exact test support `tests/common/mod.rs::TestRepo::{new,set_trunk,create_stack,create_file,commit,git,get_commit_sha}`. Add:

- `application_restack_recomputes_child_after_parent_rebase`;
- `application_restack_preserves_open_pr_fast_path`;
- `application_restack_skips_frozen_and_reports_it`;
- `application_stack_containing_excludes_unrelated_sibling_subtree`;
- `application_restack_noop_has_no_transaction`.

Each test builds the repository inline with `TestRepo`; no `RestackFixture`, `build_plan`, or synthetic executor is introduced.

- [ ] **Step 6: Run extraction regressions**

Run:

```bash
cargo nextest run --lib application::restack::tests::
cargo nextest run restack_provenance_tests::
cargo nextest run conflict_handling_tests::
bash scripts/application-boundary-lint.sh
```

Expected: scope tests and all prior provenance/conflict regressions pass; `src/commands/restack.rs` contains presentation/recovery orchestration but no second copy of the normal rebase loop.

- [ ] **Step 7: Commit the intact restack extraction**

```bash
git add src/application/mod.rs src/application/restack.rs src/commands/restack.rs tests/restack_provenance_tests.rs
git commit -m "refactor(application): extract mature restack pipeline"
```

## 6. Restack linked-worktree preflight and one failed-receipt finalizer

### Task 6: Make every post-effect restack failure truthful

**Files:**
- Modify: `src/application/restack.rs`
- Modify: `src/engine/restack_preflight.rs`
- Modify: `src/ops/tx.rs`
- Modify: `tests/application_operation_tests.rs`
- Modify: `tests/conflict_handling_tests.rs`
- Modify: `tests/restack_provenance_tests.rs`

- [ ] **Step 1: Add linked-worktree preflight tests with real repositories**

Use only `tests/common/mod.rs::TestRepo` and raw `TestRepo::{git,git_in}`. Create a stack, check its target branch out in a linked worktree with `git worktree add`, then create `<linked-git-dir>/rebase-merge` after obtaining the worktree Git directory from `git -C <linked> rev-parse --git-dir`.

Add:

```text
restack_active_rebase_in_linked_target_reports_canonical_path
restack_linked_rebase_preflight_runs_before_stash_and_transaction
restack_dirty_linked_target_requires_auto_stash
restack_auto_stash_restores_exact_linked_worktree
restack_success_receipt_persistence_failure_returns_in_memory_receipt
```

The first two assert `OperationErrorDetails::Rebase { branch: Some(target), worktree: linked.canonicalize() }`, unchanged stash count, unchanged refs, and no new receipt. The dirty tests write `dirty.txt` in the linked worktree and assert the file/index state survives exactly. The Unix-only persistence test runs a one-branch successful rebase whose `post-rewrite` hook makes only the receipt directory unwritable; assert the branch changed, error kind is `LocalGit`, side effects are `RepositoryChanged`, and `error.receipt.transaction.status` is `Succeeded`.

- [ ] **Step 2: Run linked preflight tests and verify the red state**

Run:

```bash
cargo nextest run application_operation_tests::restack_active_rebase_
cargo nextest run application_operation_tests::restack_linked_rebase_
cargo nextest run application_operation_tests::restack_dirty_linked_
cargo nextest run application_operation_tests::restack_auto_stash_
cargo nextest run restack_provenance_tests::restack_success_receipt_persistence_
```

Expected: failures because restack does not yet pass its complete scope branch set to `MutationTargets::branches` or preserve a successful in-memory receipt when its final save fails.

- [ ] **Step 3: Add the single failure state and receipt-preserving finalizer**

Define in `src/application/restack.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
struct OwnedStash {
    worktree: PathBuf,
    restored: bool,
}

struct RestackFailureState {
    transaction: Option<Transaction>,
    completed_branches: Vec<String>,
    stashes: Vec<OwnedStash>,
    side_effects: OperationSideEffects,
}

fn finalize_restack_failure(
    request: &OperationRequest,
    state: RestackFailureState,
    kind: OperationErrorKind,
    details: OperationErrorDetails,
    primary: String,
    action: String,
    source: anyhow::Error,
    failed_step: &'static str,
    failed_branch: Option<&str>,
) -> OperationError;
```

Every error after a stash is created or transaction snapshot succeeds flows through this one function: conflict, target stash failure after an earlier branch changed, ordinary Git/ref/metadata/checkout failure, stash restoration failure, and receipt persistence failure. Precondition failures before either event continue to return without a receipt.

Reuse Task 1’s `ReceiptFinalization`, then add `finish_err_preserving_receipt` inside `impl Transaction` in `src/ops/tx.rs`:

```rust
pub(crate) fn finish_err_preserving_receipt(
    mut self,
    message: &str,
    failed_step: Option<&str>,
    failed_branch: Option<&str>,
) -> ReceiptFinalization {
    self.receipt.mark_failed(message, failed_step, failed_branch);
    let persistence_error = self.receipt.save(&self.git_dir).err();
    self.finished = true;
    ReceiptFinalization {
        receipt: self.receipt.clone(),
        persistence_error,
    }
}
```

`finish_err_with_receipt` delegates to this method and returns the persistence error when present, preserving existing callers. The restack finalizer always converts the in-memory failed receipt into `TransactionSummary`; when persistence also fails, append that error to `diagnostic_chain` and set `side_effects = RepositoryChanged`. Never replace the initiating failure with the persistence error.

- [ ] **Step 4: Preserve stash ownership and success ordering**

The extracted loop records an `OwnedStash` only when `stash_push_at` returns true. It sets `restored = true` only after that exact worktree’s reverse-order pop succeeds. On conflict, do not pop operation-owned stashes. On a later non-conflict failure, attempt restoration only where no rebase is active; report every still-owned canonical path in the error action.

Do not mark the transaction successful before checkout and stash restoration. Success order is:

```text
record all after OIDs
restore original checkout
restore owned stashes in reverse order
finish_ok_preserving_receipt
return OperationReceipt, or typed LocalGit error carrying that successful in-memory receipt
```

This makes a post-rebase checkout/stash failure a failed transaction rather than a false success.

- [ ] **Step 5: Make restack boundary selection presentation-neutral**

Change `src/engine/restack_preflight.rs` to return:

```rust
pub struct RebaseBoundaryDecision {
    pub upstream: String,
    pub adjusted: bool,
    pub reason: Option<String>,
}
```

It emits no terminal output. `application::restack` maps an adjusted decision to `OperationWarning::RestackBoundaryAdjusted { branch, reason }`; the command reporter renders it.

- [ ] **Step 6: Add real post-effect failure regressions**

Extend existing integration modules using inline `TestRepo` setup:

1. `restack_conflict_failed_receipt_lists_only_completed_branches` — first branch succeeds, second conflicts; rebase and owned stash remain.
2. `restack_ref_lock_failure_after_first_branch_uses_failed_finalizer` — install a `post-rewrite` hook that creates `refs/heads/second.lock` after the first rebase; assert first is completed, transaction is failed, second is not.
3. `restack_stash_failure_after_first_branch_uses_failed_finalizer` — the hook creates the second linked worktree’s `index.lock` before its dirty target is stashed; assert completed/stash ownership facts.
4. `restack_receipt_persistence_failure_retains_in_memory_failed_receipt` — the hook removes write permission from the operation receipt directory after the first rebase; restore permissions with an RAII test guard before assertions; assert the initiating local-Git error remains primary and the persistence failure appears in diagnostics.
5. `restack_checkout_restore_failure_uses_failed_finalizer` — delete the restore branch from the hook after first rebase and assert failed receipt.
These are Unix-only where hooks/permissions require it. They use no process-global environment mutation and no failpoint API.

- [ ] **Step 7: Run restack failure and recovery gates**

Run:

```bash
cargo nextest run application_operation_tests::restack_
cargo nextest run restack_provenance_tests::restack_
cargo nextest run conflict_handling_tests::restack_
cargo nextest run continue_tests::
cargo nextest run abort_tests::
cargo nextest run --lib ops::
bash scripts/application-boundary-lint.sh
```

Expected: linked preflight, exact completed branches, exact stash ownership, conflict state, all non-conflict post-effect failures, canonical undo, continue, and abort pass.

- [ ] **Step 8: Commit restack finalization**

```bash
git add src/application/restack.rs src/engine/restack_preflight.rs src/ops/tx.rs tests/application_operation_tests.rs tests/conflict_handling_tests.rs tests/restack_provenance_tests.rs
git commit -m "fix(application): preserve failed restack receipts"
```

## 7. Complete submit pipeline extraction and configuration separation

### Task 7: Move the mature submit pipeline intact

**Files:**
- Create: `src/application/submit.rs`
- Modify: `src/application/mod.rs`
- Modify: `src/commands/submit.rs`
- Modify: `src/config/mod.rs`
- Modify: `src/remote.rs`
- Modify: `src/forge/mod.rs`
- Test: existing submit modules under `tests/`

- [ ] **Step 1: Add characterization tests before moving code**

Add integration tests using `tests/common/mod.rs::TestRepo::{new_with_remote,set_trunk,create_stack,configure_github_like_submit_remote,git,get_commit_sha,list_remote_branches}` and inline `wiremock::MockServer` routes:

```text
submit_fetch_failure_existing_remote_ref_aborts_before_publish_refs
submit_stale_metadata_rediscovers_open_pr_before_push
submit_linked_worktree_branch_uses_temporary_publish_worktree
submit_empty_branch_pushes_without_creating_pr
submit_imported_branch_is_read_only_but_remains_in_stack_links
submit_noop_returns_unchanged_pr_urls_without_transaction
submit_duplicate_pr_create_recovers_by_head_lookup
submit_force_with_lease_uses_post_fetch_remote_oid
```

Do not introduce `SubmitFixture`, `SubmitExecutionFixture`, or a mock-only parallel plan. Each test builds the repository inline with `TestRepo`, mounts only the exact forge endpoints it expects, runs the current command, and captures refs/metadata/request logs before and after.

- [ ] **Step 2: Run characterization tests against the current command**

Run:

```bash
cargo nextest run submit_fetch_failure_tests::
cargo nextest run track_all_prs_tests::submit_stale_metadata_
cargo nextest run track_all_prs_tests::submit_linked_worktree_
cargo nextest run track_all_prs_tests::submit_empty_branch_
cargo nextest run track_all_prs_tests::submit_imported_branch_
cargo nextest run track_all_prs_tests::submit_noop_
cargo nextest run track_all_prs_tests::submit_duplicate_pr_
cargo nextest run track_all_prs_tests::submit_force_with_lease_
```

Expected: existing behavior tests pass except the explicit-OID lease assertion, which fails because the current batch push uses bare `--force-with-lease`.

- [ ] **Step 3: Write the preparation ownership and cleanup-order red test**

Add private unit test `prepared_submit_drops_resources_before_mutation_lease`. Inline-initialize a temporary repository with `git init`, create a detached temporary worktree plus `refs/stax/submit/cleanup-order`, open two `RepositorySession` values for that repository, acquire the actual lease with `first.try_begin_mutation(&request)`, and construct `PreparedSubmitGuards` with both resources. Its test-only `after_cleanup` closure asserts the ref and worktree are absent, opens a probe `RepositorySession` from a cloned repository path, and asserts `probe.try_begin_mutation(&request)` returns `Busy`. After `drop(guards)`, assert the previously opened `second.try_begin_mutation(&request)` succeeds. `try_begin_mutation` is the Task 2 `pub(super)` seam, not a new lease API. The closure captures only cloned path/request data and uses no process-global environment.

Run:

```bash
cargo nextest run --lib application::submit::tests::prepared_submit_drops_resources_before_mutation_lease
```

Expected: compile failure because application-owned submit preparation/resource types do not exist.

- [ ] **Step 4: Define preparation/execution inputs around the existing model**

Move `SubmitScope` out of `src/commands/submit.rs` and make the application module its sole owner:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubmitScope {
    Branch,
    Downstack,
    Upstack,
    Stack,
}

impl SubmitScope {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Branch => "branch",
            Self::Downstack => "downstack",
            Self::Upstack => "upstack",
            Self::Stack => "stack",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SubmitOptions {
    pub scope: SubmitScope,
    pub new_pull_requests: PullRequestMode,
    pub fetch: bool,
    pub prefetched: bool,
    pub verify_hooks: bool,
    pub create_pull_requests: bool,
    pub reviewers: Vec<String>,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub rerequest_review: bool,
    pub native_stack_override: Option<NativeStackMode>,
    pub update_title: bool,
}

impl SubmitOptions {
    pub(crate) fn gui_current_stack_draft() -> Self {
        Self {
            scope: SubmitScope::Stack,
            new_pull_requests: PullRequestMode::Draft,
            fetch: true,
            prefetched: false,
            verify_hooks: true,
            create_pull_requests: true,
            reviewers: Vec::new(),
            labels: Vec::new(),
            assignees: Vec::new(),
            rerequest_review: false,
            native_stack_override: None,
            update_title: false,
        }
    }
}

pub(crate) struct SubmitConfigSources {
    pub trusted_network: Config,
    pub preferences: SubmitPreferences,
}

#[derive(Debug, Clone)]
pub(crate) struct SubmitPreferences {
    pub stack_links: StackLinksMode,
    pub single_stack: SingleStackMode,
    pub stack_links_when_native: StackLinksWhenNative,
    pub native_stack: NativeStackMode,
}
```

Keep the current `PrPlan` model and extend it; do not replace it with a reduced `SubmitPlan`/`PushedBranch` model:

```rust
#[derive(Debug, Clone)]
struct PrPlan {
    branch: String,
    parent: String,
    commit_range_base: String,
    publish_ref: String,
    publish_oid: Option<String>,
    uses_temporary_publish_ref: bool,
    remote_oid_after_fetch: Option<String>,
    existing_pr: Option<ExistingPrSnapshot>,
    tip_commit_subject: Option<String>,
    needs_title_update: bool,
    title: Option<String>,
    body: Option<String>,
    ai_title_update: Option<String>,
    generated_body_update: Option<String>,
    is_draft: Option<bool>,
    needs_push: bool,
    needs_pr_update: bool,
    needs_base_update: bool,
    is_empty: bool,
    is_imported: bool,
}

#[derive(Debug, Clone)]
struct ExistingPrSnapshot {
    number: u64,
    head: String,
    base: String,
    title: String,
    state: String,
    is_draft: bool,
    url: String,
}

pub(crate) struct PreparedSubmit {
    request: OperationRequest,
    repository_root: PathBuf,
    common_git_dir: PathBuf,
    scope: SubmitScope,
    remote: TrustedRemoteInfo,
    stack: Stack,
    current_branch: String,
    plans: Vec<PrPlan>,
    prompt_requests: Vec<SubmitPromptRequest>,
    preferences: SubmitPreferences,
    guards: PreparedSubmitGuards,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SubmitPromptRequest {
    pub branch: String,
    pub suggested_title: String,
    pub suggested_body: String,
    pub suggested_mode: PullRequestMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SubmitPromptAnswer {
    pub branch: String,
    pub title: String,
    pub body: String,
    pub mode: PullRequestMode,
}

struct PreparedSubmitGuards {
    resources: SubmitResources,
    _lease: MutationLease,
}

struct SubmitResources {
    temporary_publish_refs: TemporaryPublishRefs,
    temporary_worktrees: Vec<TemporarySubmitWorktree>,
    #[cfg(test)]
    after_cleanup: Option<Box<dyn FnOnce()>>,
}

impl SubmitResources {
    fn cleanup_best_effort(&mut self) {
        for worktree in self.temporary_worktrees.iter_mut().rev() {
            let _ = worktree.remove();
        }
        self.temporary_publish_refs.cleanup();
    }
}

impl Drop for SubmitResources {
    fn drop(&mut self) {
        self.cleanup_best_effort();
        #[cfg(test)]
        if let Some(after_cleanup) = self.after_cleanup.take() {
            after_cleanup();
        }
    }
}
```

All preparation API types are application-owned and crate-visible: `SubmitScope`, `SubmitOptions`, `PreparedSubmit`, `SubmitPromptRequest`, `SubmitPromptAnswer`, `SubmitConfigSources`, and `SubmitPreferences`. Add `pub(crate) use submit::{PreparedSubmit, SubmitConfigSources, SubmitOptions, SubmitPreferences, SubmitPromptAnswer, SubmitPromptRequest, SubmitScope};` in `src/application/mod.rs`. `src/commands/submit.rs` deletes its own `SubmitScope`/preparation definitions and imports these exact names from `crate::application`. In this same step, move the existing definitions and inherent/Drop implementations for `ExistingPrLookup`, `PublishSource`, `PushSpec`, `TemporaryPublishRefs`, `TemporarySubmitWorktree`, and `SubmitPhaseTimings` intact so the application preparation model is complete. These pipeline-only types plus `PrPlan`, `ExistingPrSnapshot`, `PreparedSubmitGuards`, and `SubmitResources` stay private.

`PreparedSubmit` fields remain private; CLI obtains only crate-visible `prompt_requests()` and `branches()` accessors. Add `TemporaryPublishRefs::cleanup(&mut self)` by moving its current ref-deletion loop into a method that drains only operation-created `refs/stax/submit/*`; its existing `Drop` delegates to `cleanup`. `TemporarySubmitWorktree::remove` already marks each worktree inactive only after successful removal. The declaration order is mandatory: `PreparedSubmit::guards` is its final field, and `PreparedSubmitGuards::_lease` is its final field, so every plan/config/resource field drops first, `SubmitResources` cleans refs/worktrees next, and `MutationLease` unlocks last. `execute_prepared_submit` does not destructure or manually move `_lease`; every return path consumes/drops the complete `PreparedSubmit`. Rerun the Step 3 test; it now passes.

- [ ] **Step 5: Separate trusted endpoints from repository submit preferences**

`Config::load_for_trusted_network(repository_root)` is the exact Task 3 helper: it reads global trust/auth/provider/API values and permits only repository-local `remote.name`. `Config::load_repository_submit_preferences(repository_root)` may merge repository `submit.*` preferences but cannot override `remote.*`, `auth.*`, or credential source. `SubmitConfigSources::load` combines those two products. `TrustedRemoteInfo::from_repo` and `ForgeClient::new_for_trusted_remote` accept only `trusted_network`.

Add config tests proving a repository-local TOML can change `submit.stack_links` and select a real custom remote named `upstream`, but cannot redirect its provider/API host or enable `gh` auth. The operation must fetch/push `upstream`, never silently fall back to `origin`.

- [ ] **Step 6: Extract the current pipeline by exact symbol/range**

Move from `src/commands/submit.rs` to `src/application/submit.rs`, preserving control flow:

1. all current preparation/execution function bodies that consume the `PrPlan`, lookup, publish-source/spec, temporary-resource, and timing types moved in Step 4;
2. fetch plus parallel `ls_remote_heads`, retry-with-existing-refs safety, trunk validation, and narrow-scope parent validation;
3. `prepare_publish_sources_for_submit`, `temporary_rebased_head`, `default_publish_source`, `ref_needs_push`, and temporary-ref/worktree cleanup;
4. metadata-first/open-PR-by-head/full-scan discovery, owner checks, stale metadata refresh, and duplicate-create recovery;
5. empty/imported classification, current base/title/draft state, `needs_push`, `needs_pr_update`, and `needs_base_update`;
6. transaction snapshot, push, PR create/update/draft/title/body/metadata, stack-link synchronization, native-stack linking, and finalization;
7. helper symbols `push_branches`, `rejected_push_branches`, `push_failure_details`, `stack_pr_infos_for_links`, `discover_stack_link_pr_infos`, `imported_branches_for_stack`, `stack_link_contexts_for_sync`, `stack_has_fork`, `maybe_link_native_stack`, `discover_existing_pr`, `recover_existing_pr_after_duplicate_create`, and all non-presentation helpers they call.

The move preserves all existing branches. Replace terminal concerns only:

```text
LiveTimer/println/eprintln        -> OperationProgress / OperationWarning
Input/Select/Editor              -> SubmitPromptRequest / SubmitPromptAnswer
open_url_in_browser              -> returned PullRequestReceipt URLs
Config::load                     -> SubmitConfigSources
quiet/verbose summaries          -> CLI reporter
```

CLI-only AI generation remains before execution and fills `SubmitPromptAnswer`; application code never imports `commands::generate`.

- [ ] **Step 7: Preserve fetch-before-lease-OID ordering and make leases explicit**

`prepare_submit` acquires the private common-repository mutation lease, calculates affected stack branches, runs `MutationTargets::branches` preflight over current and linked worktrees, then performs fetch and remote PR discovery. Only after successful fetch does it capture each `remote_oid_after_fetch`, compute `needs_push`, and create temporary publish refs. It creates no `ops::Transaction`, pushes nothing, and updates no PR.

`execute_prepared_submit` consumes `PreparedSubmit`, validates common Git directory/current branch/local OIDs have not changed, applies prompt answers, begins the transaction, and pushes with one explicit lease per destination:

```text
--force-with-lease=refs/heads/<branch>:<remote_oid_after_fetch>
```

For a branch absent from fetched remote heads, use an empty expected value after the colon. Never recalculate expected OIDs from stale metadata or after another fetch. The private lease remains owned by `PreparedSubmitGuards` through execution/drop, so another mutation cannot interleave while CLI prompts and cannot begin while temporary cleanup runs.

- [ ] **Step 8: Run parity tests after the move**

Run:

```bash
cargo nextest run submit_fetch_failure_tests::
cargo nextest run submit_pr_base_tests::
cargo nextest run submit_no_verify_tests::
cargo nextest run scoped_submit_tests::
cargo nextest run track_all_prs_tests::submit_stale_metadata_
cargo nextest run track_all_prs_tests::submit_linked_worktree_
cargo nextest run track_all_prs_tests::submit_empty_branch_
cargo nextest run track_all_prs_tests::submit_imported_branch_
cargo nextest run track_all_prs_tests::submit_noop_
cargo nextest run track_all_prs_tests::submit_duplicate_pr_
cargo nextest run track_all_prs_tests::submit_force_with_lease_
cargo nextest run --lib application::submit::tests::prepared_submit_drops_resources_before_mutation_lease
bash scripts/application-boundary-lint.sh
```

Expected: all mature-path characterization tests pass, including explicit post-fetch lease OIDs; no normal submit mutation loop remains in `src/commands/submit.rs`.

- [ ] **Step 9: Commit complete submit extraction**

```bash
git add src/application/mod.rs src/application/submit.rs src/commands/submit.rs src/config/mod.rs src/forge/mod.rs src/remote.rs tests/submit_fetch_failure_tests.rs tests/submit_pr_base_tests.rs tests/track_all_prs_tests.rs
git commit -m "refactor(application): extract mature submit pipeline"
```

## 8. Submit parity, warnings, trusted providers, and command adapters

### Task 8: Expose the extracted pipeline to GUI and both default CLI modes

**Files:**
- Modify: `src/application/submit.rs`
- Modify: `src/application/operation.rs`
- Modify: `src/commands/submit.rs`
- Modify: `src/github/client.rs`
- Modify: `src/forge/mod.rs`
- Modify: `src/forge/gitlab.rs`
- Modify: `src/forge/gitea.rs`
- Modify: `tests/application_operation_tests.rs`
- Modify: `tests/common/mod.rs`
- Modify: `tests/track_all_prs_tests.rs`
- Test: existing submit modules under `tests/`

- [ ] **Step 1: Define public preparation/execution entry points**

Add:

```rust
impl RepositorySession {
    pub(crate) fn prepare_submit(
        &self,
        options: SubmitOptions,
        reporter: &mut dyn OperationReporter,
    ) -> Result<PreparedSubmit, OperationError>;

    pub(crate) fn execute_prepared_submit(
        &self,
        prepared: PreparedSubmit,
        answers: Vec<SubmitPromptAnswer>,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult;

    pub fn submit_stack(
        &self,
        mode: PullRequestMode,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult;
}
```

`SubmitOptions` is the final application-owned preparation type from Task 7; `src/commands/submit.rs` imports it and `SubmitScope` from `crate::application` rather than defining command-local equivalents. `prepare_submit` and `execute_prepared_submit` are crate-visible application seams for the command adapter and are not re-exported as independently framed operations. `prepare_submit` calls `require_blocking_network_context`, acquires `PreparedSubmitGuards` internally, preflights every affected worktree, performs the complete extracted preparation, and returns prompt requests. `execute_prepared_submit` verifies one answer per request and executes the same extracted mutation without moving the guard fields. The command wraps prepare, CLI prompt collection, and execute in one `report_operation` call. `submit_stack` is the GUI/noninteractive convenience: it frames once, prepares with `gui_current_stack_draft`, accepts every suggested title/body as draft, then executes. It never prompts or opens a browser.

Run the ownership compile gate:

```bash
cargo check --bin st
```

Expected: pass; the red ownership/cleanup test ran in Task 7 before the move, and this binary compile gate proves the CLI now imports application-owned preparation types without a duplicate `SubmitScope`.

- [ ] **Step 2: Add linked-rebase and runtime red tests**

Use inline `TestRepo::new_with_remote()` setup. For linked rebase, check a submitted branch out in a linked worktree and create that worktree’s `rebase-merge` marker. Add:

```text
submit_active_rebase_in_linked_branch_reports_canonical_path
submit_linked_rebase_aborts_before_fetch_discovery_or_temp_refs
submit_inside_tokio_returns_runtime_without_remote_change
submit_success_receipt_persistence_failure_returns_remote_side_effects
common::tests::isolated_process_env_removes_all_forge_token_sources
```

For the first three, the mock server request count remains zero, remote refs and metadata bytes are unchanged, and no `refs/stax/submit/*` or transaction exists. The Unix-only persistence test installs a `post-receive` hook in the local bare remote that changes the submitting repository’s ops directory to `0o500` after the remote ref update, with an inline RAII permission guard restoring it before teardown. Assert the remote ref changed, the typed error is `PartialRemoteUpdate` with `RemoteMayHaveChanged`, and its receipt contains `TransactionStatus::Succeeded` plus every completed PR URL. The common-helper test calls `Command::get_envs()` on `IsolatedProcessEnv::command` and asserts each of `STAX_GITHUB_TOKEN`, `STAX_GITLAB_TOKEN`, `STAX_GITEA_TOKEN`, `STAX_FORGE_TOKEN`, `GITHUB_TOKEN`, `GH_TOKEN`, `GITLAB_TOKEN`, and `GITEA_TOKEN` is present with a removed (`None`) value.

Run:

```bash
cargo nextest run application_operation_tests::submit_active_rebase_
cargo nextest run application_operation_tests::submit_linked_rebase_
cargo nextest run application_operation_tests::submit_inside_tokio_
cargo nextest run application_operation_tests::submit_success_receipt_persistence_
cargo nextest run common::tests::isolated_process_env_removes_all_forge_token_sources
```

Expected: failures until the public boundary passes all affected branches to `MutationTargets`, applies the runtime guard before network setup, maps successful-receipt persistence failure without losing remote outcome data, and removes every ambient forge token source.

- [ ] **Step 3: Make warnings exact data variants**

Map existing reviewer/native-stack notes to:

```rust
OperationWarning::SubmitReviewersUnsupported {
    provider,
    reviewers,
}

OperationWarning::SubmitNativeStackAdvisory {
    reason,
    message,
}
```

Use these exact `NativeStackAdvisory` reasons:

```text
GhUnavailable
ExtensionMissing
ExtensionOutdated
ForkedStack
AuthenticationUnsupported
FeatureDisabled
LinkRejected
```

Unsupported reviewers do not fail a submit; preserve the exact skipped reviewer names. Native-stack advice remains non-fatal and never changes stax body/comment stack links. Add unit tests for every reason and integration tests `submit_gitlab_reviewers_return_typed_warning`, `submit_native_stack_fork_returns_typed_advisory`, and `submit_native_stack_auth_returns_typed_advisory`. Application code prints none of them.

- [ ] **Step 4: Route both default CLI modes through prepare/execute**

The normal default paths are:

```text
st submit                     -> prepare_submit; CLI title/body/type prompts; execute_prepared_submit
st submit --no-prompt         -> prepare_submit; suggested title/body + draft; execute_prepared_submit
st submit --yes               -> same automatic answers as --no-prompt
```

The command adapter alone performs template selection, `dialoguer::Input`, `dialoguer::Select`, `dialoguer::Editor`, terminal confirmation, color/output, and optional browser opening after the receipt. It passes resulting `SubmitPromptAnswer` values to application. Both paths use the same fetch/discovery/temp-ref/push/PR/link transaction pipeline.

Keep these advanced orchestrations explicitly outside the Phase 2 GUI contract: `--dry-run`, branch/downstack/upstack scoped commands, `--ai` generation, and `--squash`. They may remain CLI-only, but after producing their extra inputs they must call extracted application seams for any normal push/PR mutation they share; no GUI option exposes them.

Add command tests with a command-local injected prompt interface:

```rust
trait SubmitPrompter {
    fn answer(&mut self, request: &SubmitPromptRequest) -> anyhow::Result<SubmitPromptAnswer>;
}
```

`run_with_prompter` is the production seam; `TerminalSubmitPrompter` uses Dialoguer and tests use `RecordingSubmitPrompter`. Add `interactive_default_calls_prepare_then_execute_once` and `no_prompt_default_calls_prepare_then_execute_once`. Assert ordered reporter stages and that interactive supplied answers reach the mock PR request while no-prompt uses draft suggestions.

- [ ] **Step 5: Make partial results reflect observed side effects**

After a failed multi-ref push, query each destination ref and compare it with `remote_oid_after_fetch`; record only refs that actually changed. After PR processing failure, preserve every completed `PullRequestReceipt`. Finalize the submit transaction failed with step `push` or `pull_request`; return `PartialRemoteUpdate`, `RemoteMayHaveChanged`, and the failed receipt. A fetch/discovery/preflight failure before remote mutation has `None` side effects and no transaction.

Make these red tests pass:

```text
submit_partial_push_receipt_contains_only_changed_remote_refs
submit_partial_pr_receipt_preserves_created_and_updated_urls
submit_noop_receipt_contains_unchanged_urls_and_no_transaction
submit_duplicate_create_receipt_marks_existing_pr_unchanged
submit_success_receipt_persistence_failure_returns_remote_side_effects
```

The partial/no-op/duplicate tests use inline `TestRepo`, bare remote, and `MockServer` setup in `tests/track_all_prs_tests.rs`; no execution fixture type is added. For the already-red persistence test from Step 2, preserve the successful in-memory transaction and every completed PR URL; the persistence failure appears in diagnostics and cannot replace or erase those observed facts.

- [ ] **Step 6: Isolate child network state completely**

Extend `tests/common/mod.rs::IsolatedProcessEnv` to own separate `home`, `config`, and `gh-config` directories. Its `command` sets `HOME`, `STAX_CONFIG_DIR`, `GH_CONFIG_DIR`, `GIT_CONFIG_GLOBAL`, and `GIT_CONFIG_SYSTEM`; removes `STAX_GITHUB_TOKEN`, `STAX_GITLAB_TOKEN`, `STAX_GITEA_TOKEN`, `STAX_FORGE_TOKEN`, `GITHUB_TOKEN`, `GH_TOKEN`, `GITLAB_TOKEN`, and `GITEA_TOKEN`; and writes:

```toml
[auth]
use_gh_cli = false
allow_github_token_env = false
```

unless the test config explicitly supplies other values. No in-process test calls `set_var`, `remove_var`, or `set_current_dir`. One explicit `gh_cli_auth_is_bound_to_validated_hostname` test installs a fake `gh` executable under the child PATH, enables `use_gh_cli`, and asserts `gh auth token --hostname <validated-host>` exactly.

- [ ] **Step 7: Write provider-constructor and redirect red tests**

Add provider-private tests `forge::gitlab::tests::trusted_constructor_stops_cross_authority_redirect` and `forge::gitea::tests::trusted_constructor_stops_cross_authority_redirect`. Each builds a validated `RemoteInfo`, invokes the provider’s trusted constructor, points the trusted mock authority at a redirect to a distinct `localhost`/`127.0.0.1` authority, and asserts the redirected server receives zero requests.

Run:

```bash
cargo nextest run --lib forge::gitlab::tests::trusted_constructor_stops_cross_authority_redirect
cargo nextest run --lib forge::gitea::tests::trusted_constructor_stops_cross_authority_redirect
```

Expected: compile failure because the origin-bound builder and provider trusted constructors do not exist.

- [ ] **Step 8: Implement host-bound constructors for every forge**

In `src/forge/mod.rs`, replace the origin-agnostic authenticated constructor with:

```rust
fn build_http_client_for_origin(
    token: &str,
    auth_style: AuthStyle,
    trusted_api_base_url: &str,
) -> Result<Client>;
```

It parses the trusted API URL once, installs provider headers, and uses a custom redirect policy that follows at most ten redirects only while scheme, normalized host, and effective port equal that original trusted API authority; otherwise it stops before issuing the redirected request. `ForgeClient::new_for_trusted_remote` resolves credentials only after `TrustedRemoteInfo` validation and passes the validated API URL and token into the provider constructor.

In `src/forge/gitlab.rs`, import `build_http_client_for_origin` alongside the existing interactive constructor dependencies from `super` and add exactly:

```rust
pub(crate) fn new_for_trusted_remote(
    remote: &RemoteInfo,
    token: &str,
) -> Result<Self> {
    if remote.forge != ForgeType::GitLab {
        bail!("Internal error: expected trusted GitLab remote");
    }
    let api_base_url = remote
        .api_base_url
        .clone()
        .context("Missing trusted GitLab API base URL")?;
    Ok(Self {
        client: build_http_client_for_origin(
            token,
            AuthStyle::PrivateToken,
            &api_base_url,
        )?,
        api_base_url,
        project_id: remote.encoded_project_path(),
    })
}
```

In `src/forge/gitea.rs`, import `build_http_client_for_origin` alongside the existing interactive constructor dependencies from `super` and add exactly:

```rust
pub(crate) fn new_for_trusted_remote(
    remote: &RemoteInfo,
    token: &str,
) -> Result<Self> {
    if remote.forge != ForgeType::Gitea {
        bail!("Internal error: expected trusted Gitea remote");
    }
    let api_base_url = remote
        .api_base_url
        .clone()
        .context("Missing trusted Gitea API base URL")?;
    Ok(Self {
        client: build_http_client_for_origin(
            token,
            AuthStyle::AuthorizationToken,
            &api_base_url,
        )?,
        api_base_url,
        owner: remote.owner().to_owned(),
        repo: remote.repo.clone(),
    })
}
```

Existing interactive `new` constructors may retain their credential discovery, but no trusted application path calls them.

The complete isolated child suite is:

```text
trusted_github_sends_authorization_only_to_validated_api_host
trusted_gitlab_sends_private_token_only_to_validated_api_host
trusted_gitea_sends_authorization_only_to_validated_api_host
github_cross_authority_redirect_drops_credentials
gitlab_cross_authority_redirect_drops_credentials
gitea_cross_authority_redirect_drops_credentials
repository_preferences_cannot_redirect_trusted_provider
repository_remote_name_selects_custom_remote_without_changing_trust
```

Each test configures a Git remote, explicit global provider/base/API trust, child-only token, same-host mock endpoint, and cross-authority redirect endpoint (`localhost` versus `127.0.0.1` counts as different authority). The redirected server receives zero requests. The provider-private tests instantiate the exact new constructors, not the generic helper directly. If a provider client cannot enforce host binding and redirect stopping, its public path must return `UnsupportedCapability` before any request; do not silently weaken policy. Phase 2 implements safe coverage for GitHub, GitLab, and Gitea.

- [ ] **Step 9: Run complete submit and adapter gates**

Run:

```bash
cargo nextest run application_operation_tests::submit_
cargo nextest run submit_pr_base_tests::
cargo nextest run submit_fetch_failure_tests::
cargo nextest run submit_no_verify_tests::
cargo nextest run scoped_submit_tests::
cargo nextest run track_all_prs_tests::submit_
cargo nextest run track_all_prs_tests::trusted_
cargo nextest run track_all_prs_tests::github_cross_
cargo nextest run track_all_prs_tests::gitlab_cross_
cargo nextest run track_all_prs_tests::gitea_cross_
cargo nextest run --lib commands::submit::tests::interactive_default_
cargo nextest run --lib commands::submit::tests::no_prompt_default_
cargo nextest run --lib github::
cargo nextest run --lib forge::
cargo nextest run --lib forge::gitlab::tests::trusted_constructor_stops_cross_authority_redirect
cargo nextest run --lib forge::gitea::tests::trusted_constructor_stops_cross_authority_redirect
cargo nextest run --lib ops::
bash scripts/application-boundary-lint.sh
```

Expected: fetch freshness, stale metadata, linked worktrees/rebases, temporary publish refs, empty/imported/no-op/duplicate cases, explicit leases, both default CLI modes, exact warnings, partial receipts, and all three provider trust/redirect suites pass.

- [ ] **Step 10: Commit submit parity and adapters**

```bash
git add src/application/operation.rs src/application/submit.rs src/commands/submit.rs src/forge/mod.rs src/forge/gitlab.rs src/forge/gitea.rs src/github/client.rs tests/application_operation_tests.rs tests/common/mod.rs tests/track_all_prs_tests.rs
git commit -m "feat(application): preserve submit parity across interfaces"
```

## 9. Thin CLI/TUI adapters and framed repository execution

### Task 9: Route shared paths through application without reverse dependencies

**Files:**
- Modify: `src/application/mod.rs`
- Modify: `src/application/operation.rs`
- Modify: `src/commands/checkout.rs`
- Modify: `src/commands/branch/create.rs`
- Modify: `src/commands/restack.rs`
- Modify: `src/commands/submit.rs`
- Modify: `src/commands/resolve_pr.rs`
- Modify: `src/commands/pr.rs`
- Modify: `src/commands/open.rs`
- Modify: `src/tui/app.rs`
- Modify: `src/tui/mod.rs`
- Modify: `tests/application_operation_tests.rs`
- Modify: `tests/common/mod.rs`
- Modify: `tests/track_all_prs_tests.rs`
- Test: existing CLI/TUI modules under `tests/`

- [ ] **Step 1: Write failing event-framing tests, including open failure**

Add:

```rust
#[test]
fn repository_open_error_emits_started_then_exactly_one_failed_event() {
    let request = OperationRequest::Checkout { branch: "feature".into() };
    let mut events = Vec::new();
    let error = execute_repository_operation(
        "/definitely/missing/stax-repository",
        request.clone(),
        &mut |event| events.push(event),
    )
    .unwrap_err();
    assert_eq!(error.kind, OperationErrorKind::RepositoryUnavailable);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0], OperationEvent::Started(request));
    assert!(matches!(events[1], OperationEvent::Failed(_)));
}

#[test]
fn uninitialized_repository_maps_to_initialization_required() {
    let repository = tempfile::tempdir().unwrap();
    crate::common::init_test_repo(repository.path()).unwrap();
    let error = execute_repository_operation(
        repository.path(),
        OperationRequest::Restack { scope: RestackScope::All, auto_stash: false },
        &mut NoopOperationReporter,
    )
    .unwrap_err();
    assert_eq!(error.kind, OperationErrorKind::InitializationRequired);
}
```

Add exact mapping cases to `tests/application_operation_tests.rs`:

- `error_mapping_missing_path_is_repository_unavailable`
- `error_mapping_plain_git_is_initialization_required`
- `error_mapping_missing_token_is_authentication`
- `error_mapping_http_403_is_authorization`
- `error_mapping_dirty_tree_is_dirty_worktree`
- `error_mapping_linked_checkout_is_precondition_failed`
- `error_mapping_existing_rebase_is_rebase_in_progress`
- `error_mapping_restack_conflict_is_rebase_conflict`
- `error_mapping_ref_failure_is_local_git`
- `error_mapping_timeout_is_network`
- `error_mapping_changed_remote_is_partial_remote_update`
- `error_mapping_invalid_branch_is_invalid_input`
- `error_mapping_active_lease_is_busy`
- `error_mapping_active_tokio_is_runtime`

Each asserts exact `kind`, safe nonempty `primary`/`action`, a nonempty `diagnostic_chain`, and exact `side_effects`. Task 13 separately tests browser rejection as `UnsupportedCapability`; `application::operation::tests::internal_source_maps_to_internal` invokes the internal-error constructor with a synthetic invariant source and asserts `Internal`.

- [ ] **Step 2: Implement the single framed path dispatcher**

Expose:

```rust
pub fn execute_repository_operation(
    repository_root: impl AsRef<Path>,
    request: OperationRequest,
    reporter: &mut dyn OperationReporter,
) -> OperationResult {
    report_operation(request.clone(), reporter, |reporter| {
        let session = RepositorySession::open(repository_root.as_ref())
            .map_err(|source| map_repository_open_error(&request, repository_root.as_ref(), source))?;
        session.execute_unframed(request, reporter)
    })
}
```

`execute_unframed` is `pub(super)` and dispatches to private unframed operation bodies so events are not doubled. Public `RepositorySession::{checkout,create_empty_branch,restack,submit_stack,resolve_pull_request_url}` use the same unframed bodies inside their own `report_operation`.

Use this pattern for dispatch:

```rust
pub(super) fn execute_unframed(
    &self,
    request: OperationRequest,
    reporter: &mut dyn OperationReporter,
) -> OperationResult {
    match &request {
        OperationRequest::Checkout { branch } => {
            self.checkout_unframed(&request, branch, reporter)
        }
        OperationRequest::CreateBranch { name, parent } => {
            self.create_empty_branch_unframed(&request, name, parent, reporter)
        }
        OperationRequest::Restack { scope, auto_stash } => {
            self.restack_unframed(&request, scope.clone(), *auto_stash, reporter)
        }
        OperationRequest::SubmitStack { new_pull_requests } => {
            self.submit_stack_unframed(&request, *new_pull_requests, reporter)
        }
        OperationRequest::ResolvePullRequestUrl { branch } => {
            self.resolve_pull_request_url_unframed(&request, branch, reporter)
        }
    }
}
```

The four mutation `*_unframed` methods call `with_mutation` internally; the PR resolver does not. Thus both direct public methods and the path-based dispatcher acquire identical safety checks.

`map_repository_open_error` uses `RepositoryUnavailable` when canonicalization/discovery/opening fails. After opening, `execute_unframed` calls `ensure_initialized` before dispatch and maps a missing stax trunk marker to `InitializationRequired`, with action `Run \`st init --trunk <branch>\` and retry`. It never invokes `commands::init` or auto-initializes. Lower boundaries map invalid names/branches to `InvalidInput`, Git/ref/rebase subprocess failures to `LocalGit`, and unexpected invariant failures to `Internal`; every mapping stores the full source chain only in `diagnostic_chain`.

- [ ] **Step 3: Make command modules thin adapters**

Exact delegation:

- checkout: picker/PR-number/shell worktree navigation remain; known branch calls `session.checkout`;
- create: only plain explicit-name empty path delegates; advanced paths use shared naming/lower seams;
- restack: normal mutation delegates; continue/dry-run/prompt/submit-after remain;
- submit: both plain interactive full-current-stack and `--no-prompt`/`--yes` full-current-stack paths call `prepare_submit` then `execute_prepared_submit`; the command owns prompts/output/auto-open. Dry-run, narrow scopes, AI, and squash remain the explicitly listed CLI-only Phase 2 advanced orchestrations from Task 8.
- resolve-pr: returns application URL data without persisting fallback metadata;
- pr/open: browser opening stays in command adapter.

Delete duplicated default business implementations after adapters pass. Application modules never call commands.

- [ ] **Step 4: Define exact TUI in-process and fallback paths**

In `src/tui/app.rs`:

```rust
pub enum PendingAction {
    Operation(OperationRequest),
    LegacyCommands(Vec<Vec<String>>),
}
```

Use in-process operations for Enter checkout, explicit-name `n` create on current parent, confirmed selected/all restack, confirmed draft current-stack submit, and selected-branch PR resolution. `src/tui/mod.rs` executes `Operation` after leaving the draw path; it uses an injected browser callback only for resolved URLs and refreshes after success or side-effecting error.

Keep `LegacyCommands` only for rename, delete, reorder, advanced create, advanced submit, restack continue/dry-run, and other unmigrated actions. A migrated operation error never falls back to subprocess. The temporary TUI subprocess route always calls a thin CLI adapter, not duplicated business logic.

- [ ] **Step 5: Define exact CLI/TUI reporters**

In `src/tui/mod.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TuiOperationStatus {
    pub request: Option<OperationRequest>,
    pub stage: Option<OperationStage>,
    pub completed: usize,
    pub total: Option<usize>,
    pub branch: Option<String>,
    pub message: String,
}

pub struct TuiOperationReporter<'a> {
    status: &'a mut TuiOperationStatus,
}

impl OperationReporter for TuiOperationReporter<'_> {
    fn report(&mut self, event: OperationEvent) {
        match event {
            OperationEvent::Started(request) => {
                self.status.request = Some(request);
                self.status.stage = Some(OperationStage::Validating);
                self.status.completed = 0;
                self.status.total = None;
                self.status.branch = None;
                self.status.message = "Validating repository".into();
            }
            OperationEvent::Progress(progress) => {
                self.status.stage = Some(progress.stage);
                self.status.completed = progress.completed;
                self.status.total = progress.total;
                self.status.branch = progress.branch;
                self.status.message = progress.message;
            }
            OperationEvent::Completed(_) | OperationEvent::Failed(_) => {}
        }
    }
}
```

Terminal events are rendered from the returned `OperationResult` exactly once, not from the reporter. `CliOperationReporter` in the command layer renders the same stage/count facts to stderr and then renders the returned receipt/error once. Application operations never write terminal output.

Stable everyday-operation stage order is:

```text
checkout: Validating -> CheckingOut
create:   Validating -> CreatingBranch
restack:  Validating -> Preparing -> Restacking (0..total)
submit:   Validating -> Preparing(fetch/discovery) -> Pushing (0..total) -> UpdatingPullRequests (0..total)
open PR:  Validating -> ResolvingPullRequest
```

- [ ] **Step 6: Write TUI/CLI sharing and ordered reporter tests**

Add:

```rust
#[test]
fn migrated_tui_actions_never_use_legacy_commands() {
    for request in [
        OperationRequest::Checkout { branch: "feature".into() },
        OperationRequest::CreateBranch { name: "child".into(), parent: "feature".into() },
        OperationRequest::Restack {
            scope: RestackScope::StackContaining("feature".into()),
            auto_stash: false,
        },
        OperationRequest::Restack { scope: RestackScope::All, auto_stash: false },
        OperationRequest::SubmitStack { new_pull_requests: PullRequestMode::Draft },
        OperationRequest::ResolvePullRequestUrl { branch: "feature".into() },
    ] {
        assert!(matches!(PendingAction::Operation(request), PendingAction::Operation(_)));
    }
}

#[test]
fn tui_reporter_preserves_submit_stage_order_and_counts() {
    let request = OperationRequest::SubmitStack {
        new_pull_requests: PullRequestMode::Draft,
    };
    let progress = [
        (OperationStage::Preparing, 0, Some(3), None),
        (OperationStage::Pushing, 1, Some(3), Some("base")),
        (OperationStage::Pushing, 2, Some(3), Some("child")),
        (OperationStage::UpdatingPullRequests, 3, Some(3), Some("tip")),
    ];
    let mut status = TuiOperationStatus::default();
    let mut reporter = TuiOperationReporter { status: &mut status };
    reporter.report(OperationEvent::Started(request.clone()));
    let mut observed = Vec::new();
    for (stage, completed, total, branch) in progress {
        reporter.report(OperationEvent::Progress(OperationProgress {
            stage,
            completed,
            total,
            branch: branch.map(str::to_string),
            message: format!("{stage:?}"),
        }));
        observed.push((reporter.status.stage, reporter.status.completed, reporter.status.total));
    }
    assert_eq!(
        observed,
        vec![
            (Some(OperationStage::Preparing), 0, Some(3)),
            (Some(OperationStage::Pushing), 1, Some(3)),
            (Some(OperationStage::Pushing), 2, Some(3)),
            (Some(OperationStage::UpdatingPullRequests), 3, Some(3)),
        ],
    );
}
```

Add one ordered stage/count test for each everyday operation and assert terminal events leave `TuiOperationStatus` unchanged. Add action-level tests invoking the actual TUI action methods and asserting the resulting `PendingAction` variant; selected PR resolution must produce only `ResolvePullRequestUrl`, never a preceding checkout.

For CLI sharing, use inline `TestRepo::new_with_remote`, `IsolatedProcessEnv`, and `wiremock::MockServer` locals in `default_interactive_submit_renders_shared_application_urls` and `default_no_prompt_submit_renders_shared_application_urls`. Do not define `ChildSubmitFixture`. Assert both paths emit ordered shared stage labels, print returned URLs, and change the same bare remote refs.

In `tests/track_all_prs_tests.rs`, add `submit_custom_host_requires_explicit_global_trust`, `submit_trusted_custom_host_uses_host_bound_credentials`, and `submit_redirect_does_not_forward_credentials`. Each runs the newly delegated default no-prompt path in an isolated child process; the parent owns `MockServer`, and the child alone receives temp `STAX_CONFIG_DIR` and token variables.

- [ ] **Step 7: Run framed-event, reporter, and adapter regressions**

Run:

```bash
cargo nextest run application_operation_tests::repository_open_error_
cargo nextest run application_operation_tests::uninitialized_repository_
cargo nextest run application_operation_tests::error_mapping_
cargo nextest run navigation_tests::
cargo nextest run create_rollback_tests::
cargo nextest run restack_provenance_tests::
cargo nextest run submit_pr_base_tests::
cargo nextest run --lib commands::submit::tests::default_no_prompt_
cargo nextest run application_operation_tests::default_no_prompt_submit_
cargo nextest run track_all_prs_tests::submit_custom_host_
cargo nextest run track_all_prs_tests::submit_trusted_custom_host_
cargo nextest run track_all_prs_tests::submit_redirect_
cargo nextest run tui_commands_tests::
cargo nextest run --lib tui::tests::tui_reporter_
bash scripts/application-boundary-lint.sh
```

Expected: open/init errors have one Started/Failed pair, both default CLI submit paths share the application pipeline, migrated TUI actions use typed requests, stage/count order is exact, terminal results render once, advanced actions have explicit legacy fallback, and application boundary lint passes.

- [ ] **Step 8: Commit adapters and dispatcher**

```bash
git add src/application/mod.rs src/application/operation.rs src/commands/branch/create.rs src/commands/checkout.rs src/commands/open.rs src/commands/pr.rs src/commands/resolve_pr.rs src/commands/restack.rs src/commands/submit.rs src/tui/app.rs src/tui/mod.rs tests/application_operation_tests.rs tests/common/mod.rs tests/track_all_prs_tests.rs
git commit -m "refactor(cli): delegate repository operations"
```

## 10. Unsigned developer app and fresh-instance launcher

### Task 10: Assemble/install Stax.app and launch one fresh instance per path

**Files:**
- Create: `crates/stax-gui/resources/Info.plist.in`
- Create: `scripts/build-gui-app.sh`
- Create: `scripts/gui-app-tests.sh`
- Modify: `Makefile`
- Modify: `.github/workflows/rust-tests.yml`
- Modify: `src/cli/args.rs`
- Modify: `src/cli/mod.rs`
- Create: `src/commands/gui.rs`
- Modify: `src/commands/mod.rs`
- Create: `tests/gui_command_tests.rs`
- Modify: `tests/all_tests.rs`

- [ ] **Step 1: Write the failing bundle-template test**

Create `scripts/gui-app-tests.sh`. It writes an executable fixture binary, invokes the production assembler with `STAX_GUI_BINARY` and `STAX_GUI_OUTPUT`, then asserts:

```bash
test -x "$app/Contents/MacOS/Stax"
/usr/bin/plutil -lint "$app/Contents/Info.plist"
test "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist")" = "dev.stax.Stax"
test "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Contents/Info.plist")" = "Stax"
test "$(/usr/libexec/PlistBuddy -c 'Print :CFBundlePackageType' "$app/Contents/Info.plist")" = "APPL"
```

The same script creates a recording `lsregister` executable, sets `HOME` to the fixture directory and `STAX_GUI_LSREGISTER` to the recorder, invokes `build-gui-app.sh --install`, then asserts `$HOME/Applications/Stax.app` exists and the recorder received that exact path. It never writes the developer’s real Applications directory.

Run:

```bash
bash scripts/gui-app-tests.sh
```

Expected: failure because the template and assembler do not exist.

- [ ] **Step 2: Create the minimal Info.plist template and assembler**

`crates/stax-gui/resources/Info.plist.in` contains valid XML with:

```xml
<key>CFBundleDevelopmentRegion</key><string>en</string>
<key>CFBundleExecutable</key><string>@EXECUTABLE@</string>
<key>CFBundleIdentifier</key><string>@BUNDLE_ID@</string>
<key>CFBundleName</key><string>Stax</string>
<key>CFBundleDisplayName</key><string>Stax Developer Preview</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>LSMinimumSystemVersion</key><string>13.0</string>
<key>NSHighResolutionCapable</key><true/>
```

`scripts/build-gui-app.sh`:

1. rejects non-macOS with a clear error;
2. builds `cargo build -p stax-gui` unless `STAX_GUI_BINARY` is supplied;
3. assembles `${STAX_GUI_OUTPUT:-target/gui/Stax.app}/Contents/MacOS/Stax`;
4. replaces only `@EXECUTABLE@` and `@BUNDLE_ID@` in the plist;
5. validates the plist with `plutil`;
6. with `--install`, replaces `$HOME/Applications/Stax.app` and registers it using `${STAX_GUI_LSREGISTER:-/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister} -f <installed-app>`;
7. prints that this is unsigned developer-preview output.

The script does not create icons, sign, notarize, build a universal binary, or produce release archives.

- [ ] **Step 3: Add Make and CI targets**

Add:

```make
.PHONY: gui-app gui-app-test install-gui-app
gui-app:
	./scripts/build-gui-app.sh

gui-app-test:
	./scripts/gui-app-tests.sh

install-gui-app:
	./scripts/build-gui-app.sh --install
```

In the existing macOS `gui-quality` job, retain check/nextest/Clippy and add `make gui-app-test` plus `make gui-app`. Do not remove or weaken any existing gate.

- [ ] **Step 4: Run bundle tests and inspect the assembled preview**

Run:

```bash
make gui-app-test
make gui-app
/usr/bin/plutil -p target/gui/Stax.app/Contents/Info.plist
test -x target/gui/Stax.app/Contents/MacOS/Stax
```

Expected: all commands pass and the plist reports bundle ID `dev.stax.Stax`. Do not install or launch the app during tests.

- [ ] **Step 5: Write failing Clap and exact launcher tests**

Define `GuiArgs` and the `Gui` enum variant in `src/cli/args.rs` tests first:

```rust
#[test]
fn gui_accepts_optional_path_and_defaults_to_none() {
    let with_path = Cli::try_parse_from(["st", "gui", "/tmp/repo"]).unwrap();
    assert!(matches!(
        with_path.command,
        Some(Commands::Gui(GuiArgs { path: Some(path) }))
            if path == PathBuf::from("/tmp/repo")
    ));
    let default = Cli::try_parse_from(["st", "gui"]).unwrap();
    assert!(matches!(default.command, Some(Commands::Gui(GuiArgs { path: None }))));
}
```

In `src/commands/gui.rs` tests:

```rust
#[test]
fn launcher_opens_a_fresh_instance_with_one_canonical_path_argument() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("repo with spaces");
    std::fs::create_dir(&path).unwrap();
    let runner = RecordingCommandRunner::succeeding();

    run_with_runner(Some(path.clone()), Platform::MacOs, &runner).unwrap();

    assert_eq!(runner.program(), PathBuf::from("/usr/bin/open"));
    assert_eq!(
        runner.args(),
        vec![
            OsString::from("-n"),
            OsString::from("-b"),
            OsString::from("dev.stax.Stax"),
            OsString::from("--args"),
            path.canonicalize().unwrap().into_os_string(),
        ]
    );
}

#[test]
fn launcher_failure_points_to_the_repository_install_target() {
    let temp = tempfile::tempdir().unwrap();
    let error = run_with_runner(
        Some(temp.path().to_path_buf()),
        Platform::MacOs,
        &RecordingCommandRunner::failing(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("make install-gui-app"));
    assert!(error.to_string().contains("unsigned developer preview"));
}

#[test]
fn unsupported_platform_never_runs_a_command() {
    let temp = tempfile::tempdir().unwrap();
    let runner = RecordingCommandRunner::succeeding();
    let error = run_with_runner(
        Some(temp.path().to_path_buf()),
        Platform::Unsupported("linux"),
        &runner,
    )
    .unwrap_err();
    assert!(error.to_string().contains("only supported on macOS"));
    assert!(runner.args().is_empty());
}
```

- [ ] **Step 6: Run launcher tests and verify the red state**

Run:

```bash
cargo nextest run --lib cli::args::tests::gui_
cargo nextest run --lib commands::gui::tests::
```

Expected: compile failure because `GuiArgs`, `Commands::Gui`, and the launcher do not exist.

- [ ] **Step 7: Implement the pre-init launcher in the correct modules**

In `src/cli/args.rs`:

```rust
#[derive(Args, Debug, Clone)]
pub struct GuiArgs {
    /// Repository to open; defaults to the current directory
    pub path: Option<PathBuf>,
}
```

Add `Gui(GuiArgs)` to the `Commands` enum in that same file. `src/cli/mod.rs` only matches and dispatches it before config/repository initialization:

```rust
if let Some(Commands::Gui(args)) = &cli.command {
    return crate::commands::gui::run(args.path.clone());
}
```

`src/commands/gui.rs` canonicalizes supplied path or current directory, rejects non-macOS before spawning, and invokes `/usr/bin/open` with exactly the five arguments from the test. `CommandRunner` is injected in unit tests. Production also honors `STAX_GUI_OPEN_EXECUTABLE` as a developer/CI launcher override; it must be an absolute executable path and changes only the program, never the five LaunchServices arguments. This legitimate production seam lets binary integration tests exercise pre-init dispatch without opening an installed app. False status/spawn errors mention the attempted launcher, `make install-gui-app`, and `$HOME/Applications/Stax.app`.

`-n` is mandatory: every `st gui [path]` launches a fresh app process/window and forwards exactly one canonical repository path after `--args`.

- [ ] **Step 8: Add binary bypass/error-precedence tests that never launch an app**

Register `gui_command_tests.rs`. Test:

```rust
fn gui_command(cwd: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(crate::common::stax_bin());
    command
        .current_dir(cwd)
        .env_remove("STAX_GUI_OPEN_EXECUTABLE")
        .env_remove("STAX_CONFIG_DIR")
        .env_remove("STAX_GITHUB_TOKEN")
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_TOKEN")
        .env("STAX_DISABLE_UPDATE_CHECK", "1");
    command
}

#[cfg(unix)]
fn recording_launcher(root: &Path, exit_code: i32) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let launcher = root.join("record-open");
    let arguments = root.join("arguments");
    std::fs::write(
        &launcher,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nexit {}\n",
            arguments.display(),
            exit_code
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&launcher).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&launcher, permissions).unwrap();
    (launcher, arguments)
}

#[test]
fn gui_help_works_outside_a_repository() {
    let output = gui_command(tempfile::tempdir().unwrap().path())
        .args(["gui", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("[PATH]"));
}

#[test]
fn gui_missing_path_fails_before_platform_launch() {
    let temp = tempfile::tempdir().unwrap();
    let output = gui_command(temp.path())
        .args(["gui", "missing path"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("missing path"));
}

#[cfg(all(target_os = "macos", unix))]
#[test]
fn gui_bypasses_initialization_in_plain_git_repository() {
    let repo = crate::common::TestRepo::new();
    let recorder = tempfile::tempdir().unwrap();
    let (launcher, arguments) = recording_launcher(recorder.path(), 0);
    let refs_before = crate::common::TestRepo::stdout(&repo.git(&["show-ref"]));
    let status_before = crate::common::TestRepo::stdout(&repo.git(&["status", "--porcelain=v1"]));
    let output = gui_command(&repo.path())
        .env("STAX_GUI_OPEN_EXECUTABLE", &launcher)
        .args(["gui", repo.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        std::fs::read_to_string(arguments).unwrap().lines().collect::<Vec<_>>(),
        vec![
            "-n",
            "-b",
            "dev.stax.Stax",
            "--args",
            repo.path().canonicalize().unwrap().to_str().unwrap(),
        ],
    );
    assert_eq!(crate::common::TestRepo::stdout(&repo.git(&["show-ref"])), refs_before);
    assert_eq!(
        crate::common::TestRepo::stdout(&repo.git(&["status", "--porcelain=v1"])),
        status_before
    );
}

#[cfg(all(target_os = "macos", unix))]
#[test]
fn launcher_error_wins_over_active_rebase_and_leaves_repository_untouched() {
    let repo = crate::common::TestRepo::new();
    std::fs::create_dir_all(repo.path().join(".git/rebase-merge")).unwrap();
    let recorder = tempfile::tempdir().unwrap();
    let (launcher, _arguments) = recording_launcher(recorder.path(), 1);
    let refs_before = crate::common::TestRepo::stdout(&repo.git(&["show-ref"]));
    let output = gui_command(&repo.path())
        .env("STAX_GUI_OPEN_EXECUTABLE", &launcher)
        .args(["gui", repo.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("make install-gui-app"));
    assert!(!stderr.contains("rebase"));
    assert!(!stderr.contains("st init"));
    assert!(repo.path().join(".git/rebase-merge").is_dir());
    assert_eq!(crate::common::TestRepo::stdout(&repo.git(&["show-ref"])), refs_before);
}
```

Also add `gui_missing_app_result_is_actionable` with a recording launcher exit code matching `/usr/bin/open`’s missing-application failure and `gui_spawn_error_wins_over_repository_state` with an absolute nonexistent override path. The latter must report the spawn error/install action, not initialization/rebase. On non-macOS, add unsupported behavior before any override spawn. No test invokes `/usr/bin/open` or an installed app.

- [ ] **Step 9: Run app/launcher checks and commit**

Run:

```bash
make gui-app-test
cargo nextest run --lib cli::args::tests::gui_
cargo nextest run --lib commands::gui::tests::
cargo nextest run gui_command_tests::
```

Expected: all tests pass and no test opens Stax.app.

```bash
git add .github/workflows/rust-tests.yml Makefile crates/stax-gui/resources/Info.plist.in scripts/build-gui-app.sh scripts/gui-app-tests.sh src/cli/args.rs src/cli/mod.rs src/commands/gui.rs src/commands/mod.rs tests/all_tests.rs tests/gui_command_tests.rs
git commit -m "feat(gui): assemble and launch developer app"
```

## 11. GPUI operation service and side-effect-aware state

### Task 11: Stream framed operations and refresh safely after partial failures

**Files:**
- Modify: `crates/stax-gui/Cargo.toml`
- Modify: `Cargo.lock`
- Create: `crates/stax-gui/src/operation.rs`
- Modify: `crates/stax-gui/src/lib.rs`
- Modify: `crates/stax-gui/src/state.rs`
- Modify: `crates/stax-gui/src/views/app.rs`
- Modify: `crates/stax-gui/src/views/mod.rs`
- Create: `crates/stax-gui/src/views/operation_tests.rs`

Define all GUI test support in `crates/stax-gui/src/views/operation_tests.rs`; later tests use only this declared support:

```text
fn loaded_workspace_state(root: &str) -> WorkspaceState;
fn checkout_request() -> OperationRequest;
fn restack_request() -> OperationRequest;
fn submit_request() -> OperationRequest;
fn checkout_receipt() -> OperationReceipt;
fn conflict_error_with_failed_receipt(side_effects: OperationSideEffects) -> OperationError;
fn dirty_error(side_effects: OperationSideEffects) -> OperationError;
fn partial_submit_error(side_effects: OperationSideEffects) -> OperationError;
fn local_git_error() -> OperationError;
fn details(label: &str) -> BranchDetails;
fn snapshot_after_partial_submit() -> RepositorySnapshot;
fn run_service_to_completion(
    service: &dyn OperationService,
    repository_root: PathBuf,
    request: OperationRequest,
    cx: &mut gpui::TestAppContext,
) -> (Vec<OperationEvent>, OperationResult);

fn open_focused_branch_input(
    cx: &mut gpui::TestAppContext,
) -> (gpui::Entity<BranchNameInput>, &mut gpui::TestAppContext);
fn open_focused_branch_input_with_text(
    cx: &mut gpui::TestAppContext,
    text: &str,
) -> (gpui::Entity<BranchNameInput>, &mut gpui::TestAppContext);
fn open_input_only_action_probe(
    cx: &mut gpui::TestAppContext,
) -> (
    gpui::Entity<InputOnlyActionProbe>,
    gpui::Entity<BranchNameInput>,
    &mut gpui::TestAppContext,
);
fn open_create_overlay_with_focused_input(
    cx: &mut gpui::TestAppContext,
) -> (
    gpui::Entity<AppView>,
    &mut gpui::TestAppContext,
    Arc<FakeOperationService>,
);
fn open_loaded_app(
    cx: &mut gpui::TestAppContext,
) -> (
    gpui::Entity<AppView>,
    &mut gpui::TestAppContext,
    Arc<FakeOperationService>,
);
fn open_loaded_app_with_dirty_restack(
    cx: &mut gpui::TestAppContext,
) -> (
    gpui::Entity<AppView>,
    &mut gpui::TestAppContext,
    Arc<FakeOperationService>,
);
fn open_loaded_app_with_focused_stack_row(
    cx: &mut gpui::TestAppContext,
) -> (
    gpui::Entity<AppView>,
    &mut gpui::TestAppContext,
    Arc<FakeOperationService>,
);
fn open_loaded_app_with_browser(
    cx: &mut gpui::TestAppContext,
) -> (
    gpui::Entity<AppView>,
    &mut gpui::TestAppContext,
    Arc<FakeOperationService>,
    Arc<RecordingBrowserService>,
);
```

`loaded_workspace_state` uses one literal `RepositorySnapshot` builder in this file with branches `main -> parent -> child`; the request/receipt/error helpers construct the exact public application types from Task 1. The open helpers all call one private `open_test_app(snapshot, services, initial_focus, cx)` and differ only in snapshot dirtiness/focus/service arguments. `FakeOperationService` defines `requests`, `script_pr_url`, `complete_next_success`, `complete_next_error`, and `complete_submit_with_url`; each resolves exactly one queued retained future and emits the matching single terminal event. `RecordingBrowserService::urls` returns its recorded HTTP(S) URLs. No later task may name an undeclared fixture/helper.

- [ ] **Step 1: Add required dependencies**

Run:

```bash
cargo add async-channel url --package stax-gui
```

Expected: direct dependencies are added and `Cargo.lock` updates.

- [ ] **Step 2: Write failing state tests for success and both failure classes**

Add:

```rust
#[test]
fn successful_mutation_invalidates_before_refresh() {
    let mut state = loaded_workspace_state("/repo");
    let token = state.begin_operation(checkout_request()).unwrap();
    let generation = state.generation();
    let effect = state.finish_operation(&token, Ok(checkout_receipt())).unwrap();
    assert!(effect.refresh_snapshot);
    assert!(state.generation() > generation);
}

#[test]
fn side_effecting_failure_refreshes_and_preserves_error_and_receipt() {
    let mut state = loaded_workspace_state("/repo");
    let token = state.begin_operation(restack_request()).unwrap();
    let error = conflict_error_with_failed_receipt(OperationSideEffects::RepositoryChanged);
    let generation = state.generation();
    let effect = state.finish_operation(&token, Err(error.clone())).unwrap();
    assert!(effect.refresh_snapshot);
    assert!(state.generation() > generation);
    assert_eq!(state.operation_error(), Some(&error));
    assert_eq!(state.last_receipt(), error.receipt.as_ref());
}

#[test]
fn precondition_failure_does_not_refresh() {
    let mut state = loaded_workspace_state("/repo");
    let token = state.begin_operation(restack_request()).unwrap();
    let error = dirty_error(OperationSideEffects::None);
    let generation = state.generation();
    let effect = state.finish_operation(&token, Err(error.clone())).unwrap();
    assert!(!effect.refresh_snapshot);
    assert_eq!(state.generation(), generation);
    assert_eq!(state.operation_error(), Some(&error));
}
```

- [ ] **Step 3: Write failing stale hydration-after-partial-failure test**

```rust
#[test]
fn hydration_started_before_partial_failure_is_rejected_after_refresh() {
    let mut state = loaded_workspace_state("/repo");
    let old_details = state.begin_details_load("child").unwrap();
    let token = state.begin_operation(submit_request()).unwrap();
    let error = partial_submit_error(OperationSideEffects::RemoteMayHaveChanged);
    let effect = state.finish_operation(&token, Err(error.clone())).unwrap();
    assert!(effect.refresh_snapshot);
    assert!(!state.apply_details(old_details, details("stale")));
    state.replace_snapshot(snapshot_after_partial_submit());
    assert_eq!(state.operation_error(), Some(&error));
    assert_eq!(state.last_receipt(), error.receipt.as_ref());
}
```

- [ ] **Step 4: Run state tests and verify the red state**

Run:

```bash
cargo nextest run -p stax-gui views::operation_tests::successful_mutation_
cargo nextest run -p stax-gui views::operation_tests::side_effecting_failure_
cargo nextest run -p stax-gui views::operation_tests::hydration_started_
```

Expected: compile failure because operation state and effects do not exist.

- [ ] **Step 5: Define tokens, active state, and completion effects**

In `state.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationToken {
    pub id: u64,
    pub repository_root: PathBuf,
    pub repository_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveOperation {
    pub token: OperationToken,
    pub request: OperationRequest,
    pub progress: Option<OperationProgress>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionEffect {
    pub refresh_snapshot: bool,
    pub preferred_selection: Option<String>,
    pub open_url: Option<String>,
}
```

`begin_operation` rejects every second operation in a window and captures canonical path/generation. `apply_operation_event` requires all token fields to match and mutates state only for `Started`/`Progress`; it returns the terminal event to the coordinator without applying completion. `finish_operation` is the only terminal state transition, requires the same token match, stores error/receipt first, then invalidates repository/detail/diff/CI generations when success is mutating or error side effects require refresh. `replace_snapshot` preserves the current completion banner/error/receipt when replacing the same repository after an operation refresh; opening a different root clears them.

- [ ] **Step 6: Write failing service framing tests**

Define:

```rust
pub type OperationFuture = Pin<Box<dyn Future<Output = OperationResult> + Send + 'static>>;

pub trait OperationService: Send + Sync {
    fn execute(
        &self,
        repository_root: PathBuf,
        request: OperationRequest,
        events: async_channel::Sender<OperationEvent>,
    ) -> OperationFuture;
}

pub trait BrowserService: Send + Sync {
    fn open_url(&self, url: &str, cx: &mut gpui::App) -> Result<(), String>;
}
```

Test:

```rust
#[gpui::test]
fn native_service_open_failure_has_one_started_and_one_failed_terminal(
    cx: &mut gpui::TestAppContext,
) {
    let (events, result) = run_service_to_completion(
        &NativeOperationService,
        PathBuf::from("/missing/repository"),
        checkout_request(),
        cx,
    );
    assert!(result.is_err());
    assert_eq!(events.len(), 2);
    assert!(matches!(events[0], OperationEvent::Started(_)));
    assert!(matches!(events[1], OperationEvent::Failed(_)));
}
```

- [ ] **Step 7: Implement native/fake services and bounded coordination**

`NativeOperationService::execute` runs `execute_repository_operation` on GPUI’s background executor, passing an `OperationReporter` that uses `send_blocking`; channel capacity is 32. It does not open a session before framing, so repository-open errors receive Started/Failed from the application free function.

`NativeBrowserService` parses `url::Url`, allows only HTTP(S), and uses GPUI’s platform URL API. Add `FakeOperationService` and `RecordingBrowserService` under `#[cfg(test)]`.

`AppView::start_operation`:

1. obtains a token;
2. creates `async_channel::bounded(32)`;
3. retains the background operation task;
4. runs one foreground coordinator draining events until sender closure;
5. applies only `Started` and `Progress` events to active state; `Completed`/`Failed` are retained solely to verify service framing and never call `finish_operation`;
6. awaits the retained `OperationResult` after sender closure and calls `finish_operation` exactly once from that result;
7. refreshes same canonical root for `refresh_snapshot`;
8. ignores all effects for stale token.

`finish_operation` consumes/removes the active token, and a second call for the same token returns `None`. A debug assertion verifies the streamed terminal event has the same success/failure polarity as the retained result. No render-loop polling, `Command::new("st")`, output parsing, or process-CWD changes.

- [ ] **Step 8: Test bounded progress, stale completion, and real partial state**

Add GPUI tests:

- `coordinator_drains_more_than_channel_capacity_in_order`
- `retained_result_is_the_only_completion_state_transition`
- `terminal_event_plus_retained_result_finishes_exactly_once`
- `coordinator_ignores_completion_after_opening_another_repository`
- `restack_conflict_refreshes_actual_changed_snapshot_and_keeps_failed_receipt`
- `partial_submit_refreshes_actual_remote_state_and_rejects_old_hydration`
- `precondition_error_keeps_snapshot_generation`

For the exactly-once tests, `FakeOperationService` emits `Started`, one `Progress`, and `Completed(receipt.clone())`, then resolves its retained future with `Ok(receipt)`. Add a `#[cfg(test)] completion_transition_count` increment only inside successful `WorkspaceState::finish_operation`; assert it is `1`, refresh count is `1`, and replaying the terminal event or retained result cannot increment either. Repeat with `Failed(error)` plus `Err(error)`.

The restack test builds the real Task 6 repository inline with `TestRepo` and runs `NativeOperationService`. For submit, the Task 8 isolated child first produces real partial remote state and a failed persisted `OpReceipt`; the GUI test opens that repository, starts hydration, and scripts the matching typed terminal error through `FakeOperationService`, then verifies refreshed repository facts and stale hydration rejection. No GUI test changes process config/env, and actual application partial-submit behavior remains asserted independently in Task 8.

- [ ] **Step 9: Run coordinator checks and commit**

Run:

```bash
cargo nextest run -p stax-gui views::operation_tests::native_service_
cargo nextest run -p stax-gui views::operation_tests::coordinator_
cargo nextest run -p stax-gui views::operation_tests::retained_result_
cargo nextest run -p stax-gui views::operation_tests::terminal_event_plus_
cargo nextest run -p stax-gui views::operation_tests::restack_conflict_refreshes_
cargo nextest run -p stax-gui views::operation_tests::partial_submit_refreshes_
cargo check -p stax-gui
```

Expected: framing, bounded stream, success/partial refresh, preserved errors, stale completion, and stale hydration tests pass.

```bash
git add Cargo.lock crates/stax-gui/Cargo.toml crates/stax-gui/src/lib.rs crates/stax-gui/src/operation.rs crates/stax-gui/src/state.rs crates/stax-gui/src/views/app.rs crates/stax-gui/src/views/mod.rs crates/stax-gui/src/views/operation_tests.rs
git commit -m "feat(gui): coordinate repository operations"
```

## 12. Minimal GPUI input and explicit operation confirmations

### Task 12: Implement UTF-16/IME input and create/restack/submit overlays

**Files:**
- Modify: `crates/stax-gui/Cargo.toml`
- Modify: `Cargo.lock`
- Create: `crates/stax-gui/src/views/text_input.rs`
- Create: `crates/stax-gui/src/views/operation_overlay.rs`
- Modify: `crates/stax-gui/src/views/mod.rs`
- Modify: `crates/stax-gui/src/views/app.rs`
- Modify: `crates/stax-gui/src/views/workspace.rs`
- Modify: `crates/stax-gui/src/state.rs`
- Modify: `crates/stax-gui/src/theme.rs`
- Modify: `crates/stax-gui/src/views/operation_tests.rs`

- [ ] **Step 1: Add grapheme support**

Run:

```bash
cargo add unicode-segmentation --package stax-gui
```

Expected: direct dependency and lockfile update.

- [ ] **Step 2: Write failing text insertion, UTF-16, IME, and shortcut-context tests**

Add separate tests:

```rust
#[gpui::test]
fn branch_input_inserts_platform_text_and_backspaces_one_grapheme(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = open_focused_branch_input(cx);
    cx.simulate_input("feature-🦀");
    cx.simulate_keystrokes("backspace");
    assert_eq!(cx.update(|_, app| input.read(app).text().to_string()), "feature-");
}

#[gpui::test]
fn branch_input_maps_utf16_replacement_ranges(cx: &mut gpui::TestAppContext) {
    let (input, cx) = open_focused_branch_input_with_text(cx, "a🦀b");
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            EntityInputHandler::replace_text_in_range(input, Some(1..3), "x", window, cx);
        });
    });
    assert_eq!(cx.update(|_, app| input.read(app).text().to_string()), "axb");
}

#[gpui::test]
fn branch_input_tracks_and_commits_ime_marked_text(cx: &mut gpui::TestAppContext) {
    let (input, cx) = open_focused_branch_input(cx);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            EntityInputHandler::replace_and_mark_text_in_range(
                input, None, "に", Some(1..1), window, cx,
            );
            assert_eq!(EntityInputHandler::marked_text_range(input, window, cx), Some(0..1));
            EntityInputHandler::replace_text_in_range(input, None, "日本", window, cx);
        });
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            assert_eq!(input.text(), "日本");
            assert_eq!(EntityInputHandler::marked_text_range(input, window, cx), None);
            assert_eq!(
                EntityInputHandler::selected_text_range(input, false, window, cx)
                    .unwrap()
                    .range,
                2..2
            );
        });
    });
}

#[gpui::test]
fn input_key_context_suppresses_parent_actions_without_overlay_guard(
    cx: &mut gpui::TestAppContext,
) {
    let (probe, input, cx) = open_input_only_action_probe(cx);
    assert!(cx.update(|window, app| input.read(app).focus_handle().is_focused(window)));
    assert!(!cx.update(|_, app| probe.read(app).overlay_guard_enabled()));
    cx.simulate_keystrokes("n r shift-r s p");
    assert_eq!(cx.update(|_, app| probe.read(app).parent_action_count()), 0);
}

#[gpui::test]
fn text_insertion_is_independent_from_shortcut_suppression(
    cx: &mut gpui::TestAppContext,
) {
    let (app, cx, _service) = open_create_overlay_with_focused_input(cx);
    cx.simulate_input("nrsp");
    assert_eq!(cx.update(|_, gpui| app.read(gpui).branch_input_text()), "nrsp");
}
```

`InputOnlyActionProbe` is defined in `operation_tests.rs` as a test view that renders `BranchNameInput` with its normal key context, registers the five Workspace parent actions, increments `parent_action_count` in each handler, and deliberately returns `false` from `overlay_guard_enabled`. `open_input_only_action_probe` creates that view, focuses the input, and returns its two entities plus the test context. This proves key-context suppression independently of AppView’s overlay guard. The shortcut test uses real key actions/keystrokes; `simulate_input` appears only in insertion tests.

- [ ] **Step 3: Run input tests and verify the red state**

Run:

```bash
cargo nextest run -p stax-gui views::operation_tests::branch_input_
cargo nextest run -p stax-gui views::operation_tests::input_key_context_suppresses_
```

Expected: compile failure because `BranchNameInput` and input key context do not exist.

- [ ] **Step 4: Implement the complete minimal EntityInputHandler**

Define:

```rust
pub struct BranchNameInput {
    focus_handle: FocusHandle,
    text: SharedString,
    selected_range: Range<usize>,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
}
```

Implement all GPUI 0.2.2 `EntityInputHandler` methods: `text_for_range`, `selected_text_range`, `marked_text_range`, `unmark_text`, `replace_text_in_range`, `replace_and_mark_text_in_range`, `bounds_for_range`, and `character_index_for_point`.

Internal selections/marked ranges use UTF-8 byte offsets. `offset_from_utf16`, `offset_to_utf16`, `range_from_utf16`, and `range_to_utf16` iterate `char_indices`, clamp out-of-range offsets, and never split a scalar. `replace_and_mark_text_in_range` interprets the incoming selection relative to inserted marked text and clears marked state on commit. Left/right/backspace use Unicode grapheme boundaries.

Implement only Backspace, Delete, Left, Right, Home, and End actions in `BranchNameInput` context. No clipboard, cut/copy, drag selection, select-all, multiline, or rich-text behavior is added in Phase 2. During element paint, store shaped line/bounds and call:

```rust
window.handle_input(
    &focus_handle,
    ElementInputHandler::new(bounds, input_entity),
    cx,
);
```

Every Workspace operation handler also returns without action when an overlay is active or `BranchNameInput::focus_handle` is focused. This explicit guard suppresses Workspace `n`/`r`/Shift-R/`s`/`p` bindings even though printable text arrives through the platform input handler.

- [ ] **Step 5: Write failing overlay and focus tests**

Define exact overlay state:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationOverlay {
    CreateBranch { parent: String, validation_error: Option<String> },
    ConfirmRestack {
        scope: RestackScope,
        affected_branches: Vec<String>,
        auto_stash: bool,
    },
    ConfirmStashAndRestack {
        scope: RestackScope,
        dirty_worktrees: Vec<PathBuf>,
    },
    ConfirmSubmit {
        current_branch: String,
        affected_branches: Vec<String>,
        mode: PullRequestMode,
    },
}
```

Add these exact GPUI cases:

```rust
#[gpui::test]
fn s_opens_submit_confirmation_before_request(cx: &mut gpui::TestAppContext) {
    let (app, cx, service) = open_loaded_app(cx);
    cx.simulate_keystrokes("s");
    assert!(service.requests().is_empty());
    let overlay = cx.update(|_, gpui| app.read(gpui).operation_overlay().cloned());
    assert!(matches!(
        overlay,
        Some(OperationOverlay::ConfirmSubmit {
            affected_branches,
            mode: PullRequestMode::Draft,
            ..
        }) if affected_branches == vec!["parent", "child"]
    ));
}

#[gpui::test]
fn submit_enter_confirms_and_escape_cancels(cx: &mut gpui::TestAppContext) {
    let (app, cx, service) = open_loaded_app(cx);
    cx.simulate_keystrokes("s escape");
    assert!(service.requests().is_empty());
    cx.simulate_keystrokes("s enter");
    assert_eq!(service.requests(), vec![submit_request()]);
    assert!(cx.update(|_, gpui| app.read(gpui).operation_overlay().is_none()));
}

#[gpui::test]
fn dirty_restack_uses_explicit_stash_confirmation(cx: &mut gpui::TestAppContext) {
    let (app, cx, service) = open_loaded_app_with_dirty_restack(cx);
    cx.simulate_keystrokes("r enter");
    cx.run_until_parked();
    assert!(matches!(
        cx.update(|_, gpui| app.read(gpui).operation_overlay().cloned()),
        Some(OperationOverlay::ConfirmStashAndRestack { .. })
    ));
    cx.simulate_keystrokes("enter");
    assert!(matches!(
        service.requests().last(),
        Some(OperationRequest::Restack { auto_stash: true, .. })
    ));
}

#[gpui::test]
fn modal_cancel_and_completion_restore_prior_focus(cx: &mut gpui::TestAppContext) {
    let (app, cx, service) = open_loaded_app_with_focused_stack_row(cx);
    let prior = cx.update(|window, _| window.focused());
    cx.simulate_keystrokes("n escape");
    assert_eq!(cx.update(|window, _| window.focused()), prior);
    cx.simulate_keystrokes("s enter");
    service.complete_next_success();
    cx.run_until_parked();
    assert_eq!(cx.update(|window, _| window.focused()), prior);
    assert!(cx.update(|_, gpui| app.read(gpui).operation_overlay().is_none()));
}

#[gpui::test]
fn escape_during_restack_or_submit_does_not_cancel_active_mutation(
    cx: &mut gpui::TestAppContext,
) {
    let (app, cx, service) = open_loaded_app(cx);
    cx.simulate_keystrokes("r enter escape");
    assert!(cx.update(|_, gpui| app.read(gpui).active_operation().is_some()));
    assert_eq!(service.requests().len(), 1);
    service.complete_next_success();
    cx.run_until_parked();
    cx.simulate_keystrokes("s enter escape");
    assert!(cx.update(|_, gpui| app.read(gpui).active_operation().is_some()));
    assert_eq!(service.requests().len(), 2);
}

#[gpui::test]
fn terminal_error_restores_prior_focus(cx: &mut gpui::TestAppContext) {
    let (app, cx, service) = open_loaded_app_with_focused_stack_row(cx);
    let prior = cx.update(|window, _| window.focused());
    cx.simulate_keystrokes("enter");
    service.complete_next_error(local_git_error());
    cx.run_until_parked();
    assert_eq!(cx.update(|window, _| window.focused()), prior);
    assert!(cx.update(|_, gpui| app.read(gpui).operation_error().is_some()));
}
```

- [ ] **Step 6: Render explicit confirmations and focus lifecycle**

Create overlay cards:

- Create: explicit parent, branch input, Cancel/Create.
- Restack: exact scope and affected branch list, “Rebase rewrites local commits”; `auto_stash: false`.
- Stash-and-restack: exact dirty worktree paths, “Stashes are kept if a conflict stops the rebase”; confirm sends `auto_stash: true`.
- Submit: current-stack scope, affected branches, `New pull requests: Draft`, and “This pushes branches and may create or update remote pull requests.”

`s` only opens `ConfirmSubmit`; Enter confirms; Escape cancels. `r`/Shift-R only open restack confirmation. A `DirtyWorktree` response opens `ConfirmStashAndRestack`; it is never represented by mutating the previous overlay’s hidden state.

`AppView` records the focused handle before opening an overlay and restores it after cancel or terminal completion when that handle remains valid. There is no cancel button or Escape cancellation after an operation starts.

- [ ] **Step 7: Run input/overlay/focus tests and commit**

Run:

```bash
cargo nextest run -p stax-gui views::operation_tests::branch_input_
cargo nextest run -p stax-gui views::operation_tests::input_key_context_suppresses_
cargo nextest run -p stax-gui views::operation_tests::text_insertion_
cargo nextest run -p stax-gui views::operation_tests::submit_
cargo nextest run -p stax-gui views::operation_tests::dirty_restack_
cargo nextest run -p stax-gui views::operation_tests::modal_cancel_
cargo nextest run -p stax-gui views::operation_tests::escape_during_
cargo nextest run -p stax-gui views::operation_tests::terminal_error_
cargo check -p stax-gui
```

Expected: UTF-16, grapheme, IME, key-context suppression, create/restack/stash/submit confirmation, cancellation, and focus restoration pass.

```bash
git add Cargo.lock crates/stax-gui/Cargo.toml crates/stax-gui/src/state.rs crates/stax-gui/src/theme.rs crates/stax-gui/src/views/app.rs crates/stax-gui/src/views/mod.rs crates/stax-gui/src/views/operation_overlay.rs crates/stax-gui/src/views/operation_tests.rs crates/stax-gui/src/views/text_input.rs crates/stax-gui/src/views/workspace.rs
git commit -m "feat(gui): confirm repository operations"
```

## 13. GUI actions, non-cancellable busy state, banners, and URLs

### Task 13: Complete contextual controls and operation presentation

**Files:**
- Modify: `crates/stax-gui/src/lib.rs`
- Modify: `crates/stax-gui/src/state.rs`
- Modify: `crates/stax-gui/src/theme.rs`
- Modify: `crates/stax-gui/src/views/app.rs`
- Modify: `crates/stax-gui/src/views/workspace.rs`
- Modify: `crates/stax-gui/src/views/inspector_pane.rs`
- Modify: `crates/stax-gui/src/views/tests.rs`
- Modify: `crates/stax-gui/src/views/operation_tests.rs`

- [ ] **Step 1: Write failing interaction-disable tests**

Add:

```rust
#[test]
fn active_mutation_disables_operations_open_refresh_and_navigation() {
    let mut state = loaded_workspace_state("/repo");
    state.begin_operation(submit_request()).unwrap();
    let actions = state.interaction_state();
    assert!(!actions.checkout.enabled);
    assert!(!actions.create.enabled);
    assert!(!actions.restack.enabled);
    assert!(!actions.submit.enabled);
    assert!(!actions.open_pr.enabled);
    assert!(!actions.open_repository.enabled);
    assert!(!actions.refresh.enabled);
    assert!(!actions.navigation.enabled);
    assert!(actions.refresh.reason.unwrap().contains("operation"));
}
```

Add GPUI checks that Cmd-O, Cmd-R, Up, Down, toolbar buttons, and contextual buttons do nothing while a mutation is active. Verify they work again after terminal completion.

- [ ] **Step 2: Write failing banner-dismiss and clickable URL tests**

```rust
#[gpui::test]
fn dismiss_banner_clears_presentation_without_changing_snapshot(cx: &mut gpui::TestAppContext) {
    let (app, cx, service) = open_loaded_app(cx);
    cx.simulate_keystrokes("enter");
    service.complete_next_success();
    cx.run_until_parked();
    let snapshot = cx.update(|_, gpui| app.read(gpui).snapshot().clone());
    cx.dispatch_action(DismissOperationBanner);
    assert!(!cx.update(|_, gpui| app.read(gpui).banner_is_visible()));
    assert_eq!(cx.update(|_, gpui| app.read(gpui).snapshot().clone()), snapshot);
}

#[gpui::test]
fn clicking_submit_receipt_url_uses_browser_service(cx: &mut gpui::TestAppContext) {
    let (app, cx, service, browser) = open_loaded_app_with_browser(cx);
    cx.simulate_keystrokes("s enter");
    service.complete_submit_with_url("https://github.com/acme/repo/pull/42");
    cx.run_until_parked();
    let bounds = cx.debug_bounds("operation-receipt-url-0").unwrap();
    cx.simulate_click(bounds.center(), gpui::Modifiers::default());
    assert_eq!(browser.urls(), vec!["https://github.com/acme/repo/pull/42"]);
    assert_eq!(cx.update(|_, gpui| app.read(gpui).snapshot_refresh_count()), 1);
}

#[gpui::test]
fn browser_rejects_non_http_url_as_unsupported_capability(
    cx: &mut gpui::TestAppContext,
) {
    let (app, cx, service) = open_loaded_app(cx);
    service.script_pr_url("feature", "file:///tmp/not-allowed");
    cx.simulate_keystrokes("down p");
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).operation_error().unwrap().kind),
        OperationErrorKind::UnsupportedCapability
    );
    assert_eq!(cx.update(|_, gpui| app.read(gpui).snapshot_refresh_count()), 0);
}

#[gpui::test]
fn p_resolves_selected_branch_without_checkout_or_refresh(
    cx: &mut gpui::TestAppContext,
) {
    let (app, cx, service, browser) = open_loaded_app_with_browser(cx);
    service.script_pr_url("feature", "https://github.com/acme/repo/pull/42");
    cx.simulate_keystrokes("down p");
    cx.run_until_parked();
    assert_eq!(
        service.requests(),
        vec![OperationRequest::ResolvePullRequestUrl { branch: "feature".into() }]
    );
    assert_eq!(browser.urls(), vec!["https://github.com/acme/repo/pull/42"]);
    assert_eq!(cx.update(|_, gpui| app.read(gpui).current_branch()), "main");
    assert_eq!(cx.update(|_, gpui| app.read(gpui).snapshot_refresh_count()), 0);
}
```

Give the rendered dismiss button, diagnostic-copy button, each receipt URL row, every toolbar button, and every inspector button matching GPUI debug selectors. Tests call `TestAppContext::debug_bounds(selector)`, then `simulate_click(bounds.center(), Modifiers::default())`; they do not call AppView handlers. The URL test clicks the actual rendered row and never shells out.

- [ ] **Step 3: Run interaction/banner tests and verify the red state**

Run:

```bash
cargo nextest run -p stax-gui views::operation_tests::active_mutation_
cargo nextest run -p stax-gui views::operation_tests::dismiss_banner_
cargo nextest run -p stax-gui views::operation_tests::clicking_submit_
cargo nextest run -p stax-gui views::operation_tests::browser_rejects_
cargo nextest run -p stax-gui views::operation_tests::p_resolves_
```

Expected: compile failure because unified interaction state, dismissal, and receipt URL handlers do not exist.

- [ ] **Step 4: Register exact actions and key bindings**

Register:

```rust
actions!(
    stax_gui,
    [
        CheckoutSelected,
        CreateBranch,
        RestackSelected,
        RestackAll,
        SubmitStack,
        OpenPullRequest,
        ConfirmOverlay,
        DismissOverlay,
        DismissOperationBanner,
        OpenReceiptUrl,
    ]
);
```

Workspace bindings are Enter checkout, `n`, `r`, Shift-R, `s`, and `p`. Overlay bindings are Enter confirm and Escape dismiss. Keep Cmd-R, Cmd-O, Up, and Down. The input key context takes precedence over Workspace.

- [ ] **Step 5: Implement one interaction-availability model**

Add:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionAvailability {
    pub enabled: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionState {
    pub checkout: ActionAvailability,
    pub create: ActionAvailability,
    pub restack: ActionAvailability,
    pub restack_all: ActionAvailability,
    pub submit: ActionAvailability,
    pub open_pr: ActionAvailability,
    pub open_repository: ActionAvailability,
    pub refresh: ActionAvailability,
    pub navigation: ActionAvailability,
}
```

`interaction_state()` returns this type for operation actions, Open Repository, Refresh, and navigation.

While any mutation is active, all listed interactions are disabled because mutation cannot be cancelled. While nonmutating PR resolution is active, start of a second operation is disabled but selection may remain. Normal requirements:

- checkout: selected tracked branch is neither current nor trunk;
- create: repository loaded and no overlay;
- selected restack: selected tracked non-trunk branch;
- all restack: at least one tracked non-trunk branch;
- submit: current stack has at least one non-trunk branch;
- open PR: selected non-trunk branch; resolver handles absent cached metadata.

Buttons and shortcuts consume the same model and show identical disabled reason.

- [ ] **Step 6: Render controls and structured banners**

Workspace toolbar: Open Repository, Refresh, Create Branch, Submit Stack. Inspector: Checkout, Restack, Open PR. Controls remain visible when disabled and show busy state/reason.

One banner renders:

- progress stage/message/branch/completed/total;
- success summary, warnings, affected branches;
- submit Created/Updated/Unchanged clickable URL rows;
- error safe `primary` and `action`;
- transaction id/status/canonical `can_undo`;
- Copy Diagnostics control for `diagnostic_chain`;
- explicit dismiss action after terminal state.

Do not show a cancel control during mutation. Banner dismissal clears only GUI presentation state; persisted `OpReceipt` files and current snapshot remain.

- [ ] **Step 7: Complete refresh/browser and stale-effect behavior**

On checkout/create/restack/submit success: invalidate, refresh, and preserve/select preferred branch. On restack conflict/partial submit: invalidate and refresh actual repository state while preserving error/receipt. On guaranteed no-side-effect failure: no refresh. On resolved PR: browser only, no checkout/refresh/metadata write. On browser error: show `UnsupportedCapability` presentation with copy-URL action. On stale token: perform none.

- [ ] **Step 8: Run complete GUI action and presentation tests**

Before the aggregate run, add these real render/action cases to `operation_tests.rs`:

```text
toolbar_create_button_dispatches_create_action
toolbar_submit_button_opens_submit_confirmation
toolbar_open_and_refresh_buttons_dispatch_actions
inspector_checkout_button_dispatches_selected_checkout
inspector_restack_button_opens_selected_confirmation
inspector_open_pr_button_resolves_selected_without_checkout
enter_n_r_shift_r_s_p_shortcuts_dispatch_exact_requests
cmd_r_cmd_o_and_arrows_keep_existing_actions
progress_banner_renders_stage_branch_and_completed_total
copy_diagnostics_button_writes_only_diagnostic_chain
create_button_stays_disabled_for_empty_or_invalid_name
create_enter_shows_validation_without_request
dismiss_button_and_action_clear_only_presentation
receipt_url_rendered_row_opens_recording_browser
every_action_is_disabled_during_active_mutation
```

Button tests click stable element IDs through GPUI element event dispatch. Shortcut tests use `simulate_keystrokes`. Progress/validation tests inspect the rendered element tree/text rather than private state alone. Diagnostic copy injects `RecordingClipboardService`, clicks `operation-copy-diagnostics`, and asserts the exact `diagnostic_chain` while safe primary/action remain separately rendered. Each request assertion compares the full `OperationRequest`, including selected branch/scope/auto-stash/mode.

Run:

```bash
cargo nextest run -p stax-gui views::operation_tests::
cargo nextest run -p stax-gui views::tests::
cargo nextest run -p stax-gui
cargo check -p stax-gui
cargo clippy -p stax-gui --all-targets -- -D warnings
```

Expected: controls, shortcuts, real input suppression, submit confirmation, busy disablement, focus, banner dismissal, clickable URLs, success/partial refresh, stale rejection, and browser failures pass with no new Clippy warnings.

- [ ] **Step 9: Commit GUI controls and presentation**

```bash
git add crates/stax-gui/src/lib.rs crates/stax-gui/src/state.rs crates/stax-gui/src/theme.rs crates/stax-gui/src/views/app.rs crates/stax-gui/src/views/inspector_pane.rs crates/stax-gui/src/views/operation_tests.rs crates/stax-gui/src/views/tests.rs crates/stax-gui/src/views/workspace.rs
git commit -m "feat(gui): add stack operation controls"
```

## 14. Documentation, complete verification, and stacked PR

### Task 14: Document developer preview, run all gates, and publish

**Files:**
- Modify: `README.md`
- Create: `docs/interface/gui.md`
- Modify: `docs/commands/core.md`
- Modify: `docs/commands/reference.md`
- Modify: `mkdocs.yml`
- Modify: `skills.md`
- Verify: every file changed in Tasks 1-13

- [ ] **Step 1: Capture the documentation red state**

Run:

```bash
rg -n "install-gui-app|unsigned developer preview|open -n|fresh app|Confirm submit|Shift-R" README.md docs skills.md
```

Expected: the complete Phase 2 developer bundle, fresh-instance launch, confirmation, and shortcut contract is absent.

- [ ] **Step 2: Update README and command documentation**

README documents:

```markdown
make install-gui-app   # Build and install the unsigned local developer preview
st gui [path]          # Launch a fresh Stax.app instance for one repository
```

State that the GUI checks out selected branches, creates explicit-name empty children, restacks selected/all, confirms and submits the current stack as draft without prompts/auto-open, and opens a selected PR without checkout.

`docs/commands/core.md` adds `st gui [path]`. `docs/commands/reference.md` documents optional current-directory default, canonical path, macOS-only support, exact `open -n -b dev.stax.Stax --args <path>` contract, fresh process/window per invocation, and actionable `make install-gui-app` failure.

- [ ] **Step 3: Create the complete GUI interface guide**

`docs/interface/gui.md` has these exact sections:

1. **Developer preview install** — unsigned local target, `make gui-app`, `make install-gui-app`, `$HOME/Applications/Stax.app`.
2. **Launch and windows** — `st gui`, explicit path, canonical forwarding, `-n` fresh instance/window.
3. **Workspace** — stack/changes/inspector/background hydration.
4. **Confirmed mutations** — create, restack, stash-and-restack, submit scope/branches/draft/remote warning.
5. **Shortcuts** — Enter, `n`, `r`, Shift-R, `s`, `p`, Cmd-R, Cmd-O, arrows.
6. **Progress and receipts** — warnings as data, clickable URLs, diagnostic copy, banner dismissal.
7. **Safety and recovery** — one mutation, disabled open/refresh/navigation, no mid-rebase/push cancel, refresh after partial effects, continue/abort/resolve.
8. **Current limits** — AI/staging/below/insert/advanced submit remain CLI; icon/final metadata/signing/notarization/universal/release artifacts are Phase 4.

Add the page under Interface in `mkdocs.yml`.

- [ ] **Step 4: Update `skills.md` without changing package version**

Add `stax gui [path]`, developer-preview install, fresh-instance semantics, typed repository-scoped operations, explicit submit confirmation/draft/no-auto-open, and selected PR opening without checkout. Keep AI/staging/insert/below as CLI workflows. Keep `stax-skills-version: 0.94.0`.

- [ ] **Step 5: Verify documentation and launcher discovery**

Run:

```bash
rg -n "make install-gui-app|dev\.stax\.Stax|fresh|unsigned|Draft|Shift-R|Open PR|Phase 4" README.md docs/interface/gui.md docs/commands/core.md docs/commands/reference.md skills.md
cargo run --quiet -- gui --help
```

Expected: required facts appear and help shows optional `[PATH]` without repository initialization or app launch.

- [ ] **Step 6: Run formatting, architecture lint, and focused root tests**

Run:

```bash
cargo fmt --all -- --check
bash scripts/application-boundary-lint-tests.sh
bash scripts/application-boundary-lint.sh
make gui-app-test
cargo nextest run --lib application::
cargo nextest run --lib ops::
cargo nextest run application_operation_tests::
cargo nextest run gui_command_tests::
cargo nextest run navigation_tests::
cargo nextest run create_rollback_tests::
cargo nextest run create_ai_tests::
cargo nextest run create_below_tests::
cargo nextest run create_insert_tests::
cargo nextest run restack_provenance_tests::
cargo nextest run conflict_handling_tests::
cargo nextest run continue_tests::
cargo nextest run abort_tests::
cargo nextest run submit_pr_base_tests::
cargo nextest run submit_fetch_failure_tests::
cargo nextest run submit_no_verify_tests::
cargo nextest run scoped_submit_tests::
cargo nextest run track_all_prs_tests::
cargo nextest run tui_commands_tests::
```

Expected: formatting, architecture lint, bundle test, application/ops mappings, all operation behavior, security, adapter, and recovery tests pass.

- [ ] **Step 7: Run the exact macOS GUI quality gate**

Run:

```bash
cargo check -p stax-gui --locked
cargo nextest run -p stax-gui --locked
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
make gui-app-test
make gui-app
```

Expected: this matches the retained macOS `gui-quality` job and all commands pass.

- [ ] **Step 8: Run repository lint and full suite**

Start Docker Desktop on macOS, then:

```bash
make lint
make test
```

Expected: both pass. If Docker reports `failed to connect to the docker API`, launch Docker Desktop and rerun `make test`; do not replace the full suite with a native run.

- [ ] **Step 9: Inspect scope, architecture, security, and placeholders**

Run:

```bash
git diff --check
git status --short
for needle in \
  "TO""DO" \
  "TB""D" \
  "implement ""later" \
  "fill in ""details" \
  "appropriate error ""handling" \
  "Similar to ""Task" \
  "Co-Authored-""By" \
  "Generated ""with" \
  "Made ""with" \
  "Written ""by"; do
  if rg -n -F "$needle" docs/superpowers/plans/2026-07-12-stax-gui-phase-2.md; then
    exit 1
  fi
done
bash scripts/application-boundary-lint-tests.sh
bash scripts/application-boundary-lint.sh
if rg -n '[A-Za-z]+Fixture::|fixture\.' docs/superpowers/plans/2026-07-12-stax-gui-phase-2.md; then
  exit 1
fi
base="$(git merge-base origin/cesar/gpui-gui-phase-1-ready HEAD)"
git diff "$base"..HEAD -- src/application src/ops src/commands src/tui crates/stax-gui scripts tests README.md docs skills.md Makefile .github/workflows/rust-tests.yml
```

Expected: no whitespace/placeholders/attribution, no forbidden application dependency/output, no credentials, and no unrelated files. Confirm `src/ops` has no `crate::application` reference and only application conversion calls canonical `OpReceipt::can_undo()`.

- [ ] **Step 10: Commit documentation**

```bash
git add README.md docs/commands/core.md docs/commands/reference.md docs/interface/gui.md mkdocs.yml skills.md
git commit -m "docs(gui): document developer stack workflows"
```

- [ ] **Step 11: Verify stack and publish the Phase 2 PR**

Run:

```bash
git fetch origin cesar/gpui-gui-phase-1-ready
git merge-base --is-ancestor origin/cesar/gpui-gui-phase-1-ready HEAD
git status --short
git push -u origin cesar/gpui-gui-phase-2
gh pr create \
  --base cesar/gpui-gui-phase-1-ready \
  --head cesar/gpui-gui-phase-2 \
  --title "feat(gui): add native stack operations" \
  --body "$(cat <<'EOF'
## Summary
- share typed repository-scoped checkout, create, restack, submit, and read-only PR resolution across application adapters
- add side-effect-aware GPUI confirmations, progress, receipts, stale-result safety, and contextual controls
- assemble an unsigned developer Stax.app and launch a fresh instance with `st gui [path]`

## Test plan
- [x] targeted application, ops, command, TUI, security, and bundle tests
- [x] `cargo check -p stax-gui --locked`
- [x] `cargo nextest run -p stax-gui --locked`
- [x] macOS GUI Clippy and bundle gates
- [x] `make lint`
- [x] `make test`

## Stack
- Based on Phase 1 PR #611
- Final icon, metadata, signing, notarization, universal builds, and release distribution remain Phase 4
EOF
)"
```

Expected: merge-base exits 0 before push and GitHub returns a PR URL based on `cesar/gpui-gui-phase-1-ready`.

## Completed plan self-review

- [x] All approved operations have one typed request and shared application implementation.
- [x] `ops` remains application-agnostic; receipt conversion and canonical undo mapping live in application.
- [x] Lease/preflight APIs are private and restack/submit preflight every affected linked worktree with a typed canonical rebase path.
- [x] PR fallback is read-only and direct Tokio network paths return typed runtime errors.
- [x] Trusted GitHub/GitLab/Gitea endpoints use isolated HOME/config/GH state, disable ambient gh auth, bind credentials to validated hosts, and strip them on cross-authority redirects.
- [x] Forked restack scope, linked dirty target stashing, conflict state, and one failed-receipt finalizer for conflict/non-conflict/persistence failures are exact.
- [x] Submit extracts the existing complete pipeline intact, preserving fetch freshness, temporary publish refs/worktrees, empty/imported/no-op/duplicate behavior, explicit leases, discovery, and PR state.
- [x] Both interactive and noninteractive default CLI submit paths call the same prepared application operation; advanced CLI-only boundaries are explicit.
- [x] Application owns and crate-reexports `SubmitScope` plus every CLI preparation type; the submit command imports them and defines no duplicate.
- [x] `PreparedSubmitGuards` drops all temporary worktrees/refs/resources before its final `MutationLease`, with a real busy-during-cleanup unit test.
- [x] `Config::load_for_trusted_network(root)` preserves only repository-local `remote.name`; provider/API/auth trust remains global-only and a custom-remote test prevents origin fallback.
- [x] Successful receipt persistence failures retain a successful in-memory receipt and exact `RepositoryChanged`/`RemoteMayHaveChanged` side effects for local restack and remote submit.
- [x] Hermetic child commands remove all generic/provider token variables, while GitLab/Gitea trusted constructors bind credentials and redirects to the validated API authority.
- [x] PR resolution has one `resolve_with_lookup` seam implemented by `ForgeClient`, uses `git::refs::metadata_refname`, and never names a false metadata namespace.
- [x] Exact reviewer/native-stack submit warnings are data and covered by tests.
- [x] Recursive application architecture lint catches current/future direct, grouped, aliased, and fully-qualified command/TUI/terminal/framework/output references.
- [x] Developer app assembly/install and pre-init launcher bypass/error precedence are tested without launching.
- [x] Create, restack, stash-and-restack, and submit confirmations are explicit.
- [x] Success and side-effecting failure refresh; precondition failure does not; stale hydration is rejected.
- [x] Input tests separate platform insertion from a parent action counter proving real key-context suppression without relying on an overlay guard.
- [x] Active mutations disable open/refresh/navigation and expose no cancel.
- [x] Real GPUI element/action tests cover every button/shortcut, progress, diagnostics copy, create validation, dismissal, clickable submit URLs, and browser errors.
- [x] The retained background result is the only GUI completion authority and exactly-one transition is tested.
- [x] CLI/TUI sharing paths, ordered reporter stages/counts, terminal rendering, and legacy fallbacks are exact without application output or GUI subprocesses.
- [x] Every named test helper has an exact declaration/owner, and mature restack/submit tests use inline `TestRepo` setup rather than pseudo-fixtures.
- [x] Documentation and final verification cover the unsigned developer-preview boundary.
