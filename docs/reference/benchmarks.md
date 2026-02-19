# Benchmarks

| Command | [stax](https://github.com/cesarferreira/stax) | [freephite](https://github.com/bradymadden97/freephite) | [graphite](https://github.com/withgraphite/graphite-cli) |
|---|---:|---:|---:|
| `ls` (10-branch stack) | 22.8ms | 369.5ms | 209.1ms |

Raw `hyperfine` sample:

```text
Benchmark 1: stax ls
  Time (mean ± σ):      22.8 ms ±   1.0 ms

Benchmark 2: fp ls
  Time (mean ± σ):     369.5 ms ±   7.0 ms

Benchmark 3: gt ls
  Time (mean ± σ):     209.1 ms ±   2.8 ms
```

Summary from the sample run:

- `stax ls` was ~9.18x faster than `gt ls`
- `stax ls` was ~16.23x faster than `fp ls`
