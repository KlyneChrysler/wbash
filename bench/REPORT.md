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

## Latency smoke (50k files, cold CLI process)

| gate | median | p95 | limit | status |
|---|---|---|---|---|
| cold start | 4.8ms | 5.5ms | 50ms | pass |
| read (CLI) | 5.9ms | 6.8ms | 5ms | miss (process start included) |
| search | 3,966ms | 4,093ms | 50ms | miss (no persistent index) |

Read measured in a warm MCP/daemon process is sub-millisecond; the gate assumes
a warm transport. Search currently walks and reads each file per call. These are
recorded as A13, not claimed as passing.

## Estimator calibration (G9)

15-file scrubbed corpus (`corpus/scrubbed`) vs pinned `cl100k_base`:
MAPE 8.36%, under the 10% gate. Regenerate with:

```sh
cargo run -p wbash-core --example calibrate -- --write-expected
```
