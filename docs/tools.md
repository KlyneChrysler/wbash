# Tools

All tools accept common `format`/`budget` arguments (CLI: `--format`, `--budget`).
JSON schemas are generated from the same argument structs and exposed over MCP
`tools/list`; unknown fields are rejected.

| Tool | Tier | Purpose |
|---|---|---|
| `tree` | read_only | Compact directory tree with counts |
| `read` | read_only | File/range/symbol read with unchanged/diff verification |
| `search` | read_only | Text or regex search, definition-ranked |
| `expand` | read_only | Expand a handle with grep/lines/head/tail filters |
| `edit` | write_workspace | Literal anchor edit, line splice, create, delete |
| `run` | execute | argv execution with parsers and compact fallback |
| `check` | execute | Detect and run build/lint/type/test commands |
| `git` | read_only (writes gated) | status/diff/log/show/blame/branch/commit/push |
| `batch` | per-call | Serialized multi-call with per-call failures inline |
| `sh` | execute + `allow_shell` | Shell command with generic compaction |

## Arguments

- `tree`: `path="."`, `depth=2` (0 = unlimited), `glob`, `sizes=false`.
  Directories beyond depth or with >40 entries render `name/ (N)`; files beyond
  200 per directory become an elision handle.
- `read`: `path`, optional `lines="A:B"` (1-based, clamped), `symbol`,
  `outline=false`, `expect_hash`. Large files emit an outline plus head and an
  elision handle for the remainder.
- `search`: `pattern`, `mode="text"|"regex"`, `path="."`, `glob`, `context=0`,
  `limit`.
- `expand`: `ref="~id"`, `grep`, `lines`, `head`, `tail`, `offset=0`.
- `edit`: `path` plus exactly one operation: `find`+`replace` (unique literal
  match), `lines`+`content` (splice), `create=true`+`content`, `overwrite=true`
  + `content`, or `delete=true`. `expect_hash` rejects stale writes. Writes are
  atomic; a post-write syntax check is advisory.
- `run`: `argv=[...]` (no shell), `cwd="."`, `timeout_ms`, `stdin`, `env`
  (allowlisted keys only). Parsers consume injected/machine streams when used,
  while rendered tails/`raw_text`/`full` handles use user-relevant output with
  injected machine records stripped. Headers include parsed counts when known
  (e.g. passed/warnings/errors); failures preserve error/test signals and terminal
  summaries with compact tails and a `full: ~id` handle as needed.
- `check`: `kind="all"|"build"|"lint"|"type"|"test"`, `path="."`. Runs detected
  project commands (Cargo, npm scripts, pytest, go, ruff, tsc, etc.) and
  reports count-bearing headers (`ok, N warnings, N passed` / errors/failures).
- `git`: `op` (`status`, `diff`, `log`, `show`, `blame`, `branch`, `commit`,
  `push`) plus op fields: `pathspec`, `staged`, `context`, `limit`, `ref`,
  `path`, `lines`, `message`, `all`, `allow_empty`, `remote`, `branch`,
  `set_upstream`. `commit` requires `git_write`; `push` requires `git_write`.
- `batch`: `calls=[{tool, args}]`. Calls run serially; each result is delimited
  (`#N tool ok|error`) and failures do not abort siblings. Nested `batch` is
  rejected.
- `sh`: `command`, `cwd`, `timeout_ms`, `stdin`. Requires `execute` tier and
  `exec.allow_shell=true`.

## Errors

Codes include `invalid_args`, `not_found`, `outside_workspace`,
`path_is_dir`, `ambiguous_anchor`, `anchor_not_found`, `hash_mismatch`,
`line_range`, `policy_denied`, `handle_not_found`,
`handle_expired`, `timeout`, `spawn_failed`, `git_failed`, `parse_failed`,
`binary_file`, `too_large`,
`file_exists`, `unknown_tool`, `batch_nested`, `not_trusted`.

Each error message includes a concrete next action (for example a suggested
`w read` range or `w edit` line splice).
