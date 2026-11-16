# w-bash

**w-bash** (`w`) is an agent-native workspace tool for exploring and working in codebases with compact, deterministic output. Instead of dumping raw command results, it returns focused, budget-aware responses for common tasks like navigating files, reading code, searching, editing, running commands, checking builds/tests, and inspecting git.

It is designed to help AI agents (and CLIs) get the signal they need with fewer tokens: outputs are ranked and can elide details behind expandable handles so full content is still available when needed. The same tool surface is available both as a CLI and as an MCP server.

- **Workspace-aware tools:** tree, read, search, edit, run, check, git, batch, expand
- **Token-efficient output:** structured `lean`/JSON responses that stay within a budget
- **Deterministic & verifiable:** stable formatting, explicit elisions, and hash/ledger-aware reads
- **Safe by design:** path confinement, permission tiers, redaction, and gated execution

See `docs/spec-v3.md` and the `docs/` directory for details.
