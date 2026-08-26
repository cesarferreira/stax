# stax Coding Patterns Reference

## New Command Registration (5-step chain)

Every new command touches these files in order:

**Step 1 — `src/cli/args.rs`**: Add a variant to the `Commands` enum.
```rust
/// Short description shown in --help
Foo {
    /// Flag description
    #[arg(short, long)]
    bar: bool,
},
```

**Step 2 — `src/cli/mod.rs`**: Add a dispatch arm in `pub fn run()`.
```rust
Commands::Foo { bar } => commands::foo::run(bar),
```
Commands that don't need a repo (like `auth`, `doctor`) must be dispatched *before* `ensure_initialized()`.

**Step 3 — `src/commands/foo.rs`**: Implement the command.
```rust
use anyhow::Result;
pub fn run(bar: bool) -> Result<()> {
    // ...
    Ok(())
}
```

**Step 4 — `src/commands/mod.rs`**: Register the module.
```rust
pub mod foo;
```

**Step 5 (if undo-supported) — `src/ops/receipt.rs`**: Extend `OpKind`.
```rust
pub enum OpKind {
    // ...existing...
    Foo,
}
impl OpKind {
    pub fn display_name(&self) -> &'static str {
        match self {
            // ...existing...
            OpKind::Foo => "foo",
        }
    }
}
```

## Error Handling

Use `anyhow` throughout — no `unwrap()` or `expect()` outside tests.
```rust
use anyhow::{bail, anyhow, Result};

fn example() -> Result<()> {
    bail!("something went wrong: {}", reason);
    // or
    Err(anyhow!("context: {}", detail))
}
```

## Progress Spinners

```rust
use crate::progress::LiveTimer;

let _timer = LiveTimer::maybe_new(!quiet, "Doing work...");
// spinner stops when _timer is dropped
```

## Transaction Support (for undo-able ops)

```rust
use crate::ops::tx::Transaction;
use crate::ops::receipt::OpKind;

let mut tx = Transaction::begin(OpKind::Foo, &repo, quiet)?;
tx.plan_branch("feature/foo")?;
tx.snapshot()?;  // writes backup refs + in-progress receipt

// ... do work ...

tx.record_after("feature/foo", new_oid)?;
tx.finish_ok()?;  // or tx.finish_err("message")?;
```

## Async GitHub Calls

GitHub API methods are async but most command code is sync. Wrap with a runtime:
```rust
let rt = tokio::runtime::Runtime::new()?;
let result = rt.block_on(async {
    github_client.some_method().await
})?;
```

## Borrow Checker: Vec Iteration + Mutation

When iterating with an index and mutating a `Vec`, clone `String` values early:
```rust
// Wrong — borrow conflict:
for i in 0..branches.len() {
    process(&branches[i].name, &mut branches)?;
}

// Right — clone early:
for i in 0..branches.len() {
    let name = branches[i].name.clone();
    process(&name, &mut branches)?;
}
```

## cascade.rs Dependency

`cascade.rs` calls `restack::run()` directly. When changing `restack::run()`'s signature, update `cascade.rs` too.

## TUI Background Operations

Background ops use mpsc channels:
```rust
let (tx, rx) = std::sync::mpsc::channel::<MyUpdate>();
std::thread::spawn(move || {
    // ... long work ...
    let _ = tx.send(MyUpdate::Done(result));
});
// Poll in refresh_background():
while let Ok(update) = rx.try_recv() { ... }
```

Status bar priority: removal_status > status_message > loading_summary > default.

Two-stage confirmation pattern: check dirty state first, show force modal if needed.

## Test Registration

New integration test files must be registered in `tests/all_tests.rs`:
```rust
#[path = "foo_tests.rs"]
mod foo_tests;
```
Reach shared helpers with `use crate::common;` (not `mod common;`).

Scope a test run to a module: `cargo nextest run foo_tests::`.

## Key File Locations

| Concern | File |
|---------|------|
| CLI args / Commands enum | `src/cli/args.rs` |
| Command dispatch | `src/cli/mod.rs` |
| Module registration | `src/commands/mod.rs` |
| OpKind enum | `src/ops/receipt.rs` |
| Transaction API | `src/ops/tx.rs` |
| GitRepo methods | `src/git/repo.rs` |
| Metadata storage | `src/engine/metadata.rs` |
| Stack tree | `src/engine/stack.rs` |
| GitHub client | `src/github/` |
| User config | `src/config/mod.rs` |
| Integration test entry | `tests/all_tests.rs` |
