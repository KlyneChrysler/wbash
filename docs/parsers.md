# Output parsers

`run` and `check` map command output to normalized `Diagnostic` and
`TestResult` records. Parsing is selected by argv and applied to stdout; stderr
is scanned as a fallback. The raw output always remains available in the `full`
handle.

## Native parsers

| Parser | Matches | Extracts |
|---|---|---|
| `cargo` | `cargo test`/`check`/`build`/`clippy` | JSON `compiler-message` diagnostics (`E####` codes, spans); JSON libtest events; human `test ... ok/FAILED` and panic messages |
| `pytest` | `pytest`/`py.test` | `FAILED id - message`, pass/fail summary |
| `jest` | `jest` | `testResults[].assertionResults[]` JSON (name, status, first failure line, duration) |
| `vitest` | `vitest` | same JSON shape |
| `tsc` | `tsc` | `file(line,col): error TS####: message` |
| `eslint` | `eslint` | JSON formatter messages (file, line/col, rule id, severity) |
| `ruff` | `ruff check` | JSON diagnostics (filename, row/col, code, message) |
| `go-test` | `go test` | `--- PASS/FAIL: Test...` and package summaries |
| `go-vet` | `go vet` | `file.go:line:col: message` |

If no parser matches, `run` uses generic compaction (§7.5): ANSI stripping,
whitespace normalization, error-line preservation, head+tail windows, and a
`# parse_fallback` advisory. Exit code, duration, and must-keep errors are still
emitted.

## TOML parsers (v1)

User rules: `~/.config/w-bash/parsers.d/*.toml`.
Project rules: `.w-bash/parsers.d/*.toml` (trusted projects only).
Lower `priority` wins among matches.

```toml
schema_version = 1
name = "demo"
priority = 100

[match]
argv0 = "demo"
argv_contains = ["run"]

[inject]
args = ["--json"]
inject_if_absent = "--json"

[parse]
format = "jsonl"            # json | jsonl | text

[map.test_result]
id = "$.name"
status = "$.state"
message = "$.msg"

[map.status]
ok = "pass"
bad = "fail"
```

- `match.argv0` is required; `argv_contains` is optional.
- Injection appends missing arguments once; they are never duplicated.
- `format="text"` with a `line_regex` captures named groups into the same map
  keys.
- Invalid rule files are ignored with a startup warning; rules never override
  built-in parsers for the same argv0 unless priority is lower.
- Declarative parsers cannot execute code. WASM parsers are deferred to v2.
