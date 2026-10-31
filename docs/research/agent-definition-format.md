# Agent definition format: validation evidence

Evidence for [Agent definition format](https://github.com/samfaina/config-digger/issues/21). The decisions themselves live in the ticket's resolution comment and in ADR 0005. This file holds the material behind them:

1. The draft catalog of building blocks as it was before validation. The ticket resolution amends it.
2. Facts about Zed and Windsurf / Devin Desktop, gathered for the acceptance test on 2026-10-01. Zed's source was checked at `zed-industries/zed` commit `95cd535a5fad`. Zed's local trust database was also inspected on the reference machine: its schema is `trusted_worktrees(trust_id, absolute_path, user_name, host_name)`, with one row per trusted worktree in native spelling, and the registry is `workspaces(paths)`.
3. One validation report per v0.1 Agent, plus skill managers, mapping that Agent onto the draft catalog. Each report gives fits, gaps, corrections and facts to mark `unverified`. The gaps were accepted as generic catalog changes (see the resolution). Each report's *Unknowns* list is the starting point for the `unverified` marks in that Agent's definition.

---

## 1. Draft catalog (pre-validation)


Settled decisions this catalog must respect:
- An Agent definition is a TOML file (one per Agent) of data that names building blocks ("primitives") implemented in the Rust core, each with parameters. A new Agent that only uses existing primitives needs no code. Definitions ship inside the app; a `--definitions <dir>` flag loads extra ones for dev/tests. Each file has `format_version`.
- Skill manager definitions are separate files (one per skill manager: npx skills, skillfish, openskills, Claude Code synced skills?) whose detectors reclassify the Provenance of any skill Entry.
- Shared locations: each definition declares its own reads independently; the core identifies an Entry by real path + key path and computes the reach of a write by finding every definition that reads that file+key. No cross-references between definitions.
- Precedence is ordered by **location** (or group of locations with a tie-break rule), not by Scope. Scope is a per-Agent label (name + display order).
- Each location maps its fields onto a common server model (transport, command, args, env, url, headers, cwd) with a transport-detection primitive; unmapped fields are kept as "other".
- Secret masking: core defaults on the common model (env and header values, URL userinfo/query, args after --token/--api-key-like flags); definitions add Agent-specific extra secret fields.
- Process detection per Agent: image names, optional path glob, optional cmdline_contains, optional data_folder_switch (the switch through which a process names the data folder it reads, which tells whether it reads a home the user added), optional session files with PID (valid only if the PID is alive). Each writable location has a store_class: sqlite | rewritten-while-running | plain.
- Definitions carry an optional `verified` version range and per-mechanism `unverified` marks; no version-conditional variants in v0.1.
- Definitions carry no free text for users; mechanisms and gates map to closed reason codes (shadowed_by_precedence, disabled_by_name, parked, disabled_by_rule, disabled_globally, untrusted_folder, not_approved, policy, lookup_order_skips_home, project_missing, transport_conflict, …) and note codes.
- Versioned (git index) detection is done by the core for every Entry, never declared.

### Skeleton of one Agent definition

- `[agent]` id, format_version, verified range, item kinds
- `[process]` images[], path_glob?, cmdline_contains?, data_folder_switch?, session_files?
- `[[homes]]` id, named roots (a home may have several roots, e.g. config dir + data dir), locator, lookup order; `[[mirrored_homes]]` owner tool, path, source home
- `[[scopes]]` id, display order
- `[projects]` registries[], walk
- `[[locations]]` id, anchor, path, format, root, item kind, scope, base provenance, store_class, field map + transport, extra secret fields, conditions (`when`)
- `[items.<kind>]` precedence (ordered location ids / groups + tie-break), competition rules (per Provenance pair), mode (whole | field-merge + merge primitive), invalid detectors, check order, disable mechanisms (ordered), gates, notes
  - An item of `precedence` is a location id or a group of two or more, `[["a", "b"], "c"]`: the group's locations rank together, at the group's place. Of two same-Item Entries of one group, the one whose folder comes first by path wins, name by name from the root down, case aside, and of two found in one folder, the one whose location comes first in the group. An Entry's folder is the one the walk down found holding its location's path (`app\x` for `app\x\.agents\skills`). Every other Entry is in its anchor's own folder, taken as one folder for them all, ahead of those a walk found, so between two such Entries the order in the group decides alone. Where a location ranks without two folders to compare, such as the one Entry kept when several locations reach one file, Disable's choice, merges and the other rules that read `precedence`, the group's ids rank as if written out flat. Each location appears once, groups included; a group of one and a group inside a group are rejected.
- `[[feature_gates]]` name, assumed default, affected locations/rules (caveat on affected Entries)

### Primitive families

1. **Path expansion and home locators**: `%ENV%`/`~` expansion; `env-override` (CODEX_HOME, GEMINI_CLI_HOME); `glob` (MSIX `Packages\Claude_*`); `lookup-order` (first existing wins, others Unread with lookup_order_skips_home); `homes-from-json-list` (VS Code profiles from a JSON file listing them); user-added homes (core).
2. **Location anchors and enumeration**: `home:<root>`, `project`, `system` (%ProgramData%); `each-dir` (glob directories, e.g. extensions/plugins installed folders); `json-registry` (enumerate installed plugins from a JSON registry file, then a relative path inside each).
3. **Formats**: `json`, `jsonc`, `toml`, `skill-folder` (folder with SKILL.md + frontmatter), `manifest` (JSON or TOML manifest of an extension/plugin that contributes Entries); `sqlite-value` (read-only Entries from a JSON value stored in an SQLite row, e.g. a runtime cache). `root` is a key path that may contain `{project}` (project-keyed roots like `projects.{project}.mcpServers`) with a path spelling.
4. **Project registries and path spelling**: `json-keys`, `toml-table-keys`, `json-list`, `sqlite-row` (generic: table, match column, optional value column, optional JSON path; also lists rows, e.g. a `paths` column split by newline); spellings `native`, `forward-slash`, `file-uri`, `slash-drive` (`/E:/`). Project walk: `folder-only`, `repo-root-to-folder`, `workspace-members` (.code-workspace), `folder-and-subfolders`.
5. **Field map and transport detection**: `by-field-presence` (command vs url), `by-type-field` (`type`/`transport` field with value table).
6. **Competition rules**: `exact-name`, `case-insensitive-name`, `endpoint`, `skill-name` (frontmatter name), `path-key`, `never`; each applies between given Provenance/location groups. Tie-breaks: `first-loaded`, `deeper-wins`, `path-key-order`.
7. **Merge and invalid detection**: mode `whole` | `field-merge` with merge primitives `deep-merge-table` (Codex layers), `overlay-fields` (a settings Entry overlays fields of a same-named extension/plugin server); invalid detectors `transport-conflict`, `duplicate-manifest-name`.
8. **Conditions (`when`)**: `setting-equals` (a key in some location has a value, with a default — e.g. VS Code `collisionBehavior`, Cursor import switch), `file-owner-admin` (Gemini CLI system files count only when admin-owned), `feature-gate` (assumed default + caveat), `platform` (reserved for v0.2).
9. **Disable mechanisms** — each with target location, reach (`entry` | `name` | `project` | `global` | `rule`), enable_offered, unverified, reason code; ordered by preference (the one the Agent's UI writes first):
   - `json-name-list` (add the name to a list; e.g. `disabledMcpServers`)
   - `json-field` (set a field at a key-path template to a value; optional insert-if-missing template; e.g. `isEnabled`, `enabledPlugins.<id>`, `skillOverrides.<name> = "off"`)
   - `toml-field` (e.g. `enabled = false`), `toml-array-entry` (e.g. `[[skills.config]]` path + enabled)
   - `sqlite-json` (read-modify-write a JSON value in an SQLite row; e.g. a list of disabled ids or an enablement map), with an id template (Cursor `user-<name>`, `project-<idx>-<folder>-<name>`)
   - `side-file-json` (separate JSON file keyed by name or by path rules)
   - `park` (config-digger moves the Entry to its store)
   - read-only rules for reason codes only (Enable not offered): `allow-deny-list`, `global-switch`
10. **Gates** (Blocked): `trust-store` via `json-keys` / `toml-table-keys` / `sqlite-row` lookups with ancestor matching option; `approval-list` (named approvals / approve-all flag); `approval-hash` (approval bound to a hash of the server config — algorithm is code); `path-rules` (glob include/exclude rules per Project); `managed-exclusive` (presence of a managed file disables other Scopes); `registry-policy` (Windows registry value); each maps to a reason code.
11. **Provenance**: base Provenance per location (`file`, `plugin`, `extension`, `managed`, `cloud-mirror`); skill manager detectors in Skill manager definitions: `link-target-glob`, `marker-file`, `lock-file-entry`.
12. **Notes**: partial switches map to note codes (e.g. `skillOverrides: name-only`, `disabled_tools`, `disable-model-invocation`).

### Open: Zed acceptance walk (Zed may use only primitives the six v0.1 Agents also need)

Zed: config `%APPDATA%\Zed` (settings.json, global_settings.json, JSONC, root `context_servers`), data `%LOCALAPPDATA%\Zed`; project `.zed/settings.json`; extensions `%LOCALAPPDATA%\Zed\extensions\installed\<id>\extension.toml` contributing context servers; untagged enum (command → stdio, url → http, settings-only → extension settings); `enabled` field default true; layers replace a same-named enum value whole; Disable UI writes `enabled:false` to user settings.json, inserting an Extension stub when missing; trust in SQLite `db\0-stable\db.sqlite` table `trusted_worktrees(absolute_path)` one row per trusted worktree, native spelling; workspaces registry in table `workspaces(paths)`; skills `~/.agents/skills`, `<worktree>/.agents/skills`, skill-name competition project > user > built-in, project skills need trust, no Disable, `disable-model-invocation` frontmatter; process `Zed.exe`; `session.trust_all_worktrees` bypasses trust; `disable_ai` global switch.

---

## 2. Zed and Windsurf / Devin Desktop facts

### Local machine

- **Zed is installed.** `%APPDATA%\Zed` (settings.json, AGENTS.md), `%LOCALAPPDATA%\Zed` (db\0-stable, extensions\installed\html, external_agents…), `%LOCALAPPDATA%\Programs\Zed\Zed.exe`.
- **Windsurf is not installed.** No `.codeium`, `.windsurf`, `%LOCALAPPDATA%\Programs\Windsurf` or `%APPDATA%\Windsurf`.

### Windsurf, renamed Devin Desktop (2026-06-02)

docs.windsurf.com redirects to docs.devin.ai/desktop. Source: https://docs.devin.ai/desktop/devin-desktop-faq.md

**The app**
- **Two harnesses in one app.** Legacy **Cascade** (kept "through July") and **Devin Local**, the default, which uses the Devin CLI harness. Each has its own config model.
- **Install and process.** `Devin.exe` (formerly Windsurf.exe), installed in `C:\Program Files\Devin` or `%LOCALAPPDATA%\Programs\Devin`. The CLI is `%LOCALAPPDATA%\devin\bin\devin.exe`.
- **Editor data and extensions.** Data in `%APPDATA%\Devin\` (legacy `%APPDATA%\Windsurf\`). Extensions in `~/.devin/extensions` (legacy `~/.windsurf/extensions`).

**Cascade MCP** (https://docs.devin.ai/desktop/cascade/mcp.md)
- **File and format.** `%APPDATA%\devin\mcp_config.json`, JSON, root `mcpServers`. Stdio entries use `command`/`args`/`env`. Remote entries use `serverUrl` or `url` plus `headers`. Transport is inferred from which fields are present.
- **Disable.** `disabledTools` turns off individual tools. There is a per-server UI toggle, but where it stores the state is not documented. There is no project file.
- **Docs disagree on the path.** The FAQ says `~/.codeium/mcp_config.json`. The JetBrains plugin page says `~/.codeium/windsurf[-next]/mcp_config.json` is a separate file used by `chat.mcp.discovery`.
- **Admin controls.** An MCP on/off switch and an anchored regex allowlist. Org-level overrides replace lists instead of merging them (https://docs.devin.ai/enterprise/local/controls.md).

**Devin Local / Devin CLI MCP** (https://cli.devin.ai/docs/extensibility/mcp/configuration.md)
- **Scopes.** User `%APPDATA%\devin\mcp_config.json`, project `.devin/mcp_config.json`, local `.devin/mcp_config.local.json`. JSON with comments, root `mcpServers`.
- **Disable.** `"disabled": true`, written by `devin mcp disable [-s scope]`.
- **Reads other Agents' config.** It imports the MCP config of Claude Code, Cursor, Windsurf, OpenCode and Zed. Each import can be turned off with `read_config_from.<tool>: false`.
- **Precedence across Scopes** is not documented.

**Skills**
- **Cascade folders.** Project `.devin/skills` and `.windsurf/skills`. Global `~/.codeium/windsurf/skills` or `~/.config/devin/skills`. Also reads `.agents/skills`, plus `.claude/skills` when importing Claude config. System skills in `C:\ProgramData\Devin\skills`.
- **Not documented:** which skill wins on a name clash, and how to disable one skill.

**Plugins and trust**
- **Plugins** bundle skills, MCP servers and hooks, recorded in a cloud manifest. Their location on disk is not documented.
- **Trust.** Since v3.6.21, agents are unavailable until the workspace is trusted.

### Zed (source at `zed-industries/zed` @ `95cd535a5fad`)

**Paths** (`crates/paths/src/paths.rs`)
- Config is in `%APPDATA%\Zed`, data in `%LOCALAPPDATA%\Zed`. `--user-data-dir <d>` moves config to `<d>\config`. There is no environment variable override on Windows.
- Settings files: `settings.json`, `global_settings.json`, and project `.zed/settings.json`, which can also sit in subdirectories.

**MCP** (`crates/settings_content/src/project.rs:508`)
- JSONC, root `context_servers`.
- Each entry is an untagged enum: Stdio (`command`, `args`, `env`, `timeout`, `remote`), Http (`url`, `headers`, `timeout`, `oauth`) or Extension (`settings`). Every variant has `enabled`, default true.

**Precedence** (`crates/settings/src/settings_store.rs:1376-1469`)
- Layer order, low to high: defaults < extension settings < `global_settings.json` < user `settings.json` (with release-channel and OS sub-blocks) < active profile < remote-server settings < `.zed/settings.json`. Deeper directories win.
- Maps merge per key, but an enum value is replaced whole.
- In a multi-root workspace, the first worktree that defines a name wins (`context_server_store.rs:1135`).

**Disable**
- The UI toggle writes `enabled` into the user `settings.json`. If the server has no entry there, it first inserts an `Extension{enabled,remote:false,settings:{}}` stub (`crates/settings_ui/src/pages/mcp_servers_page.rs:358`).
- Inferred, not tested: toggling a project-defined server won't stick, because the project entry replaces the stub.
- Other controls: `disable_ai`, agent profiles, `agent.tool_permissions`.

**Skills** (`crates/agent_skills`, `crates/agent/src/agent.rs`)
- Folders: `%USERPROFILE%\.agents\skills` (global), `<worktree>/.agents/skills` (project), plus built-ins.
- Name clash: project beats global beats built-in. Zed does not read `.claude/skills`.
- No setting disables a skill. The only per-skill control is the `disable-model-invocation` frontmatter.
- Project skills load only in a trusted worktree.

**Extensions**
- Installed in `%LOCALAPPDATA%\Zed\extensions\installed\<id>`. Each manifest declares `context_servers`.
- An extension server is on unless a `context_servers.<id>.enabled:false` entry exists.

**Trust** (`crates/project/src/trusted_worktrees.rs`)
- An untrusted worktree's `.zed/settings.json` is not applied at all.
- Trust is stored in Zed's local DB. `session.trust_all_worktrees` bypasses the check.

**Rewrites** (`settings_store.rs:559, 851`)
- Zed rewrites settings atomically, from range-based text edits. Comments outside the edited range survive. Inferred from the code, not tested.

**Docs vs code**
- zed.dev/docs/ai/mcp doesn't mention project scope, `enabled` or trust.

---

## 3. Per-Agent validation reports

### Claude Code

**Fits**
- Agent: kinds mcp and skill, `verified` set to 2.1.286 or later. Scopes are managed, local, project, user, plugin and synced, each with a display order.
- Home roots: `~/.claude` uses `env-override CLAUDE_CONFIG_DIR`; plugins root uses `env-override CLAUDE_CODE_PLUGIN_CACHE_DIR`; `~/.claude.json` is a file root.
- MCP locations, all json with root `mcpServers`:
  - `home:.claude.json` user, and `projects.{project}.mcpServers` local. Both are rewritten while the Agent runs.
  - `project:.mcp.json`, plain.
  - `managed-mcp.json` and the managed settings files, read-only, managed Provenance.
  - `~/.claude/remote-settings.json`, managed.
  - Plugin servers via `json-registry` (`installed_plugins.json`), plugin Provenance.
- Skill locations, all `skill-folder`: managed `.claude/skills`, `~/.claude/skills`, project `.claude/skills` (`repo-root-to-folder`), plugin `skills/`, and `each-dir skills/synced/*/*` as cloud-mirror.
- Project registry: `json-keys` over `.claude.json` `projects`. Entries keyed to a missing Project are Unread with `project_missing`.
- Mode `whole` for both kinds; no invalid detectors.
- MCP competition:
  - managed, local, project and user use `exact-name`, in order managed-mcp.json > managedMcpServers > local > project > user > plugin.
  - plugin against anything else uses `endpoint`.
  - Nested and plugin skills use `never`, since they are namespaced.
- Skill order: enterprise > personal > project > synced.
- **Linked worktrees (#474):** `project-skills`'s anchor sets `linked_worktree = "main-checkout-stands-in"`, which only `walk = "up-to-repo-root"` takes. In a linked worktree, whose `.git` file names `<main>/.git/worktrees/<name>`, the walk still stops at the worktree root, and when that root does not hold the anchor's path, as a file or a folder, the path in the main checkout's root is read too, ranked below every folder of the walk. Its Entries are the worktree Project's, at their path in the main checkout. Without it, or with `"own"`, a linked worktree is read as any repo.
- **Nested skills (#352):** a `project` location, `nested-skills`, reads `.claude/skills` in every folder below the Project folder with the Cursor walk's rules set to Claude Code's: `walk = { down = { needs_git = true, skip = [], ignore_files = [".gitignore"], ignores_path = false, follow_links = true } }`, so that only a `.gitignore` leaving out a folder that may hold `.claude` hides it, as `git check-ignore` on that folder does, and one leaving out `.claude` alone does not. Claude Code loads them outside a git repo too; `needs_git` keeps config-digger from walking a whole home folder or drive. `loaded_on_file_under_folder = true` gives each Entry the Note `loaded_on_file_under_folder`, naming its folder relative to the Project, its State unchanged: Claude Code loads it only once it reads or edits a file below that folder. The skill competition keeps the location apart, `exact-name` with `apart = { locations, on_clash }`: its Entries compete with none, each its own Item, named by `on_clash`, here `{subfolder}:{name}` (`apps/web:deploy`), while another Entry of the kind in the context has its name. Disable writes `skillOverrides` under that name and reads the folder's name as an alias, through `also_names = ["{own_name}"]`.
- Gates:
  - `managed-exclusive` (managed-mcp.json) gives `policy`.
  - `approval-list` (`enabledMcpjsonServers`, approve-all `enableAllProjectMcpServers`) gives `not_approved`.
  - `trust-store json-keys` (`hasTrustDialogAccepted`).
- Disable mechanisms:
  - `/mcp` toggle: `json-name-list projects.{project}.disabledMcpServers`, reach name per project, first in order.
  - `json-name-list disabledMcpjsonServers` in settings.local.json.
  - `json-field skillOverrides.<name>="off"`.
  - `json-field enabledPlugins.<id>=false`.
  - Rules with Enable not offered: `allow-deny-list` for `deniedMcpServers`/`allowedMcpServers` and `permissions.deny Skill(name)`; `global-switch` for `disableClaudeAiConnectors`, `disableBundledSkills`, `syncClaudeAiSkills`, `syncClaudeAiPlugins`.
- Conditions: `setting-equals` for `managedSourcesBehavior` and `allowManagedMcpServersOnly`.
- Process: `claude.exe`. Session files `~/.claude/sessions/<pid>.json`.
- Notes: `skillOverrides` name-only and user-invocable-only, `disable-model-invocation`, the bare-name fallback for plugin skills, `${VAR}` shown unexpanded.
- Extra secret fields: none beyond the core defaults.

**Gaps**
1. **Settings come in layers.** Most switches resolve per key across managed > `--settings` > settings.local > settings > user. Lists concatenate. `enabledPlugins` merges key by key, and a managed value forces it either way. `setting-equals` and the mechanisms each read one location. Fix: a `[[settings_layers]]` primitive with ordered locations and a per-key policy (override, concat, union or per-key). Conditions, approvals, rules and mechanisms would reference a setting key, not a file. The write target is the most specific layer that isn't versioned. If a higher layer overrides the value, that gives Blocked or "not offered". Generic: Gemini CLI, VS Code and Codex layers need it too.
2. **Several spellings per project key.** `E:/x`, `E:\x` and `e:/x` all appear, holding different data. Fix: `{project}` match mode `normalized`, insensitive to case and separator and many-to-one, plus a `write_spelling` parameter. Generic: Codex trust keys, VS Code URIs.
3. **The project walk is per Agent, but differs per location.** Skills use repo-root-to-folder with a fallback to the main checkout in a linked worktree; `.mcp.json` is unknown. Fix: `walk` per location plus a `worktree_fallback` flag. The per-location walk is generic; the fallback flag is Claude Code-specific. Done: the fallback is `linked_worktree` on the Project anchor (#474), and `.mcp.json` takes none.
4. **Managed source selection.** Remote > HKLM > files > HKCU, and only the first source that has a policy key applies. HKCU counts only when no admin source exists. Fix: a location group with `select = first-with-key`, which generalises `lookup-order` to locations. Probably Claude Code-specific.
5. **Registry value holding JSON as a location.** HKLM/HKCU `Policies\ClaudeCode` `Settings`. Fix: a `registry-value` anchor that reuses the json format. Mostly Claude Code; the anchor itself is generic.
6. **No file-glob anchor.** `managed-settings.d/*.json` needs one. Fix: generalise `each-dir` to `each-path` with a kind filter. Generic.
7. **Plugin MCP sources.** The manifest's `mcpServers` can be a path, an inline object, a .mcpb bundle or an array, and later sources win. Registry values are arrays per install scope, with `projectPath` binding a plugin to a Project. Fix: a manifest `ref-or-inline` resolver, `json-registry` parameters `array` and `project_field`, and a `last-loaded` tie-break. Generic: Codex and Cursor plugins share this format.
8. **Disable reach for a whole plugin.** Fix: add reach `container`. Generic: Desktop, Gemini, Codex and Cursor extensions.
9. **Name templates.** `/mcp` writes `plugin:<plugin>:<server>` and `claude.ai <Name>`. Fix: a `name_template` parameter on competition and mechanisms. Generic, since Cursor ids are similar.
10. **Transport needs a type field with a fallback.** `type` is optional when `command` is present; `streamable-http` is an alias of http; `ws` is a value. Fix: a `fallback` parameter on `by-type-field`, an alias table, and `ws` in the common model. Generic: Cursor, Gemini.
11. **Process detection.** Fix: `path_glob` becomes a list (native, npm `node_modules`, WinGet unknown). Add `start_time_field` (`procStartFt`, FILETIME) to the session-file check. The session-file reader must read only declared fields, because these files hold `peerToken`.
12. **Plugins that are skills-dir folders.** A skill folder containing `.claude-plugin/plugin.json` is a plugin. Fix: an `exclude_if_contains` parameter. Claude Code-specific.
13. **Plugin-level precedence.** For plugins with the same manifest name, managed > `--plugin-dir` > marketplace > skills-dir > synced. The losing plugin's Entries should become Shadowed. There is no primitive for container competition. This is Claude Code-specific code, or defer it.
14. **Exclusive control has exemptions.** `managed-exclusive` needs `except = [managedMcpServers]` and should apply to the MCP kind only.

**Corrections**
- The `system` anchor is fixed to %ProgramData%. Claude Code reads `%ProgramFiles%\ClaudeCode` and explicitly ignores the old ProgramData path. Make the base an expandable env path.
- Synced skills are not a skill manager. They belong in a location with cloud-mirror Provenance, two levels deep. The NTFS junctions into `~/.agents/skills` with `.skill-lock.json` are the skill manager, detected with `lock-file-entry` and `link-target-glob`.
- `by-type-field` alone is the wrong transport primitive for Claude Code (see gap 10).
- Exclusive control yields Blocked (`policy`), not "disables other Scopes".
- `json-name-list` and `trust-store` targets must accept a `{project}` key template, with the normalisation from gap 2.

**Unknowns** (to mark unverified)
- Whether `CLAUDE_CONFIG_DIR` moves `.claude.json`.
- Whether the running Agent re-reads `.claude.json` before writing it.
- Which duplicate project key the Agent reads and writes.
- Whether `.mcp.json` is also looked up in parent directories.
- The check order: deny relative to precedence relative to `disabledMcpjsonServers` relative to approval relative to the `/mcp` toggle.
- Whether `disabledMcpServers` really covers `.mcp.json` servers. It does on disk, but the docs don't say so.
- Endpoint equality for `command` (whether args count).
- Whether trust gates `.mcp.json` and project skills.
- Where `enabledMcpjsonServers` is stored: `.claude.json` `projects` or settings.
- The managed skills path, which is derived by joining two doc statements.
- Synced skills sitting one level deeper than the docs say.
- Whether `skillOverrides` applies to managed and synced skills.
- Between project levels, whether the deeper skill wins.
- The WinGet binary path.
- The `ENABLE_CLAUDEAI_MCP_SERVERS` env switch, which can't be observed from disk.
- Legacy `.claude/commands/*.md`: is it in scope for v0.1? It would need a single-file skill format.
### Claude Desktop
**Fits**
- Homes: none from an `env-override`, since signed builds ignore `CLAUDE_USER_DATA_DIR` (#368); a folder given with `--user-data-dir` is a home the user adds. Store and enterprise MSIX homes via `glob` `%LOCALAPPDATA%\Packages\{Claude_*,AnthropicPBC.Claude_*}\LocalCache\Roaming\Claude`. Classic home `%APPDATA%\Claude`. 3P home `%LOCALAPPDATA%\Claude-3p`.
- Scopes: `managed`, `user`. `[projects]` is empty and every State is per (home, none).
- User-file location: `home:data` + `claude_desktop_config.json`, `json` (core strips the BOM), root `mcpServers`, Scope user, Provenance `file`, store_class `rewritten-while-running`. Field map is command/args/env; `extensionId` goes to "other".
- Extension location: `each-dir` `Claude Extensions\*\manifest.json`, `manifest` format, root `server.mcp_config`, Provenance `extension`.
- Skills mirror: `each-dir` `local-agent-mode-sessions\skills-plugin\*\*\skills\*`, `skill-folder`, Provenance `cloud-mirror`, read-only.
- Competition: `exact-name` between user-file and extensions. Precedence is [user-file, extensions], mode `whole`.
- Check order: policy gates (Blocked) → global switches → per-Entry Disable → precedence. A Disabled extension does not compete.
- Disable mechanisms:
  - One extension: `side-file-json` `isEnabled`, reach `entry` (the whole extension).
  - User-file server: `park`, which needs the app closed (issue 20 §11).
  - `features.isLocalDevMcpEnabled` / `features.isDxtEnabled` in the user file: `global-switch`, `disabled_globally`, Enable not offered.
- Gates: the same keys in policy use `registry-policy` → `policy`.
- Process: image `claude.exe` with `path_glob` `C:\Program Files\WindowsApps\{Claude,AnthropicPBC.Claude}_*\app\*`. The glob is required because the Claude Code CLI uses the same image name. `CoworkVMService` must not count, since it always runs.
- Notes: per-tool toggles (`mcp-user-tool-toggles.json`) and managed `toolPolicy: "blocked"` leave the Entry Effective with a note.
- Feature gates: the org allowlist cache is encrypted and unreadable. Resolve as allowed and put a caveat on extension Entries. The `isDesktopExtensionEnabled` default also gets a feature gate (see Unknowns).
- Secrets: the core defaults cover env and args.

**Gaps**
1. **Registry as a location.** `managedMcpServers` is a JSON string in a registry value, and the draft has registry only as a gate. Fix: anchor `registry:<hive>\<key>`, format `registry-json-value`, plus a tier rule "HKLM with any value hides HKCU entirely" (a `lookup-order` over keys, where "exists" means "has any value"), used by both gates and locations. Generic: Claude Code and VS Code read Windows policy keys.
2. **Missing-file default on side files.** The settings file is one file per extension *id* (directory name), not keyed by Item name, and a missing file means `isEnabled: false`. Fix: `side-file-json` takes a `file_template` (`{entry_dir}.json`), `key_by = entry-id|name` and `missing_value`. Small; probably Desktop-only.
3. **Field gate on a side file.** If `orgBlockedReason` is present, the Entry is Blocked (`policy`). Fix: gate `field-present` on any JSON lookup. Generic enough: it reuses the same lookup machinery.
4. **Manifest name and variables.** The name is `display_name` falling back to `name`; a duplicate is renamed to the extension id instead of becoming Invalid. Args use `${__dirname}` and `${user_config.*}`. Fix: `name_from = [..]`, `on_duplicate = rename-to-dir|invalid`, and a `vars` table on `manifest`. The vars table is generic, since Gemini CLI has `${extensionPath}`. Rename-on-duplicate is Desktop-only.
5. **Per-location transport whitelist.** A `url` Entry in the user file is skipped by the app. Fix: field-map param `allowed_transports`, plus an invalid detector `unsupported-transport` with `effect = entry` (the app still starts) instead of the Codex-style "Agent cannot start". Desktop-specific today.
6. **Skill enabled flag lives in an array.** The flag is `manifest.json` → `skills[name={name}].enabled`. Fix: an array selector `[field={var}]` in key-path templates. Generic: Codex `[[skills.config]]` needs it too.
7. **Values that are secret by prefix.** Values starting `__encrypted__:` in `userConfig` must be masked. Fix: `extra_secret` accepts a `value_prefix`. Generic, cheap.
8. **`unverified` beyond mechanisms.** The rank of managed against user-file servers is unknown. Fix: allow `unverified` on precedence and competition rules. Generic.
9. **Connectors.** Only per-session snapshots exist: 172 files, each with an array root `remoteMcpServersConfig` and no enabled flag. Expressing them would need array roots plus multi-file aggregation, which is Desktop-specific. Recommend leaving them out of v0.1, with an Agent-level note code `cloud_connectors_not_on_disk`.
10. **Per-home process attribution.** Classic and MSIX homes run from different binaries. Fix: an optional `process` override per home. Generic for VS Code too (stable, Insiders, portable).
11. **Alias keys.** Legacy `isDxtEnabled` must be read alongside `isDesktopExtensionEnabled`. Fix: `aliases[]` on setting keys. Small, generic.

**Corrections**
- Extensions use `each-dir`, not `json-registry`. Any folder with a readable manifest counts as installed. `extensions-installations.json` only adds Provenance detail (source, signature).
- Do not apply `by-type-field` to manifest `server.type`: node/python/binary/uv is a runtime, not a transport. Extensions are always stdio, so use `by-field-presence` or a `fixed: stdio` primitive.
- `lookup-order` must not cover Store vs classic. The glossary makes them separate Agent homes, so neither should be Unread. Lookup-order applies only to `Claude-Data`, which wins only when `%APPDATA%` is a UNC path. That needs a `when: path-is-unc` condition on a home candidate, which is Desktop-specific.
- `glob Packages\Claude_*` misses the enterprise family `AnthropicPBC.Claude_*`.
- `session_files` cannot use `<userData>\lockfile`: it is an Electron single-instance lock with no PID.
- Claude Code also reads `claude_desktop_config.json` (Code tab). Under the no-cross-reference rule, Claude Code's definition has to repeat these home locators.

**Unknowns (mark `unverified`)**
- Managed vs user-file precedence in Chat, and the field map and transports of `managedMcpServers`.
- The whole extension layout, which comes from code only (no extension is installed): settings file names, `source` values, the order of duplicate names, and whether settings files are rewritten while running (store_class).
- `features.isDxtEnabled` / `isLocalDevMcpEnabled` in the file acting as a Disable when no policy is set.
- What `isDesktopExtensionDirectoryEnabled` does to extensions already installed from the directory.
- Whether the `isDesktopExtensionEnabled` default is true for 1P and false for 3P (a per-home default).
- Whether `isDesktopExtensionSignatureRequired` acts as a gate on `signatureInfo`.
- Whether an unknown key such as `disabled` is stripped or rejected. Parking stays the mechanism either way.
- How `__encrypted__:` values are encrypted.
- How tool-toggle keys map to Entries.
- Which folder each build uses when classic and MSIX are both installed, and the process path of a classic install.
- Whether any local cache of connectors exists.

### VS Code / Copilot
**Fits**
- Agent: `verified = "1.129.x"`. Item kinds are mcp and skill. Merge mode is `whole` for both kinds, with no invalid detectors.
- Process: images `Code.exe`; path_glob `%LOCALAPPDATA%\Programs\Microsoft VS Code\*`, `%ProgramFiles%\Microsoft VS Code\*`.
- Homes: config root `%APPDATA%\Code` plus a `%USERPROFILE%\.vscode` root. Profiles use `homes-from-json-list` over `User\globalStorage\storage.json` → `userDataProfiles`. `--user-data-dir` is covered by user-added homes.
- MCP locations (jsonc):
  - `home:user/mcp.json`, root `servers`, rewritten-while-running.
  - `project/.vscode/mcp.json`, root `servers`, plain.
  - `project/.mcp.json`, root `mcpServers`, plain, shared with Claude Code.
  - The order-400 discovery files (Claude Desktop, `~/.cursor`, `<folder>/.cursor`, Windsurf) each use their own format and get `when setting-equals chat.mcp.discovery.enabled.<src>`, default false.
- Skills: six `skill-folder` locations (`.agents`/`.github`/`.claude` under project, `.agents`/`.copilot`/`.claude` under `~`). Extension skills come from `manifest` `contributes.chatSkills[].path` over `json-registry` `extensions.json`. Provenance through skill manager detectors (`.skillfish.json` is a `marker-file`).
  - A `skill-folder` block's optional `unlisted` lists why the Agent leaves a skill it loads out of the skills it lists to its model (#343): `no-description`, a `SKILL.md` with no frontmatter or whose YAML frontmatter has no `description` key, and `disable-model-invocation`, a YAML frontmatter that sets it to the boolean `true`. Such a skill keeps its State, with an `unlisted` note that names each reason; a frontmatter config-digger reads as malformed, one that never closes, and one with a `[` or `{` value that never closes get neither of these two, unless the location reads it with `malformed_frontmatter = "keep-values"`, and only the plain value `true` counts as the boolean in YAML. Every VS Code skill location has `keep-values` (#473): config-digger reads every frontmatter, YAML or not, closed or not, with a port of VS Code's own parser, which never fails and keeps the values it can read, its first value of a key given twice, an empty `description` as none and the value `true`, quoted or not, as the boolean. A third value, `malformed-frontmatter` (#345), is a frontmatter config-digger reads as malformed, YAML that does not parse, one that never closes or a line that is no key, and one with a `[` or `{` value that never closes; it is the only reason such a frontmatter gives, and an Agent that reads a frontmatter that never closes as none gives no reason for it. It is empty by default, and the `skill-list` and manifest skill blocks take it through their `skill-folder` reading. Every VS Code skill location lists `no-description` and `disable-model-invocation`, and every Cursor skill location but `built-in-skills`, `cursor-plugin-skills`' manifest skills included, lists `no-description` and `malformed-frontmatter`.
- Projects:
  - `sqlite-row` on `history.recentlyOpenedPathsList` with a JSON path.
  - `json-keys` on `storage.json` `profileAssociations.workspaces`. Its optional `last_opened` maps each key's value, the profile VS Code last opened the workspace in, to the Project's Last-opened home (#128): `named` gives the values that name a home themselves (`__default__profile__` names `default`), `listed` the `json-list` home whose item a value names by its field (`profile`, by `location`), `name` the item's field shown to the user, and `left_out` the home an item left out by the list's `unless` reads (`default`, for a profile that shares MCP). A value naming no item names no home. It changes no State.
  - Spelling `file-uri`. Walk `folder-only`, plus `workspace-members` for `.code-workspace`.
- MCP precedence: `.vscode` (0) > `.mcp.json` (1) > `.code-workspace` (100) > user (200) > extension (300) > plugin (350) > discovery (400). Competition `case-insensitive-name` when `collisionBehavior = "disable"`, otherwise `never`.
- Skill precedence: workspace > personal > plugin > extension; list order inside each group; `first-loaded`.
- Field map: `by-type-field` (`stdio`/`http`/`sse`). `envFile`, `dev`, `sandboxEnabled`, `gallery`, `version` → other.
- Disable mechanisms:
  - `sqlite-json` `mcp.enablement`, with id template `<collectionId>.<name>`.
  - `sqlite-json` `chat.disabledPromptFiles.skill`.
  - `sqlite-json` `extensionsIdentifiers/disabled`, id `{"id":…}`.
  - Read-only `global-switch`: `chat.mcp.access: none`, `chat.useAgentSkills: false`.
- Gates:
  - `registry-policy` HKLM `ChatMCP`, `ChatPluginsEnabled`, `AllowedExtensions` → `policy`.
  - `approval-hash` for discovered servers (`TrustedOnNonce`; the nonce algorithm is code).
  - `trust-store` (sqlite-row, ancestor matching) → `untrusted_folder`.
- store_class: `state.vscdb` = sqlite; user `mcp.json` and `settings.json` = rewritten-while-running.
- Notes: `disable-model-invocation`, `user-invocable:false`.

**Gaps**
1. **Per-Project storage anchor.** `workspaceStorage\<id>\{workspace.json,state.vscdb}` is both the project registry (one file per dir, field `folder|workspace`) and the target for the workspace-scoped `mcp.enablement` and `mcp.extCachedServers`. Fix: registry `each-dir-json-field` that returns (project, entry dir), plus anchor `project-registry-dir`. Generic: Cursor uses the same layout.
2. **Layered enablement.** A workspace value overrides the profile value, both true and false; no entry means inherit. Fix: on the mechanism, `overrides = <mechanism id>` with tri-state values. Generic: Gemini per-Project rules, Codex layered `enabled`.
3. **Reach must have two axes:** target (entry | name | owner) × context (project | all). For example, profile-scope `mcp.enablement` is entry × all, and `extensionsIdentifiers/disabled` is owner × all. Generic: the owner target is needed by every plugin and extension Disable (ADR decision 1).
4. **Profile flags.** `useDefaultFlags.mcp` sends a profile's `mcp.json` to the default home, but `globalState` is never shared (built-in Agents profile). Fix: `homes-from-json-list` exposes per-home attributes, and locations take `when home-flag` plus `resolve_in = default-home`. Probably Cursor too, since it is a VS Code fork.
5. **`setting-equals` must layer settings:** default < user < `.code-workspace` < `.vscode/settings.json` < policy. Fix: an ordered list of sources, the last one wins. Generic: Gemini and Cursor layered settings.
6. **Root alternatives.** `.mcp.json` is either `mcpServers` or a bare map. Fix: `root = { any_of = ["mcpServers","."] }`. This one is VS Code-specific.
7. **Extension cache shape.** `mcp.extCachedServers` is `{<pub.ext>/<provider>: {servers:[{id,label,launch}]}}`. Fix: a root wildcard `*`, `name_field = "label"`, `owner_from = key-before-/`, and a nested field map (`launch.type` 1 → stdio, 2 → http). The wildcard and name field are generic (array roots); owner_from is close to `json-registry`.
8. **The Project is a file.** The `.code-workspace` file is itself a location (`settings.mcp.servers`). Fix: anchor `project-file`.
9. **Tie-break `member-order`** for same-order collections across workspace folders (`ws0`, `ws1`, "primary root wins"). The id template also needs `{member_index}`.
10. **Missing reason code.** No code covers a per-Entry native Disable. Add `disabled_natively`; Claude Desktop `isEnabled`, Codex and Cursor need it too.
11. **Built-in extensions dir** `<install>\<commit>\resources\app\extensions`: a `glob` home root that may match several commit folders. Fix: pick the newest, or the one matching `product.json`. VS Code-specific.

**Corrections**
- `skill-name` (frontmatter name) is wrong for VS Code: the skill name is the folder name. Add a parameter `name_from = folder | frontmatter`.
- store_class must apply to Disable targets and gate stores (`state.vscdb`), not only to locations.
- The draft example `setting-equals … collisionBehavior` treats it as one key in one location; it is layered (see gap 5).
- `file-uri` spelling must decode `%3A`, ignore drive-letter case and skip `vscode-remote://` URIs.
- `json-registry` needs field selection (`location` URI or `relativeLocation`), `.obsolete` exclusion and a per-profile `extensions.json`.
- `global-switch` must accept sqlite sources: Copilot Chat itself disabled via `extensionsIdentifiers/disabled`.

**Unknowns** (mark unverified)
- `mcp.enablement` value shape and definition-id formats, including the discovery ids.
- URI spelling inside `chat.disabledPromptFiles.skill`.
- Where agent plugin Disable is stored, and the plugin MCP file format.
- Workspace Trust store key, and whether workspace skills need trust.
- Trust prompt for `mcp.json` (source says trusted, docs say a dialog appears).
- Whether `.vscode/settings.json` `mcp` is still read, and whether `chat.mcp.workspaceRootConfig.enabled` gates `.mcp.json`.
- Whether list order inside a skill group is guaranteed, and whether skill-name matching is case-sensitive.
- `recentlyOpenedPathsList` location (the key was absent on this machine).
- Header and env encoding in `extCachedServers`, and how stale that cache gets.
- What a disabled Copilot Chat does to MCP and skills.
- Insiders: image, paths and whether it is a home or a separate Agent.
- Not shipped in 1.129.1: `~/.copilot/mcp-config.json` (`COPILOT_HOME`) and the `ChatAllowed`/`ChatDeniedMcpServers` policies. Leave them out, or add them under a feature gate.

### Cursor
**Fits**
- **Homes:** `home:dot` = `%USERPROFILE%\.cursor`, `home:data` = `%APPDATA%\Cursor`. No env override for the IDE (`CURSOR_CONFIG_DIR`/`CURSOR_DATA_DIR` belong to the CLI, a separate Agent). Profiles can use `homes-from-json-list` (`storage.json`), as VS Code does.
- **Scopes:** user, project, plugin, built-in.
- **MCP locations:** `home:dot/mcp.json` and `project/.cursor/mcp.json` → `jsonc`, root `mcpServers`, provenance `file`. Project walk is `folder-only` (root of each workspace folder). `workspace-members` handles `.code-workspace`.
- **Transport:** `by-field-presence`, where `command` gives stdio and `url` gives streamableHttp. `type`, `envFile`, `enabledTools`, `placement` and `auth` go to "other". `cwd` is not mapped.
- **Extra secret fields:** `auth.CLIENT_SECRET`, `envFile` (it points at a file of secrets).
- **Mode:** `whole` for both kinds. No Merged, no Invalid detectors.
- **MCP competition:** `never` by name. Same-named Entries are separate Items.
- **URL dedupe:** `endpoint` between non-project remote groups, `when feature-gate dedupe_mcp_servers`. Order: user > extension > team > plugin.
- **Skills:** `skill-folder` × {.cursor, .claude, .codex, .grok, .agents}/skills under `home:` and `project`. Competition `path-key` with tie-break `path-key-order` (.cursor > .claude > .codex > .grok > .agents). Different roots → `never`.
- **Nested skills (#134):** the same five folders deeper in each Project folder, five `project` locations whose anchor walks down: `walk = { down = { needs_git, skip, ignore_files, ignores_path, follow_links } }` on a `project` anchor reads the path in every folder below the Project folder that holds it, each its own location, never in the Project folder itself. Its rules are data: `needs_git` reads nothing outside a git work tree, `skip` names folders, case aside, never entered nor read through at any depth, below the path found too, `ignore_files` names the ignore files, here `[".ignore", ".gitignore"]`, whose patterns in the folders entered and in those above the Project folder leave out the folders they match on the way to the path, not what lies below that (git's patterns, a name over those after it, the deepest file of a name deciding; `.git\info\exclude` and git's global excludes are not read), `ignores_path = true` lets them leave out the path's first folder too, and `follow_links` enters links and junctions. Folders are walked in the order of their paths, each before those below it. The nested locations come first in `precedence`, since Cursor lists a nested skill over a root or user one at the same path below the marker. Of two nested ones, the folder holding the marker decides first, by path, and the marker's order only between two in one folder: the five nested locations are one group of `precedence` (#359).
- **Claude Code plugins:** `json-registry` `~/.claude/plugins/installed_plugins.json` → installPath. Loading follows `setting-equals` on `enabledPlugins[id]` (user file, project `.claude/settings(.local).json`).
- **Third-party switch:** `global-switch` `cursor/thirdPartyExtensibilityEnabled` (default true) over .claude/.codex/.grok skills and Claude plugins. Reason `disabled_globally`, Enable not offered.
- **MCP Disable:** `sqlite-json` on workspace `state.vscdb` key `cursor/disabledMcpServers`, id template, reach `entry` (per workspace), enable offered, unverified. Gated variant `cursor/disabledGlobalMcpServers` in the global DB.
- **Project plugin Disable:** `json-field` `plugins.<key>.enabled` (versioned, so not offered). User skills → `park`.
- **Approval gate:** `approval-hash` on global `cursor/approvedProjectMcpServers` (`<id>:<hash>`, legacy bare id accepted). Hash over command, args, env, envFile, url, headers, auth. Reason `not_approved`.
- **Project registries:** `sqlite-row` for `workspaceMetadata.entries` (folderUri) and `cursor/glass.additionalProjects`, spelling `file-uri`.
- **store_class:** `state.vscdb` = sqlite. `mcp.json` (user and project) = rewritten-while-running. `skills-cursor` = rewritten-while-running.
- **Process:** `Cursor.exe`, path globs `%LOCALAPPDATA%\Programs\cursor\*` and `%ProgramFiles%\cursor\*`. No session files.
- **Feature gates:** `dedupe_mcp_servers`, `mcp_profile_scoped_global_disabled_state`, `enable_cc_plugin_import` (assumed on, since supabase loads).
- **Notes:** `disable-model-invocation`, `enabledTools`/`mcpDisabledTools` (code `disabled_tools`).

**Gaps**
1. **Agent-specific identifier template** on a location (`user-{name}`, `project-{folder_index}-{folder_basename}-{name}`, `plugin-{plugin}-{server}`). It needs project-context variables (folder index) and should be shared by `sqlite-json`, `approval-hash` and Item display. Likely generic: VS Code `mcp.enablement` keys by collection id.
2. **Per-Project store anchor.** The workspace DB is found by scanning `workspaceStorage\*\workspace.json` for the Project's file-URI, with `empty-window` used when there is no Project. New anchor `project-storage` (glob dir + json key match). The same scan is a missing registry type (`each-file-json-value`). Generic: VS Code has identical storage.
3. **`when` on mechanisms, gates and rules**, not only locations. Needed for the gated Disable key and for dedupe. Generic.
4. **Source for read-only rules.** `global-switch` and `setting-equals` need a value source that can be an `sqlite-row` (with default), not only a JSON/TOML key. Generic (VS Code settings live in both places).
5. **Per-location project walk.** MCP is `folder-only`. Skills are `folder-and-subfolders`, but only for `.cursor`/`.agents` nested dirs. Generic: Claude Code nested skills.
6. **`skill-folder` params:** recursive SKILL.md discovery, skip dot-folders, name exclude list (Codex system skills under `.codex`). Recursion is generic; the exclude list is plain data.
7. **Plugin-level competition.** Marketplace beats local by plugin name. A Claude project/local install matching projectPath beats the user install. Project plugin keys are deduped by first folder. New key `container-name` so all of a plugin's Entries become Shadowed. Generic: Claude Code and Codex plugins.
8. **json-registry params:** array-per-key, `project_field` filter (projectPath == Project), and a path template from the key plus a version-dir glob (`cache/<mkt>/<name>/<sha>`). Manifest candidate order: `.cursor-plugin` > `.claude-plugin` > `plugin.json`, with `mcpServers` given as a path, inline object or array, or `mcp.json`/`.mcp.json`. Generic: Claude Code.
9. **User Marketplace plugins.** Install state is account-side, cached as numeric ids in SQLite; id→folder only via the Customize snapshot. Needs an `sqlite-registry` join or Cursor-specific code. Recommendation: list read-only from the cache with a caveat, or skip in v0.1.
10. **Dedupe selection override:** `mcpService.selectionPreferences` (profile SQLite). New `selection-override` primitive; the URL normalization is code. Cursor-only.
11. **Provenance values:** `builtin` (`skills-cursor`; Codex `.system` is the same case, so generic) and `account-managed` (Codex remote plugins share it).
12. **Reach computation** must count keys read by conditions and mechanisms. `enabledPlugins` is shared with Claude Code. Generic.
13. **Write-format constraint.** The user `mcp.json` must stay strict JSON, because the deeplink installer wipes a file with comments. Needs a format flag `write: strict-json` or a note code. Cursor-only.

**Corrections**
- Family 8 lists the Cursor import switch as `setting-equals`. Per #27 it is a Disable (`disabled_globally`, no Enable), and the value is in SQLite, not settings. It should be `global-switch`.
- `file-uri` spelling must decode `%3A` and compare the drive case-insensitively (`file:///e%3A/...`).
- Never use `by-type-field` for Cursor: `type` is ignored even when present. No `transport-conflict` either.
- `endpoint` dedupe excludes project locations and stdio servers, so "precedence by location" holds only inside the gate.
- Extension, team and built-in servers are not Entries, but they can win the dedupe. Add a caveat rather than modelling them as locations.

**Unknowns** (mark unverified)
- Values of `dedupe_mcp_servers` and `mcp_profile_scoped_global_disabled_state`.
- Shape of `disabledMcpServers` once a server is toggled.
- Whether `disabledMcpServers` applies to plugin servers.
- Whether `enabled:false` in `.cursor/settings.json` means "installed but off".
- User-plugin Disable via the account.
- Check order of approval vs Disable.
- Whether Workspace Trust gates MCP (not researched).
- Which same-named skill `/name` picks.
- Target DB for a Disable with no Project (`empty-window`?).
- `displayPath` spelling.
- Marketplace cache vs install drift.
- Enterprise allowlist (server side, not observable).
### Codex
**Fits**
- Homes: `env-override` `CODEX_HOME` (falls back to `~/.codex`) plus user-added homes. Named roots: `config` = CODEX_HOME and `os_home` = `~` (for `.agents/skills`, which CODEX_HOME does not move). `[[mirrored_homes]]`: owner Orca, `%APPDATA%\orca\codex-runtime-home\home`, source `~/.codex`.
- Scopes: managed, user, project, system (bundled), admin, plugin.
- MCP locations (`toml`, root `mcp_servers`):
  - `system:OpenAI\Codex\config.toml` (managed)
  - `home:config/config.toml` (user)
  - `project` walk `repo-root-to-folder`, `.codex/config.toml` (project)
- Skill locations (`skill-folder`):
  - `project` walk: `.codex/skills` and `.agents/skills`
  - `home:config/skills`
  - `home:os_home/.agents/skills`
  - `home:config/skills/.system`
  - `system:OpenAI\Codex\skills`
- Transport: `by-field-presence` (command → stdio, url → http). Map `http_headers` → headers.
- Extra secret fields: `oauth.client_secret`, `http_headers_helper`. `bearer_token_env_var` and `env_http_headers` hold env var names, not secrets.
- MCP precedence (low to high): system < user < project group (tie-break `deeper-wins`). Competition `exact-name`, mode `field-merge` with `deep-merge-table` (arrays replace whole, tables recurse).
- Skills: competition `never`.
- Invalid: `transport-conflict` on the merged server.
- Gate: `trust-store` via `toml-table-keys` on `projects` in user config.toml, mapped to `untrusted_folder`. Not applied to `.codex/skills`.
- Disable:
  - MCP: `toml-field` `mcp_servers.<n>.enabled=false`
  - plugin MCP: `toml-field` `plugins."<id>".mcp_servers.<n>.enabled=false`
  - plugin: `toml-field` `plugins."<id>".enabled=false`
  - skill: `toml-array-entry` `[[skills.config]]`, path first (reach entry), then name (reach name)
  - `[skills.bundled] enabled=false`: `global-switch`, Enable not offered
- Registries: `toml-table-keys` `projects`; `sqlite-row` on `state_5.sqlite` `threads.cwd` and `project_roots.path`.
- store_class: config.toml is `rewritten-while-running`, skill folders `plain`.
- Process: `codex.exe`; `node.exe` with cmdline_contains `@openai\codex`; session file `<home>\app-server-daemon\daemon.pid`.
- Notes: `enabled_tools`, `disabled_tools`, `tools.<t>`, `default_tools_approval_mode`, `required`.

**Gaps** (G = generic, C = Codex-only)
1. Mode is per item kind, but Codex uses field-merge between layers and whole-Entry Shadowing between config and plugin servers. Fix: move `mode` onto each competition rule. G (Gemini: settings vs extension uses overlay, settings vs settings uses whole).
2. Tie-break among plugin servers: the plugin sorted first by `<name>@<marketplace>` wins. Fix: tie-break `sort-key` (lexicographic on a template). G (Gemini extension load order).
3. Trust candidates are the folder, the project root and the git main-checkout root (for linked worktrees). They are not ancestors. Keys must match `trust_level="trusted"`. Trust is read from the merged non-project layers. Fix: `trust-store` params `candidates=[self, project_root, git_common_root]`, `value_equals`, `sources=[locations]`. Candidates are G; multi-source is C.
4. Project root markers: default `.git` (with HEAD), overridable by the `project_root_markers` setting. A `.codex` that resolves to CODEX_HOME is skipped. Fix: walk params `markers` and `markers_from_setting`, plus `exclude_home`. G (other Agents also find the git root).
5. Spelling: trust keys are ASCII-lower-cased native paths. `skills.config` paths must be canonical absolute paths. Fix: spelling modifiers `lowercase` and `canonical`. G (VS Code and Cursor URIs are case-folded).
6. Plugins: pick the highest valid version folder under `cache/<mkt>/<plugin>/`; manifest at the first of several alternative paths; MCP servers from manifest `mcpServers` (inline or a path), else `.mcp.json`. Fix: `each-dir` `pick=highest-version`, `path` as a list of alternatives, `manifest` ref-or-inline-or-fallback. G (Claude Code, Cursor plugins).
7. Remote plugins are marked by `.codex-remote-plugin-install.json`. Fix: per-location `provenance_if_marker` → `cloud-mirror`. G (Cursor account plugins).
8. Plugin skill names are `<plugin>:<skill>`. Fix: `name_template`. G (Claude Code).
9. Disable reach `plugin` (all Items of one plugin) is missing from the reach enum. Add it. G.
10. MCP `enabled` is a merged field, so reach is the Item (name). A Disable for a versioned project Entry can be a partial `enabled=false` table in user config.toml, valid only if no higher layer sets `enabled`. Fix: `toml-field` param `target=<location>` with `requires_unset_above`; the core already has per-field origins. C.
11. `[[skills.config]]` counts only from user/profile layers, and later rules win. Fix: `honoured_from` and `eval=last-wins`. C.
12. Requirements allowlist matches name + `command`/`url` identity and Blocks with `policy`. Fix: make `allow-deny-list` usable as a gate with `match_by=[name, command, url]`. G (Claude Code managed `allowedMcpServers`).
13. Provenance `bundled` (rewritten by the Agent; only a switch can Disable) for `skills/.system`. G (Gemini, Zed built-in skills).
14. Invalid detectors `missing-transport` (merged server has neither key) and `forbidden-field` (`bearer_token`), plus `agent_wide=true` because Codex refuses to start. Mostly C; `agent_wide` is G.
15. Session-file registry `sessions/**/rollout-*.jsonl`, first line `session_meta.cwd`. Fix: `jsonl-field` over a glob. G (Claude Code transcripts). `sqlite-row` also needs a file glob (`state_*.sqlite`).
16. Profile files `<name>.config.toml` count only with `--profile`. Fix: glob location + note code `launch_flag_only`. C.

**Corrections**
- Trust "ancestor matching" is wrong for Codex: an intermediate ancestor is not consulted (see Gap 3).
- `toml-field` MCP Disable reach is `name`, not `entry`.
- Plugin `.mcp.json` is JSON. Its field map can't reuse the TOML map, and it may use `type`, so `by-type-field` may be needed there.
- `cloud-mirror` fits remote plugin enablement only. Their servers stay plugin-sourced and are read-only.
- Not locations (no file): packaged defaults, cloud bundles, `-c`. `managed_config.toml` is ignored on Windows, so leave it out.

**Unknowns** (mark unverified)
- `enabled` merge across layers (ticket 20).
- Plugin MCP `enabled` and plugin `enabled` (source only).
- `requirements.toml` gate (docs and source only).
- Remote plugin enabled state offline.
- Whether untrusted `.codex/skills` loading is intended; whether trust applies to project `.agents/skills`.
- Field map of plugin `.mcp.json`.
- Whether a transport-less partial table crashes Codex.
- Whether packaged defaults hold `mcp_servers`.
- Exact canonical spelling for `skills.config` paths (`\\?\` prefix?).
- Desktop app process name and home; how Orca's mirror syncs.
- The `state_N` schema generation will change.
### Gemini CLI
**Fits**
- `[agent]`: gemini-cli, verified 0.62.0, item kinds mcp and skill.
- `[process]`: `node.exe` with cmdline_contains `gemini-cli` and `gemini.js`. Both strings must match, so cmdline_contains has to be a list joined by AND. The relaunched child process matches the same way.
- Homes: one home with root = `GEMINI_CLI_HOME`, falling back to `%USERPROFILE%`. Locations are `.gemini/...` and `.agents/skills` under it. No Mirrored home.
- Scopes, in display order: system-defaults, user, project, system.
- MCP locations (`jsonc`, root `mcpServers`):
  - `system`: `gemini-cli\system-defaults.json` and `gemini-cli\settings.json`, Provenance managed.
  - `home`: `.gemini/settings.json`.
  - `project`: `.gemini/settings.json`.
  - `manifest` over `each-dir` on `.gemini/extensions/*`, Provenance extension.
- Skill locations (`skill-folder`): `.gemini/skills` and `.agents/skills` (home and project), plus extension `skills/`. `~/.agents/skills` is a shared read.
- Antigravity is excluded because definitions only list paths. No format sniffing is involved.
- Project registries: `json-keys` on `projects.json` (root `projects`, native lower-cased) and on `trustedFolders.json` (forward-slash). Walk: `folder-only`.
- MCP competition: `exact-name` (case-sensitive). Precedence: system > project > user > system-defaults. Between two extensions, `first-loaded`.
- Settings Entry vs extension Entry: `field-merge` / `overlay-fields`.
- Invalid: `duplicate-manifest-name`.
- Skills: `skill-name` competition, precedence built-in < extension < `~/.gemini/skills` < `~/.agents/skills` < `.gemini/skills` < `.agents/skills`, mode whole.
- System files: `when file-owner-admin`.
- Disable:
  - MCP: `side-file-json` on `mcp-server-enablement.json`, reach name+global.
  - Skills: `json-name-list` on `skills.disabled` (user or project), reach name.
  - Read-only rules: `allow-deny-list` for `mcp.allowed`/`mcp.excluded`, `global-switch` for `skills.enabled`.
- Gate: `trust-store` with `json-keys` on `trustedFolders.json` and ancestor matching. It applies to all MCP locations, user ones included, and to project skills, with reason `untrusted_folder`.
- Feature gate: cloud `admin.mcp|skills|extensions.enabled`, assumed true, with a caveat.
- Provenance: file, managed, extension. skillfish is detected by `marker-file` (`.skillfish.json`).
- store_class: both `settings.json` files are rewritten-while-running. The enablement files are plain.
- Extra secret field: `oauth.clientSecret`.
- Notes: `includeTools`/`excludeTools` (partial switch), `trust:true`, and `cli_view_differs` (`mcp list` hides the extension half of a merge).

**Gaps**
1. **Env override on a single file.** `GEMINI_CLI_SYSTEM_SETTINGS_PATH`, `GEMINI_CLI_SYSTEM_DEFAULTS_PATH` and `GEMINI_CLI_TRUSTED_FOLDERS_PATH` each move one file. When the defaults variable is unset, the defaults file follows the folder of the (possibly moved) system file. Fix: `env_override` on a location or gate file, plus `default = sibling-of:<location>`. Only Gemini CLI needs this, but it is a small parameter.
2. **Project file is skipped when Project = home.** At home, `<home>\.gemini\settings.json` is the same file as the user one. Fix: walk parameter `skip_if_home`. Generic: Codex's `~/.codex/config.toml` has the same overlap.
3. **Linked extensions.** For `link` installs the manifest and `skills/` live in `source` from `.gemini-extension-install.json`. Fix: `each-dir.base_from = {file, key, when}`. Generic: Claude Code directory/local plugin sources are similar.
4. **Merge mode per pair.** Settings Entries compete whole; settings vs extension is a field-merge. Fix: move `mode` from the item kind to each competition rule (per Provenance pair). Generic: Codex needs it too (layers merge, plugin Shadowed whole).
5. **Merge strategies per field.** Scalars: settings wins. `env`: merged, settings wins. `excludeTools`: union. `includeTools`: intersection. Fix: `overlay-fields.strategies` table. Gemini CLI-specific values, generic parameter.
6. **Tie-break `last-loaded`.** Skills: the last one loaded wins, within the extension tier too. Fix: generalise to `load-order = first|last`.
7. **Name normalisation.** Enablement, `mcp.allowed`/`mcp.excluded` and `skills.disabled` compare lower-cased, trimmed names. Skill names replace ``: \ / < > * ? " |`` with `-`. Fix: `name_match` on mechanisms and `name_sanitize` on `skill-name`. Gemini CLI-specific.
8. **Enable deletes the key.** For `mcp-server-enablement.json`, Enable removes the key instead of setting a value. Fix: `enable_action = delete-key|set`. Generic.
9. **Combining lists across files.** `mcp.allowed` is the intersection of every file, `mcp.excluded` the union. The reason code depends on where the rule sits: a user file gives `disabled_by_rule`, a managed file gives `policy`. Fix: `combine` plus `reason_by_scope`. Generic: Claude Code's allowed/denied lists.
10. **Extension enablement is a writable Disable, not a gate.** The file holds path rules: last match wins, `!` negates, a trailing `*` covers subfolders, matching is a case-sensitive regex on `/<path>/`, and no rule means enabled. Writes have to be per Project or per drive root. Fix: new mechanism `path-rule-list` (grammar, slash-drive spelling with original case, write strategy `project|drive-root`). Gemini CLI-specific code.
11. **Reach `container`.** Disabling an extension takes its servers and skills with it. Generic: Claude Code plugins, Claude Desktop extensions, Codex plugins.
12. **Trust values.** `trustedFolders.json` has trust and distrust values, longest prefix wins, and no match means untrusted. The gate is on only when `security.folderTrust.enabled` (default true) is set in the system-defaults, user or system file. Fix:
    - `trust-store` gets `value_map`, `match=longest-prefix` and `default`.
    - `setting-equals` gets `over = [locations]`, read with precedence. Generic: VS Code `collisionBehavior`.
13. **Built-in skills** (`bundle\builtin\`). There is no install-dir anchor and no `builtin` Provenance. Fix: either an `install-dir` locator (Volta/npm globs) or a static built-in names list. Possibly generic (Codex `.system` skills).
14. **New codes.**
    - Reason `system_file_not_admin_owned`: the file is present but ignored. Unread fits better than Blocked.
    - Note `no_project_trust_unknown`.

**Corrections**
- **Transport detection is a hybrid.** `type` (`sse`/`http`) when present; otherwise presence in the order `httpUrl` > `url` > `command`. `httpUrl` aliases to url with transport http. `tcp` is websocket, which the common model lacks, so it goes to "other". Neither existing transport primitive alone covers this.
- **No transport-conflict detector.** Several transports in one Entry is not Invalid in Gemini CLI; it resolves by the order above.
- **Family 10 `path-rules` is the wrong family here.** In Gemini CLI path rules are a Disable (Enable offered, #27 lists them under Disabled), not Blocked.
- **`GEMINI_CLI_HOME` behaves differently from `CODEX_HOME`.** It replaces the user's home folder, not `.gemini`. With it set, `~/.agents/skills` is no longer the folder VS Code shares.
- **`file-owner-admin` should give Unread with its own reason code**, not plain skipping.

**Unknowns (mark unverified)**
- None of the native mechanisms was tested on disk: the enablement file, `skills.disabled`, and the effect of extension rules.
- The user-scope extension rule missing `E:\` projects (open question 1).
- The default scope for `skills disable`: code says workspace, docs say user. This affects the preference order.
- Extension load order is `readdir` order (tie-breaks for both MCP and skills).
- `mcp.allowed: []` vs an absent key.
- Overlay field strategies (source only).
- Cloud `admin.*` values and `requiredConfig` servers. Nothing local can be read, so feature-gate caveat only.
- Process detection for a plain npm install (not Volta).
- Whether Antigravity also reads `~/.gemini/skills` or `settings.json` (shared reach).
- Folder trust defaults to on: the source and the disk agree, the docs disagree.
### skill managers (npx skills, skillfish, openskills, Claude Code synced skills)
**Fits**
- skillfish and openskills Provenance: `marker-file` with `.skillfish.json` and `.openskills.json`. graphify also fits, with `.graphify_version`.
- `npx skills` linked Entry: `link-target-glob` with `**/.agents/skills/*`. It must match the resolved target (an absolute junction on Windows, a relative symlink on POSIX) whether or not the target exists.
- `npx skills` copy mode or copy fallback: `lock-file-entry` on `~/.agents/.skill-lock.json` (root `skills`) and on `<project>/skills-lock.json` (root `skills`). Format `json`. Each lock covers one Scope, so the detector's optional `scope` names the id of a Scope, `user` for the user lock and `project` for the project lock (#117). It matches only a skill whose location the Agent definition gives that Scope id, and without it the lock covers every Scope.
- `~/.agents/skills` read by Codex, Gemini CLI, Cursor, Copilot and Zed: this is a shared location that each Agent definition declares itself. The core computes the reach.
- Lock files: store_class `plain`.
- None needed: process detection, gates, feature gates, merge mode, invalid detection, Scopes, project registries. `npx` runs briefly and has no persistent process.
- Disable of a manager Entry: comes from the Agent definition (`skillOverrides`, etc.) or `park`. Parking a junction moves the link and keeps its target, which is core work.
- Versioned project locks: core git detection.

**Gaps**
1. **The skill manager skeleton has no homes or env overrides.** The user lock moves to `$XDG_STATE_HOME/skills/.skill-lock.json` when that variable is set, so the relative subpath changes too, not just the root. Fix: let Skill manager definitions reuse family 1 `[[homes]]`, and add `path_when_set` to `env-override`. Generic: Codex and Gemini CLI also have env-moved homes.
2. **Lock key ≠ folder name.** `skills` stores the key `ce:review` under the folder `ce-review`. Fix: a `key_transform` parameter on `lock-file-entry` that names a core function (`skills-sanitize`: lowercase, `[^a-z0-9._]`→`-`, trim `.-`, max 255). The function is code specific to skill managers.
3. **Which lock goes with which Entry.** Locks record neither Agents nor Scope labels. Fix: `applies_to = profile | entry-project`. `profile` covers every user-level skill location of any Agent; `entry-project` means the lock sits at `{project-anchor}/skills-lock.json` of the Entry's location instance. Generic across manager definitions only.
4. **Lock schema guard.** A user lock with `version < 3` or no `skills` key reads as empty. Fix: `min_version` on `lock-file-entry`. Specific to skill managers.
5. **Canonical folder signal.** An Entry inside `.agents/skills` is weak evidence of `npx skills`, while being the target of a manager link is strong evidence. Fix: a `location-glob` detector, plus a `link_target_of` flag. Small and generic for managers.
6. **Detector strength and order across definitions.** The research ranks the signals: link, then marker, then synced path, then canonical or lock name match, then file. A hand-made folder with the same name as a locked skill matches the lock too. Fix: `strength = strong | weak` per detector, with the core taking the strongest match. Weak matches get a note code (e.g. `provenance_by_name_only`). Specific to skill managers, but every manager definition needs it.
7. **Remove side effects.** Remove deletes the link or folder directly; never shell out to `skills remove`. Fix: `on_remove = delete-key-when-last` on `lock-file-entry`. The lock key is dropped only when no Entry with that sanitized name is left in that lock's Scope, across all Agent folders and the canonical folder. The file is kept even if `skills` becomes `{}`, and the project lock keeps its sorted keys. Removing a canonical target must put every Linked Entry pointing at it in the reach preview. That part is core and generic.
   - skillfish and openskills need nothing from the definition. Their marker goes away with the folder, and skillfish's `skillfish.json` bundle is left alone, as skillfish itself does.
8. **Parked Entries can be restored by the manager.** `skills experimental_install` recreates entries listed in `skills-lock.json`. Fix: a note code (e.g. `manager_may_restore`) on the lock detector. Specific to skill managers.

**Corrections**
- **Claude Code synced skills are not a skill manager.** Model them as a Claude Code Agent definition location instead: anchor `home:claude`, path `skills/synced/*/*`, base Provenance `cloud-mirror` (read-only, no Remove), and exclude `.trash/`. The `manifest.json` sits at the account folder, so `marker-file` in each skill folder would miss it.
  - Competition: synced skills appear namespaced (`anthropic-skills:`) side by side with local ones, so the rule is `never` against file skills, marked unverified.
- **Family 1 examples miss `CLAUDE_CONFIG_DIR`.** It moves Claude Code's user skills folder, and `skills` honours it. The `XDG_*` variables are also missing.
- **`~/.agents/skills` needs a profile anchor (`~`), not `home:<root>`.** For Codex it sits outside `CODEX_HOME`, and every Codex home sees the same folder. Proposed fix: add anchor `profile`. It is generic, used by five Agents.
- **Item 5 of the draft should name three skill managers, not four.** They are `npx skills`, skillfish and openskills, with graphify optional. Drop the "Claude Code synced skills?" candidate.

**Unknowns (to mark unverified)**
- How Rust `std::fs` classifies junctions. This decides whether `link-target-glob` fires on Windows.
- How Claude Code treats `skills/synced/` and `.trash/`, including the namespacing and competition above.
- The precedence each universal Agent gives between `~/.agents/skills`, its own folder and project folders. This belongs to the per-Agent tickets.
- When the copy fallback fires. Fallback copies can only be matched by the weak lock signal.
- The `XDG_STATE_HOME` lock path and the lock version guard (both from source only).
- The project lock `subagents` field. It is optional and only used by Eve.
- skillfish's Agent folder table (npm 1.0.39 differs from the repo's 1.0.31) and everything about openskills (source only, not installed here).
- Whether any v0.1 Agent reads openskills' `.agent/skills` (singular). If none does, those Entries are never found.
- Junctions committed to a project with absolute targets: how git versions them, and what State a dangling link gets.
- Whether the lock's `sourceUrl` can carry credentials. If it can, it needs masking as an extra secret field.
