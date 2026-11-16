# w-bash

Agent-native workspace tools with token-efficient output. `w` exposes navigation,
reading, search, editing, execution, lint checks, git, and handle expansion as
deterministic, budgeted JSON or `lean@0.2` records. The same surface is served
over MCP stdio as `w-bash`.

Status: v0.1.0 development (spec: `docs/spec-v3.md`). Linux/macOS.

## Build and install

```sh
cargo build --release
install -m 755 target/release/w /usr/local/bin/w
# optional MCP command name
install -m 755 target/release/w-bash /usr/local/bin/w-bash
```

## CLI

```sh
w tree src --depth 2 --sizes
w read src/main.rs --lines 1:60
w search parse_config --mode text src
w edit src/main.rs find=old replace=new
w --tier execute run -- cargo test
w --tier execute check --kind all
w git status
w expand '~r7' --grep error
w gain
w config show
```

Global flags: `--root DIR`, `--session ID`, `--format lean|json`, `--budget N`,
`--tier read_only|write_workspace|git_write|execute`. Default tier is
`write_workspace`. Destructive tools require an explicit higher tier.

## MCP

```json
{
  "mcpServers": {
    "w-bash": {"command": "w-bash", "args": ["--root", "/path/to/workspace"]}
  }
}
```

Sessions are selected with `_meta.session_id` on `tools/call`; omission uses a
default session. Exposed tool schemas stay within the 1500-token budget (tested).

## Security defaults

- Path jail: all filesystem arguments resolve under the workspace root (plus
  configured allowlist); symlink escapes are rejected.
- Tiers gate writes, git commits/pushes, and process execution.
- Project config and `.w-bash/` rules require explicit trust
  (`w config trust`).
- Environment for executed commands is allowlisted; secrets are redacted before
  storage and before emission.
- `sh` is disabled unless `exec.allow_shell=true`.
- No network, no telemetry.

## Development

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Benchmarks and gates:

```sh
python3 bench/run.py --runners bash,w      # 30 scripted tasks
python3 bench/perf.py --strict             # G6-G8 latency smoke
cargo test -p wbash-core --test calibration # G9 estimator MAPE
```

Layout: `crates/wbash-{core,index,parsers,tools,adapters,cli}`, tests in
`crates/*/tests`, goldens in `tests/golden`, fixtures in `tests/fixtures`,
fuzz targets in `fuzz/`. See `docs/` for wire format, tools, parsers, security,
and recorded spec amendments.
