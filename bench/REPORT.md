# Benchmark report (scripted, v0.2 state)

Environment: macOS arm64, rustc 1.95, release `w`, local APFS.
Scripted tasks: 30 (`bench/tasks/*.yaml`, fixture-based fix/lint/rename flows + in-repo locate/explain/diff). Token metrics shown as chars/4 (and cl100k when `tiktoken` is available).

## Latest run (release, 2026-11)

`python3 bench/run.py --runners bash,w --w-bin target/release/w`

| runner | passed | steps | tokens (est/4) | wall |
|---|---:|---:|---:|---:|
| bash | 30/30 | 52 | 11,638 | 18.5s |
| w | 30/30 | 61 | 5,137 | 19.0s |

- Overall token delta (w vs bash): **-55.9%** (chars/4). With cl100k enabled by the harness, report `tokenizer` and cl100k deltas as primary comparison.
- Correctness: 30/30 for both scripted strategies (same success checks).

### By kind (chars/4)

| kind | bash passed | w passed | bash tok | w tok | bash steps | w steps |
|---|---:|---:|---:|---:|---:|---:|
| explain | 5/5 | 5/5 | 9,480 | 921 | 10 | 5 |
| fix_failing_test | 6/6 | 6/6 | 859 | 2,272 | 12 | 18 |
| fix_lint_or_type | 4/4 | 4/4 | 492 | 1,129 | 8 | 12 |
| locate | 8/8 | 8/8 | 365 | 380 | 8 | 8 |
| rename_symbol | 4/4 | 4/4 | 189 | 298 | 8 | 12 |
| summarize_diff | 3/3 | 3/3 | 253 | 137 | 6 | 6 |

Notes:
- Largest savings: `explain` (outline/compact reads vs full files) and `summarize_diff` (compact git output).
- `fix_*`/`lint`/`rename` use explicit `read`+`edit` steps in the scripted `w` flows (more steps/tokens) but preserve correctness and now emit signal-first, count-bearing failure/success output (no injected cargo JSONL noise, warnings collapsed on error paths).
- Locate is roughly parity in this harness; search/outline wins are larger in broader/agentic navigation scenarios.

## Latency

**Warm in-process (gated):** meets G6/G7 targets in `crates/wbash-tools/tests/warm_perf.rs` / `bench/warm_gate.py` on warmed cache fixtures.

**CLI per-invocation (advisory, `bench/perf.py`, warmed cross-process cache):** cold start ~5ms (pass <50ms); read ~6–7ms p95 (startup-bound, CLI G6 miss <5ms); search ~15–25ms p95 on representative warmed fixtures (pass <50ms, varies by size/FS). See `docs/AMENDMENTS.md` A13.

## Estimator calibration (G9)

15-file scrubbed corpus vs pinned `cl100k_base`: MAPE 8.36% (<10%). Regenerate:
```sh
cargo run -p wbash-core --example calibrate -- --write-expected
```

## Status / gaps

- Scripted token-reduction goal recovered overall (-55.9% est/4); continue validating with real cl100k and agentic end-to-end runs.
- Agentic E2E turns/retries/model-token comparison and compactor baseline: **not yet run** (requires model/agent environment and credentials).
- Release ops pending: tag/artifacts, `dist/w-bash.rb` checksums, crates.io publish decisions.
