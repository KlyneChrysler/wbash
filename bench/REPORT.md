# Benchmark report

Environment: macOS arm64, rustc 1.95, release `w`, local APFS.
Token metrics: chars/4 and `tiktoken cl100k_base` (durations stripped from output for stability).

## Scripted baseline v0.2.2 (30 tasks)

`python3 bench/run.py --runners bash,w --w-bin target/release/w`

| runner | passed | steps | tok/4 | cl100k | wall |
|---|---:|---:|---:|---:|---:|
| bash | 30/30 | 52 | 11,546 | 11,996 | 22.7s |
| w | 30/30 | 43 | 2,834 | 3,774 | 22.9s |

- Token delta (w vs bash): **-75.5%** (chars/4), **-68.5%** (cl100k).
- Correctness: 30/30 for both strategies.

### By kind (cl100k)

| kind | bash | w | delta | bash steps | w steps |
|---|---:|---:|---:|---:|---:|
| explain | 9,025 | 1,439 | -84.1% | 10 | 5 |
| fix_failing_test | 1,154 | 1,092 | -5.4% | 12 | 12 |
| fix_lint_or_type | 684 | 403 | -41.1% | 8 | 8 |
| locate | 483 | 459 | -5.0% | 8 | 8 |
| rename_symbol | 296 | 176 | -40.5% | 8 | 4 |
| summarize_diff | 354 | 205 | -42.1% | 6 | 6 |

Changes from v0.2.1: `replace` tool with compact per-file counts, scripted rename flows use one `w replace` step, bench token counting normalizes durations, warm perf gates tightened (read <5ms, search <25ms). `fix_failing_test` and `rename_symbol` no longer exceed bash on this harness.

## Agent benchmark (6 multi-step tasks)

`python3 bench/run.py --tasks-dir bench/tasks-agent --runners bash,w --w-bin target/release/w`

| runner | passed | steps | tok/4 | cl100k | wall |
|---|---:|---:|---:|---:|---:|
| bash | 6/6 | 19 | 1,228 | 1,724 | 10.4s |
| w | 6/6 | 18 | 663 | 938 | 8.9s |

- Token delta (w vs bash): **-46.0%** (chars/4), **-45.6%** (cl100k).
- Tasks: fix failing test, fix compile error, rename across files, clippy fix, git edit/summarize, parse bug fix.

## Latency

Warm in-process (gated, release): read p95 ~0.23ms, search p95 ~10.48ms @50k small files; CI gate defaults read <5ms, search <25ms @10k (`bench/warm_gate.py`).

CLI per-invocation remains startup-bound for reads (~6-7ms p95 advisory); warmed cross-process search typically ~15-25ms depending on fixture/FS (`bench/perf.py`).

## Gates and CI

- `bench-scripted` runs with `--baseline bench/baselines/v0.2.1-baseline.json --max-regress-pct 10`.
- Baselines saved: `bench/baselines/v0.2.2-baseline.json`, `bench/baselines/agent-current.json`.
- Warm perf, fmt/clippy, tests, MCP schema budget, estimator calibration, security, and fuzz jobs remain in CI.

## Gaps

- Agent benchmarks are scripted multi-step fixture tasks, not live model E2E turn/retry measurements.
- Cross-process CLI read latency remains startup-bound; further gains require daemon/MCP warm transport or deeper persistent hot cache.
- Release ops (tag/artifacts, formula checksums, crates.io publish) remain operational.
