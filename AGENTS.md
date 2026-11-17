# AGENTS.md

## Dev workflow

- Use the `wbash`/`w` tool in our dev workflow always.
- Prefer `./target/release/w` (or `w` if on PATH) over raw shell for workspace ops: `tree`, `read`, `search`, `edit`, `run`, `check`, `git`, `expand`, `batch`.
- Examples:
  - `w --root . search <pattern>`
  - `w --root . read <path> --lines A:B`
  - `w --tier execute --root . run -- cargo test -q`
  - `w --tier execute --root . check --kind all`
  - `w --root . git status`
- `run`/`check`/`sh` require `--tier execute`; `git commit|push` require `--tier git_write`.
- Use raw shell only when `w` cannot perform the needed action.
