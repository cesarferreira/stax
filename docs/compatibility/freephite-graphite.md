# Freephite and Graphite Compatibility

stax uses the same metadata format as freephite (`refs/branch-metadata/<branch>`) so your existing stacks work immediately after install — no migration needed.

## Command mapping

| freephite | graphite | stax |
|-----------|----------|------|
| `fp ss` | `gt submit` | `st stack submit` / `st ss` / `st s s` |
| `fp sr` | — | `st stack restack` / `st sr` / `st s r` |
| `fp bs` | `gt branch submit` | `st branch submit` / `st bs` |
| `fp us submit` | `gt upstack submit` | `st upstack submit` |
| `fp ds submit` | `gt downstack submit` | `st downstack submit` |
| `fp rs` | `gt sync` | `st sync` / `st rs` |
| `fp bc` | `gt create` | `st create` / `st bc` |
| `fp bco` | `gt checkout` | `st checkout` / `st co` |
| `fp bu` | `gt up` | `st up` / `st bu` |
| `fp bd` | `gt down` | `st down` / `st bd` |
| `fp ls` | `gt log` | `st status` / `st ls` |
| — | `gt modify` | `st modify` / `st m` |
| `fp restack` | `gt restack` | `st restack` / `st sr` |
| — | `gt restack --upstack` | `st upstack restack` |
| — | `gt merge` | `st merge` |
| — | — | `st cascade` |
| — | `gt absorb` | `st absorb` |
| — | — | `st undo` / `st redo` |

## Short alias: `st`

stax also installs as `st` — a shorter alias for the same binary:

```bash
st ss       # same as st stack submit (or st s s)
st sr       # same as st stack restack (or st s r)
st rs       # same as st sync
st ls       # same as st status
```

> **Note:** `st s` opens the `stack` subcommand group. Use `st ls` or `st status` for the status view.

## Migration is instant

Install stax and your existing freephite or graphite stacks work immediately. The metadata format is identical.

```bash
cargo install stax
# or: brew install cesarferreira/tap/stax

st status   # your existing stack appears immediately
```
