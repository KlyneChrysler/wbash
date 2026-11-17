# Wire format `lean@0.2`

Derived from `docs/spec-v3.md` §6. Normative summary of the implemented format.

## Grammar

```text
response := header LF body?
header   := summary
body     := (line LF)* line?
line     := record | elision | advisory | error | detail
elision  := "...+" N SP unit SP "(" handle ")"
advisory := "# " text
error    := "!" code SP message ["." SP hint]
handle   := "~" [a-z0-9]{1,8}
```

UTF-8, `\n` only, final newline required, no ANSI/CSI, no trailing whitespace,
no blank separator lines. Output is deterministic for fixed inputs and state.

## Rules

- Header is always line 1 and carries tool-specific counts/status/exit/duration.
- Omitted content is represented explicitly: `...+N lines (~id)`,
  `...+N more (~id)`, or `...+N bytes (~id)`. Every elision resolves through
  `expand` to the exact omitted payload until TTL/cap eviction.
- Error, failure, exit, timeout, redaction, and security records are must-keep:
  the budget fitter emits them even when it exceeds the budget, then appends
  `# budget_exceeded_must_keep`. On error/failure paths, compaction prioritizes
  error signals, failing-test facts, error-tied context/frames, and terminal
  summaries; warning-only bodies are collapsed/counted rather than emitted.
- Paths are workspace-relative with `/`. Indentation is semantic only (tree
  depth, outline nesting).
- Durations: `Nms` below 10s, otherwise `N.Ns`.
- Errors use `!code message. hint` and are followed by a newline.
- `unchanged` and diff bases are emitted only after hash verification of the
  current scope.
- Lean `verbosity=minimal` (default): on clean success, omit redundant boilerplate
  and keep count-bearing summaries; failures/warnings preserve must-keep signals.
  `edit` confirmations default to `edited <path> (+ins -del)`.

## Tokens and budgets

Runtime estimation is deterministic: `max(ceil(chars/4), lines/2)`. Default
budgets are 600 tokens for most tools, 800 for `run`/`check`/`sh`, and 1600 for
`batch`. An explicit budget overrides the default and must be in `64..=32768`.

## JSON mode

Selected with `--format json` (or `"format":"json"` in tool args).

```json
{
  "ok": true,
  "tool": "search",
  "meta": {
    "duration_ms": 12,
    "counts": {"files": 3, "matches": 7},
    "redactions": 0,
    "notes": []
  },
  "data": {},
  "diagnostics": [],
  "handles": [{"ref": "~s1", "kind": "search", "bytes": 4096, "hash": "b3:..."}],
  "errors": []
}
```

Top-level key order is stable. `meta.ok` is omitted (top-level `ok` is
authoritative). Errors carry `{code, message, hint?}` and `ok=false`; the CLI
exits nonzero for tool errors and for failed `check` exit statuses; for `run`/`sh`
the CLI propagates the child exit code (timeout `124`).

## Examples

```text
# tree
./ (4 files, 2 dirs)
src/
  config.rs
  main.rs
tests/
  basic.rs
Cargo.toml

# read (first view)
src/config.rs (5 lines, b3:8a581f63fd7b8007)
1: //! Config.
2: pub fn parse_config(path: &str) -> Result<String, String> {

# read (verified unchanged)
unchanged src/config.rs (b3:8a581f63fd7b8007) expand ~1

# read (external change)
src/config.rs diff (b3:8a581f63fd7b8007)
@@ -3,3 +3,4 @@
 3:     if path.is_empty() { return Err("empty".into()); }
 4:     Ok(path.to_string())
 5: }
+6: // added

# search
2 files, 2 matches
src/config.rs:2: pub fn parse_config(path: &str) -> Result<String, String> {
src/main.rs:4: let _ = config::parse_config("x");

# run failure
exit 101, 43ms, 1 passed, 1 failed
test result: FAILED. 1 passed; 1 failed
FAIL fails
full: ~r7

# run/check success (counts when known)
exit 0, 12 passed, 2 warnings

# error
!ambiguous_anchor 2 matches in dup.txt at 1:1, 2:1. try: w edit path=dup.txt lines=1:1 replace="..."
```
