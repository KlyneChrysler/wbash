# w-bash

Tool for AI agents (in developer tools or inside other systems), not for humans.

Runs workspace tasks (list/search/read/edit files, run commands/tests, git) and returns short structured output to use fewer tokens. Keeps errors, failing tests, file locations, exit status, and changed lines. Full output can be retrieved with `expand ~id` when shortened.

## Install

Requires Rust and Git.

```sh
git clone https://github.com/KlyneChrysler/wbash.git
cd wbash && cargo build --release
# binaries: target/release/w (CLI), target/release/w-bash (MCP server)
```

## Usage

```sh
w [--root DIR] [--session ID] [--format lean|json] [--budget N] [--tier read_only|write_workspace|execute|git_write] <tool>

w tree src --depth 2
w search "pattern" src
w read src/lib.rs --lines 1:60
w edit src/lib.rs --find 'old' --replace 'new'
w --tier execute run -- cargo test -q
w --tier execute check --kind all
w git status
w --tier git_write git commit -m "msg"
w expand ~1
```

Use `--` before argv for `run` or values starting with `-`.

## MCP

```sh
w-bash --root /path/to/project
```

Stdio MCP server for agent clients.

## Config

- User: `$XDG_CONFIG_HOME/w-bash/config.toml` or `~/.config/w-bash/config.toml`
- Project: `.w-bash/config.toml` (trusted only). `w config trust`, `w config show`

## License

MIT
