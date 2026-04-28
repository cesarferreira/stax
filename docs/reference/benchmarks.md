# Benchmarks

Absolute times vary by repo and machine. These `hyperfine` samples were captured in this repo.

| Command | [stax](https://github.com/cesarferreira/stax) | [freephite](https://github.com/bradymadden97/freephite) | [graphite](https://github.com/withgraphite/graphite-cli) |
|---|---:|---:|---:|
| `ls` | **11.2 ms** | 2.413 s | 783.3 ms |
| `rs` | **2.807 s** | 6.769 s | — |

```text
  ls — mean execution time (lower is better)

  stax       ▏                                                   11.2 ms
  graphite   ████████████████                                    783.3 ms
  freephite  ██████████████████████████████████████████████████  2.413 s
             ┬─────────┬─────────┬─────────┬─────────┬─────────┬
             0        500      1000      1500      2000      2500 ms
```

`gt sync` was not captured, so the `rs` row has no Graphite comparison.

**Summary**

- `st ls` was ~**214.76×** faster than `fp ls`
- `st ls` was ~**69.72×** faster than `gt ls`
- `st rs` was ~**2.41×** faster than `fp rs`

## `ls`

```bash
hyperfine 'stax ls' 'fp ls' 'gt ls' --warmup 5
```

```text
Benchmark 1: stax ls
  Time (mean ± σ):      11.2 ms ±   0.8 ms    [User: 13.8 ms, System: 11.0 ms]
  Range (min … max):     9.7 ms …  13.9 ms    174 runs

Benchmark 2: fp ls
  Time (mean ± σ):      2.413 s ±  0.011 s    [User: 0.406 s, System: 0.250 s]
  Range (min … max):    2.396 s …  2.427 s    10 runs

Benchmark 3: gt ls
  Time (mean ± σ):     783.3 ms ±  38.0 ms    [User: 223.6 ms, System: 71.3 ms]
  Range (min … max):   749.5 ms … 835.8 ms    10 runs

Summary
  stax ls ran
   69.72 ± 6.02 times faster than gt ls
   214.76 ± 15.35 times faster than fp ls
```

## `rs`

```bash
hyperfine 'stax rs' 'fp rs'
```

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
