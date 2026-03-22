# Benchmarks

| Command | [stax](https://github.com/cesarferreira/stax) | [freephite](https://github.com/bradymadden97/freephite) | [graphite](https://github.com/withgraphite/graphite-cli) |
|---|---:|---:|---:|
| `ls` (10-branch stack) | 24.2ms | 418.3ms | 217.4ms |

```mermaid
%%{init: {'theme': 'base', 'themeVariables': {'primaryColor': '#4CAF50'}}}%%
xychart-beta
    title "ls command – mean execution time (lower is better)"
    x-axis ["stax", "graphite", "freephite"]
    y-axis "Time (ms)" 0 --> 450
    bar [24.2, 217.4, 418.3]
```

Raw `hyperfine` sample:

```text
Benchmark 1: stax ls
  Time (mean ± σ):      24.2 ms ±   3.6 ms    [User: 9.8 ms, System: 10.9 ms]
  Range (min … max):    20.1 ms …  41.5 ms    106 runs

Benchmark 2: fp ls
  Time (mean ± σ):     418.3 ms ±  34.8 ms    [User: 348.3 ms, System: 188.2 ms]
  Range (min … max):   381.2 ms … 487.0 ms    10 runs

Benchmark 3: gt ls
  Time (mean ± σ):     217.4 ms ±  13.3 ms    [User: 133.4 ms, System: 49.6 ms]
  Range (min … max):   206.6 ms … 247.0 ms    12 runs
```

Summary from the sample run:

- `st ls` was ~8.97x faster than `gt ls`
- `st ls` was ~17.26x faster than `fp ls`
