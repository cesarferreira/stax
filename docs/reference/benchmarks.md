# Benchmarks

Absolute times vary by repo and machine. Each row below comes from a separate `hyperfine` sample; see the section notes for the exact context.

| Sample | Command | [stax](https://github.com/cesarferreira/stax) | [freephite](https://github.com/bradymadden97/freephite) | [graphite](https://github.com/withgraphite/graphite-cli) |
|---|---|---:|---:|---:|
| `project-x` on `main` | `ls` | 830.5ms | 12.327s | 3.221s |
| Separate sample | `rs` | 2.807s | 6.769s | — |

```text
  ls on project-x/main — mean execution time (lower is better)

  stax       ███                                                830.5 ms
  graphite   █████████████                                      3.221 s
  freephite  ██████████████████████████████████████████████████ 12.327 s
             ┬─────────┬─────────┬─────────┬─────────┬─────────┬
             0        2.5       5.0       7.5      10.0     12.5 s
```

`gt sync` was not included in the `rs` sample, so that row does not include a Graphite comparison.

Summary from the sample runs:

- `st ls` on `project-x`/`main` was ~14.84x faster than `fp ls`
- `st ls` on `project-x`/`main` was ~3.88x faster than `gt ls`
- `st rs` in a separate sample was ~2.41x faster than `fp rs`

## `ls`

Sample context: `project-x` on `main`.

Command:

```bash
hyperfine 'stax ls' 'fp ls' 'gt ls' --warmup 5
```

Raw output:

```text
Benchmark 1: stax ls
  Time (mean ± σ):     830.5 ms ± 119.8 ms    [User: 28.5 ms, System: 39.3 ms]
  Range (min … max):   711.7 ms … 1109.1 ms    10 runs

Benchmark 2: fp ls
  Time (mean ± σ):     12.327 s ±  0.250 s    [User: 0.593 s, System: 0.509 s]
  Range (min … max):   11.967 s … 12.724 s    10 runs

Benchmark 3: gt ls
  Time (mean ± σ):      3.221 s ±  0.121 s    [User: 0.312 s, System: 0.130 s]
  Range (min … max):    3.098 s …  3.430 s    10 runs

Summary
  stax ls ran
    3.88 ± 0.58 times faster than gt ls
   14.84 ± 2.16 times faster than fp ls
```

## `rs`

Sample context: separate benchmark run.

Command:

```bash
hyperfine 'stax rs' 'fp rs'
```

Raw output:

```text
Benchmark 1: stax rs
  Time (mean ± σ):      2.807 s ±  0.129 s    [User: 0.365 s, System: 0.361 s]
  Range (min … max):    2.543 s …  3.006 s    10 runs

Benchmark 2: fp rs
  Time (mean ± σ):      6.769 s ±  0.717 s    [User: 0.673 s, System: 0.981 s]
  Range (min … max):    6.038 s …  7.824 s    10 runs

Summary
  stax rs ran
    2.41 ± 0.28 times faster than fp rs
```
