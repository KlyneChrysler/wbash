# Benchmark report (scripted, v0.2.1 state)

Environment: macOS arm64, rustc 1.95, release `w`, local APFS.
Scripted tasks: 30 (`bench/tasks/*.yaml`, fixture-based fix/lint/rename flows + in-repo locate/explain/diff). Token metrics shown as chars/4 (and cl100k when `tiktoken` is available).

## Baseline v0.2.1 (release, 2026-11-17, scripted optimized `w` flows, `tiktoken cl100k_base`)

`python3 bench/run.py --runners bash,w --w-bin target/release/w`

| runner | passed | steps | tok/4 | cl100k | wall |
|---|---:|---:|---:|---:|---:|
| bash | 30/30 | 52 | 11,616 | 12,063 | 23.0s |
| w | 30/30 | 51 | 3,264 | 4,366 | 25.4s |

- Token delta (w vs bash): **-71.9%** (chars/4), **-63.8%** (cl100k). Correctness: 30/30 for both scripted strategies.

### By kind

| kind | bash cl100k | w cl100k | bash tok/4 | w tok/4 | bash steps | w steps |
|---|---:|---:|---:|---:|---:|---:|
| explain | 9,102 | 1,341 | 9,458 | 1,015 | 10 | 5 |
| fix_failing_test | 1,169 | 1,547 | 859 | 1,144 | 12 | 12 |
| fix_lint_or_type | 684 | 383 | 492 | 291 | 8 | 8 |
| locate | 458 | 483 | 365 | 376 | 8 | 8 |
| rename_symbol | 296 | 416 | 189 | 302 | 8 | 12 |
| summarize_diff | 354 | 196 | 253 | 136 | 6 | 6 |

Changes vs v0.2.0: minimal-success `run`/`check` is now header-only (no passthrough of clean cargo/libtest bodies), compiler/linter help hints are retained, singular/plural count headers are grammatical, `read force=true` and recoverable `unchanged ... expand ~h` are available, and `run`/`sh` propagate child exit codes. Binary detection now treats only NUL/invalid UTF-8 as binary (truncated multi-byte tails are text), and zero-result searches note skipped binary files. Git `log -n <count>` is accepted.

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

- Scripted token-reduction goal met/exceeded on current harness: **-63.8% cl100k overall** with 30/30 correctness. `explain`/`diff`/`lint` dominate savings; `fix_failing_test` remains the hardest workflow (1,547 vs 1,169 cl100k); `rename` remains above bash (523 vs 296 cl100k) from explicit search+edits. Continue with agentic E2E validation and further per-workflow/output optimization where overhead remains.
- Agentic E2E turns/retries/model-token comparison and compactor baseline: **not yet run** (requires model/agent environment and credentials).
- Release ops pending: tag/artifacts, `dist/w-bash.rb` checksums, crates.io publish decisions.
