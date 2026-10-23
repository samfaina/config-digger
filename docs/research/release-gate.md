# Release gate: the unverified marks against the real Agents

Issue: [#80](https://github.com/samfaina/config-digger/issues/80). Terms follow [`CONTEXT.md`](../../CONTEXT.md).

Before v0.1 ships, every Unverified mark of the built-in definitions is tested against the real Agent on the reference machine (Windows 11 Pro, home `C:\Users\kgon1`, a second drive `E:`). Each mark is then removed, or the rule is corrected, with the evidence in the definition as a `[test]` tag and here. This file covers the three CLI Agents, tested on 2026-10-23. Claude Desktop, Cursor and VS Code, which need their windows driven, come next.

## The probes

A probe per Agent in `uat/agents/` builds throwaway homes and Projects, runs the real Agent on them and checks what it loads:

```powershell
node uat/agents/claude-code.mjs [--bin <claude.exe>]
node uat/agents/codex.mjs       [--bin <codex.exe or codex.cmd>]
node uat/agents/gemini-cli.mjs  [--bin <gemini.cmd>]
```

Each check names the table of the definition it tests, as `cargo xtask unverified` prints it, and passes when the Agent does what the definition now says. A probe exits 1 when a check fails. `CD_PROBE_VERBOSE=1` prints what each check saw, and `CD_PROBE_JSON=<file>` writes the results. Run a probe again with each new version of its Agent: when every check passes, the version can join the definition's `verified` range.

What keeps a probe off the user's own configuration:

- Everything sits under `CD_PROBE_ROOT`, `E:\cd-throwaway-agents` by default, whose name must start with `cd-throwaway`. It is outside the user's home folder on purpose: Claude Code reads `CLAUDE.md` and `.mcp.json` in every folder above the one it starts in, so a Project under `C:\Users\<me>` would pick up the user's own files.
- Each Agent gets a throwaway home: `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, or `GEMINI_CLI_HOME` in the temp folder. Gemini CLI's home is on the system drive and its Projects on `E:`, so that a rule for the home folder and one for a Project sit on different drives.
- Servers are `cmd /c <tag>`, which exits at once, or an HTTP URL on a closed local port. Entries are `cd-throwaway-*` and the only key is `not-a-real-secret`.
- No request reaches a model. Claude Code's Messages API is a local server in the probe, which keeps each request so the probe can read the skills Claude Code lists to its model. Codex and Gemini CLI point at a closed local port.
- Claude Code still fetches its GrowthBook features at start, without an account. Older Claude Code versions come from npm into the probe root (`npm install --prefix E:\cd-throwaway-agents\cc-<version> @anthropic-ai/claude-code@<version>`).

How each probe reads what the Agent loaded:

| Agent | Oracle |
|---|---|
| Claude Code | `claude mcp list` (approval status, parse diagnostics), `claude mcp get`, `claude plugin list`, the `init` event of `claude -p --output-format stream-json --verbose` (servers, skills, plugins), and the request Claude Code sends its model: the skill listing, `- <name> (<alias>): <description>`, and the body of a skill a `/<name>` prompt runs |
| Codex | `codex mcp list`, `codex debug prompt-input` (the skills its model gets), the files `codex exec` and `codex archive` write |
| Gemini CLI | `gemini extensions list` (`Enabled (Workspace)` for the folder it starts in), `gemini mcp list`, `gemini -p` |

## Versions

| Agent | Versions every check passes on | `verified` |
|---|---|---|
| Claude Code | 2.1.299, 2.1.300, 2.1.302, 2.1.304, 2.1.308, 2.1.309 | 2.1.299 to 2.1.309 (was 2.1.286 to 2.1.302) |
| Codex | 0.159.3 | unchanged |
| Gemini CLI | 0.62.0 | unchanged |

Claude Code 2.1.286 fails 7 checks, all on what 2.1.299 changed: a plain-string `enabledMcpjsonServers` or `disabledMcpjsonServers` voids its settings file, there is no name rule, and the skill listing gives no alias. The definition describes 2.1.299 and later, so the range starts there.

## The marks

The inventory is the 80 marks of `main` at `5cb2ad5` (`cargo xtask unverified`): 20 in Claude Code, 15 in Codex, 4 in Gemini CLI, 4 in Claude Desktop, 17 in Cursor and 20 in VS Code. The skill manager definitions hold none. Line numbers are those of `5cb2ad5`.

Outcomes:

- **Confirmed**: the Agent does what the mark assumed. The mark is gone and the fact carries `[test]`.
- **Corrected**: the Agent does something else, and the definition now says what. The mark is gone.
- **Ticket**: the Agent does something else that the definition format or the core cannot express yet. The mark is gone, the definition states the fact with `[test]` and names the ticket.
- **Pending**: not tested yet. The mark stays, and the reason says what the test needs.

### Claude Code

| Mark | What the probe saw | Outcome |
|---|---|---|
| `:86` `settings[managed]`: `remote-settings.json` is one JSON object | With an API key, Claude Code does not read `remote-settings.json` at all | Pending: needs an account with server-managed settings |
| `:140` `locations[managed-mcp]`: unparsable `managed-mcp.json` | Not tested: the file sits in `%ProgramFiles%` | Pending: Windows Sandbox |
| `:155` `locations[managed-settings-mcp]`: unparsable managed settings | Not tested: the file sits in `%ProgramFiles%`, the registry values in `HKLM` or in the user's `HKCU` | Pending: Windows Sandbox |
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
| `:536` `locations[synced-skills]`: skipped while the sync gate is off | With an API key the synced folder is not read even with the gate on | Pending: needs a claude.ai account |
| `:537` `locations[synced-skills]`: read only at `synced/<org>_<account>/<name>` | As above | Pending: needs a claude.ai account |
| `:560` `items.skill`: names are not rewritten | Confirmed: capitals and `_` are kept | Confirmed |
| `:561` `items.skill`: the 2.1.299 name rule | Pinned, see below | Ticket [#171](https://github.com/samfaina/config-digger/issues/171) |
| `:695` `feature_gates[tengu_claudeai_skills_sync_enabled]`: what the gate does | As for `:536` | Pending: needs a claude.ai account |

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
| `:341` `items.mcp.rules[0]`: requirements on args or URL patterns | Not tested: `requirements.toml` sits in `%ProgramData%` | Pending: Windows Sandbox |
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

### Claude Desktop, Cursor and VS Code

Pending: these 41 marks need the apps' windows driven, on throwaway profiles. Claude Desktop `:8`, `:9`, `:234`, `:280`; Cursor `:109`, `:110`, `:145`, `:146`, `:170`, `:194`, `:211`, `:234`, `:235`, `:254`, `:299`, `:595`, `:596`, `:700`, `:709`, `:720`, `:721`; VS Code `:8`, `:90`, `:142`, `:160`, `:161`, `:177`, `:201`, `:221`, `:239`, `:240`, `:258`, `:385`, `:417`, `:444`, `:473`, `:474`, `:503`, `:601`, `:615`, `:616`.

## Found beside the marks

- **Claude Code names a skill by its folder**, on every version tested from 2.1.286 to 2.1.309, against its docs and the definition, which named it by its frontmatter. The folder is the name skills compete by: a user skill in `fold-b` shadows a Project skill in `fold-b` whatever their `name`s. The frontmatter `name`, when the name rule lets it, is an alias: `/fold-a` and `/<name>` both run the skill. `skillOverrides` turns a skill off by its folder, so Disable wrote a key Claude Code never read for a skill whose `name` differs from its folder. A plugin's skill is `<plugin>:<folder>`, with `<plugin>:<name>` as an alias. Corrected: every Claude Code skill location is `name_from = "folder"`. This answers [#105](https://github.com/samfaina/config-digger/issues/105). One consequence: a skill folder Claude Code and VS Code both read is now one name in both, which `show` cannot pick between ([#301](https://github.com/samfaina/config-digger/issues/301)).
- **Claude Code approvals need trust**: see `:290`, ticket [#293](https://github.com/samfaina/config-digger/issues/293).
- **Claude Code reads a `directory` marketplace's plugin from the marketplace's folder**, not from the cache config-digger reads: ticket [#296](https://github.com/samfaina/config-digger/issues/296).
- **Codex reads a frontmatter that is not YAML line by line**: `name: [unclosed` names a skill `[unclosed`. config-digger skips such a skill, for Codex and Gemini CLI alike: ticket [#299](https://github.com/samfaina/config-digger/issues/299).
- **Codex has `codex plugin remove <id>`** in 0.159.3, which takes out the plugin's cache folder and its table in `config.toml`. The definition said Codex had no such command. Corrected: it is the plugin's `uninstall` command.
- **`gemini mcp list` lists the servers of an extension that is off** in the folder it runs in. Only `gemini extensions list` says whether it is on.

## What is left

| Needs | Marks |
|---|---|
| Windows Sandbox, for files under `%ProgramFiles%` and `%ProgramData%` | Claude Code `:140`, `:155`; Codex `:341` |
| A claude.ai account, with server-managed settings for `:86` | Claude Code `:86`, `:536`, `:537`, `:695`, and the new `locations[synced-skills]` mark that a synced skill is named by its folder |
| A Codex enterprise account, with remote plugins | Codex `:175`, `:176`, `:233`, `:287` |
| Orca | Codex `:63` |
| The apps' windows | The 41 marks of Claude Desktop, Cursor and VS Code |
