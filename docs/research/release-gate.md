# Release gate: the unverified marks against the real Agents

Issue: [#80](https://github.com/samfaina/config-digger/issues/80). Terms follow [`CONTEXT.md`](../../CONTEXT.md).

Before v0.1 ships, every Unverified mark of the built-in definitions is tested against the real Agent on the reference machine (Windows 11 Pro, home `C:\Users\kgon1`, a second drive `E:`). Each mark is then removed, or the rule is corrected, with the evidence in the definition as a `[test]` tag and here. This file covers the three CLI Agents, tested on 2026-10-23; the marks that need Windows Sandbox, since their files sit in `%ProgramFiles%` or `%ProgramData%`, or come with an installer; Claude Code's marks that need a claude.ai account, tested on 2026-10-24 with the owner's Max account; and VS Code's marks about the MCP files it reads, tested on 2026-10-24 on throwaway profiles. Cursor, the rest of VS Code and the rest of Claude Desktop, which need their windows driven, come next.

## The probes

A probe per Agent in `uat/agents/` builds throwaway homes and Projects, runs the real Agent on them and checks what it loads:

```powershell
node uat/agents/claude-code.mjs [--bin <claude.exe>]
node uat/agents/codex.mjs       [--bin <codex.exe or codex.cmd>]
node uat/agents/gemini-cli.mjs  [--bin <gemini.cmd>]
node uat/agents/vscode.mjs      [--bin <Code.exe>]
```

Each check names the table of the definition it tests, as `cargo xtask unverified` prints it, and passes when the Agent does what the definition now says. A probe exits 1 when a check fails. `CD_PROBE_VERBOSE=1` prints what each check saw, and `CD_PROBE_JSON=<file>` writes the results. Run a probe again with each new version of its Agent: when every check passes, the version can join the definition's `verified` range.

What keeps a probe off the user's own configuration:

- Everything sits under `CD_PROBE_ROOT`, `E:\cd-throwaway-agents` by default, whose name must start with `cd-throwaway`. It is outside the user's home folder on purpose: Claude Code reads `CLAUDE.md` and `.mcp.json` in every folder above the one it starts in, so a Project under `C:\Users\<me>` would pick up the user's own files.
- Each Agent gets a throwaway home: `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, or `GEMINI_CLI_HOME` in the temp folder. Gemini CLI's home is on the system drive and its Projects on `E:`, so that a rule for the home folder and one for a Project sit on different drives.
- Servers are `cmd /c <tag>`, which exits at once, or an HTTP URL on a closed local port. Entries are `cd-throwaway-*` and the only key is `not-a-real-secret`.
- No request reaches a model. Claude Code's Messages API is a local server in the probe, which keeps each request so the probe can read the skills Claude Code lists to its model. Codex and Gemini CLI point at a closed local port.
- Claude Code still fetches its GrowthBook features at start, without an account. Older Claude Code versions come from npm into the probe root (`npm install --prefix E:\cd-throwaway-agents\cc-<version> @anthropic-ai/claude-code@<version>`).
- VS Code gets a throwaway profile per launch: `--user-data-dir`, `--extensions-dir`, `--shared-data-dir` and `--agent-plugins-dir`, and `USERPROFILE` and `APPDATA` in the launch's folder, so that `~\.vscode\argv.json` and Claude Desktop's config are throwaway too. It runs beside a VS Code the user has open, and the probe stops only the processes given its own data folder. Its servers are this machine's Node writing a marker file, and nothing signs in. Another version comes as the zip archive from `update.code.visualstudio.com/<version>/win32-x64-archive/stable`, unpacked into the probe root.

How each probe reads what the Agent loaded:

| Agent | Oracle |
|---|---|
| Claude Code | `claude mcp list` (approval status, parse diagnostics), `claude mcp get`, `claude plugin list`, the `init` event of `claude -p --output-format stream-json --verbose` (servers, skills, plugins), and the request Claude Code sends its model: the skill listing, `- <name> (<alias>): <description>`, and the body of a skill a `/<name>` prompt runs |
| Codex | `codex mcp list`, `codex debug prompt-input` (the skills its model gets), the files `codex exec` and `codex archive` write |
| Gemini CLI | `gemini extensions list` (`Enabled (Workspace)` for the folder it starts in), `gemini mcp list`, `gemini -p` |
| VS Code | The servers it starts: a probe extension, loaded with `--extensionDevelopmentPath`, runs `workbench.mcp.startServer *`, trusting discovered servers, and each server writes a marker with the arguments and variables it got; the folders the window opened; the workspace's `mcp.extCachedServers` in its `state.vscdb` |

### In Windows Sandbox

Three probes write what only an administrator or an installer writes: `claude-code-machine.mjs` (`%ProgramFiles%\ClaudeCode`, `HKLM` and `HKCU\SOFTWARE\Policies\ClaudeCode`), `codex-machine.mjs` (`%ProgramData%\OpenAI\Codex\requirements.toml`) and `claude-desktop.mjs` (the classic installer and the MSIX package claude.ai serves). Each refuses to run outside Windows Sandbox, whose only user is `WDAGUtilityAccount`, so nothing reaches the reference machine. `uat/agents/sandbox.ps1` runs them there:

```powershell
.\uat\agents\sandbox.ps1                                  # the three probes
.\uat\agents\sandbox.ps1 -Probes claude-code-machine -Claude <claude.exe>, <claude.exe>
.\uat\agents\sandbox.ps1 -Probes claude-desktop -KeepOpen # leaves the Sandbox open
```

It maps into the Sandbox, read-only, the probes, this machine's Node (Volta's image, not its shim), a copy of each Claude Code binary `-Claude` names (`~\.local\share\claude\versions\<version>` included), Codex's `codex.exe` from its npm package, and the Claude Desktop installers, which it downloads from `claude.ai/api/desktop/win32/x64/{exe,msix}/latest/redirect`. The Sandbox's logon command, `in-sandbox.ps1`, runs each probe with `CD_PROBE_VERBOSE`, `claude-code-machine.mjs` once per Claude Code binary, writes `<probe>.log` and `<probe>.json` to `%TEMP%\cd-agents-sandbox\results`, and shuts the Sandbox down.

| Probe | Oracle |
|---|---|
| `claude-code-machine.mjs` | `claude mcp list` (servers, parse diagnostics) and whether `claude -p` answers the local model stand-in |
| `codex-machine.mjs` | `codex mcp list`: `enabled`, or `disabled: requirements (<file>)` |
| `claude-desktop.mjs` | The files the installers write, and the servers the app starts: each `claude_desktop_config.json` declares a server whose command writes a marker file named after its folder |

### With a claude.ai account

`claude-account.mjs` tests the skills Claude Code syncs from claude.ai and its sync gate. It needs a home signed in to claude.ai, and the owner signs in to a throwaway one once, outside their own home folder:

```bash
CLAUDE_CONFIG_DIR='E:\cd-throwaway-agents\claude-account-home' claude auth login --claudeai
node uat/agents/claude-account.mjs [--bin <claude.exe>]
CLAUDE_CONFIG_DIR='E:\cd-throwaway-agents\claude-account-home' claude auth logout
```

`!` in a Claude Code prompt runs bash, so `$env:CLAUDE_CONFIG_DIR=…` there signs in the user's own home instead. `CD_PROBE_ACCOUNT_HOME` names another signed-in home, a folder named `claude-account-home*` in the probe root; the probe refuses any other, and runs only when `claude auth status` says `claude.ai` and no variable that takes Claude Code off the account or its gates is set. Its model is the local stand-in, so no prompt reaches a model; Claude Code still signs in, fetches its gates and downloads the account's skills. It plants skills beside the synced ones, sets the gate's cached value in `.claude.json`, and puts both back at the end. Its oracle is the `init` event's skills, the skill listing and the body of a skill a `/<name>` prompt runs, as for `claude-code.mjs`, and what Claude Code downloads into `skills/synced/<organizationUuid>_<accountUuid>`.

## Versions

| Agent | Versions every check passes on | `verified` |
|---|---|---|
| Claude Code | 2.1.299, 2.1.300, 2.1.302, 2.1.304, 2.1.308, 2.1.309, with `claude-code-machine.mjs` too; `claude-account.mjs` on 2.1.299, 2.1.302, 2.1.309 and 2.1.310, with a Max account | 2.1.299 to 2.1.309 (was 2.1.286 to 2.1.302) |
| Codex | 0.159.3, with `codex-machine.mjs` too | unchanged |
| Gemini CLI | 0.62.0 | unchanged |
| VS Code | 1.129.1, 1.135.0, 1.141.0, `vscode.mjs` | 1.129.1 to 1.141.0 (was 1.129.1) |
| Claude Desktop | 3.6556.2, `claude-desktop.mjs` only | unchanged, 2.16120: 3.x keeps a package's data elsewhere ([#305](https://github.com/samfaina/config-digger/issues/305)) |

Claude Code 2.1.286 fails 7 checks, all on what 2.1.299 changed: a plain-string `enabledMcpjsonServers` or `disabledMcpjsonServers` voids its settings file, there is no name rule, and the skill listing gives no alias. The definition describes 2.1.299 and later, so the range starts there.

## The marks

The inventory is the 80 marks of `main` at `5cb2ad5` (`cargo xtask unverified`): 20 in Claude Code, 15 in Codex, 4 in Gemini CLI, 4 in Claude Desktop, 17 in Cursor and 20 in VS Code. The skill manager definitions hold none. Line numbers are those of `5cb2ad5`.

Outcomes:

- **Confirmed**: the Agent does what the mark assumed. The mark is gone and the fact carries `[test]`.
- **Corrected**: the Agent does something else, and the definition now says what. The mark is gone.
- **Ticket**: the Agent does something else that the definition format or the core cannot express yet. The mark is gone, the definition states the fact with `[test]` and names the ticket.
- **Pending**: not tested yet. The mark stays, and the reason says what the test needs.
- **Pending, under the exception**: #80's last criterion lets a mark stay untested when its test needs an account or a deployment the project does not have: a Codex enterprise account, Claude Teams or Enterprise, or a Claude Desktop deployment for a third-party provider. The release notes list such a mark with what its test needs.

### Claude Code

| Mark | What the probe saw | Outcome |
|---|---|---|
| `:86` `settings[managed]`: `remote-settings.json` is one JSON object | With an API key, Claude Code does not read `remote-settings.json` at all; a Max account has no server-managed settings | Pending, under the exception: server-managed settings need Claude Teams or Enterprise |
| `:140` `locations[managed-mcp]`: unparsable `managed-mcp.json` | Skipped; `claude mcp list` reports "[Failed to parse] Enterprise config"; the file keeps its exclusive control, so only `managedMcpServers` load, and Claude Code starts | Corrected: `ignores_file`; the exclusive gate already counts a file whatever it holds |
| `:155` `locations[managed-settings-mcp]`: unparsable managed settings | An HKLM value, `managed-settings.json` or drop-in that does not parse stops Claude Code ("Managed settings document could not be parsed as a JSON object"), even beside a source that parses; an HKCU value that does not parse is skipped. `remote-settings.json` not tested | Corrected: each source of `settings[managed]` now says what Claude Code does about it (`on_read_failure` on a source, new in the format). The mark stays for `remote-settings.json`: Pending, under the exception, as for `:86` |
| `:189` `locations[project-mcp]`: parent folders | `.mcp.json` is read in every folder above the Project, past the repo root and outside a repo; of same-named servers the nearest folder's wins | Ticket [#292](https://github.com/samfaina/config-digger/issues/292) |
| `:190` `locations[project-mcp]`: unparsable `.mcp.json` | Skipped; the other servers load; `claude mcp list` reports "[Failed to parse] Project config" | Corrected: `ignores_file` |
| `:226` `locations[plugin-mcp]`: unparsable plugin `.mcp.json` | The plugin loads without those servers | Corrected: `ignores_file` |
| `:251` `locations[plugin-manifest-mcp]`: unparsable `plugin.json` | The plugin fails to load whole ("Manifest parse error"), the servers of its `.mcp.json` included | Corrected to `ignores_file` for the file's own Entries; ticket [#295](https://github.com/samfaina/config-digger/issues/295) for the rest of the plugin |
| `:272` `items.mcp`: two plugins with one endpoint | The plugin first in `enabledPlugins`, merged user, Project, local, each plugin at its first place, wins | Ticket [#294](https://github.com/samfaina/config-digger/issues/294) |
| `:290` `items.mcp.gates[0]`: the Project's state holds `enableAllProjectMcpServers` | It does, under the Project's forward-slash key, once `hasTrustDialogAccepted` is true | Confirmed; trust is ticket [#293](https://github.com/samfaina/config-digger/issues/293) |
| `:291` `items.mcp.gates[0]`: plain-string `enabledMcpjsonServers` approves nothing | In a settings file it approves that server; in the Project's state, nothing | Ticket [#172](https://github.com/samfaina/config-digger/issues/172) |
| `:367`, `:391`, `:402` `items.mcp.disable[1]`, `[3]`, `[4]`: plain-string `disabledMcpjsonServers` skips nothing | It skips that server, in the user's, the Project's and the local settings | Ticket [#172](https://github.com/samfaina/config-digger/issues/172) |
| `:438` `items.mcp.disable[6]`: a plugin missing from every settings file is on | It is off: no server, no skill | Corrected: `default_value = false` on the user's `enabledPlugins` |
| `:506` `locations[project-skills]`: the nearest same-named skill wins | Confirmed | Confirmed |
| `:536` `locations[synced-skills]`: skipped while the sync gate is off | Signed in to claude.ai, Claude Code reads the folder whatever the gate: with the gate cached off it downloads nothing and lists what is there, a skill `manifest.json` does not list included. With an API key it reads none of it | Corrected: no `when` on the gate. Ticket [#308](https://github.com/samfaina/config-digger/issues/308): read only while signed in to claude.ai |
| `:537` `locations[synced-skills]`: read only at `synced/<org>_<account>/<name>` | Read at that depth only, and only in the signed-in account's folder, `<organizationUuid>_<accountUuid>`: a skill directly under `synced/`, in another account's folder or deeper is not read. A sync round that downloads deletes the first two | Confirmed for the depth; ticket [#308](https://github.com/samfaina/config-digger/issues/308) for the account's folder |
| `locations[synced-skills]`, added at `9e628d3`: a synced skill is named by its folder | It is, as `anthropic-skills:<folder>`, its frontmatter `name` an alias with that prefix. It never competes with a local skill: both are listed, and the bare `/<folder>` runs the local one, or the synced one when no local skill takes it. `skillOverrides` turns it off by `anthropic-skills:<folder>` or by `<folder>` | Corrected: `name_template = "anthropic-skills:{name}"`, so it no longer competes. Ticket [#309](https://github.com/samfaina/config-digger/issues/309) for the bare `skillOverrides` key |
| `:560` `items.skill`: names are not rewritten | Confirmed: capitals and `_` are kept | Confirmed |
| `:561` `items.skill`: the 2.1.299 name rule | Pinned, see below | Ticket [#171](https://github.com/samfaina/config-digger/issues/171) |
| `:695` `feature_gates[tengu_claudeai_skills_sync_enabled]`: what the gate does | It turns the sync on: while it is on, Claude Code downloads the account's skills at start, by the value it cached last, then refreshes the cache. With `DISABLE_GROWTHBOOK` set it downloads nothing even with the gate cached on, so its default is off | Corrected: `assumed = false`; the gate is a caveat on synced skills, saying whether Claude Code keeps them in step with claude.ai, and closes nothing |

The name rule, on 2.1.299 to 2.1.309 with `tengu_slate_wicket` at its default:

- A skill folder is skipped when its name starts or ends with a space, or holds U+200B, U+202E, U+00A0 or U+3000. A space inside it, `.`, capitals and letters outside ASCII are kept.
- A frontmatter `name` is refused as an alias when it holds any blank (a space anywhere, a tab, a space next to `:`), `/`, `<`, `>`, U+200B, U+202E or U+00A0, or is over 256 characters. 256 characters, capitals, `_`, `.`, letters outside ASCII and `:` anywhere, leading and trailing included, are kept. The binary's leading and trailing `:` refusal did not show.

### Codex

| Mark | What the probe saw | Outcome |
|---|---|---|
| `:63` `mirrored_homes[orca]`: Orca's copy follows `~\.codex` | Not tested: Orca's own behaviour | Pending: Orca |
| `:94` `projects`: archived sessions are not kept by date | `codex archive` moves the rollout to `archived_sessions\rollout-*.jsonl`, keeping its `cwd` | Confirmed |
| `:175`, `:176` `locations[cloud-mcp]`: an expired or unparsable bundle cache | Not tested: the bundle is signed and comes with an enterprise sign-in | Pending: needs an enterprise account |
| `:219` `locations[plugin-mcp]`: unparsable plugin `.mcp.json` | Codex starts without those servers and keeps the plugin's skills | Corrected: `ignores_file` |
| `:233` `locations[remote-plugin-mcp]`: the same in a plugin the account installed | Not tested | Pending: needs an account with remote plugins |
| `:271` `items.mcp.disable[0]`: a higher `enabled = true` wins | Confirmed, and a higher `false` over a lower `true` | Confirmed |
| `:272` `items.mcp.disable[0]`: Codex starts with a table holding `enabled` alone | It stops with "invalid transport" in every folder where no other layer declares the server. Disable's fallback wrote such a table in the user's `config.toml` for a git-versioned Project layer's server, which stopped Codex in every other folder | Corrected: the fallback is gone, so such a server has no Disable; ticket [#297](https://github.com/samfaina/config-digger/issues/297) to give one back |
| `:287` `items.mcp.disable[1]`: an account plugin honours its servers' switch | Not tested | Pending: needs an account with remote plugins |
| `:302` `items.mcp.disable[2]`: a plugin missing from `config.toml` is on | It is off, with its servers and skills | Corrected: `default_value = false` |
| `:341` `items.mcp.rules[0]`: requirements on args or URL patterns | Codex reads `identity = { command = { executable, args = [<matcher>] } }` and `identity = { url = <matcher> }`, a matcher being exact, prefix or a regex of the whole value; a regex that does not compile stops Codex. The plain `{ command }` form ignores args | Ticket: config-digger reads the matchers ([#141](https://github.com/samfaina/config-digger/issues/141)); that a regex that does not compile stops Codex, which the core cannot say yet, needs a ticket, not filed yet |
| `:384` `locations[project-agents-skills]`: trust does not apply | `.agents\skills` is read in an untrusted Project | Confirmed |
| `:474` `items.skill`: skills at any depth | Skill folders are found up to 6 folders below a root, not 7 | Ticket [#298](https://github.com/samfaina/config-digger/issues/298) |
| `:475` `items.skill`: an unreadable `SKILL.md` is skipped | A `SKILL.md` that is not UTF-8 or cannot be opened is skipped | Confirmed |
| `:493` `items.skill.disable[0]`: the spelling of a canonical path | A rule's path matches in any case, with either slash, with or without `\\?\` | Confirmed |

### Gemini CLI

| Mark | What the probe saw | Outcome |
|---|---|---|
| `:93` `plugins[extensions]`: two extensions with one name are both refused | Gemini CLI does not start at all: "Extension with name … already was loaded" | Ticket [#300](https://github.com/samfaina/config-digger/issues/300) |
| `:192` `locations[extension-mcp]`: unparsable manifest | The extension is skipped with a warning; the others load | Corrected: `ignores_file` |
| `:278` `items.mcp.disable[1]`: the drive letter is upper case | Gemini CLI spells it as the folder it starts in is spelled: upper case from PowerShell, lower case after `cd /d e:\…` in cmd, and compares with regard to case | Corrected: the comment says so; config-digger keeps upper case, PowerShell's spelling |
| `:279` `items.mcp.disable[1]`: a rule on another drive reaches the folders below | Confirmed for a folder and for the drive's root | Confirmed |

### Claude Desktop

Tested on 3.6556.2, the version claude.ai served on 2026-10-23, in Windows Sandbox. Nobody signs in, and the app starts no MCP server before someone does, so the probe reads which `claude_desktop_config.json` the app read from the dialog naming the entries it skips.

| Mark | What the probe saw | Outcome |
|---|---|---|
| `:8` (top): a classic `%APPDATA%\Claude` beside a package's `LocalCache` takes precedence for the packaged app too | The packaged app's data folder is `%APPDATA%\Claude`, with its `LocalCache` laid over it file by file. Started beside a classic folder, it reads the classic files and creates no `LocalCache`; once its `LocalCache` holds a `claude_desktop_config.json`, it reads that one and not the classic file | Ticket [#306](https://github.com/samfaina/config-digger/issues/306): a lookup order picks a whole folder. It keeps picking the classic one, which is right until the package holds a file of its own |
| `:9` (top): where the classic installer puts the app | Squirrel installs for the user in `%LOCALAPPDATA%\AnthropicClaude`: a launcher `claude.exe` with the app's version, `Update.exe`, and `app-<version>\claude.exe`, which runs the app. The uninstall key is `HKCU\…\Uninstall\AnthropicClaude`. The classic app reads `%APPDATA%\Claude` | Corrected: a `classic` Installation, and its process path |
| `:234`, `:280` | Not tested | Pending: `:234` needs a deployment for a third-party provider; `:280` needs the app's windows driven, signed in |

### VS Code

The MCP files, tested with `vscode.mjs` (#311) on 1.141.0, the installed version, and on 1.129.1 and 1.135.0 from their zip archives, which all behave alike. A file VS Code cannot parse holds `,,` between two servers, which no JSON parser accepts, and each such launch keeps a server in two other files.

| Mark | What the probe saw | Outcome |
|---|---|---|
| `:90` `plugins[agent-plugins]`: each plugin sits directly in `~\.vscode\agent-plugins`, with `plugin.json` at its root, in `.github\plugin` or `.claude-plugin` | VS Code reads the plugins `agent-plugins\installed.json` lists, each `pluginUri` a plugin's folder; the probe puts them where VS Code's code clones a marketplace, `<host>\<owner>\<repo>\<source>`, which no real install checked. `plugin.json` at the root, in `.github\plugin`, `.claude-plugin` or `.plugin`, or none, all load. A plugin folder directly in `agent-plugins` that `installed.json` does not list is not read | Corrected: `.plugin\plugin.json` marks a plugin too. Ticket [#321](https://github.com/samfaina/config-digger/issues/321) for the rest: no registry block reads a JSON list of folder URIs |
| `:142` `locations[folder-mcp]`: unparsable `.vscode\mcp.json` | Skipped; the other files load; the file is left as it is | Corrected: `ignores_file` |
| `:160` `locations[dot-mcp]`: references | `${NAME}`, `${env:NAME}` and `$NAME`, in `args` and `env`, reach the server unchanged | Confirmed |
| `:161` `locations[dot-mcp]`: unparsable `.mcp.json` | Skipped; the other files load | Corrected: `ignores_file` |
| `:177` `locations[workspace-mcp]`: unparsable `.code-workspace` | Its servers are skipped, but VS Code opens its folders, reads their files, and applies its other settings, such as `chat.mcp.discovery.enabled`, read around the error | Corrected: `ignores_file` for the servers; ticket [#322](https://github.com/samfaina/config-digger/issues/322) for the folders and settings config-digger cannot read |
| `:201` `locations[user-mcp]`: unparsable profile `mcp.json` | Skipped; the other files load; the file is left as it is | Corrected: `ignores_file` |
| `:221` `locations[extension-mcp]`: how the cache spells a launch | `env` an object; `headers` `[name, value]` pairs; `args` an array; `uri` a URI object with `scheme`, `authority`, `path`, `query` and `fragment`, and from 1.135.0 `external`, the whole URL with the `=` and `&` of its query and fragment encoded | Corrected: header names and the URI's `scheme` and `path` are shown; env and header values, args, and the URI's `authority`, `query`, `fragment` and `external` are Secrets |
| `:239` `locations[plugin-mcp]`: servers in the `.mcp.json` at the plugin's root | Confirmed for every manifest layout; a manifest's `mcpServers` naming a file adds that file's servers too | Confirmed; the named file is in ticket [#321](https://github.com/samfaina/config-digger/issues/321) |
| `:240` `locations[plugin-mcp]`: unparsable plugin `.mcp.json` | VS Code reads it around the error and starts both servers | Ticket [#322](https://github.com/samfaina/config-digger/issues/322); `unknown` stays, so the Items it reaches keep a caveat |
| `:258` `locations[claude-desktop-mcp]`: which settings hold `chat.mcp.discovery.enabled` | The user's turn a source on. A folder opened alone reads its `.vscode\settings.json`, over the user's; a workspace file reads its `settings`, over the user's, and never a member folder's `.vscode\settings.json` | Ticket [#323](https://github.com/samfaina/config-digger/issues/323): the core reads a member folder's settings first |

### Cursor, and the rest of VS Code

Pending: these 27 marks need the apps' windows driven, on throwaway profiles. Cursor `:109`, `:110`, `:145`, `:146`, `:170`, `:194`, `:211`, `:234`, `:235`, `:254`, `:299`, `:595`, `:596`, `:700`, `:709`, `:720`, `:721`; VS Code `:8`, `:385`, `:417`, `:444`, `:473`, `:474`, `:503`, `:601`, `:615`, `:616`.

## Found beside the marks

- **Claude Code names a skill by its folder**, on every version tested from 2.1.286 to 2.1.309, against its docs and the definition, which named it by its frontmatter. The folder is the name skills compete by: a user skill in `fold-b` shadows a Project skill in `fold-b` whatever their `name`s. The frontmatter `name`, when the name rule lets it, is an alias: `/fold-a` and `/<name>` both run the skill. `skillOverrides` turns a skill off by its folder, so Disable wrote a key Claude Code never read for a skill whose `name` differs from its folder. A plugin's skill is `<plugin>:<folder>`, with `<plugin>:<name>` as an alias. Corrected: every Claude Code skill location is `name_from = "folder"`. This answers [#105](https://github.com/samfaina/config-digger/issues/105). One consequence: a skill folder Claude Code and VS Code both read is now one name in both, which `show` cannot pick between ([#301](https://github.com/samfaina/config-digger/issues/301)).
- **Claude Code approvals need trust**: see `:290`, ticket [#293](https://github.com/samfaina/config-digger/issues/293).
- **Claude Code reads a `directory` marketplace's plugin from the marketplace's folder**, not from the cache config-digger reads: ticket [#296](https://github.com/samfaina/config-digger/issues/296).
- **Codex reads a frontmatter that is not YAML line by line**: `name: [unclosed` names a skill `[unclosed`. config-digger skips such a skill, for Codex and Gemini CLI alike: ticket [#299](https://github.com/samfaina/config-digger/issues/299).
- **Codex has `codex plugin remove <id>`** in 0.159.3, which takes out the plugin's cache folder and its table in `config.toml`. The definition said Codex had no such command. Corrected: it is the plugin's `uninstall` command.
- **`gemini mcp list` lists the servers of an extension that is off** in the folder it runs in. Only `gemini extensions list` says whether it is on.
- **Claude Code never reads `managedMcpServers` from the HKCU policy**, though it applies the value's other settings when no admin source holds any. config-digger lists such a server as Effective: ticket [#303](https://github.com/samfaina/config-digger/issues/303).
- **Claude Code drops a `managedMcpServers` entry with a command, or at an `http://` URL**, and loads the others: only remote servers at an `https://` URL load. The definition's shape cannot say so: ticket [#304](https://github.com/samfaina/config-digger/issues/304).
- **Claude Desktop 3.x keeps its MSIX package's data in `%LOCALAPPDATA%\Claude-Data\<family>`**, leaving a `.user-data-left.<family>` marker in the package's `LocalCache`. On the reference machine, with 3.6556.2.0, config-digger finds no Claude Desktop home at all. A fresh install in Windows Sandbox did not move over several launches, so when it moves is not known yet: ticket [#305](https://github.com/samfaina/config-digger/issues/305).
- **Skills sync works on a Max account.** It does not need Teams or Enterprise: the owner's Max account (`organizationType: claude_max`) syncs the Anthropic example skills, so the `synced-skills` marks were tested and do not fall under the exception.
- **Claude Code does not download again a synced skill deleted by hand** while the account's folder holds `.last-complete-round`. Without that file, the next run with the gate on downloads every skill again. The probe clears the copy that way before each gate check.
- **`syncClaudeAiSkills: false` in the user's settings** stops the reading, as the rule says, and Claude Code deletes the skills it downloaded, `manifest.json` included. A skill planted in the account's folder stays.
- **Claude Desktop starts no MCP server before someone signs in**, but reads `claude_desktop_config.json` at start: an entry it cannot use is named in a dialog, "Some MCP servers couldn't be loaded", which is what `claude-desktop.mjs` reads.
- **VS Code reads `.mcp.json` with `chat.mcp.workspaceRootConfig.enabled` off.** The setting decides only whether VS Code adds a server to `.mcp.json` or to `.vscode\mcp.json`, so `dot-mcp` needs no `when`. This answers the research's open question on that gate.
- **VS Code reads `chat.mcp.collisionBehavior` for the window**, as it reads `chat.mcp.discovery.enabled` [binary]: in a workspace file, a member folder's value is ignored, while config-digger reads it first. Not tested; [#323](https://github.com/samfaina/config-digger/issues/323) covers both.

## What is left

Marks are named by their table, as `cargo xtask unverified` prints them; line numbers drift. Each row still to test has a child ticket of #80, and `docs/agents/real-agent-probes.md` is how to work one.

| Needs | Marks | Ticket |
|---|---|---|
| Server-managed settings: Claude Teams or Enterprise | Claude Code `settings[managed]`, `locations[managed-settings-mcp]` (a `remote-settings.json` that does not parse) | Under the exception |
| A Codex enterprise account, with remote plugins | Codex `locations[cloud-mcp]` (2), `locations[remote-plugin-mcp]`, `items.mcp.disable[1]` | Under the exception |
| A deployment of Claude Desktop for a third-party provider | Claude Desktop `items.mcp.rules[1]` | Under the exception |
| The same, and a GitHub account | VS Code's stores, policy and skills (10) | [#316](https://github.com/samfaina/config-digger/issues/316) |
| Cursor's windows, on a throwaway profile | Cursor's MCP files and plugins (10) | [#312](https://github.com/samfaina/config-digger/issues/312) |
| The same, and maybe an account for its gates | Cursor's Disabled store, skills and gates (7) | [#317](https://github.com/samfaina/config-digger/issues/317) |
| The app signed in, after [#305](https://github.com/samfaina/config-digger/issues/305) | Claude Desktop `locations[mirrored-skills]` | [#313](https://github.com/samfaina/config-digger/issues/313) |
| Orca | Codex `mirrored_homes[orca]` | [#314](https://github.com/samfaina/config-digger/issues/314) |
| A throwaway plugin with a `.mcpb` bundle and a `skills` field | Claude Code `locations[plugin-manifest-mcp]` (the bundle cache), `locations[plugin-skills]` (a declared folder holding a `SKILL.md`), both from [#115](https://github.com/samfaina/config-digger/issues/115) | To file under [#80](https://github.com/samfaina/config-digger/issues/80) |
| A throwaway profile with an extension updated, and one with a built-in extension's id installed | VS Code `plugins[built-in-extensions]` (2: the built-ins ranked first, and an installed extension with a built-in one's id), `plugins[extensions]` (the shape of `.obsolete`), all from [#127](https://github.com/samfaina/config-digger/issues/127) | To file under [#80](https://github.com/samfaina/config-digger/issues/80) |

Each mark of the three rows under the exception is written as a table of its `sentence` and what its test `needs`. The release workflow runs `cargo xtask unverified --gate` before the build and fails, listing them, while any other mark is left; the release notes list each mark under the exception with what its test needs ([#315](https://github.com/samfaina/config-digger/issues/315)).
