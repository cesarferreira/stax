# Freephite and Graphite Compatibility

stax uses freephite-compatible metadata and offers matching command paths for common operations.

| freephite | stax | graphite | stax |
|---|---|---|---|
| `fp ss` | `stax ss` | `gt submit` | `stax submit` |
| `fp rs` | `stax rs` | `gt sync` | `stax sync` |
| `fp bc` | `stax bc` | `gt create` | `stax create` |
| `fp bco` | `stax bco` | `gt checkout` | `stax co` |
| `fp bu` | `stax bu` | `gt up` | `stax u` |
| `fp bd` | `stax bd` | `gt down` | `stax d` |
| `fp ls` | `stax ls` | `gt log` | `stax log` |

Migration is immediate for most repositories: install stax and continue with your existing stack metadata.
