# v0.1 benchmark report

Environment: macOS arm64, rustc 1.95, release build, local APFS.
Scripted tasks: 30 (see `bench/tasks` and `bench/README.md`).

## Scripted token/wall baseline

| runner | passed | steps | tokens (est) | wall |
|---|---|---|---|---|
| bash | 30/30 | 52 | 11,545 | 18.7s |
| w | 30/30 | 61 | 4,977 | 20.0s |

Token delta (w vs bash): -56.9%. Biggest wins are `explain_*` (outline vs full
files) and `diff_*` (compact git diff). Fix/rename tasks cost similar or slightly
more because `w` explicitly reads the target before editing.

## Latency (current)

**Warm in-process (same dispatcher/session, warmed cache), gated in CI:** meets G6/G7 targets in `crates/wbash-tools/tests/warm_perf.rs` (e.g. read/search p95 well under 5ms/50ms on 10k–50k small-file fixtures).

**CLI per-invocation (warmed cross-process cache, advisory, `bench/perf.py`):**

| gate | typical p95 | limit | status |
|---|---|---|---|
| cold start | ~5ms | 50ms | pass |
| read (CLI) | ~6-7ms | 5ms | miss (process startup dominates) |
| search (CLI, warmed cache) | ~15-25ms @5k-20k fixtures | 50ms | pass on warmed representative cases (varies by size/FS) |

Durable dir-fingerprint-validated index + bounded text sidecar now accelerates warm `search`; `read` remains startup-bound per CLI invocation. See `docs/AMENDMENTS.md` A13.

## Estimator calibration (G9)

15-file scrubbed corpus (`corpus/scrubbed`) vs pinned `cl100k_base`:
MAPE 8.36%, under the 10% gate. Regenerate with:

```sh
cargo run -p wbash-core --example calibrate -- --write-expected
```

## Agentic E2E / compactor baseline

Not yet run. Requires a model/agent execution environment and credentials to measure turns, retries, and token variance against a compactor baseline.
