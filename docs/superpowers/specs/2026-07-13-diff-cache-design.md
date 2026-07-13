# Revision-Keyed Diff Cache Design

## Status

Approved direction for implementation on `cesar/gpui-gui-diff-cache`, stacked
on `cesar/gpui-gui-phase-4`.

## Problem

The shared diff cache currently serializes every cached patch into one
`.git/stax/tui-diff-cache.json` file. Looking up one branch reads and parses the
entire file. Updating one branch reads and rewrites the entire file. The GUI
also retains only the selected patch, so selecting a previously visited branch
shows a loading state while the same aggregate file is parsed again.

In the repository used to reproduce the issue, the aggregate file was 20.3 MB
with 27 entries and 166,175 patch lines. A single command-line JSON parse took
about 260 ms, which is enough to make every branch selection feel uncached.

## Goals

- Show patches for branches visited during the current GUI session without a
  loading-state round trip.
- Read and deserialize only the requested persistent cache entry.
- Preserve revision-safe reuse across the CLI, TUI, and GUI.
- Keep the implementation dependency-free and easy to inspect.
- Bound cache growth without putting cleanup work on the selection path.

## Non-goals

- A general-purpose database or query layer.
- Persisting rendered GPUI elements or scroll state.
- Guaranteeing that cache data survives corruption or format changes; cache
  contents are disposable.
- Migrating the aggregate cache in place.

## Architecture

### Session memory cache

`WorkspaceState` will retain a bounded set of ready branch diffs for the
current repository snapshot. Entries are keyed by branch and parent names,
because the presentation snapshot does not expose object IDs. Applying either
a persistent-cache result or a freshly calculated result records the patch.

Selecting a branch already present in this session cache immediately restores
its ready patch before hydration begins. Hydration may continue in the
background, but the changes pane remains populated and presents only its
existing subtle refreshing indicator.

The memory cache is cleared when a refreshed repository snapshot is applied or
after an operation changes repository state. The currently displayed patch may
remain visible under the existing stale-while-refreshing behavior. This avoids
serving a name-keyed patch after an external ref update while still making
ordinary A → B → A navigation synchronous.

The cache is capped at 32 entries with least-recently-used eviction. This is
large enough for normal stacks while preventing large patches from remaining
resident without bound.

### Persistent per-entry cache

The shared persistent cache will store one JSON document per immutable diff
revision under:

```text
.git/stax/diff-cache/v1/
└── <parent-oid>-<branch-oid>-<merge-base-oid>.json
```

The three object IDs are required because a branch diff is not identified by
the branch commit alone. Branch and parent names are intentionally excluded:
renames may reuse identical content, and object IDs produce portable filenames
without escaping user input.

Each document contains the existing `DiskCachedDiff` payload. Reads deliberately
acquire an exclusive lock for that entry so deserialization and the best-effort
modification-time touch happen atomically, and clone no unrelated patches.
Writes acquire an exclusive entry lock and use the existing temporary-file plus
atomic-rename helper. Concurrent writers may calculate the same deterministic
patch; the last complete atomic write wins.

The old `tui-diff-cache.json` file is not read or migrated. It is disposable
cache data and will be removed best-effort during the first successful cleanup.
The first lookup after upgrading may therefore be cold once.

### Cleanup

Cleanup runs opportunistically after a successful cache write, never during a
read. It scans only `.git/stax/diff-cache/v1`, orders regular JSON entries by
modification time, and removes the oldest entries until both limits hold:

- no more than 128 entries;
- no more than 100 MiB in total serialized size.

Cleanup failures do not fail diff calculation or overwrite the successfully
written entry. Lock files are excluded from count and byte accounting, while
orphan cache-owned entry locks are reaped. Temporary files, directories,
symlinks, and unrelated files are ignored. A cache hit updates the entry's
modification time best-effort so the cleanup policy approximates persistent LRU
behavior.

## Error handling

- A missing entry is a normal cache miss.
- An unreadable or malformed entry is treated as a miss by the high-level diff
  path and removed best-effort so it does not fail repeatedly.
- Persistent-cache read, write, touch, and cleanup failures never replace a
  successfully calculated diff with an error.
- Revision resolution and actual Git diff failures keep their existing error
  behavior because they are not cache failures.

## Testing

Tests will be written before production changes and will cover:

- one entry can be read without parsing a malformed unrelated entry;
- different revision keys create independent files;
- malformed selected entries degrade to misses and can be replaced;
- cleanup enforces entry-count and total-size limits while ignoring unrelated
  files;
- concurrent writes produce a complete readable entry;
- selecting A → B → A restores A synchronously from session memory;
- session memory is invalidated when repository state is refreshed;
- stale asynchronous results still cannot overwrite the current selection.

Targeted cache, application-session, and GPUI state tests will provide the
tight feedback loop. Final validation will use the repository-required
`make test` Docker path.

## Documentation impact

The behavior is internal performance work and adds no command or flag. The GUI
guide and `skills.md` need no workflow changes. The pull request description
will explicitly note that user-facing documentation is unchanged because the
existing cached-diff promise is being made responsive rather than redefined.
