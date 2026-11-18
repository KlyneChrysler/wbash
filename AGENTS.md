# AGENTS.md

## Dev workflow

- Always use our own `wbash` (`w`) in development. Do not use native shell commands directly.
- Perform workspace, execution, and git operations through `w` (`tree`, `read`, `search`, `edit`, `expand`, `run`, `check`, `git`, `batch`).
- Examples:
  - `w --root . search <pattern>`
  - `w --root . read <path> --lines A:B`
  - `w --tier execute --root . run -- cargo test -q`
  - `w --tier execute --root . check --kind all`
  - `w --root . git status`
  - `w --tier git_write --root . git commit -m "message"`
- `run`/`check`/`sh` require `--tier execute`; `git commit|push` require `--tier git_write`.
