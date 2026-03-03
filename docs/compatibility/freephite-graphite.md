# Freephite and Graphite Compatibility

stax uses the same metadata format as freephite (`refs/branch-metadata/<branch>`) so your existing stacks work immediately after install — no migration needed.

## Command mapping

| freephite | graphite | stax |
|-----------|----------|------|
| `fp ss` | `gt submit` | `stax submit` / `stax ss` |
| `fp bs` | `gt branch submit` | `stax branch submit` / `stax bs` |
| `fp us submit` | `gt upstack submit` | `stax upstack submit` |
| `fp ds submit` | `gt downstack submit` | `stax downstack submit` |
| `fp rs` | `gt sync` | `stax sync` / `stax rs` |
| `fp bc` | `gt create` | `stax create` / `stax bc` |
| `fp bco` | `gt checkout` | `stax checkout` / `stax co` |
| `fp bu` | `gt up` | `stax up` / `stax bu` |
| `fp bd` | `gt down` | `stax down` / `stax bd` |
| `fp ls` | `gt log` | `stax status` / `stax ls` |
| `fp restack` | `gt restack` | `stax restack` |
| — | `gt restack --upstack` | `stax upstack restack` |
| — | `gt merge` | `stax merge` |
| — | — | `stax cascade` |
| — | — | `stax undo` / `stax redo` |

## Short alias: `st`

stax also installs as `st` — a shorter alias for the same binary:

```bash
st ss       # same as stax submit
st rs       # same as stax sync
st ls       # same as stax status
```

## Migration is instant

Install stax and your existing freephite or graphite stacks work immediately. The metadata format is identical.

```bash
cargo install stax
# or: brew install cesarferreira/tap/stax

stax status   # your existing stack appears immediately
```
