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

## A12. Gates/tests status (v0.1 working state)
- Property-style invariant tests added (`crates/wbash-core/tests/properties.rs`, I1–I7 representative cases) and golden coverage expanded for read/edit/run/check/git/expand/batch/sh/errors (`tests/golden/*`, `crates/wbash-tools/tests/golden.rs`).
- Fuzz targets expanded (args/path/policy/parser matrix/formatter budgets+formats) with seeded corpora (`fuzz/corpus/*`, 50+ cases incl. edge/large/binary inputs). CI runs `fuzz-quick` on push (2k runs/30s per target) and deeper smoke on PR/schedule (10k runs/120s). Long-running continuous fuzz campaigns remain pending.
- Estimator calibration (G9) is implemented and gated: `cl100k_base` MAPE 8.36% on `corpus/scrubbed` (`crates/wbash-core/tests/calibration.rs`, `crates/wbash-core/examples/calibrate.rs`, `corpus/expected/estimator.json`).
- Security/tier/redaction/path-jail checks are exercised in core/tools/adapters tests and a dedicated CI `security` job; broader adversarial matrices remain incremental.
- CI includes `perf-smoke` warm in-process gating (`crates/wbash-tools/tests/warm_perf.rs`, `bench/warm_gate.py`) and advisory CLI latency reporting (`bench/perf.py`).

## A13. Performance gates G6-G8
- Cold start passes in release builds. Warm in-process dispatcher read/search meet G6/G7 targets in tests (e.g. ~<1ms read, ~<30ms search p95 @50k small files with warmed workspace/content cache in the same process).
- Durable cross-process warm cache added: `Workspace` persists a dir-fingerprint-validated metadata index and bounded text sidecar under `<cache>/w-bash/index/*.walk.{json,content}` (ignores `.cache`; invalidated on dir-mtime changes/writes). On warmed CLI runs this brings `search` into G7 range on representative fixtures (e.g. ~25ms p95 @20k small files); very large/uncached cases can still vary and `bench/perf.py` remains advisory.
- CLI per-invocation `read` remains startup-bound (~6-7ms p95, A13/G6 CLI) though warm-transport/in-process reads meet target. Further gains need persistent per-file hot cache integration beyond sidecar and/or daemon/MCP warm transport.
- Scripted benchmark coverage is 30 tasks (`bench/tasks`, `bench/run.py`) with baseline/report artifacts. Agentic E2E runs (turns/retries/model token variance) and a compactor baseline are not yet automated (require model-run environment/credentials).

## A15. Packaging/release scaffolding
- Workspace/crate publish metadata added (repository/homepage/readme/authors/descriptions; library crates marked `publish = false` pending API stabilization). CLI crate carries bin targets `w`/`w-bash`.
- Release automation scaffold added: `.github/workflows/release.yml` (tag-triggered build/package artifacts) and Homebrew formula template `dist/w-bash.rb` (checksums to be filled from release artifacts). Actual `v0.1.0` tag/release and crates.io publish are operational steps not completed in-repo.

## A14. Benchmark harness scope
- Tasks are single-line YAML with a minimal parser (no nested YAML). This keeps
  the harness dependency-free and reviewable; richer task schemas should use a
  real YAML dependency if needed.


## A16. Injected parser output separation and cargo JSONL stripping
- Parsers may inject machine-readable flags (e.g. `cargo --message-format=json`). Parsing consumes the injected/machine stream; user-visible rendered output, `raw_text`, and stored `full` handles are derived from a user-relevant stream with injected machine records stripped when not required to preserve a signal (e.g. cargo JSONL `compiler-artifact`/build records).
- Parse-miss/fallback compaction must not dump injected machine-format noise. Full fidelity remains available via parsing/handles as applicable without leaking JSONL into tails.
- Reason: token regressions from injected JSON in run/check tails while preserving parse correctness.
- Tests: `crates/wbash-tools/tests/{differential.rs,invariants.rs}`, parser/golden coverage.

## A17. Error-first compaction and warning collapse
- When errors/failures/terminal failure summaries are present, compaction prioritizes exit/error/fail signals, failing-test facts, terminal summaries (`could not compile`, aborting, test result), and error-tied code frames/context; warning-only bodies/context are collapsed/counted rather than emitted as blocks. Warning lines are not treated as error anchors.
- Generic tails preserve first/each error signals and terminal summaries with explicit elision/handles (I1/I2).
- Reason: large noisy logs kept warning `-->` blocks and risked displacing real error context.
- Tests: `crates/wbash-tools/src/util.rs` unit tests, differential/invariant parse-miss and no-dropped-marker cases.

## A18. Count-bearing success headers and passthrough/min-gain
- `run`/`check` headers surface parsed counts when known (e.g. `exit 0, N passed, N warnings`, `ok, N warnings, N passed`); semantic success is no errors/failures, but warnings must be reported.
- Lean formatter applies passthrough/min-gain for small/unstructured outputs when structured formatting is not a win, avoiding redundant handles/full markers for fully emitted small bodies while never suppressing diagnostics/errors or required elisions.
- Config: `[output] passthrough_tokens`, `min_gain_pct` (defaults 160/15).
- Tests: formatter/core and tool golden/invariant coverage.

## A19. Env/toolchain fidelity
- Child env allowlist (with wildcard/prefix patterns) preserves semantics-relevant toolchain variables (incl. `CARGO_*`/`CARGO_TARGET_DIR`, Rust/Go/Python/Node/Java/Maven/Gradle/etc. families as configured) subject to secret-key denial.
- Tests: `run_env_allowlist_patterns_apply`, `run_env_honors_cargo_target_dir`.


## A20. Multi-path search
- `search` accepts multiple scope paths (`paths`/multiple positional CLI paths) so a single query can span several roots (e.g. `w search old_name src tests`) without missing matches outside the first path.
- Tests: CLI/task coverage in scripted bench rename flows.


## A21. Output verbosity and minimal success (spec v3.2)
- Lean `verbosity` (`minimal` default, `normal`, `verbose`) controls success detail: minimal emits count-bearing summaries and omits redundant boilerplate/low-signal bodies on clean success; failures/warnings preserve must-keep signals (I1/I2). CLI: `--verbosity`.
- `output.edit_confirm` (`compact` default, `detailed`) controls edit confirmations (`edited <path> (+ins -del)` vs includes hash).
- Config: `[output] verbosity`, `edit_confirm`. Tests: goldens updated for compact edit/minimal success behavior.

## A22. Run/check success compression and handle discipline
- In minimal mode, successful `run/check` avoid dumping full bodies when summary/counts suffice; lossless handles retained when needed for failures/elisions/truncation or verbose mode.
- Goal: reduce verify/success token cost (observed gap vs RTK) without dropping error/fail facts.

## A23. Reference reuse and benchmark fairness (planned/strengthened)
- I8: prefer `unchanged`/handle refs for identical verified in-session scopes (ledger-backed).
- Benchmark claims must use fair command mapping for baselines (incl. RTK) and agentic E2E protocol (pinned model/harness, turns/retries/model tokens) for “beats X” assertions; scripted benchmarks remain required evidence.

## A24. Binary detection boundary fix
- `is_binary` now checks the full buffer: NUL bytes or invalid UTF-8 mark a file binary, while an incomplete trailing multi-byte sequence (`Utf8Error::error_len() == None`) is text. Previously a code point split at the 8192-byte probe boundary misclassified valid text (e.g. `gh_cmd.rs`), and `search` silently returned `0 matches`.
- Zero-result `search` now notes `# skipped N binary file(s)` when binary files were skipped.
- Tests: `crates/wbash-index` unit tests (boundary split, truncated tail, invalid byte, NUL) and search/tool coverage.

## A25. Git habit flags (`-n`, display flags)
- `w git log -n 50` / `-n50` / `--limit 50` map to the tool `limit`; `--oneline`, `--graph`, `--decorate`, `--no-pager` are accepted as display-only. Unknown short flags are rejected with a hint instead of becoming a bogus `ref`.
- Tests: `crates/wbash-cli` `build_git_args` unit tests.

## A26. CLI exit-code propagation for `run`/`sh`
- `run`/`sh` propagate the child exit code when nonzero (clamped to 1..=255); timeout maps to `124`. Other tool/domain failures stay `1`; invocation/policy stays `2`; internal stays `70`. MCP `isError` semantics are unchanged (content still carries the exit fact).
- Tests: `crates/wbash-tools/tests/invariants.rs` (child exit 3).

## A27. Recoverable `unchanged` reads
- `unchanged <path> (b3:...)` appends `expand ~h` when the snapshot handle is still valid; if the handle is gone the content is re-emitted. `read force=true` bypasses the ledger short-circuit and diff view.
- Tests: `crates/wbash-tools/tests/{invariants.rs,golden.rs}`, `tests/golden/read/unchanged.lean`.

## A28. Bounded compiler/linter hints
- Cargo JSON `children` with level `help`/`note` (first two) and ESLint `suggestions[].desc` are appended to the diagnostic message as `; help: ...`, capped at 300 chars total. Generic tails already retain `= help:`/`= note:` within the error window.
- Tests: `crates/wbash-parsers/tests/parsers.rs`.
