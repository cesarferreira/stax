# Lessons

- When changing the `stax co` UI, match the `stax ls` visual language (colors, tree/indentation) and confirm it visually. Do not ship a redesign without verifying the output looks like the `ls` tree and that selection emphasis is obvious.
- If interactive lists scroll the terminal on navigation, clear and position the cursor before invoking the dialog to avoid rendering into the lower viewport.
- When adding or changing CLI commands/flags, update both `README.md` and `docs/` command references in the same change and verify parity against `stax --help` before marking docs complete.
- When sync reparents children off merged branches, never clear `parent_branch_revision`; preserve the old-base boundary (or merged parent tip) so restack can run `git rebase --onto <new> <old>` and avoid replaying already-integrated commits.
- Integration tests that shell out to `git`/`stax` must be hermetic: strip GitHub token env and force `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM` to null so contributor-specific git config (URL rewrites, filters, hooks) cannot change behavior or add large runtime overhead.
- Integration tests that shell out to the compiled `stax` binary must resolve a path that exists at runtime; prefer helper logic that falls back from `CARGO_BIN_EXE_stax` to the sibling `target/.../stax` binary so `nextest` and Docker runs stay stable.
- Tests that must run outside any Git repository must not use temp dirs rooted under `STAX_TEST_TMPDIR`/`TMPDIR` inside the workspace; `git discover` walks parent directories, so those fixtures can accidentally execute inside the repo during `make test-native`.
- For full-suite test runs, use `make test` or `just test` (never `cargo test`); on macOS the default should use Docker for performance and consistency.
- For stack-merge flows that delete merged branches, always rebase and retarget descendant branches/PR bases before cleanup; deleting a base branch first can auto-close descendant PRs on GitHub.
- Any descendant-rebase path (`merge`, `merge --when-ready`, `restack`, `upstack restack`, `sync --restack`) must preserve provenance boundaries (`parent_branch_revision` / old parent tip) and use provenance-aware rebase logic; plain `git rebase <trunk>` will replay already-integrated parent history after squash merges.
- Configure explicit connect/read/write timeouts for GitHub API clients (and other network clients); never rely on library defaults for long-running CLI flows where silent waits look like hangs.
