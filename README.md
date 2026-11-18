# w-bash

Workspace operations tool for AI agents in developer tools and other systems, not intended for direct human use.

Provides file discovery, code search, file read and edit, command and test execution, and Git operations. Returns compact, structured output to reduce token consumption while preserving diagnostics, test failures, source locations, exit codes, and modified lines. Truncated output can be retrieved with `expand ~id`.

## Build

Requires Rust and Git.

```sh
git clone https://github.com/KlyneChrysler/wbash.git
cd wbash && cargo build --release
# target/release/w (CLI), target/release/w-bash (MCP server)
```

## Usage

```sh
w [--root DIR] [--session ID] [--format lean|json] [--budget N] [--tier read_only|write_workspace|execute|git_write] <tool> [args]

w tree src --depth 2
w search "pattern" src
w read src/lib.rs --lines 1:60
w edit src/lib.rs --find 'old' --replace 'new'
w --tier execute run -- cargo test -q
w --tier execute check --kind all
w git status
w --tier git_write git commit -m "message"
w expand ~id
```

Use `--` to pass arguments to `run` or positional values beginning with `-`.

## MCP

```sh
w-bash --root /path/to/project
```

Starts a stdio MCP server for agent integration.

## Configuration

- User: `$XDG_CONFIG_HOME/w-bash/config.toml` or `~/.config/w-bash/config.toml`
- Project: `.w-bash/config.toml` (loaded only when trusted)
- Commands: `w config trust`, `w config show`

## License

MIT
