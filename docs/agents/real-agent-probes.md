# Real-Agent probes: closing a release-gate ticket

Each child ticket of #80 tests some of the definitions' `unverified` marks against the real Agent. This file is what such a session needs; it does not need #80's comments.

## What to read, and what not to

- Your ticket, and the marks it names in `cargo xtask unverified`.
- `docs/research/release-gate.md`: "The probes" (how a probe runs and stays off the user's configuration), "The marks" (what each outcome means) and your Agent's section. Skip the other Agents' sections.
- The research file of your Agent in `docs/research/`, only the parts about your marks.
- `uat/agents/lib.mjs`, and the probe of an Agent close to yours as a model.

## The loop for each mark

1. Build throwaway homes and Projects in a probe, `uat/agents/<agent>.mjs`: `cd-throwaway-*` Entries, the fake Secret `not-a-real-secret`, everything under `CD_PROBE_ROOT` (`E:\cd-throwaway-agents`).
2. Run the real Agent on them and read what it loaded.
3. Assert the fact in the probe. A fact the probe only prints in `observed` is not tested, and review sends it back.
4. Remove the mark. The fact then carries `[test]` in the definition: as the mark assumed (Confirmed), corrected (Corrected), or stated with a new ticket when the format or the core cannot express it yet (Ticket).
5. Record the outcome in your Agent's table in `release-gate.md`, and take the mark out of "What is left".

When every check passes on a version, it can join the definition's `verified` range. Each probe must pass on every version in the range.

## What cost time before

Folders and models:

- Keep test folders outside `C:\Users\<me>`. Claude Code reads `CLAUDE.md` and `.mcp.json` in every folder above the one it starts in, and Codex does the same with its files.
- Never let a request reach a model. `modelServer()` in `lib.mjs` is a local stand-in that also keeps each request. Point Codex and Gemini CLI at a closed local port.

Running the Agents:

- Volta's `codex`, `gemini` and `npm` shims are `.cmd` files, which Node cannot spawn without `cmd.exe`. `run()` in `lib.mjs` handles that. On a timeout, kill the process tree with `taskkill /t /f`, or the CLI keeps its output open.
- Older Claude Code versions: `npm install --prefix E:\cd-throwaway-agents\cc-<version> @anthropic-ai/claude-code@<version>`, then pass `--bin node_modules\@anthropic-ai\claude-code-win32-x64\claude.exe`.
- Gemini CLI prints `extensions list` on stderr. `gemini mcp list` lists an extension's servers even when the extension is off.

Windows paths:

- In the Bash tool, `\\` inside a heredoc becomes `\`, and Windows paths in an inline Python script break too. Write any script or body that holds Windows paths with the Write or Edit tool.

Windows Sandbox:

- `uat/agents/sandbox.ps1` runs the probes that write `%ProgramFiles%`, `%ProgramData%` or `HKLM`, and each such probe calls `sandboxOnly()`. Never write those paths on the reference machine.
- A full run takes about 20 minutes, and only one Sandbox runs at a time. `-Claude` takes several binaries, and one run tests them all.
- In `in-sandbox.ps1`, PowerShell unrolls a `foreach` that yields one array. Yield objects.

Desktop apps (Claude Desktop, Cursor, VS Code):

- Give each one a throwaway profile. VS Code and Cursor take `--user-data-dir` and `--extensions-dir`. Read their logs and `state.vscdb`.
- `--user-data-dir` on an Electron app's Chromium processes shows the data folder it chose.
- VS Code also needs `--shared-data-dir`, `--agent-plugins-dir`, and `USERPROFILE` and `APPDATA` in the launch's folder, or it reads the owner's `~\.vscode\argv.json` and other apps' configs. Stop only the processes whose command line holds the launch's data folder: the owner may have VS Code open.
- VS Code starts an MCP server only when asked. `vscode.mjs`'s probe extension runs `workbench.mcp.startServer *` with `autoTrustChanges`, and each server writes a marker. Extend that probe rather than drive the window.
- Claude Desktop starts no MCP server before someone signs in. It still reads `claude_desktop_config.json` at start, and names an entry it cannot use in the dialog "Some MCP servers couldn't be loaded", which UI Automation reads. It names each entry only once, so give each launch new names.

Accounts:

- The owner signs in on a throwaway home, outside their own home folder: `CLAUDE_CONFIG_DIR='E:\cd-throwaway-agents\claude-account-home' claude auth login --claudeai`. `!` in a Claude Code prompt runs bash, so `$env:…` there signs in the owner's own home.
- A mark whose test needs an account or a deployment the project does not have (a Codex enterprise account, Claude Teams or Enterprise, a Claude Desktop deployment for a third-party provider) stays. Its reason says what the test needs, and `release-gate.md` lists it under the exception.
