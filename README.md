# w-bash

`w-bash` is a tool for AI agents, not for humans. It is built to be called by agents in developer tools and by agents inside other systems.

Its purpose is to let agents do workspace tasks—list files, search code, read and edit files, run commands and tests, and use git—without using a lot of tokens. Instead of returning full raw command output, it returns shorter structured results that keep the important information: errors, failing tests, file locations, exit status, and changed lines. When output is shortened, the full result can be retrieved later with `expand`.

## What it provides

- Workspace operations for agents: `tree`, `read`, `search`, `edit`, `expand`
- Command execution for agents: `run`, `check`, `sh` (gated), `batch`
- Git operations: `git status|diff|log|show|blame|branch|commit|push`
- Output limits and saved full results (`expand ~id`) so agents use less context on large output
- Path checks, permission tiers, and optional shell restrictions
- CLI (`w`) and MCP stdio server (`w-bash`) for agent integration

## Install

Requires Rust (stable, 1.80+ recommended) and Git.

```sh
git clone https://github.com/KlyneChrysler/wbash.git
cd wbash
cargo build --release
```

Binaries:
- `./target/release/w` — CLI for agent tool calls
- `./target/release/w-bash` — MCP stdio server for MCP-capable agents

To make the binaries available to an agent runtime:

```sh
cargo install --path crates/wbash-cli
```

## Example agent commands

Run from the project root (or pass `--root /path/to/project`).

```sh
w tree . --depth 2
w search pattern="fn main" path=.
w read path=src/main.rs --lines 1:80
w git status
```

```sh
# Explore
w tree src --depth 2
w search "parse_config" src
w search "Result<" mode=regex --glob 'src/**/*.rs'

# Read (ranges/symbols/outline, hash-aware)
w read src/lib.rs --lines 1:60
w read src/lib.rs --symbol parse_args
w read src/lib.rs --outline

# Edit (exact find/replace or line splice; atomic)
w edit src/lib.rs --find 'old' --replace 'new'
w edit src/lib.rs --lines 10:12 --replace 'new line 10\nnew line 11'
w edit src/new_file.rs --create --content 'hello\n'

# Run & check (requires --tier execute)
w --tier execute run -- cargo test -q
w --tier execute check --kind all

# Git
w git diff --staged
w git log -n 10
w --tier git_write git commit -m "fix: message"
w --tier git_write git push origin main

# Retrieve full output when shortened
w expand ~1
w expand ~1 --grep error
```

### Global options

`--root`, `--session`, `--format lean|json`, `--budget N`, `--tier read_only|write_workspace|execute|git_write`, `--verbosity minimal|normal|verbose`, `--config`, `--no-project-config`

Use `--` to pass argv to `run` or to pass positional values that begin with `-`:

```sh
w --tier execute run -- cargo test -q -- --nocapture
w read -- -filename-starting-with-dash.txt
```

## Configuration

- User config: `$XDG_CONFIG_HOME/w-bash/config.toml` (or `~/.config/w-bash/config.toml`)
- Project config: `.w-bash/config.toml` in the workspace root (loaded only if trusted)
- Override: `--config /path/to/config.toml` or `WBASH_CONFIG=...`

Trust a project config:

```sh
w config trust
w config show
```

## MCP server

Run the MCP stdio server so an MCP-capable agent can call these tools:

```sh
w-bash --root /path/to/project
```

Configure the agent's MCP client to launch `w-bash` with the desired `--root`/`--tier`.

## Development

```sh
cargo fmt --all --check
cargo test -q
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release
```

## License

MIT
