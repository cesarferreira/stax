# Benchmarks

Absolute times vary by repo and machine. These `hyperfine` samples were captured in this repo.

| Command | [stax](https://github.com/cesarferreira/stax) | [freephite](https://github.com/bradymadden97/freephite) | [graphite](https://github.com/withgraphite/graphite-cli) |
|---|---:|---:|---:|
| `ls` | **45.5 ms** | 739.7 ms | 457.7 ms |
| `rs` | **2.807 s** | 6.769 s | — |

```text
  ls — mean execution time (lower is better)

  stax       ███                                                 45.5 ms
  graphite   ███████████████████████████████                    457.7 ms
  freephite  ██████████████████████████████████████████████████ 739.7 ms
             ┬─────────┬─────────┬─────────┬─────────┬─────────┬
             0        150       300       450       600       750 ms
```

`gt sync` was not captured, so the `rs` row has no Graphite comparison.

**Summary**

- `st ls` was ~**16.25×** faster than `fp ls`
- `st ls` was ~**10.05×** faster than `gt ls`
- `st rs` was ~**2.41×** faster than `fp rs`

## `ls`

```bash
hyperfine 'stax ls' 'fp ls' 'gt ls' --warmup 2
```

```text
Benchmark 1: stax ls
  Time (mean ± σ):      45.5 ms ±   6.9 ms    [User: 10.0 ms, System: 12.0 ms]
  Range (min … max):    40.3 ms …  89.5 ms    59 runs

Benchmark 2: fp ls
  Time (mean ± σ):     739.7 ms ±  23.9 ms    [User: 353.1 ms, System: 208.9 ms]
  Range (min … max):   705.2 ms … 769.8 ms    10 runs

Benchmark 3: gt ls
  Time (mean ± σ):     457.7 ms ±  96.8 ms    [User: 239.3 ms, System: 88.4 ms]
  Range (min … max):   355.0 ms … 647.4 ms    10 runs

Summary
  stax ls ran
   10.05 ± 2.61 times faster than gt ls
   16.25 ± 2.50 times faster than fp ls
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
