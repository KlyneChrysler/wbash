# Spec amendments

Deviations from `docs/spec-v3.md` implemented in v0.1.0. Each entry records the
reason and the affected gate/test.

## A1. Search does not fold multi-line hits
- Spec example renders `path:12,58: snippet` for repeated hits on one line set.
- Implementation emits one record per hit (`path:line: snippet`) and lets the
  budget fitter elide contiguous records.
- Reason: deterministic ordering and simpler exact-line handle payloads.
- Tests: `tests/golden/search/basic.lean`, differential grep parity.

## A2. Git tier is enforced at call time
- `git` advertises `read_only` via `Tool::min_tier` because most operations are
  reads. `commit`/`push` derive `git_write` inside `GitTool::call` and are
  rejected by `policy::check_tier` before touching the repository.
- Tests: `crates/wbash-tools/tests/invariants.rs` (denied commit, allowed after
  tier escalation).

## A3. Ledger persistence
- Read/search/run verification state is persisted per `(session, root)` as JSON
  under `<cache>/w-bash/ledger/<hash16>.json` rather than held only in memory.
- Reason: the CLI is a fresh process per invocation; `unchanged` must survive.
- Tests: repeated `read` calls in `tool_invariants` and CLI smoke tests.

## A4. `unchanged` ignores a body budget
- A verified `unchanged <path> (b3:...)` line is emitted without applying the
  body budget. If the caller needs content they can `expand` the prior handle.
- Tests: `tool_invariants` (second read is `unchanged`).

## A5. Symbol outline fallback
- If tree-sitter cannot parse a language/file, `read --outline` falls back to a
  regex heuristic and adds `# heuristic_outline`.
- Reason: outline must never fail hard on partially invalid source.
- Tests: manual; covered by parse/index unit paths.

## A6. Native parser set
- Nine native parsers ship (`cargo`, `pytest`, `jest`, `vitest`, `tsc`,
  `eslint`, `ruff`, `go-test`, `go-vet`) plus TOML v1 rules. The M2 wording
  ("parsers (10)") is satisfied at ten including the TOML engine.
- `cargo test --message-format=json` still emits human libtest output; the
  parser merges JSON diagnostics with human test lines and panic messages.
- Tests: `crates/wbash-parsers/tests/parsers.rs`.

## A7. TOML `jsonl` mapping scope
- TOML rules support `format = "json" | "jsonl" | "text"` with fixed map keys
  for test results and status words. Nested arrays and arithmetic transforms are
  out of scope; WASM parsers remain deferred.
- Tests: `toml_parser_and_injection`.

## A8. Wire error punctuation
- Errors render `!code message. hint` with the period always inserted before a
  hint, matching the golden error example. Messages already ending in `.` do not
  gain a duplicate separator.
- Tests: `tests/golden/errors/not_found.lean`, ambiguous-anchor assertion.

## A9. Tree rendering details
- `depth=0` means unlimited; directories with more than 40 entries or beyond
  the depth render as `name/ (N)`; more than 200 files in one directory become a
  resolvable elision. Roots include files and dirs counts.
- Tests: `tests/golden/tree/basic.lean`, differential find parity.

## A10. Protocol version
- MCP replies advertise `2024-11-05`. HTTP transport and protocol negotiation
  beyond echoing the supported revision are deferred.
- Tests: `crates/wbash-adapters/tests/mcp.rs`.

## A11. Budget floor
- `--budget` below 64 is rejected (`!invalid_args`) to prevent unusable
  must-keep-only renders; per-tool maxima still clamp upward requests.
- Tests: CLI smoke; schema budget test asserts exposed schemas ≤1500 est tokens.

## A12. Deferred gates
- Fuzz targets exist for args, paths, parsers, and formatter, but CI runs them
  in smoke mode only; no long-running fuzz corpus is committed yet.
- Estimator calibration against a pinned tokenizer (G9) is not wired; the
  deterministic char/line heuristic is used and documented.
- Benchmark harness (30+ tasks) is not yet implemented; perf smoke thresholds
  are not gated in CI.

## A13. Performance gates G6-G8
- Cold start and warm-cache reads pass in release builds; CLI read timing still
  includes process startup (~5-7ms) and misses the 5ms gate. The gate is met for
  a warm MCP process, not per-CLI-invocation.
- `search` has no persistent index: a 50k-file CLI search takes ~4s, far above
  the 50ms warm-index target. Planned fix: serialized symbol/text index under the
  cache directory, invalidated by mtime. Until then `bench/perf.py` reports the
  miss and CI runs it as an advisory artifact.
- The scripted benchmark (30 tasks) is not an agentic turn/token trial; it
  isolates tool output cost for a fixed strategy. Agentic E2E and variance over
  multiple model runs remain future work.

## A14. Benchmark harness scope
- Tasks are single-line YAML with a minimal parser (no nested YAML). This keeps
  the harness dependency-free and reviewable; richer task schemas should use a
  real YAML dependency if needed.
