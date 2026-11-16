# w-bash

`w-bash` is a local, agent-native workspace tool server and CLI (`w`) that returns compact, deterministic, lossless-by-reference results for file, search, edit, execution, and git operations.

## Build

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --release
# binaries: target/release/w and target/release/w-bash
```

## CLI usage

```sh
w --root . tree
w --root . read src/main.rs --lines 1:80
w --root . search parse_config
w --tier execute --root . run -- cargo test -q
w --tier execute --root . check --kind all
w --root . git status
w --root . expand ~r1 --grep error
```

- Global opts: `--root`, `--session`, `--format lean|json`, `--budget`, `--tier`, `--config`, `--no-project-config`
- Tools: `tree`, `read`, `search`, `edit`, `run`, `check`, `git`, `expand`, `batch`, `sh`
- `run`/`check`/`sh` require `--tier execute`; `sh` also requires `exec.allow_shell=true` in config
- `git commit`/`push` require `--tier git_write`

## Output model

- Lean wire format by default: compact header + records, explicit elisions (`...+N ... (~id)`), and `expand <~id>` for lossless detail
- Error/fail/exit/timeout signals are preserved; on error paths compaction is signal-first (error context/frames + terminal summaries, warning bodies collapsed/counted)
- `run`/`check` headers include parsed counts when known (e.g. `exit 0, 12 passed, 2 warnings`, `ok, 3 warnings`)
- Parsers may consume injected machine streams (e.g. cargo JSON) but user-visible tails/handles strip injected machine records

## MCP server

```sh
w-bash --root .
```

Exposes the same tool set over MCP stdio (`tools/list`, `tools/call`).

## Config and state

- User config: `$XDG_CONFIG_HOME/w-bash/config.toml` or `~/.config/w-bash/config.toml`
- Cache/handles/index/ledger/stats: `$XDG_CACHE_HOME/w-bash/` or `~/.cache/w-bash/`
- `[output] passthrough_tokens` / `min_gain_pct` control small-output passthrough behavior
- Env allowlist supports patterns (e.g. `CARGO_*`) with secret-key denial enforced

## Development

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -q
python3 bench/run.py --runners bash,w --w-bin target/release/w
```

See `docs/spec-v3.md`, `docs/wire-format-v0.2.md`, `docs/tools.md`, `docs/parsers.md`, `docs/AMENDMENTS.md`, and `bench/REPORT.md`.
