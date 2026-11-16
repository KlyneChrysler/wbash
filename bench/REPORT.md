# Benchmark report (scripted, v0.2 state)

Environment: macOS arm64, rustc 1.95, release `w`, local APFS.
Scripted tasks: 30 (`bench/tasks/*.yaml`, fixture-based fix/lint/rename flows + in-repo locate/explain/diff). Token metrics shown as chars/4 (and cl100k when `tiktoken` is available).

## Latest run (release, 2026-11, scripted optimized `w` flows, `tiktoken cl100k_base`)

`python3 bench/run.py --runners bash,w --w-bin target/release/w`

| runner | passed | steps | tok/4 | cl100k | wall |
|---|---:|---:|---:|---:|---:|
| bash | 30/30 | 52 | 11,669 | 12,093 | 20.8s |
| w | 30/30 | 51 | 3,213 | 4,423 | 21.8s |

- Token delta (w vs bash): **-72.5%** (chars/4), **-63.4%** (cl100k). Correctness: 30/30 for both scripted strategies.

### By kind

| kind | bash cl100k | w cl100k | bash tok/4 | w tok/4 | bash steps | w steps |
|---|---:|---:|---:|---:|---:|---:|
| explain | 9,132 | 1,224 | 9,511 | 921 | 10 | 5 |
| fix_failing_test | 1,169 | 1,630 | 859 | 1,179 | 12 | 12 |
| fix_lint_or_type | 684 | 367 | 492 | 250 | 8 | 8 |
| locate | 458 | 483 | 365 | 380 | 8 | 8 |
| rename_symbol | 296 | 523 | 189 | 346 | 8 | 12 |
| summarize_diff | 354 | 196 | 253 | 137 | 6 | 6 |

Changes vs prior scripted run: removed redundant `read` steps from fix/lint `w` flows (leveraging compact failure/test/lint signals + targeted `edit`), and enabled multi-path `search` so rename flows inspect all relevant locations in one step.

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

- Scripted token-reduction goal met/exceeded on current harness: **-63.4% cl100k overall** with 30/30 correctness. `explain`/`diff`/`lint` dominate savings; `fix_failing_test` improved substantially (1,630 vs 1,169 cl100k); `rename` remains above bash (523 vs 296 cl100k) from explicit search+edits. Continue with agentic E2E validation and further per-workflow/output optimization where overhead remains.
- Agentic E2E turns/retries/model-token comparison and compactor baseline: **not yet run** (requires model/agent environment and credentials).
- Release ops pending: tag/artifacts, `dist/w-bash.rb` checksums, crates.io publish decisions.
