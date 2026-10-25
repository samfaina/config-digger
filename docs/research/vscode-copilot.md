# Research: VS Code / GitHub Copilot configuration on Windows

Issue: [#10](https://github.com/samfaina/config-digger/issues/10). Terms follow [`CONTEXT.md`](../../CONTEXT.md).

Verified against **VS Code 1.129.1 stable** (user install at `%LOCALAPPDATA%\Programs\Microsoft VS Code\`, commit `8a7abeba6e03`, built 2026-07-17) with the bundled **GitHub Copilot Chat 0.57.0** built-in extension, on Windows 11, home `C:\Users\kgon1`, on 2026-10-01. VS Code was not running during the checks. SQLite databases were opened with `mode=ro&immutable=1` so no `-wal`/`-shm` files were created. Source citations point at `microsoft/vscode` `main` at commit [`64e91bc`](https://github.com/microsoft/vscode/tree/64e91bc28bed22fb021da2cbb776de8f37e3463d), which is newer than the installed build. Where the two differ, the text says so.

Each fact is tagged:

- **[disk]**: verified read-only on this machine. This includes reading the installed app's own shipped code under `resources\app\out\` and its `package.json`/`product.json` files.
- **[docs]**: stated by a primary source (code.visualstudio.com, docs.github.com, or the `microsoft/vscode` source) and not observable on this machine.
- **[contradicts docs]**: what's on disk or in the source differs from, or goes beyond, what the docs say.
- **[test]**: tested against the real VS Code by the release gate's probe, `uat/agents/vscode.mjs`, on 1.129.1, 1.135.0 and 1.141.0 (see [`release-gate.md`](release-gate.md)).

Secret values are never reproduced. Where one would appear, the text shows only the key name or `***`.

## Summary

VS Code has no single MCP file. It builds a list of **collections**, each with its own `order` number, and each server becomes `<collectionId>.<name>`. The file-based collections are: the profile's `mcp.json` (`%APPDATA%\Code\User\mcp.json`, or `...\User\profiles\<id>\mcp.json` for a non-default profile that does not share MCP), `<folder>\.vscode\mcp.json`, `<folder>\.mcp.json` (Claude Code's file, read as-is), `settings.mcp` inside a `.code-workspace` file, and the remote user `mcp.json`. Extensions add collections at runtime through `contributes.mcpServerDefinitionProviders`. Agent plugins add collections too. The `chat.mcp.discovery.enabled` setting (off by default) can import Claude Desktop, Cursor and Windsurf configs. The VS Code format uses root key `servers`, not `mcpServers`. The `.mcp.json` file accepts both.

**Nothing is merged.** Two Entries with the same name are two separate servers. With the default `chat.mcp.collisionBehavior: "disable"`, servers whose labels match case-insensitively form a group. The one in the collection with the lowest `order` is Effective and the rest are Shadowed (disabled by default): `.vscode/mcp.json` (0) > `.mcp.json` (1) > `.code-workspace` (100) > remote user (150) > user `mcp.json` (200) > extension (300) > agent plugin (350) > discovered external configs (400). With `"suffix"` every server runs and nothing is Shadowed.

**Disable is native and separate from the files.** It lives in the SQLite `state.vscdb` under the key `mcp.enablement`, as a JSON array of `[serverDefinitionId, boolean]` pairs. There is one at profile scope (`User\globalStorage\state.vscdb`) and one per workspace (`User\workspaceStorage\<id>\state.vscdb`), and the workspace entry wins. Skills are Disabled through the profile-scoped key `chat.disabledPromptFiles.skill`, a list of URIs. Extensions are Disabled through `extensionsIdentifiers/disabled`. On this machine that key holds `[{"id":"github.copilot-chat"}]`, so Copilot Chat itself is Disabled [disk].

**Skills** are `<name>/SKILL.md` folders read from `.agents/skills`, `.github/skills` and `.claude/skills` in each workspace folder, and from `~/.agents/skills`, `~/.copilot/skills` and `~/.claude/skills`. Extensions (`contributes.chatSkills`) and agent plugins add more. On a name clash the order is workspace > personal > plugin > extension, and the first one wins. **VS Code reads `~/.claude/skills` and `<project>\.claude\skills`, the same folders Claude Code reads.** One skill folder is therefore one Entry that two Agents use. Prompt files (`*.prompt.md`), custom agents and instructions are separate kinds and not skills.

**VS Code rewrites files while running.** It re-serializes the user `mcp.json` on every install or remove (comments are lost, and the file is deleted once empty). At startup it moves a legacy `mcp` key out of `settings.json` into `mcp.json`, and it migrates renamed settings inside `settings.json`. It flushes `state.vscdb` on its own schedule. A running instance shows as `Code.exe`. Writing `state.vscdb` while `Code.exe` runs will race with it.

---

## 1. MCP server locations per Scope

### 1.1 Locations

| Scope (config-digger) | Windows path | Root key | Collection id / order | Tag |
|---|---|---|---|---|
| user | `%APPDATA%\Code\User\mcp.json` (default profile) | `servers`, `inputs`, `sandbox` | `mcp.config.usrlocal` / 200 | [docs] path from [profile source][S-profile] (`joinPath(location, 'mcp.json')`) and [docs][D-mcp-servers]. **Absent on this machine** [disk] |
| user (other profile) | `%APPDATA%\Code\User\profiles\<id>\mcp.json`, unless the profile's `useDefaultFlags.mcp` is `true`, in which case the default profile's file is used | same | same | [docs] [S-profile]. Only profile here is the built-in **Agents** profile at `profiles\builtin\agents`, which shares MCP with the default (`useDefaultFlags.mcp: true`) [disk] |
| project (versioned) | `<folder>\.vscode\mcp.json` | `servers`, `inputs`, `sandbox` | `mcp.config.ws<folderIndex>` / 0 | [docs] [S-workbench], [D-mcp-config]. None found under `E:\projects` [disk] |
| project (versioned, cross-agent) | `<folder>\.mcp.json` | `mcpServers` **or** bare map of servers | `workspace-dot-mcp.<folderIndex>` / 1 | [docs] [S-dotmcp], [S-rootcfg]. Present in `E:\projects\claudio\petme` and `...\stint` (Claude Code files) [disk]. The installed build contains the `workspace-dot-mcp.` reader [disk] |
| project (multi-root) | `settings.mcp` in a `<name>.code-workspace` file | `settings.mcp.servers` | `mcp.config.workspace` / 100 | [docs] [S-workbench], [S-types] |
| user (remote) | the remote machine's user `mcp.json` (WSL, SSH, dev container) | `servers` | `mcp.config.usrremote` / 150 | [docs] [S-workbench]. Out of scope (see §10) |
| plugin | the agent plugin's MCP file | plugin format | `plugin.<pluginUri>` / 350 | [docs] [S-plugin], [D-plugins] |
| extension | not in a file: supplied at runtime by the extension | n/a | `<publisher.extension>/<providerId>` / 300 | [docs] [D-ext-mcp]; cache [disk] (see §5) |
| external import | Claude Desktop `%APPDATA%\Claude\claude_desktop_config.json`, `~/.cursor/mcp.json`, `<folder>/.cursor/mcp.json`, `~/.codeium/windsurf/mcp_config.json` | each app's own format | 400 | [docs] [D-mcp-config], [S-config] |
| managed | no MCP server file. Policies only restrict access (see 1.3) | n/a | n/a | [docs] [D-policies] |

The user `mcp.json` is a roaming user-data file. With Settings Sync and the "MCP Servers" option on, it syncs across devices ([mcp-servers docs][D-mcp-servers]) [docs].

**GitHub Copilot CLI config (`~/.copilot/mcp-config.json`).** On `main`, VS Code can read it in two ways. It is a `copilot` discovery source under `chat.mcp.discovery.enabled` ([S-config] L27–56), and it is a "Copilot Global" resource read through the agent host ([S-copilot-global], [S-copilot-global-path]). The docs present it as the user file of the "portable format" ([D-mcp-config]). **[contradicts docs]** for the installed 1.129.1: the shipped bundle contains no `mcp-config.json` string and only the `claude-desktop`, `windsurf`, `cursor-global` and `cursor-workspace` discovery sources [disk]. The docs describe a newer build than the one installed. `~/.copilot` on this machine holds only `ide\` (empty) and `skills\` [disk].

### 1.2 Legacy `mcp` key in `settings.json`

- In user `settings.json` (and the remote one), the `mcp` key is no longer read as a source. At startup the `workbench.mcp.config.migration` contribution reads `settings.mcp.servers`, installs each server into the profile `mcp.json`, and then **deletes the `mcp` key from `settings.json`**. If the key reappears later, VS Code shows an error notification offering to migrate it ([S-migration]; same logic in the installed bundle [disk]).
- In a `.code-workspace` file, `settings.mcp` is still a live collection (`WorkspaceConfiguration` provenance) ([S-types] L52–61) [docs].
- `.vscode/settings.json` → `mcp`: no source found that reads it. See Open questions.
- This machine's `settings.json` has no `mcp` key [disk].

### 1.3 `chat.mcp.*` settings and policies

Defaults below were read from the installed bundle's configuration registration [disk] unless noted.

| Setting | Default | Effect | Policy |
|---|---|---|---|
| `chat.mcp.access` | `"all"` (`"none"`, `"registry"`, `"all"`) | `none` turns MCP off. `registry` allows only servers from the configured registry | `ChatMCP` |
| `chat.mcp.discovery.enabled` | object, every source `false` | per-source import of other apps' configs. A legacy boolean is migrated to the object form | none |
| `chat.mcp.collisionBehavior` | `"disable"` (`"suffix"`) | how same-label servers resolve (see §4) | none |
| `chat.mcp.autostart` | `"newAndOutdated"` (`never`, `onlyNew`) | auto-start on chat submit (experimental) | none |
| `chat.mcp.gallery.enabled`, `chat.mcp.gallery.serviceUrl` | — | MCP gallery in the Extensions view (`@mcp`) | `McpGalleryServiceUrl` |
| `chat.mcp.serverSampling` | — | which models servers may sample | none |
| `chat.mcp.apps.enabled` | — (experimental) | MCP Apps UI ([D-mcp-config]) | none |
| `chat.mcp.workspaceRootConfig.enabled` | — | constant exists on `main` ([S-config] L60). **Not in the installed bundle** [disk] | none |
| `mcp.enterpriseManagedAuth.idp` | — | hidden. Holds `issuer`, `clientId`, **`clientSecret`** | `McpEnterpriseManagedAuthIdp` |

Policies on Windows come from `HKLM\Software\Policies\Microsoft\VSCode` (ADMX) ([D-policies]). The installed `policies\VSCode.admx` defines `ChatMCP`, `McpGalleryServiceUrl`, `McpEnterpriseManagedAuthIdp`, `ChatPluginsEnabled`, `ChatEnabledPlugins`, `ChatHooks`, `AllowedExtensions` and others [disk]. **[contradicts docs]**: the docs also list `ChatAllowedMcpServers`, `ChatDeniedMcpServers` and `ChatAllowManagedMcpServersOnly`, which the 1.129.1 ADMX does not contain [disk]. No `Policies\Microsoft\VSCode` key exists in HKLM or HKCU here [disk]. Policies restrict or deny MCP servers. They never declare a server, so VS Code has no managed MCP Entry.

### 1.4 Environment-variable and CLI overrides

- `--user-data-dir <dir>` moves `%APPDATA%\Code` and therefore `User\mcp.json`, `User\profiles\` and all `state.vscdb` files. Portable mode (`VSCODE_PORTABLE`) puts them under `<portable>\user-data`. [docs] ([D-settings], [D-profiles])
- Extensions: `--extensions-dir` > `VSCODE_EXTENSIONS` > `VSCODE_PORTABLE\extensions` > `%USERPROFILE%\.vscode\extensions` (the `.vscode` comes from `product.json` `dataFolderName`). The resolution code was read in the installed `out\main.js` [disk]. The default location is also in the docs ([D-ext]).
- Agent plugins home: `--agent-plugins-dir` > `VSCODE_AGENT_PLUGINS` > `VSCODE_PORTABLE\agent-plugins` > `%USERPROFILE%\.vscode\agent-plugins` (installed `out\main.js`) [disk]. See §5.3.
- `COPILOT_HOME` relocates the Copilot CLI's `mcp-config.json` ([S-copilot-global-path]) [docs].
- No `VSCODE_*` or `COPILOT_*` variables are set on this machine [disk].

---

## 2. File format and secret-bearing fields

The `mcp.json` schema (`servers`, `inputs`, `sandbox`, comments and trailing commas allowed) is defined in [S-config] L159–331 [docs], and matches [D-mcp-config].

| Transport | Fields | May hold secrets |
|---|---|---|
| stdio (`type: "stdio"` or omitted with `command`) | `command`, `args`, `cwd`, `env`, `envFile`, `sandboxEnabled`, `dev` | `env` values, `args` (tokens passed as flags), and `envFile` (points at a file of secrets) |
| http / sse (`type: "http"`/`"sse"`) | `url`, `headers`, `transport`, `oauth.clientId`, `oauth.enterpriseManaged`, `dev` | `headers` (e.g. `Authorization`), and the query string of `url` |
| gallery-installed (either transport) | also `gallery` (URL or `true`) and `version` | no |

- `${input:<id>}` placeholders refer to the top-level `inputs` array (`promptString`, `pickString`, `command`). Values the user typed are stored outside the file in storage key `mcpInputs` (profile or workspace scope). Inputs marked `password: true` are encrypted with AES-GCM. The key, `mcpEncryptionKey`, is kept in VS Code's secret storage ([S-inputs] L16–27) [docs]. The secret storage shows up in `state.vscdb` as `secret://...` keys, which hold encrypted values. Such keys exist here, for example for `vscode.microsoft-authentication`, but none for MCP [disk].
- The OAuth client secret is never written to the file. It goes to the OS secret store ([S-config] L246) [docs].
- `.mcp.json` (root) accepts only `type`, `command`, `args` and `env` for stdio, and `type`, `url`, `headers` and `transport: "http"` for remote. It has no `inputs`, and VS Code refuses `${...}` interpolation when it writes there ([S-rootcfg] L21–69) [docs]. Both `{ "mcpServers": {...} }` and a bare map are read ([S-rootcfg] L86–105) [docs]. The two files here use the wrapped form: `supabase:{type,url}` and `webstorm:{url,type}` [disk].

---

## 3. Skills per Scope

Default folders, from [S-locations] L172–179 [docs], are also present as a literal array in the installed bundle [disk]:

| Scope | Folder | Source tag |
|---|---|---|
| project | `<folder>\.agents\skills\<name>\SKILL.md` | `agents-workspace` |
| project | `<folder>\.github\skills\<name>\SKILL.md` | `github-workspace` |
| project | `<folder>\.claude\skills\<name>\SKILL.md` | `claude-workspace` |
| user | `~\.agents\skills\<name>\SKILL.md` | `agents-personal` |
| user | `~\.copilot\skills\<name>\SKILL.md` | `copilot-personal` |
| user | `~\.claude\skills\<name>\SKILL.md` | `claude-personal` |
| extension | path from `contributes.chatSkills[].path` in the extension's `package.json` | extension contribution |
| plugin | `skills/` inside an agent plugin, invoked as `/<plugin>:<skill>` | plugin |

- The docs list the same six folders ([D-skills]). The GitHub docs list only `~/.copilot/skills` and `~/.agents/skills` as personal folders ([D-gh-skills]). That difference concerns other Copilot surfaces, not VS Code.
- `chat.agentSkillsLocations` adds folders but is deprecated and "only used by the Local agent" ([D-skills]) [docs]. `chat.useAgentSkills` turns skills on or off as a whole. A legacy `chat.useClaudeSkills` is migrated into it ([S-cfgkeys] L106; migration found in the installed bundle [disk]).
- Skill name = folder name ([S-prompts-svc] L582–586) [docs]. Frontmatter: `name`, `description`, `argument-hint`, `user-invocable`, `disable-model-invocation`, `context` ([D-skills]) [docs].
- On disk here: `~\.copilot\skills\jetpack-compose\` holds `SKILL.md` and a `.skillfish.json` marker, which points to a skill manager as Provenance. `~\.agents\skills\` holds `computer-use`, `orca-cli` and `orchestration` [disk].
- The built-in Copilot extension contributes 13 skills through `chatSkills`, for example `troubleshoot`, `create-skill` and `init`. Some are gated by `when` clauses [disk].
- **Not skills.** These are separate customization kinds with their own folders, and config-digger v0.1 does not inventory them [docs] ([S-locations] L97–132, [D-prompts]):
  - Prompt files: `*.prompt.md` in `.github/prompts` and the profile's `User\prompts\` folder ([S-profile] `promptsHome`).
  - Instructions: `.github/instructions`, `.claude/rules`, `~/.copilot/instructions`, `copilot-instructions.md`, `AGENTS.md`, `CLAUDE.md`.
  - Custom agents: `.github/agents`, `.claude/agents`, `~/.copilot/agents`.
  - Hooks: `.github/hooks`, `.claude/settings*.json`, `~/.copilot/hooks`.

---

## 4. Precedence and merge rules

**MCP servers**

1. Each location is its own collection. Within a file, server names are JSON keys and so unique. Across files, **no field-level or file-level merge** happens. Every Entry becomes its own server with id `<collectionId>.<name>` ([S-installed] L80, L95–97) [docs].
2. Collections are sorted ascending by `order` ([S-registry] L129). The values come from [S-types] L144–153: WorkspaceFolder 0, `.mcp.json` 1, Workspace 100, User 200, Extension 300, Plugin 350, Filesystem discovery 400, and RemoteBoost −50 (remote user = 150) [docs].
3. With `chat.mcp.collisionBehavior: "disable"` (the default), servers whose **labels** (the JSON key, compared case-insensitively) match form a group. A server is reported disabled when an earlier server in the group is enabled ([S-service] L243–300, [S-enablement] L174–236) [docs]. The earliest enabled server is therefore Effective and the others are Shadowed. If the user explicitly enables a Shadowed one, VS Code writes `DisabledWorkspace` for every other member of the group, so the Effective Entry changes and the change is persisted in workspace storage.
4. With `"suffix"`, every server runs and tool names get a numeric suffix ([S-server] L226–252). Nothing is Shadowed.
5. The docs say nothing about precedence beyond "use a unique name for each server to avoid conflicts" ([D-mcp-config]). **[contradicts docs]**: the source implements the deterministic Shadowing described here.

**Skills**: candidates are sorted workspace (0) > personal (1) > plugin (2) > extension API (3) > extension contribution (4). The first occurrence of a name wins, and later ones are recorded as `skipped: duplicate-name` ([S-prompts-svc] L565–630, L1488–1505) [docs]. Across workspace roots, "the primary root takes precedence" ([D-skills]) [docs]. The order among the three workspace folders, and among the three personal folders, follows list order in [S-locations]. Whether that order is a guarantee or an accident of the code is an open question.

**Settings** (`chat.*`): the normal VS Code order applies: default < user < remote < workspace < folder, with policy on top ([D-settings]) [docs].

---

## 5. How extensions and plugins are recorded on disk (attribution)

### 5.1 Extensions

- Installed extensions are listed in `%USERPROFILE%\.vscode\extensions\extensions.json`, an array of `{identifier.id, version, location, relativeLocation, metadata{source, installedTimestamp, isBuiltin, isApplicationScoped, pinned, targetPlatform, preRelease, ...}}`. There are 24 entries here [disk]. A non-default profile keeps its own list at `User\profiles\<id>\extensions.json` unless `useDefaultFlags.extensions` is set ([S-profile] L202) [docs]. Old versions waiting for removal are listed in `.obsolete` [disk].
- **`.obsolete`**, checked on 2026-10-24 in VS Code 1.140.0's `out\vs\code\electron-utility\sharedProcess\sharedProcessMain.js` [binary]. No `.obsolete` was on disk that day: VS Code deletes the file when it empties it.
  - It is a JSON object whose keys are extension keys, each holding `true`: `{"acme.helper-0.9.0": true}`. An extension key is `<id>-<version>`, with `-<targetPlatform>` added when the extension has one, as in `rust-lang.rust-analyzer-0.3.3081-win32-x64`. The id is `<publisher>.<name>`, lowercased.
  - VS Code extracts an extension into a folder named by that same key, so a key names a folder of `~\.vscode\extensions`.
  - `setExtensionsForRemoval` adds a key when an update or an uninstall leaves a version behind. `deleteExtensionsMarkedForRemoval` deletes those folders when VS Code next starts.
  - VS Code reads the file with `JSON.parse` and takes a file that does not parse as empty.
- **Which registry to read.** VS Code scans a profile's user extensions from its `extensions.json` (`scanUserExtensions` with the profile's location) [binary]. When a folder appears in `~\.vscode\extensions` from outside VS Code, VS Code adds it to the default profile's list ("Added extensions to default profile from external source") [binary]. So for the default profile, the folders without the ones `.obsolete` lists are the extensions `extensions.json` lists, and config-digger reads the folders. A non-default profile with its own list is not read apart.
- Built-in extensions live in `<install>\<commit>\resources\app\extensions\`. **Copilot Chat ships built in** as `extensions\copilot` (`GitHub.copilot-chat` 0.57.0) [disk]. In 1.140.0 it is 0.69.0 and contributes 15 skills [disk].
- VS Code ranks the extensions it registers with built-in extensions first, then the user's, and orders each group by the name of the extension's folder (`workbench.desktop.main.js` of 1.140.0) [binary]. Contributions such as `chatSkills` arrive in that order.
- What VS Code does with a user-installed extension that has a built-in one's id was not checked. Its scanner dedupes extensions by id and logs "Skipping user installed builtin extension …" in some cases [binary]. config-digger reads both.
- An extension that can add MCP servers declares `contributes.mcpServerDefinitionProviders` (`id`, `label`, `when`) in `package.json` ([S-config] L333–366). The servers themselves exist only at runtime through `vscode.lm.registerMcpServerDefinitionProvider` ([D-ext-mcp]) [docs]. Providers on this machine [disk]:
  - `dart-code.dart-code`: `dart-sdk-mcp-servers`
  - `ms-python.vscode-pylance`: `pylanceMcp`
  - built-in Copilot: `github`
- **Last-known servers cache.** VS Code persists each extension collection's server definitions in **workspace** storage key `mcp.extCachedServers`, as `{ "<publisher.ext>/<providerId>": { servers: [{id, label, cacheNonce, launch}] } }` ([S-extdisc] L21, L49–72) [docs]. Five of the twelve workspace DBs here contain it [disk], for example:
  - `dart-code.dart-code/dart-sdk-mcp-servers`, holding one stdio server (`launch.type` 1, `command` = `dart.exe`, nine `args`, `env` keys only).
  - `github.copilot-chat/github`, holding one HTTP server (`launch.type` 2) at `api.githubcopilot.com/mcp/`.

  This lets config-digger list extension Entries without running the extension. The cache can be stale and is per workspace. Tool metadata and trust also go into `mcpToolCache.extensionServers` / `serverTools` at profile and workspace scope ([S-server] L149–183) [docs].
- The extension id is the part before `/` in the collection id. That is how an Entry is attributed to an extension. The Provenance is "plugin or extension", so the Remove action is to uninstall or Disable the extension. config-digger cannot edit the Entry itself.
- Extension skills: `contributes.chatSkills[].path`, relative to the extension folder [disk].

### 5.2 MCP gallery installs

- Installing from the gallery (Extensions view → `@mcp` → Install) writes the server into the **profile `mcp.json`**. "Install in Workspace" writes `.vscode/mcp.json` ([D-mcp-servers]) [docs]. The entry carries `gallery` (URL or `true`) and `version`.
- Gallery metadata is written to `%APPDATA%\Code\User\mcp\<name with '/'→'.'>-<version>\manifest.json` (fields `galleryUrl`, `galleryId`, `name`, `publisher`, `version`, `repositoryUrl`, `manifest`, ...) with an optional `README.md` ([S-mgmt] L548, L572–598, L635–637) [docs]. That folder is absent here [disk]. The `gallery`/`version` fields plus this folder mark the Provenance as "gallery install". Remove = uninstall, which deletes the key from `mcp.json`.
- Other install paths that write the same files: the `vscode:mcp/install?{json}` URL and `code --add-mcp` ([D-ext-mcp]) [docs].

### 5.3 Agent plugins

- Marketplace clones are cached under `%APPDATA%\Code\agentPlugins\github.com\{org}\{repo}`. Plugins installed by the Copilot CLI are discovered from `~/.copilot/installed-plugins/` ([D-plugins]) [docs].
- The installed-plugins store lives at the profile's `agentPluginsHome`, which by default is `%USERPROFILE%\.vscode\agent-plugins` (resolution read in the installed `out\main.js`) [disk]. On `main`, a non-default profile gets `<profile>\agent-plugins` ([S-profile] L205) [docs]. None of these folders exist here [disk].
- Tested by the release gate ([`release-gate.md`](release-gate.md)): VS Code reads the plugins `agent-plugins\installed.json` lists, `{"version": 1, "installed": [{"pluginUri", "marketplace", "name"}]}`, and only those. A marketplace's plugin sits in VS Code's clone of it, `agent-plugins\<host>\<owner>\<repo>\<source>`, by VS Code's code. Any manifest layout loads, and a plugin's servers are in the `.mcp.json` at its root, plus any file its manifest's `mcpServers` names.
- Settings: `chat.plugins.enabled`, `chat.plugins.marketplaces`, `chat.pluginLocations` (a map of path → enabled) ([D-plugins]) [docs].
- Plugin MCP servers get collection id `plugin.<pluginUri>` and order 350 ([S-plugin] L84–124) [docs].

---

## 6. Native Disable mechanisms

| Item | How | Where it is stored | Tag |
|---|---|---|---|
| MCP server (any collection) | Extensions view / MCP list → Disable (globally) or Disable (workspace); commands `mcpServer.disableGlobally` and `mcpServer.disableForWorkspace` | Key `mcp.enablement` = JSON array `[[definitionId, false], ...]`. **Profile scope**: `User\globalStorage\state.vscdb` (default profile) or `User\profiles\<id>\globalStorage\state.vscdb`. **Workspace scope**: `User\workspaceStorage\<id>\state.vscdb`. A workspace entry beats the profile entry, and when no entry exists the server is enabled | [docs] [S-enablement] L44–172, [S-service] L52; command ids and key in installed bundle [disk]. No `mcp.enablement` key on this machine [disk] |
| MCP server (Shadowed by collision) | automatic, see §4 | the same key, written when the user picks a different winner | [docs] |
| all MCP | `chat.mcp.access: "none"` or policy `ChatMCP` | settings / registry | [disk] (bundle) |
| skill | Agent Customizations UI → disable | Key `chat.disabledPromptFiles.skill` = JSON array of file URIs, **profile scope only** (`StorageTarget.USER`, so it may sync) | [docs] [S-prompts-svc] L1004–1030. Prefix `chat.disabledPromptFiles.` is in the installed bundle [disk] |
| skill (soft) | frontmatter `disable-model-invocation: true` or `user-invocable: false` | inside `SKILL.md` | [docs] [D-skills]. Edits a shared file, so this is not a config-digger Disable |
| all skills | `chat.useAgentSkills: false` | settings | [docs] |
| extension | Extensions view → Disable / Disable (Workspace) | Global state key `extensionsIdentifiers/disabled` = `[{"id":"<ext id>"}]`. Here it holds `github.copilot-chat` [disk]. The per-workspace variant is in workspace storage | [disk] (global), [docs] [D-ext] (workspace) |
| agent plugin | Extensions view / Agent Customizations → Disable | "stored separately from the plugin configuration" ([D-plugins]). `chat.pluginLocations` also maps path → bool | [docs] |
| trust | `MCP: Reset Trust` | `trustedAtNonce` inside `mcpToolCache` (profile and workspace) | [docs] [S-server] L121, [S-service] L160–167 |

**Trust.** Collections from `mcp.json`, `.vscode/mcp.json`, `.mcp.json`, extensions and plugins are `Trusted`. Workspace-scoped ones also require Workspace Trust. Only collections imported from other apps are `TrustedOnNonce`: the user is asked again whenever the launch config hash changes ([S-registry] L224–283, [S-installed] L136, [S-dotmcp] L69) [docs]. **[contradicts docs]**: the docs say a trust dialog appears "when a server first starts or its configuration changes" ([D-mcp-servers]) without saying that this applies only to discovered servers ([D-mcp-config] does say "non-workspace sources").

**Agents window caveat.** VS Code 1.129 has a built-in "Agents" profile at `User\profiles\builtin\agents` with its own `globalStorage\state.vscdb`. Its `useDefaultFlags` share settings, MCP, prompts and extensions with the default profile, but **not `globalState`** [disk]. Profile-scoped Disable state (`mcp.enablement`, `chat.disabledPromptFiles.skill`) set from the Agents window is therefore stored in that separate DB ([S-profile] L24, L232, L402) [docs]. The window's workspace is `User\agent-sessions.code-workspace` [disk].

---

## 7. Does the Agent rewrite files while running? Process names

- **User `mcp.json`**: add, install, uninstall and sandbox edits go through `McpResourceScannerService`. It parses the file and writes it back with `JSON.stringify(..., '\t')`, so **comments and formatting are lost**, and **it deletes the file when no servers, inputs or sandbox remain** ([S-scanner] L237–290) [docs]. It also watches the file and reloads on change ([S-mgmt] L400) [docs].
- **`.vscode/mcp.json`, `.mcp.json`, `.code-workspace`**: written only on explicit workspace install or remove. `.mcp.json` edits keep the wrapped or bare shape ([S-scanner] L82–170) [docs].
- **`settings.json`**: modified at startup by the legacy-`mcp` migration (§1.2) and by setting migrations such as `chat.useClaudeSkills` → `chat.useAgentSkills` and the boolean → object form of `chat.mcp.discovery.enabled`. The migration code is in the installed bundle [disk].
- **`state.vscdb`** (global, per profile and per workspace): SQLite. Mementos are flushed on `onWillSaveState` and at shutdown, and `state.vscdb.backup` sits beside the DB [disk]. config-digger should only write it while VS Code is stopped, or should avoid writing it.
- **Process**: `Code.exe` under `%LOCALAPPDATA%\Programs\Microsoft VS Code\` (user install) [disk]. The system install path `C:\Program Files\Microsoft VS Code` is absent here [disk]. VS Code was not running during the checks, so child process names (extension host, stdio MCP servers) were not observed. Insiders: see §9.
- VS Code also watches the external files it imports (for example `.mcp.json`) and reloads them ([S-dotmcp] L114–119) [docs].

---

## 8. Where VS Code records opened projects

- `%APPDATA%\Code\User\workspaceStorage\<id>\workspace.json` holds `{"folder": "<uri>"}` or `{"workspace": "<.code-workspace uri>"}`. There are 12 folders here. They include `file:///e%3A/projects/claudio/petme`, a WSL remote `vscode-remote://wsl%2Bubuntu/...`, and the Agents `agent-sessions.code-workspace` [disk]. Map `<id>` → project by reading this file. Do not try to recompute the hash.
- `%APPDATA%\Code\User\globalStorage\storage.json` holds `profileAssociations.workspaces` (folder URI → profile id; here every folder is `__default__profile__`), `backupWorkspaces.folders`, `windowsState`, and `userDataProfiles` (id, relative `location`, `useDefaultFlags`) [disk].
- Which profile a workspace was last opened in, which config-digger reads as the Project's Last-opened home (#128):
  - The value of `profileAssociations.workspaces` is the profile's id, which is the basename of the profile's stored `location`, such as `-5a1b2c3d` [test] ([S-profile]). The default profile's id is `__default__profile__` [disk], and `userDataProfiles` never lists the default profile ([S-profile]) [docs]. The built-in Agents profile (`builtin/agents`, id `agents`) can never be associated: VS Code throws ([S-profile]) [docs].
  - The key is the workspace's `URI.toString()`: the folder's URI for a folder, and the `.code-workspace` file's URI for a multi-root workspace, its drive in lower case and its `:` encoded, as `file:///e%3A/projects/app` [test]. None of this machine's entries is a workspace file [disk]. Empty windows go to `emptyWindows` ([S-profile]) [docs].
  - VS Code writes the entry on every window open and on every profile switch, and keeps one entry per workspace; `--profile <name>` overwrites it ([S-windows], [S-profile]) [docs]. An extension development window writes none [test].
  - An entry whose id names no profile is skipped, and the next save drops it ([S-profile]) [docs]. A workspace with no entry opens per `--profile`, then `window.newWindowProfile` (a profile name), then the active window's profile, then Default ([S-windows]) [docs]. config-digger reads none of these: they say where the next open goes, not where the last one went.
- The recently-opened list is stored under global state key `history.recentlyOpenedPathsList` ([S-history]). The key is in the installed `out\main.js`, but **it is absent from this machine's `state.vscdb`** [disk]. See Open questions.

---

## 9. macOS / Linux and Insiders differences (not verified)

| Item | Windows | macOS | Linux | Tag |
|---|---|---|---|---|
| User data (`User\mcp.json`, `profiles\`, `globalStorage\`, `workspaceStorage\`) | `%APPDATA%\Code\User` | `~/Library/Application Support/Code/User` | `~/.config/Code/User` | [docs] [D-profiles], [D-settings] |
| Insiders user data | `%APPDATA%\Code - Insiders\User` | `.../Code - Insiders/User` | `~/.config/Code - Insiders/User` | [docs] [D-profiles]. Not installed here [disk] |
| Extensions | `%USERPROFILE%\.vscode\extensions` | `~/.vscode/extensions` | `~/.vscode/extensions` | [docs] [D-ext] |
| Insiders extensions | `%USERPROFILE%\.vscode-insiders\extensions` | same pattern | same pattern | [docs], derived from `dataFolderName` + [disk] resolution code. `~/.vscode-insiders` absent here |
| Agent plugin cache | `%APPDATA%\Code\agentPlugins` | `~/Library/Application Support/Code/agentPlugins` | `~/.config/Code/agentPlugins` | [docs] [D-plugins] |
| Policies | `HKLM\Software\Policies\Microsoft\VSCode` | `.mobileconfig` | `/etc/vscode/policy.json` | [docs] [D-policies] |
| Claude Desktop import | `%APPDATA%\Claude\claude_desktop_config.json` | `~/Library/Application Support/Claude/...` | `$XDG_CONFIG_HOME/Claude/...` | [docs] [D-mcp-config] |
| MCP sandbox (`sandboxEnabled`) | not supported | supported | supported | [docs] [D-mcp-config] |

## 10. Dev containers and remote (out of scope)

- Dev containers: `devcontainer.json` → `customizations.vscode.mcp` is written by VS Code into the **remote** user `mcp.json` ([D-mcp-servers]) [docs].
- Remote (WSL/SSH/tunnels): the remote user `mcp.json` (`mcp.config.usrremote`, order 150) and remote extensions live on the remote host. Remote extensions are not synced ([D-ext]) [docs]. This machine has used WSL (`vscode-remote://wsl+ubuntu/...` in workspace storage, and the `ms-vscode-remote.remote-wsl` extension) [disk].
- The Copilot CLI (`~/.copilot/`) and other Copilot surfaces are separate Agents. They need their own Agent definition.

---

## Product implications for config-digger

1. The VS Code Agent definition needs one Entry source per collection type, and each Entry carries its `order`. Effective/Shadowed is computed per **case-insensitive name** across collections, with the lowest order winning, only when `chat.mcp.collisionBehavior` is `"disable"`, and taking any `mcp.enablement` overrides into account.
2. `.mcp.json` and `~/.claude/skills` / `.claude/skills` are shared with Claude Code. The same file or folder is an Entry for two Agents, and a Remove affects both. Disable should use each Agent's own mechanism.
3. A config-digger Disable for VS Code means writing `mcp.enablement` or `chat.disabledPromptFiles.skill` into SQLite `state.vscdb`, and that is safe only while `Code.exe` is not running. A file-only alternative does not exist for MCP servers short of Remove.
4. Removing from the user `mcp.json` must keep a backup, because VS Code itself may rewrite the file and drop comments.
5. Extension Entries can only be listed from the `mcp.extCachedServers` cache or `package.json` provider ids. Their Provenance is "extension", and Remove/Disable means acting on the extension.

---

## Sources

Docs:

- [D-mcp-config](https://code.visualstudio.com/docs/copilot/reference/mcp-configuration): https://code.visualstudio.com/docs/copilot/reference/mcp-configuration
- [D-mcp-servers](https://code.visualstudio.com/docs/copilot/customization/mcp-servers): https://code.visualstudio.com/docs/copilot/customization/mcp-servers
- [D-skills](https://code.visualstudio.com/docs/copilot/customization/agent-skills): https://code.visualstudio.com/docs/copilot/customization/agent-skills
- [D-prompts](https://code.visualstudio.com/docs/copilot/customization/prompt-files): https://code.visualstudio.com/docs/copilot/customization/prompt-files
- [D-plugins](https://code.visualstudio.com/docs/copilot/customization/agent-plugins): https://code.visualstudio.com/docs/copilot/customization/agent-plugins
- [D-profiles](https://code.visualstudio.com/docs/configure/profiles): https://code.visualstudio.com/docs/configure/profiles
- [D-settings](https://code.visualstudio.com/docs/configure/settings): https://code.visualstudio.com/docs/configure/settings
- [D-ext](https://code.visualstudio.com/docs/configure/extensions/extension-marketplace): https://code.visualstudio.com/docs/configure/extensions/extension-marketplace
- [D-ext-mcp](https://code.visualstudio.com/api/extension-guides/ai/mcp): https://code.visualstudio.com/api/extension-guides/ai/mcp
- [D-policies](https://code.visualstudio.com/docs/enterprise/policies): https://code.visualstudio.com/docs/enterprise/policies
- [D-gh-skills](https://docs.github.com/en/copilot/concepts/agents/about-agent-skills): https://docs.github.com/en/copilot/concepts/agents/about-agent-skills

Source (`microsoft/vscode` @ `64e91bc`):

- [S-config](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/mcpConfiguration.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/mcpConfiguration.ts
- [S-types](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/mcpTypes.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/mcpTypes.ts
- [S-service](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/mcpService.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/mcpService.ts
- [S-enablement](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/chat/common/enablement.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/chat/common/enablement.ts
- [S-registry](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/mcpRegistry.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/mcpRegistry.ts
- [S-server](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/mcpServer.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/mcpServer.ts
- [S-inputs](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/mcpRegistryInputStorage.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/mcpRegistryInputStorage.ts
- [S-installed](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/discovery/installedMcpServersDiscovery.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/discovery/installedMcpServersDiscovery.ts
- [S-dotmcp](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/discovery/workspaceDotMcpDiscovery.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/discovery/workspaceDotMcpDiscovery.ts
- [S-extdisc](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/discovery/extensionMcpDiscovery.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/discovery/extensionMcpDiscovery.ts
- [S-plugin](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/discovery/pluginMcpDiscovery.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/discovery/pluginMcpDiscovery.ts
- [S-workbench](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/browser/mcpWorkbenchService.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/browser/mcpWorkbenchService.ts (L841–916; config ids in https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/services/mcp/common/mcpWorkbenchManagementService.ts L32–35)
- [S-migration](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/browser/mcpMigration.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/browser/mcpMigration.ts
- [S-copilot-global](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/mcpCopilotGlobalConfigurationService.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/mcpCopilotGlobalConfigurationService.ts
- [S-copilot-global-path](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/platform/mcp/common/mcpCopilotGlobalConfiguration.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/platform/mcp/common/mcpCopilotGlobalConfiguration.ts
- [S-rootcfg](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/platform/mcp/common/mcpWorkspaceConfiguration.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/platform/mcp/common/mcpWorkspaceConfiguration.ts
- [S-scanner](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/platform/mcp/common/mcpResourceScannerService.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/platform/mcp/common/mcpResourceScannerService.ts
- [S-mgmt](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/platform/mcp/common/mcpManagementService.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/platform/mcp/common/mcpManagementService.ts
- [S-profile](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/platform/userDataProfile/common/userDataProfile.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/platform/userDataProfile/common/userDataProfile.ts
- [S-locations](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/chat/common/promptSyntax/config/promptFileLocations.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/chat/common/promptSyntax/config/promptFileLocations.ts
- [S-cfgkeys](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/chat/common/promptSyntax/config/config.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/chat/common/promptSyntax/config/config.ts
- [S-prompts-svc](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/chat/common/promptSyntax/service/promptsServiceImpl.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/chat/common/promptSyntax/service/promptsServiceImpl.ts
- [S-history](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/platform/workspaces/electron-main/workspacesHistoryMainService.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/platform/workspaces/electron-main/workspacesHistoryMainService.ts
- [S-windows](https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/platform/windows/electron-main/windowsMainService.ts): https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/platform/windows/electron-main/windowsMainService.ts

On-disk evidence (read-only): `%APPDATA%\Code\User\` (`settings.json`, `globalStorage\state.vscdb`, `globalStorage\storage.json`, `workspaceStorage\*\{workspace.json,state.vscdb}`, `profiles\builtin\agents\`), `%USERPROFILE%\.vscode\extensions\extensions.json`, `%LOCALAPPDATA%\Programs\Microsoft VS Code\8a7abeba6e\{policies\VSCode.admx, resources\app\product.json, resources\app\out\main.js, resources\app\out\vs\workbench\workbench.desktop.main.js, resources\app\extensions\copilot\package.json}`, `%USERPROFILE%\.copilot\`, `%USERPROFILE%\.agents\skills\`, `E:\projects\claudio\{petme,stint}\.mcp.json`.

## Open questions

1. **Version skew.** The docs and `main` describe features that the installed 1.129.1 lacks: the `copilot` discovery source / `~/.copilot/mcp-config.json`, `chat.mcp.workspaceRootConfig.enabled`, and the `ChatAllowedMcpServers`/`ChatDeniedMcpServers` policies. The Agent definition may need a minimum-version field, or detection by feature.
2. **`.vscode/settings.json` → `mcp`.** Is a folder-level `mcp` key in `settings.json` still read, or ignored, now that `mcp` is a standalone `.vscode/mcp.json` configuration? No source line confirming either was found.
3. **Live verification of Disable.** No MCP server is configured or Disabled on this machine, so the `mcp.enablement` value shape (`[[id,false]]`) and the definition-id formats (`mcp.config.usrlocal.<name>`, `mcp.config.ws0.<name>`, `workspace-dot-mcp.0.<name>`) come from source only. They should be confirmed by toggling a test server in a disposable profile. Answered by the release gate ([`release-gate.md`](release-gate.md)): with those pairs planted in a workspace's store and the profile's, VS Code gives the model no tool of the servers they name, those ids, the extension's `<publisher.extension>/<label>`, a plugin's `plugin.<folder URI>.<name>`, a workspace file's `mcp.config.workspace.<name>` and the discovered `claude-desktop.null.<name>`, `cursor-workspace.<folder>.<name>`, `cursor.null.<name>` and `windsurf.null.<name>` alike.
4. **Is `.mcp.json` read unconditionally** in 1.129.1? On `main` a `chat.mcp.workspaceRootConfig.enabled` gate is defined, but this research did not find where it is applied. Answered by the release gate ([`release-gate.md`](release-gate.md)): VS Code reads `.mcp.json` with the setting off; the setting decides only where VS Code adds a server.
5. **Skill precedence within one Scope** (for example `.github/skills` vs `.claude/skills` vs `.agents/skills`): is list order a contract or an implementation detail? Answered by the release gate ([`release-gate.md`](release-gate.md)): VS Code 1.129.1 to 1.141.0 keep the `.agents` skill, then the `.github` one, over the `.claude` one.
6. **Recently opened list.** `history.recentlyOpenedPathsList` is missing from this machine's global `state.vscdb`. Does 1.129 store it elsewhere (for example in application-scoped storage), or was the history cleared?
7. **Insiders process name** (`Code - Insiders.exe`) and paths were not verified because Insiders is not installed.
8. **Agent plugin Disable storage**: the docs say it is "stored separately". The exact key (enablement model on `chat.pluginLocations` vs a storage key) was not traced.
9. **Trust prompts for `mcp.json` servers**: the source marks them `Trusted`, but the docs describe a first-start dialog. This should be confirmed empirically.

<!-- link reference definitions -->
[D-mcp-config]: https://code.visualstudio.com/docs/copilot/reference/mcp-configuration
[D-mcp-servers]: https://code.visualstudio.com/docs/copilot/customization/mcp-servers
[D-skills]: https://code.visualstudio.com/docs/copilot/customization/agent-skills
[D-prompts]: https://code.visualstudio.com/docs/copilot/customization/prompt-files
[D-plugins]: https://code.visualstudio.com/docs/copilot/customization/agent-plugins
[D-profiles]: https://code.visualstudio.com/docs/configure/profiles
[D-settings]: https://code.visualstudio.com/docs/configure/settings
[D-ext]: https://code.visualstudio.com/docs/configure/extensions/extension-marketplace
[D-ext-mcp]: https://code.visualstudio.com/api/extension-guides/ai/mcp
[D-policies]: https://code.visualstudio.com/docs/enterprise/policies
[D-gh-skills]: https://docs.github.com/en/copilot/concepts/agents/about-agent-skills
[S-config]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/mcpConfiguration.ts
[S-types]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/mcpTypes.ts
[S-service]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/mcpService.ts
[S-enablement]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/chat/common/enablement.ts
[S-registry]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/mcpRegistry.ts
[S-server]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/mcpServer.ts
[S-inputs]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/mcpRegistryInputStorage.ts
[S-installed]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/discovery/installedMcpServersDiscovery.ts
[S-dotmcp]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/discovery/workspaceDotMcpDiscovery.ts
[S-extdisc]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/discovery/extensionMcpDiscovery.ts
[S-plugin]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/discovery/pluginMcpDiscovery.ts
[S-workbench]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/browser/mcpWorkbenchService.ts
[S-migration]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/browser/mcpMigration.ts
[S-copilot-global]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/mcp/common/mcpCopilotGlobalConfigurationService.ts
[S-copilot-global-path]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/platform/mcp/common/mcpCopilotGlobalConfiguration.ts
[S-rootcfg]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/platform/mcp/common/mcpWorkspaceConfiguration.ts
[S-scanner]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/platform/mcp/common/mcpResourceScannerService.ts
[S-mgmt]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/platform/mcp/common/mcpManagementService.ts
[S-profile]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/platform/userDataProfile/common/userDataProfile.ts
[S-locations]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/chat/common/promptSyntax/config/promptFileLocations.ts
[S-cfgkeys]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/chat/common/promptSyntax/config/config.ts
[S-prompts-svc]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/workbench/contrib/chat/common/promptSyntax/service/promptsServiceImpl.ts
[S-history]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/platform/workspaces/electron-main/workspacesHistoryMainService.ts
[S-windows]: https://github.com/microsoft/vscode/blob/64e91bc28bed22fb021da2cbb776de8f37e3463d/src/vs/platform/windows/electron-main/windowsMainService.ts
