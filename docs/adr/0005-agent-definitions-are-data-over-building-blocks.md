# Agent definitions are data over a closed set of building blocks

An Agent definition is a TOML file of data. It names building blocks that the core implements in Rust, each with parameters: file formats, competition rules, merge modes, Disable mechanisms, gates, Provenance detectors. Supporting an Agent whose behaviour is covered by existing blocks needs no code. An Agent with new behaviour needs a new block, written once in a form any Agent can use. We rejected a data-only format, because describing Codex's layered merge, Cursor's hash-bound approvals or VS Code's SQLite enablement purely in data would turn the format into a programming language with no tooling. We also rejected one Rust module per Agent: that hides each Agent's rules in code, and a reader could no longer compare Agents side by side or check a definition against the Agent's documentation.

Definitions ship inside the app and change only with a release. They are never downloaded, since config-digger makes no network calls, and v0.1 has no user folder for them, because a user-written definition would steer writes to real configuration. A `--definitions <dir>` flag loads extra definitions for development, tests and contributors.

## Consequences

- Precedence is ordered by location, not by Scope. Several Agents rank two locations of the same Scope (VS Code puts `.vscode/mcp.json` above `.mcp.json`, Cursor puts `.cursor` skills above `.claude` skills), so a Scope is only a label the Agent declares, with a display order.
- Definitions never refer to each other. Each declares the files it reads, the core identifies an Entry by its real path and key, and the reach of a write is computed by finding every definition that reads the same file and key.
- Fixing an Agent's behaviour after it updates requires a release. The `verified` version range and `unverified` marks in each definition make that drift visible until then.
- Acceptance test: Zed, which is outside v0.1, must be described using only blocks the six v0.1 Agents already need.
