# Freephite and Graphite compatibility

stax uses the same metadata format as freephite (`refs/branch-metadata/<branch>`). Your existing stacks work immediately after install — no migration needed.

## Instant migration

```bash
cargo install stax
# or: brew install cesarferreira/tap/stax

st status   # your existing stack appears immediately
```

## Command mapping

| freephite | graphite | stax |
|---|---|---|
| `fp ss` | `gt submit` | `st stack submit` · `st ss` · `st s s` |
| `fp sr` | — | `st stack restack` · `st sr` · `st s r` |
| `fp bs` | `gt branch submit` | `st branch submit` · `st bs` |
| `fp us submit` | `gt upstack submit` | `st upstack submit` |
| `fp ds submit` | `gt downstack submit` | `st downstack submit` |
| `fp rs` | `gt sync` | `st sync` · `st rs` |
| `fp bc` | `gt create` | `st create` · `st bc` |
| — | `gt create --insert` | `st create --insert` |
| — | — | `st create --below` |
| — | `gt get` | `st get` |
| `fp bco` | `gt checkout` | `st checkout` · `st co` |
| `fp bu` | `gt up` | `st up` · `st bu` |
| `fp bd` | `gt down` | `st down` · `st bd` |
| `fp ls` | `gt log` | `st status` · `st ls` |
| — | `gt modify` | `st modify` · `st m` |
| — | `gt edit` | `st edit` · `st e` |
| — | `gt upstack onto` | `st upstack onto` |
| `fp restack` | `gt restack` | `st restack` · `st sr` |
| — | `gt restack --upstack` | `st upstack restack` |
| — | `gt merge` | `st merge` |
| — | — | `st cascade` |
| — | `gt absorb` | `st absorb` |
| — | — | `st undo` · `st redo` |
| — | `gt split -f <path>` | `st split --file <path>` |

`st get` imports the remote branch as a read-only stack base: Stax can create and restack local branches on top of it, but submit skips pushing or updating the imported branch itself. Sync cleanup may delete the imported local support branch after it is merged or upstream-gone, but it will not push-delete the imported remote branch. If the imported branch already has a PR, stack-link comments still include and sync that PR, with labels relative to the PR being rendered.

Like Graphite's branch/upstack submit flow, `st branch submit` and `st upstack submit` can publish stale local branches without forcing an immediate local restack when the excluded parent is already synced to the remote. Stax does this without a central server by creating internal temporary refs for the push; local branch tips and stack metadata stay unchanged until you run `st restack`.

## Short alias: `st`

stax also installs as `st`:

```bash
st ss   # same as st stack submit (or st s s)
st sr   # same as st stack restack (or st s r)
st rs   # same as st sync
st ls   # same as st status
```

> **Note:** `st s` opens the `stack` subcommand group. Use `st ls` or `st status` for the status view.
