# Edit fixtures

- `mcp.jsonc` and `config.toml` come from the format-edit spike (`spikes/format-edits/fixtures/` on `research/format-preserving-edits`).
- Real files, kept byte for byte so that round-trips are tested on files nobody wrote for these tests:
  - `vscode-settings.json`: [microsoft/vscode `.vscode/settings.json`](https://github.com/microsoft/vscode/blob/main/.vscode/settings.json), MIT.
  - `codex-defaults.toml`: [openai/codex `codex-rs/config/defaults.toml`](https://github.com/openai/codex/blob/main/codex-rs/config/defaults.toml), Apache-2.0.
  - `codex-feiskyer.toml`: [feiskyer/codex-settings `config.toml`](https://github.com/feiskyer/codex-settings/blob/main/config.toml), MIT. Its `sk-dummy` bearer token is a placeholder, not a credential.
  - `codex-pulumi-awsx.toml`: [pulumi/pulumi-awsx `.codex/config.toml`](https://github.com/pulumi/pulumi-awsx/blob/master/.codex/config.toml), Apache-2.0.

Git checks these files out with LF (see `.gitattributes`). Tests build CRLF, BOM and other byte-level variants in code.
