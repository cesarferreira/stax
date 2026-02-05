# Plan
- [x] Inspect current `stax ls` renderer and checkout picker implementation to identify shared styling hooks and tree layout logic.
- [x] Design `stax co` list to reuse `stax ls` styling: colored tree/indentation, branch status coloring, and a clear selection emphasis.
- [x] Implement the `stax co` renderer changes and selection highlighting, keeping behavior stable with filtering and navigation.
- [x] Add or update tests for the formatter/renderer behavior where feasible.
- [x] Verify behavior by running the relevant tests and sanity-checking the visual output.

# Review
- [x] Updated `stax co` to render ls-style colored tree lines with branch status and restack hints; improved selection prominence and defaulted selection to current branch.
- [x] Tests: `cargo test checkout` (passes).
