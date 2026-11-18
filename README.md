# w-bash

`w-bash` is a command-line tool for AI coding agents. Its main purpose is to run common coding tasks—listing files, searching code, reading and editing files, running commands and tests, and git operations—and return shorter, structured output instead of the full raw output.

When output is shortened, the full result is saved and can be retrieved later with `expand`. Errors, failing tests, file locations, and exit status are kept in the output.

## Features

- **Workspace tools:** `tree`, `read`, `search`, `edit`, `expand`
- **Execution:** `run`, `check`, `sh` (gated), `batch`
- **Git:** `git status|diff|log|show|blame|branch|commit|push` with compact output
- **Safety & policy:** workspace-root path jail, tiered permissions, optional shell gate
- **Compaction:** budget-aware lean output, value-ranked lines, elision handles
- **MCP server:** `w-bash` binary serves tools over stdio for MCP clients

## Requirements

- Rust toolchain (stable, Rust 1.80+ recommended) and Cargo
- Git (for `w git ...` operations)

## Setup

### 1. Clone

```sh
git clone git@github-personal:KlyneChrysler/wbash.git
cd wbash
# or: git clone https://github.com/KlyneChrysler/wbash.git
```

### 2. Build

```sh
cargo build --release
```

Binaries are produced at:

- `./target/release/w` — CLI
- `./target/release/w-bash` — MCP stdio server

### 3. (Optional) Install on PATH

```sh
cargo install --path crates/wbash-cli
# or copy the binaries:
# cp target/release/w target/release/w-bash ~/.local/bin/
```

Verify:

```sh
w --help
```

## Quick start

Run from inside any project (or pass `--root /path/to/project`):

```sh
w tree . --depth 2
w search pattern="fn main" path=.
w read path=src/main.rs --lines 1:80
w git status
```

### Common commands

```sh
# Explore
w tree src --depth 2
w search "parse_config" src
w search "Result<" mode=regex --glob 'src/**/*.rs'

# Read (hash-aware, supports ranges/symbols/outline)
w read src/lib.rs --lines 1:60
w read src/lib.rs --symbol parse_args
w read src/lib.rs --outline

# Edit (exact find/replace or line splice; atomic, preserves mode)
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

# Recover full/elided output
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

Run the MCP stdio server from a workspace:

```sh
w-bash --root /path/to/project
```

Configure your MCP-capable client to launch `w-bash` with the desired `--root`/`--tier` globals.

## Development

```sh
cargo fmt --all --check
cargo test -q
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release
```

`AGENTS.md` in this repo documents the preferred dev workflow (`w`/`wbash` over raw shell for supported ops).

## Commit and push

Typical flow for contributing changes:

```sh
# review
w git status
w --tier execute check --kind all

# stage and commit
git add -A
git commit -m "fix: describe the change"

# push to your remote/branch
git push origin <branch>
# or, via w-bash (gated):
# w --tier git_write git push origin <branch>
```

## License

Apache-2.0
