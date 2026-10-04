# Prior art: mTarsier, everymcp and MCP Manager

Research for [#17](https://github.com/samfaina/config-digger/issues/17). Read on 2026-10-01 from shallow clones at the commits pinned below. Every claim cites a file in those commits. Nothing was built or run. Behaviour described as a bug comes from reading the code and was not reproduced.

| Project | Repository | Commit read |
| --- | --- | --- |
| mTarsier | [mcp360/mTarsier](https://github.com/mcp360/mTarsier) | `8f14301` (2026-04-20) |
| everymcp | [am-will/everymcp](https://github.com/am-will/everymcp) | `c6b9e23` (2026-02-14) |
| MCP Manager | [xjeway/mcp-manager](https://github.com/xjeway/mcp-manager) | `15e1186` (2026-09-30) |

All three were found. No close alternatives were needed.

## Summary

- None of the three answers config-digger's main question. They are installers and sync tools. Each one treats "the servers of client X" as one flat list. None of them works out which Entry is Effective and which are Shadowed when the same name appears in several Scopes of one Agent. MCP Manager comes closest: it lists the Scopes per client in a [capability matrix](https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/docs/mcp-client-capability-matrix.md), then marks precedence as an open verification item.
- None of them reads Provenance. Claude Code plugins, Gemini CLI extensions, MCP servers contributed by VS Code extensions, Claude Desktop `.mcpb` extensions and managed configuration are all invisible to them.
- Agent modelling. mTarsier is the only one that keeps Agent facts as data: a flat record per client with per-OS paths, root key and format. Two copies of that table exist, one in TypeScript and one in Rust, and they have already drifted apart. everymcp and MCP Manager use one code adapter per Agent.
- Edits. Only everymcp preserves comments and formatting. It uses `jsonc-parser`'s `modify`, but only for JSON. For TOML it shells out to `codex mcp add`. mTarsier and MCP Manager parse into `serde_json::Value` or `toml::Value` and write the whole file back, which drops comments and, per their declared Cargo features, reorders keys. MCP Manager has a worse problem. Its apply path parses the target file as strict JSON and falls back to an empty object when that fails, so a JSONC file with a comment would be replaced by one containing only the managed field.
- Disable means the same thing in mTarsier and MCP Manager: delete the key from the Agent's file and keep a copy in the tool's own store. Neither uses an Agent's native disable flag.
- Several Windows path facts contradict what we would assume. On this machine Claude Desktop (MSIX) keeps its config under `%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Claude\`, and all three tools look only in `%APPDATA%\Claude\`. Claude Code's `projects` map in `~/.claude.json` holds the same folder under several spellings. The three tools disagree on VS Code, OpenCode and Antigravity paths. Details are in [Agent path facts](#agent-path-facts).
- Reuse. All three are MIT, which is compatible with MIT OR Apache-2.0 as long as we keep the notice. The best candidates are MCP Manager's write-safety design (atomic write, per-batch snapshot, fingerprinted undo history, lock file, "overwrite unmanaged" preview action) and the agent fact tables from all three as cross-checks for our own Agent definitions. I would not copy their write code. Each one fails our "touch only the affected Entry, keep comments and order" rule.

## mTarsier

**License.** [`LICENSE`][mt-license] is MIT, "Copyright (c) 2026 mTraiser". The metadata disagrees with it. [`package.json`][mt-pkg] says `"license": "ISC"` and [`src-tauri/Cargo.toml`][mt-cargo] has `license = ""`. Both MIT and ISC are permissive, so this does not change what we may do, but attribution should cite the `LICENSE` file.

**Stack and activity.** Tauri 2 desktop app with a React 19, Zustand and Monaco front end, a Rust back end, and a second Rust binary `tsr` used as a CLI ([`Cargo.toml`][mt-cargo]). It has 97 commits and v1.2.0 was released 2026-04-20. The last push was the same day and 9 issues are open. It is maintained by the MCP360 team.

**How it models Agents.** This is mostly data. Each client is a `ClientMeta` record in [`src/lib/clients.ts`][mt-clients-ts] with `configPath`, `configPathWin`, `configPathLinux`, `configKey` (a dotted path), `configFormat` (`json`, `toml`, `json-opencode`), a `detection` rule (`app_bundle` with `|`-separated candidate paths, `cli_binary`, `vscode_extension`), `skillsPath` and `npxAgentId` ([`src/types/client.ts`][mt-client-types]). The Rust side receives these fields per request ([`commands/clients.rs`][mt-clients-rs] `DetectionRequest`). The CLI cannot read the TypeScript, so it carries its own copy in [`src-tauri/src/registry.rs`][mt-registry]. The two copies differ. One example: Claude Desktop's Linux path is `~/.config/Claude/...` in TypeScript and "not supported" in Rust. Format quirks still need code. OpenCode's `json-opencode` has its own read and write branches ([`commands/config.rs` L114-170][mt-config-opencode]).

**Agents and Scopes covered.** There are 15 clients, including ChatGPT, Claude web and "MCPorter". Web clients only show setup steps. Scopes exist only for Claude Code. In [`store/configStore.ts` L111-121][mt-scope] "user" is the top-level `mcpServers` in `~/.claude.json`, "local" is `projects[HOME].mcpServers`, and the other `projects[<path>]` keys are offered as per-project Scopes ([`list_claude_code_scopes`][mt-clients-rs-scopes]). There is no project-file Scope for any Agent: no `.mcp.json`, `.cursor/mcp.json`, `.vscode/mcp.json` or `.gemini/settings.json`. The JSON key path is split on `.` ([`utils.rs` L46-56][mt-utils-nav]), so a project path that contains a dot breaks the lookup.

**How it edits files.**
- Writes replace the whole server map under the key. Reading uses `serde_json::from_str` or `toml::from_str` and writing uses `to_string_pretty` ([`commands/config.rs` L212-290][mt-config-write]). Comments are lost. Strict parsing means a JSONC file fails to load ("Failed to parse config") instead of being overwritten, which is the safer failure. The raw editor writes whatever text the user typed after a JSON validity check ([`write_raw_config`][mt-config-raw]).
- `serde_json` is declared without `preserve_order` ([`Cargo.toml`][mt-cargo]). Unless a dependency turns that feature on, which I did not check, object keys come back sorted.
- Backups go to `~/.mtarsier/backups/<client>/<timestamp>.json` before each save ([`create_backup` L317-342][mt-config-backup]). The extension is `.json` even for Codex TOML. Restore takes a backup of the current file first ([L386-423][mt-config-restore]). The UI previews a restore as added, removed, changed and unchanged server names ([`configStore.ts` `previewRestore`][mt-store-preview]). It does not preview the diff of a normal save.
- Every write, backup and restore is appended to an audit log at `~/.mtarsier/audit.json` ([`commands/audit.rs`][mt-audit]).
- Writes are not atomic: plain `std::fs::write` with no temp file. There are no file locks and no check for a running Agent.
- Disable removes the key from the Agent file and records `{enabled:false, config}` in `~/.mtarsier/clients/<client>[_<scope>].json` ([`commands/server.rs`][mt-server-store], [`configStore.ts` L283-317][mt-store-sync]). On each load it reconciles. An entry that has disappeared from the Agent file is flagged `externally_removed`, and an entry that reappears becomes enabled again. The CLI's `tsr disable` rebuilds the stored entry from the fields `command`, `args`, `env`, `url`, `headers` and `auth` only ([`bin/tsr.rs` L440-462][mt-tsr-disable]). Any other field, such as `type`, `timeout` or `cwd`, is lost on re-enable.

**Plugins, extensions, skills.** Skills are supported, at user Scope only. `list_skills` reads `<skillsPath>/*/SKILL.md` and its front matter, follows symlinks, and skips targets outside the home directory ([`commands/skills.rs` L925-980][mt-skills-list]). Install tries `npx skills add <src> --agent <id> --yes` and falls back to downloading a GitHub tarball into `~/.agents/skills` and copying ([L717-807][mt-skills-npx]). Delete is dangerous. When a skill folder is a symlink, `delete_skill` removes the symlink *and* `remove_dir_all`s its target ([L1016-1035][mt-skills-delete]). That target is usually the shared `~/.agents/skills/<name>`, so removing a skill from one Agent removes it from every Agent that links to it. config-digger should treat this as the case to avoid. There is no awareness of plugins or extensions. Its only detection of VS Code extensions is checking `~/.vscode/extensions/github.copilot-chat*` to see whether Copilot is installed ([`clients.rs` L146-166][mt-clients-rs-ext]).

**Secrets.** Env values in the form editor are masked with a reveal toggle ([`EnvVarEditor.tsx`][mt-env-editor]). The Monaco raw mode shows everything.

## everymcp

**License.** No `LICENSE` file, and GitHub reports no license. [`package.json`][em-pkg] and the [README][em-readme] say "MIT", and so does the npm package `everymcp@0.1.0`. With no copyright line, the notice we would have to reproduce does not exist. Copying code would rest on the declared intent alone. Re-implementing ideas is safe. Copying code should wait until the author adds a LICENSE file.

**Stack and activity.** A Node/TypeScript CLI built on Commander, Ink (a React TUI), `jsonc-parser` and `diff`. It has 8 commits, all from 2026-02-13 to 2026-02-14, and nothing since. It looks like a weekend project generated from a plan file ([`everymcp-plan.md`][em-plan]). The README says it "has not been tested with every supported IDE/agent".

**How it models Agents.** Code adapters. [`BaseAdapter`][em-base] provides read, add and remove on a JSONC document. Each Agent subclasses it with `getConfigPaths()`, a `rootKey` and `transformSpec()` (canonical spec to Agent format). Codex overrides everything and drives the `codex` CLI ([`agents/codex.ts`][em-codex]). The separate [`AGENT_CONFIG_REFERENCE.md`][em-ref] holds the most complete written table of the three: paths per OS, root keys, transports, env-var interpolation syntax per Agent, URL field names (`serverUrl` for Windsurf), and gotchas.

**Agents and Scopes covered.** 15 Agents. The Scopes are only `global` and `project`, and project is the process cwd ([`types/index.ts`][em-base]). Claude Code's local Scope is missing. "global" writes the top-level `mcpServers` in `~/.claude.json` ([`agents/claude-code.ts`][em-claude-code]). Gemini CLI and Codex are global only.

**How it edits files.**
- JSON edits use `jsonc-parser` `modify` plus `applyEdits` on the original text ([`base-adapter.ts` L237-267][em-base-modify]). Only the affected key changes, and comments and the rest of the formatting survive. Of the three projects, this is the only one that meets our "touch only the affected Entry" rule for JSON.
- Writes are atomic: temp file plus rename ([`writeConfig` L269-275][em-base-write]). [`core/config-manager.ts`][em-config-manager] adds an in-process per-file promise lock. That lock does nothing across processes.
- `--dry-run` prints a unified diff of the whole file built with `diff.createPatch` ([`core/diff-engine.ts` L31-48][em-diff]). Secrets appear in that diff unmasked.
- Backups are file copies under `~/.everymcp/backups/<agent>/` with a `manifest.json` index. Restore backs up the current file first ([`core/backup-manager.ts`][em-backup]). There is no undo history apart from restoring a backup.
- On a JSONC parse error it logs a warning and carries on with a fallback object ([`config-manager.ts` `parseSource`][em-config-manager]). Edits still go to the original text, so the outcome depends on how `modify` handles broken input.
- For Codex it delegates to `codex mcp list --json`, `codex mcp add` and `codex mcp remove` ([`agents/codex.ts` L108-145][em-codex]). The Agent's own CLI handles the TOML. This is a real option for any Agent whose CLI can edit its config. The cost is that we cannot preview the exact bytes it will write.
- There is no disable command. `transformSpec` passes a `disabled` flag through when one is given.

**Plugins, extensions, skills.** None.

**Windows specifics.** For Claude Desktop on Windows it wraps commands as `cmd /c "<npx path> ..."` ([`utils/platform.ts` L159-172][em-platform]). Its VS Code variant is picked by probing for `antigravity`, `code-insiders`, `codium` and then `code`, in that order ([`getVSCodeVariant`][em-platform]). Only one variant is handled at a time.

## MCP Manager (xjeway)

**License.** [`LICENSE`][mm-license] is MIT, "Copyright (c) 2026 xJeway", and the workspace `Cargo.toml` matches. The README says the code is "largely AI-generated and reviewed by a human maintainer".

**Stack and activity.** Tauri 2 with React 19. The Rust workspace has a core library [`crates/mcp-manager-core`][mm-core-cargo] (`serde_json`, `serde_yaml`, `toml` 1.x), a CLI `mcpmgr` ([`crates/mcp-manager-cli`][mm-cli]) and the app in `src-tauri`, so its crate layout is close to the one we plan. It is i18n'd in English and Chinese. It has 122 commits, v0.2.0 was released 2026-09-30, and it was pushed the same day. It is actively developed.

**How it models Agents.** Code adapters. The `AppAdapter` trait has `detect_sources`, `parse_source` and `plan_apply` ([`adapters/mod.rs` L48-64][mm-adapter-trait]), with one Rust file per client. User paths are a `match` on `SupportedApp` and the OS ([`platform/mod.rs` L78-115][mm-paths]). Project paths are a second `match` ([`adapters/mod.rs` L72-89][mm-workspace-paths]). Write formatting is a small per-app profile: headers key (`http_headers` for Codex, none for Claude Desktop) and `tools: ["*"]` for Copilot CLI ([L255-279][mm-profile]). The canonical store is `servers.yaml`. Each server has `apps{}` and `placements[]` (app, Scope, path, enabled, managed).

**Agents and Scopes covered.** 15 clients. The Scopes are `user` and `workspace`. The workspace is the process cwd, and a GUI launch from home or `/` counts as "no project" ([`platform/mod.rs` L55-62][mm-has-workspace]). For Claude Code it reads the top-level `mcpServers` and `projects[<cwd>].mcpServers` and merges them into one map in which local wins. Both end up attributed to `~/.claude.json` ([`parser/mod.rs` L336-364][mm-claude-parse]). The [capability matrix][mm-matrix] says the local Scope is read-only "until the adapter can round-trip them safely". Precedence questions are listed as open: VS Code and Cursor user vs workspace, and OpenCode's three files.

**How it edits files.**
- Write plan, then preview, then apply. `preview` sorts each server per file into `Add`, `Update`, `Remove`, `Unchanged` or `OverwriteUnmanaged`, where the last means the file already has a hand-written entry with this id that the tool did not write before ([`workflow.rs` L163-250][mm-preview]). This is the most useful UX idea in the three projects for us.
- Writes are atomic: temp file plus rename, with stale temp files cleaned up ([`storage/mod.rs` L101-160][mm-atomic]). Backups go to `<appdata>/mcp-manager/backups/<mirrored absolute dir>/<file>.<stamp>.bak` ([L35-99][mm-backup]). If a batch fails, every file written so far is rolled back ([`apply_operations`][mm-apply-ops], `Snapshot` [L176-270][mm-snapshot]).
- Undo is a history of the last 20 changes in `history.json`. Each records FNV-1a fingerprints of every client file before and after. A change is undone only if nothing has touched those files since, and the undo can safely be re-run after a crash ([`history.rs`][mm-history], [`store.rs` L41-66][mm-fingerprint]).
- Cross-process locking uses an exclusive `std::fs::File::lock` on `<file>.lock`, shared by the app and the CLI ([`store.rs` L82-110][mm-lock]). A `servers.yaml` with a higher format version is read but never written ([`store.rs` L18-27][mm-store-version]).
- Comments and order are not preserved. Apply reads the client file with strict `serde_json::from_str(...).unwrap_or_else(|_| Value::Object(Map::new()))`, inserts the managed entries and writes `to_string_pretty` ([`storage/mod.rs` L334-382][mm-merge-json]). If the file has a comment or a trailing comma, which is normal in VS Code's `mcp.json`, the parse fails, the host becomes `{}`, and the rewrite drops every other key in the file. The preview path strips comments before parsing ([`workflow.rs` L267-295][mm-existing]), so the preview looks correct while the apply does something else. I did not run this, but the code path is direct. TOML goes through `toml::Table`, which drops comments. With no `preserve_order` features declared, both formats come back with sorted keys.
- Managed entries are regenerated from the canonical model with only `command`, `args`, `env` or `url`, plus `type`, headers and tools ([`standard_mcp_servers_at` L330-411][mm-standard]). Per-entry fields the model does not know, such as `cwd`, `timeout`, `disabled`, Codex `enabled_tools` and `startup_timeout_sec`, or VS Code `${input:...}` with `inputs`, are lost once the tool manages an entry. `store.rs` does keep unknown fields in its own `servers.yaml` ([L205-249][mm-store-preserve]). It does not do the same for client files.
- Disabling a client removes the managed key from that file (`remove_keys`) and keeps the server in `servers.yaml` ([`adapters/mod.rs` L197-233][mm-managed-writes]). On import it reads native flags `disabled: true` and `enabled: false` ([`parser/mod.rs` L120-217][mm-parse-server]), but never writes them.
- The risk check rejects headers sent in cleartext (`http://` to a non-loopback host), duplicate header names and empty ids ([`src/services/risk.ts`][mm-risk]). The Rust "security" module is a placeholder ([`security/mod.rs`][mm-security]).

**Plugins, extensions, skills.** None. MCP servers only.

**Secrets.** Header values are masked in the editor with a per-row reveal ([`ServerEditor.tsx` L143, L640-660][mm-editor]). Env values are not masked, and the JSON mode shows everything.

## Comparison

| | mTarsier | everymcp | MCP Manager | config-digger target |
| --- | --- | --- | --- | --- |
| License | MIT (metadata says ISC) | Declared MIT, no LICENSE file | MIT | MIT OR Apache-2.0 |
| Stack | Tauri 2, React, Rust, `tsr` CLI | Node, TS, Ink CLI | Tauri 2, React, Rust core crate plus `mcpmgr` CLI | Tauri 2, React, Rust core plus CLI |
| Activity | v1.2.0, last push 2026-04 | 8 commits, 2026-02, dormant | v0.2.0, active 2026-09 | |
| Agent model | Data records (TS) duplicated in Rust | Code adapter per Agent | Code adapter per Agent | Data file per Agent |
| v0.1 Agents covered | All 6 | All 6 | All 6 | 6 |
| Scopes | Claude Code user, local and per-project keys only | global and cwd project | user and cwd workspace; Claude local merged read-only | managed, user, project, local |
| Effective/Shadowed | No | No | No (merges, no explanation) | Core feature |
| Provenance (plugins, extensions, managed) | No | No | No | Yes |
| Comment and format preservation | No (full rewrite; strict parse rejects JSONC) | JSON yes (`jsonc-parser` modify); TOML through `codex` CLI | No; JSONC can be wiped on apply | Required (JSONC and TOML) |
| Atomic write | No | Yes | Yes | |
| Backups | Per client, timestamped | Per agent, manifest | Mirrored path, `.bak`, batch rollback | Required |
| Preview | Restore diff by server name only | Unified text diff (`--dry-run`) | Per-server action list incl. "overwrite unmanaged" | Required |
| Undo | Restore a backup | Restore a backup | 20-step history guarded by fingerprints | Required |
| Disable | Remove key, park in own store | n/a | Remove key, keep in `servers.yaml` | Open, decided in #20 |
| Skills | User Scope, symlink-aware; unsafe delete | No | No | Yes |
| Running-Agent check | No | "restart required" flag only | No | Required |
| Secret masking | Env in form | No | Headers in form | Required |

## Agent path facts

Issue [#1](https://github.com/samfaina/config-digger/issues/1) does not contain a path table. Its Agent facts are delegated to the per-Agent research tickets [#8](https://github.com/samfaina/config-digger/issues/8) to [#13](https://github.com/samfaina/config-digger/issues/13). The list below records what the three projects claim, where they disagree, and two read-only checks on this machine, so those tickets can confirm or reject each point. "This machine" means a `test -e` or a key listing, nothing more.

**Claude Code** (for #8)
- All three agree on `~/.claude.json` with top-level `mcpServers` for the user Scope, `projects[<path>].mcpServers` for the local Scope, and `<project>/.mcp.json` for the project Scope ([mTarsier][mt-scope], [everymcp ref][em-ref], [MCP Manager matrix][mm-matrix]).
- On this machine the `projects` keys in `~/.claude.json` mix spellings for the same folder: `E:/projects/claudio/petme`, `E:\\projects\\claudio\\petme` and `e:/projects/claudio/petme` all appear. mTarsier looks up `projects.{HOME}` with the native backslash home path ([`utils.rs` L86-93][mt-utils-home]), and MCP Manager looks up `projects[<cwd>]` verbatim ([`parser/mod.rs` L349-356][mm-claude-parse]). Both would miss entries stored under another spelling. Item identity (#18) and project discovery (#19) need a path normalisation rule.
- None handles `CLAUDE_CONFIG_DIR`, `managed-mcp.json`, plugin-provided servers, or project-level `.claude/skills`. mTarsier lists skills from `~/.claude/skills` only.

**Claude Desktop** (for #9)
- All three use `%APPDATA%\Claude\claude_desktop_config.json` on Windows ([mTarsier][mt-clients-ts], [everymcp][em-claude-desktop], [MCP Manager][mm-paths]). On this machine, which has the Store/MSIX install, that file does not exist, and the live one is `%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Claude\claude_desktop_config.json`. All three tools would show no servers here, and a write from them would create a file the app may never read. This confirms the MSIX concern in #9.
- Detection hints: mTarsier uses `%LOCALAPPDATA%\Microsoft\WindowsApps\Claude.exe` (the MSIX execution alias) or `%LOCALAPPDATA%\AnthropicClaude\Update.exe` (the Squirrel installer) ([`clients.ts` L15][mt-clients-ts]). MCP Manager uses `AppData/Local/AnthropicClaude/Claude.exe` or `AppData/Local/Programs/Claude/Claude.exe` ([`platform/mod.rs` L153-160][mm-detect]).
- The config file holds local (stdio) servers only. Remote servers are Connectors managed in the app ([everymcp ref][em-ref]). MCP Manager will not write headers for Claude Desktop for this reason ([`adapters/mod.rs` L266-268][mm-profile]).
- mTarsier's Claude Desktop `skillsPath` is `~/Library/Application Support/Claude/skills` with no Windows variant ([`clients.ts` L18][mt-clients-ts]), so it is wrong on Windows.

**VS Code / Copilot** (for #10)
- User file: mTarsier and MCP Manager use `%APPDATA%\Code\User\mcp.json` with root `servers`. everymcp instead writes `%APPDATA%\<variant>\User\settings.json` under the old `mcp.servers` setting ([`agents/vscode.ts`][em-vscode]). everymcp also reads that setting as a nested `mcp` → `servers` path, so it will not find the usual flat `"mcp.servers"` key.
- Workspace file: `.vscode/mcp.json` with root `servers` (everymcp, MCP Manager). The `inputs` array carries `${input:id}` secrets ([everymcp ref][em-ref]).
- None handles VS Code profiles. everymcp handles one of Insiders, VSCodium or Antigravity at a time.
- "GitHub Copilot" means two different things. In mTarsier it is the VS Code `mcp.json`, detected through the `github.copilot-chat` extension. In MCP Manager it is the Copilot CLI's `~/.copilot/mcp-config.json` (root `mcpServers`, writes `tools: ["*"]`), which also imports VS Code files ([`adapters/github_copilot.rs`][mm-copilot]). `~/.copilot` exists on this machine. v0.1 needs to decide whether Copilot CLI is part of the "VS Code/Copilot" Agent or a separate Agent.
- mTarsier puts Copilot skills at `~/.copilot/skills` (for #14).

**Cursor** (for #11)
- All three agree on `%USERPROFILE%\.cursor\mcp.json` and `<project>\.cursor\mcp.json` with root `mcpServers`. mTarsier puts skills at `~/.cursor/skills`. Install detection looks at `%LOCALAPPDATA%\Programs\cursor\Cursor.exe` ([mTarsier][mt-clients-ts], [MCP Manager][mm-detect]).

**Codex** (for #12)
- All three agree on `%USERPROFILE%\.codex\config.toml` with `[mcp_servers.<name>]`. Remote headers go under `http_headers` ([MCP Manager][mm-profile], [everymcp][em-codex]).
- MCP Manager also imports `~/.codex/mcp.json` ([`adapters/codex.rs` L13-28][mm-codex]). I have not seen this file documented by Codex. #12 should confirm or rule it out.
- None reads project `.codex/config.toml`. MCP Manager says to keep it disabled "until there is authoritative documentation" ([matrix][mm-matrix]). None honours `CODEX_HOME`. mTarsier puts skills at `~/.codex/skills`. The Codex desktop app is detected at `%LOCALAPPDATA%\Programs\Codex\Codex.exe` ([`clients.ts` L105][mt-clients-ts]).
- `codex mcp list --json` exists and returns a structured list ([everymcp][em-codex]). It could serve as a cross-check during verification.

**Gemini CLI** (for #13)
- User file: `%USERPROFILE%\.gemini\settings.json` under `mcpServers` (all three). Project file: `.gemini/settings.json` (MCP Manager only). The parser also accepts `httpUrl` as a URL field ([`parser/mod.rs` L145-149][mm-parse-server]).
- Antigravity shares `~/.gemini`. mTarsier and MCP Manager use `~/.gemini/antigravity/mcp_config.json`. mTarsier's README table says `~/.antigravity/mcp.json`, which disagrees with its own code. everymcp treats Antigravity as a VS Code fork with `%APPDATA%\Antigravity\User\settings.json`. mTarsier puts Gemini skills at `~/.gemini/skills` and Antigravity skills at `~/.antigravity/skills`.

**OpenCode** (the v0.1 fallback Agent)
- mTarsier's Windows path is `%APPDATA%\opencode\opencode.json` ([`clients.ts` L237][mt-clients-ts]). MCP Manager uses `~/.config/opencode/opencode.json` on every OS ([`platform/mod.rs` L102][mm-paths]). On this machine `~/.config/opencode` exists and `%APPDATA%\opencode` does not, which supports MCP Manager.
- The format differs from the others: root `mcp`, `type: local|remote`, `command` as an array, `environment` instead of `env`, and `enabled` ([mTarsier `config.rs` L114-170][mt-config-opencode]). Project files are `opencode.json` and `.opencode/opencode.jsonc` ([matrix][mm-matrix]). mTarsier puts skills at `~/.agents/skills`.

**Out-of-scope Agents, useful for the Agent definition acceptance test in #21**
- Windsurf: `%USERPROFILE%\.codeium\windsurf\mcp_config.json`, root `mcpServers`, with `serverUrl` as the remote URL field.
- Zed: `%APPDATA%\Zed\settings.json` under `context_servers`, with `"source": "custom"`.
- Kiro: `~/.kiro/settings/mcp.json`, project `.kiro/settings/mcp.json`, plus an "agent" Scope.
- Sources: [everymcp ref][em-ref], [MCP Manager paths][mm-paths], [matrix][mm-matrix]. Zed (a settings file shared with non-MCP keys) and Kiro (three Scopes) are good stress cases for #21.

## Reuse candidates

All three are MIT or declared MIT. MIT code can go into an MIT OR Apache-2.0 project. The copied portions stay MIT, so we must keep their copyright and permission notice, for example in a `THIRD_PARTY_NOTICES` file and a header comment in any file that holds copied code. Apache-2.0's patent grant does not extend to code we did not write. Facts such as paths, key names and field names are not copyrightable, so re-deriving them is free. Copying a whole table verbatim deserves attribution all the same.

| Candidate | From | What to take | License action |
| --- | --- | --- | --- |
| Undo history guarded by fingerprints | MCP Manager [`history.rs`][mm-history], [`store.rs`][mm-fingerprint] | The design: before and after fingerprints per file, undo refused if anything changed since, re-runnable after a crash, bounded depth. It fits our "verify, undo" step directly. | Re-implement (ideas are free). Keep the MIT notice if code is copied. |
| Batch snapshot and rollback | MCP Manager [`storage/mod.rs`][mm-snapshot] | `Snapshot` captures every file, including ones that do not exist yet and the directories created for them, and puts everything back if any write in the batch fails. | Same |
| Atomic write with stale temp cleanup | MCP Manager [L101-160][mm-atomic], everymcp [`writeConfig`][em-base-write] | Temp file in the same directory, then rename. Clean up leftovers by a recognisable marker. | Small enough to re-implement |
| Cross-process lock | MCP Manager [`store.rs` L82-110][mm-lock] | Lock on `<file>.lock` shared by app and CLI (`File::lock` needs Rust 1.89 or later). We only need it for our own state files, not Agent files. | Re-implement |
| "Overwrite unmanaged" preview action | MCP Manager [`workflow.rs`][mm-preview] | Show per Entry what will happen, including when a write would clobber something the user wrote by hand. | UX pattern, free |
| Edit only the affected key in JSONC | everymcp [`base-adapter.ts`][em-base-modify] | The approach, applied with a Rust equivalent (see #15): compute minimal text edits for one path and apply them to the original text. | Approach only (TS, and no LICENSE file) |
| Delegating to the Agent's CLI | everymcp [`agents/codex.ts`][em-codex] | Use `codex mcp list --json` as a verification oracle. It is a poor write path for us because we cannot preview its bytes. | Idea |
| Agent fact cross-check | mTarsier [`clients.ts`][mt-clients-ts], everymcp [`AGENT_CONFIG_REFERENCE.md`][em-ref], MCP Manager [`platform/mod.rs`][mm-paths], [matrix][mm-matrix] | Seed and cross-check our Agent definitions: paths, root keys, URL field names, env interpolation syntax, install detection paths. Each fact must still be verified per #8 to #13. | Facts are free. Credit the sources in the definitions' comments. |
| Shape of the data record | mTarsier [`types/client.ts`][mt-client-types] | Per-OS path fields, a dotted root key, format, a detection rule with alternative paths, skills path. It is a usable starting point for #21. It lacks Scopes, precedence, Provenance and disable mechanisms, and its dot-split key path breaks on keys that contain dots, so we need an array path or an escaping rule. | Idea |
| Risk checks | MCP Manager [`risk.ts`][mm-risk] | Flag headers sent over plain `http://` to a non-loopback host, and duplicate header names. Cheap warnings for the inventory view. | Re-implement |
| Reconciling "externally removed" | mTarsier [`configStore.ts`][mt-store-sync] | If we ever keep Disabled Entries in our own store, detect Entries that changed outside the app on next load. | Idea |

Things to avoid:
- mTarsier deletes a symlink target when removing a skill.
- MCP Manager parses strictly then falls back to `{}` on apply.
- All three regenerate whole entries from a canonical model and drop unknown fields.
- mTarsier and MCP Manager remove an Entry to Disable it when the Agent has a native flag. Whether that is acceptable belongs in #20.
- MCP Manager's preview and apply parse the same file in two different ways.

## Sources

Repositories and metadata were read with `gh repo view`, `gh api` and `npm view everymcp` on 2026-10-01:

- https://github.com/mcp360/mTarsier at `8f14301b7fa9566fcf0315a00feb8ca3838f912a`
- https://github.com/am-will/everymcp at `c6b9e231c9ff0f556e879b440f0b8de506996d4f`
- https://github.com/xjeway/mcp-manager at `15e11863ccac22ab7344180a6ea39a8c67f3972f`

Every inline link in this document points to a file or line range at those commits. The files read most closely:

- mTarsier: [`src/lib/clients.ts`](https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src/lib/clients.ts), [`src-tauri/src/commands/config.rs`](https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src-tauri/src/commands/config.rs), [`src-tauri/src/commands/skills.rs`](https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src-tauri/src/commands/skills.rs), [`src/store/configStore.ts`](https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src/store/configStore.ts), [`src-tauri/src/bin/tsr.rs`](https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src-tauri/src/bin/tsr.rs)
- everymcp: [`AGENT_CONFIG_REFERENCE.md`](https://github.com/am-will/everymcp/blob/c6b9e231c9ff0f556e879b440f0b8de506996d4f/AGENT_CONFIG_REFERENCE.md), [`src/agents/base-adapter.ts`](https://github.com/am-will/everymcp/blob/c6b9e231c9ff0f556e879b440f0b8de506996d4f/src/agents/base-adapter.ts), [`src/agents/codex.ts`](https://github.com/am-will/everymcp/blob/c6b9e231c9ff0f556e879b440f0b8de506996d4f/src/agents/codex.ts), [`src/core/backup-manager.ts`](https://github.com/am-will/everymcp/blob/c6b9e231c9ff0f556e879b440f0b8de506996d4f/src/core/backup-manager.ts)
- MCP Manager: [`docs/mcp-client-capability-matrix.md`](https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/docs/mcp-client-capability-matrix.md), [`platform/mod.rs`](https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/platform/mod.rs), [`adapters/mod.rs`](https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/adapters/mod.rs), [`storage/mod.rs`](https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/storage/mod.rs), [`workflow.rs`](https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/workflow.rs), [`history.rs`](https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/history.rs), [`store.rs`](https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/store.rs)

Read-only checks on this machine: whether the Claude Desktop, VS Code and OpenCode config paths named above exist, and how the `projects` keys in `~/.claude.json` are spelled. No values were printed.

Related tickets: [#1](https://github.com/samfaina/config-digger/issues/1), [#8](https://github.com/samfaina/config-digger/issues/8) to [#15](https://github.com/samfaina/config-digger/issues/15), [#18](https://github.com/samfaina/config-digger/issues/18) to [#21](https://github.com/samfaina/config-digger/issues/21).

## Open questions

1. Does Claude Code normalise the `projects` keys in `~/.claude.json` (slash direction, drive-letter case) when it looks one up? If it does not, which spelling wins when several exist for one folder? This matters for #8, #18 and #19.
2. With the MSIX Claude Desktop installed, does the app also read `%APPDATA%\Claude\claude_desktop_config.json` when no private copy exists? Which file should config-digger show, and which should it write? This belongs to #9.
3. Is `~/.codex/mcp.json`, which MCP Manager imports, a real Codex location, or an old or third-party one? This belongs to #12.
4. Is the Copilot CLI (`~/.copilot/mcp-config.json`, `~/.copilot/skills`) part of the v0.1 "VS Code/Copilot" Agent or a separate Agent? This affects #2 and #10.
5. Do mTarsier's and MCP Manager's Rust binaries actually build `serde_json` and `toml` without `preserve_order`, which would mean they sort keys? Answering needs a `cargo tree -e features` run, which I did not do. It does not change any of our decisions, since neither tool is a candidate for its write code.
6. Should config-digger's Disable ever fall back to "remove and park in our own store", as both desktop tools do, for Agents that have no native disable flag? This belongs to #20.

[em-backup]: https://github.com/am-will/everymcp/blob/c6b9e231c9ff0f556e879b440f0b8de506996d4f/src/core/backup-manager.ts
[em-base]: https://github.com/am-will/everymcp/blob/c6b9e231c9ff0f556e879b440f0b8de506996d4f/src/agents/base-adapter.ts
[em-base-modify]: https://github.com/am-will/everymcp/blob/c6b9e231c9ff0f556e879b440f0b8de506996d4f/src/agents/base-adapter.ts#L237-L267
[em-base-write]: https://github.com/am-will/everymcp/blob/c6b9e231c9ff0f556e879b440f0b8de506996d4f/src/agents/base-adapter.ts#L269-L275
[em-claude-code]: https://github.com/am-will/everymcp/blob/c6b9e231c9ff0f556e879b440f0b8de506996d4f/src/agents/claude-code.ts
[em-claude-desktop]: https://github.com/am-will/everymcp/blob/c6b9e231c9ff0f556e879b440f0b8de506996d4f/src/agents/claude-desktop.ts
[em-codex]: https://github.com/am-will/everymcp/blob/c6b9e231c9ff0f556e879b440f0b8de506996d4f/src/agents/codex.ts
[em-config-manager]: https://github.com/am-will/everymcp/blob/c6b9e231c9ff0f556e879b440f0b8de506996d4f/src/core/config-manager.ts
[em-diff]: https://github.com/am-will/everymcp/blob/c6b9e231c9ff0f556e879b440f0b8de506996d4f/src/core/diff-engine.ts#L31-L48
[em-pkg]: https://github.com/am-will/everymcp/blob/c6b9e231c9ff0f556e879b440f0b8de506996d4f/package.json
[em-plan]: https://github.com/am-will/everymcp/blob/c6b9e231c9ff0f556e879b440f0b8de506996d4f/everymcp-plan.md
[em-platform]: https://github.com/am-will/everymcp/blob/c6b9e231c9ff0f556e879b440f0b8de506996d4f/src/utils/platform.ts
[em-readme]: https://github.com/am-will/everymcp/blob/c6b9e231c9ff0f556e879b440f0b8de506996d4f/README.md
[em-ref]: https://github.com/am-will/everymcp/blob/c6b9e231c9ff0f556e879b440f0b8de506996d4f/AGENT_CONFIG_REFERENCE.md
[em-vscode]: https://github.com/am-will/everymcp/blob/c6b9e231c9ff0f556e879b440f0b8de506996d4f/src/agents/vscode.ts
[mm-adapter-trait]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/adapters/mod.rs#L48-L64
[mm-apply-ops]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/storage/mod.rs#L478-L503
[mm-atomic]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/storage/mod.rs#L101-L160
[mm-backup]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/storage/mod.rs#L35-L99
[mm-claude-parse]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/parser/mod.rs#L336-L364
[mm-cli]: https://github.com/xjeway/mcp-manager/tree/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-cli
[mm-codex]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/adapters/codex.rs#L13-L28
[mm-copilot]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/adapters/github_copilot.rs
[mm-core-cargo]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/Cargo.toml
[mm-detect]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/platform/mod.rs#L134-L194
[mm-editor]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/src/components/ServerEditor.tsx#L640-L660
[mm-existing]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/workflow.rs#L267-L295
[mm-fingerprint]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/store.rs#L41-L66
[mm-has-workspace]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/platform/mod.rs#L55-L62
[mm-history]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/history.rs
[mm-license]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/LICENSE
[mm-lock]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/store.rs#L82-L110
[mm-managed-writes]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/adapters/mod.rs#L197-L233
[mm-matrix]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/docs/mcp-client-capability-matrix.md
[mm-merge-json]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/storage/mod.rs#L334-L382
[mm-parse-server]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/parser/mod.rs#L120-L217
[mm-paths]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/platform/mod.rs#L78-L115
[mm-preview]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/workflow.rs#L163-L250
[mm-profile]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/adapters/mod.rs#L255-L279
[mm-risk]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/src/services/risk.ts
[mm-security]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/security/mod.rs
[mm-snapshot]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/storage/mod.rs#L176-L270
[mm-standard]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/adapters/mod.rs#L330-L411
[mm-store-preserve]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/store.rs#L205-L249
[mm-store-version]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/store.rs#L18-L27
[mm-workspace-paths]: https://github.com/xjeway/mcp-manager/blob/15e11863ccac22ab7344180a6ea39a8c67f3972f/crates/mcp-manager-core/src/adapters/mod.rs#L72-L89
[mt-audit]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src-tauri/src/commands/audit.rs
[mt-cargo]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src-tauri/Cargo.toml
[mt-client-types]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src/types/client.ts
[mt-clients-rs]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src-tauri/src/commands/clients.rs
[mt-clients-rs-ext]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src-tauri/src/commands/clients.rs#L146-L166
[mt-clients-rs-scopes]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src-tauri/src/commands/clients.rs#L409-L439
[mt-clients-ts]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src/lib/clients.ts
[mt-config-backup]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src-tauri/src/commands/config.rs#L317-L342
[mt-config-opencode]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src-tauri/src/commands/config.rs#L114-L170
[mt-config-raw]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src-tauri/src/commands/config.rs#L292-L315
[mt-config-restore]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src-tauri/src/commands/config.rs#L386-L423
[mt-config-write]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src-tauri/src/commands/config.rs#L212-L290
[mt-env-editor]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src/components/config/EnvVarEditor.tsx
[mt-license]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/LICENSE
[mt-pkg]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/package.json
[mt-registry]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src-tauri/src/registry.rs
[mt-scope]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src/store/configStore.ts#L111-L121
[mt-server-store]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src-tauri/src/commands/server.rs
[mt-skills-delete]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src-tauri/src/commands/skills.rs#L1016-L1035
[mt-skills-list]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src-tauri/src/commands/skills.rs#L925-L980
[mt-skills-npx]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src-tauri/src/commands/skills.rs#L717-L807
[mt-store-preview]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src/store/configStore.ts#L533-L580
[mt-store-sync]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src/store/configStore.ts#L283-L317
[mt-tsr-disable]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src-tauri/src/bin/tsr.rs#L402-L491
[mt-utils-home]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src-tauri/src/commands/utils.rs#L86-L93
[mt-utils-nav]: https://github.com/mcp360/mTarsier/blob/8f14301b7fa9566fcf0315a00feb8ca3838f912a/src-tauri/src/commands/utils.rs#L46-L56
