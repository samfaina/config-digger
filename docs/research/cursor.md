# Research: Cursor configuration on Windows

Issue: [#11](https://github.com/samfaina/config-digger/issues/11). Terms follow [`CONTEXT.md`](../../CONTEXT.md).

Verified against **Cursor 3.22.12** (commit `3a92974361033b2051526321308c2740fe5912c0`, built 2026-09-26), a per-user install at `%LOCALAPPDATA%\Programs\cursor\`, on Windows 11, home `C:\Users\kgon1`, on 2026-10-01. Issue #7 installed 3.19.7. Cursor updated itself to 3.22.12 before this research ran, so the Agent definition can't assume a pinned version. Cursor was **running** during the checks, with the Agents Window open on the scratch project `E:\scratch\config-digger-agents\cursor`. SQLite databases were copied to a temp folder together with their `-wal`/`-shm` files and opened with `sqlite3 -readonly`. The live files were never opened.

Cursor is closed source. Code citations point at the minified JavaScript shipped in the install. Each one gives the file and a literal string that can be grepped:

- **[W]** `resources\app\out\vs\workbench\workbench.desktop.main.js` (the MCP service, storage keys, the Customize UI)
- **[X]** `resources\app\extensions\cursor-agent-exec\dist\main.js` (plugin and skill loading)
- **[CLI]** `%APPDATA%\Cursor\User\globalStorage\anysphere.cursor-agent-worker\agent-cli\.local\share\cursor-agent\versions\2026.09.28-64d2043\index.js` (the bundled `cursor-agent` CLI)

Minified identifiers such as `KZe` or `Pl` change between builds. The quoted string literals are the stable anchors.

Each fact is tagged:

- **[disk]**: verified read-only on this machine, in config files, state databases, logs or running processes.
- **[code]**: read from Cursor's shipped JavaScript.
- **[docs]**: stated on cursor.com/docs and not observable here.
- **[test]**: tested on the real Cursor by the release gate's probe, `uat/agents/cursor.mjs` (`docs/research/release-gate.md`).
- **[contradicts docs]**: the code or disk differs from the docs, or goes beyond them.

Secret values are never reproduced. Where one would appear, the text shows only the key name or `***`.

## Summary

Cursor has **two MCP files**: the user file `%USERPROFILE%\.cursor\mcp.json` and the project file `<workspace folder>\.cursor\mcp.json`. Both use root key `mcpServers` and are parsed as JSONC. Plugins add more servers: Cursor Marketplace plugins, local plugins and **Claude Code plugins**. Cursor reads `~/.claude/plugins/installed_plugins.json` and honours Claude's `enabledPlugins`. VS Code extensions, team (dashboard) servers and built-ins add more at runtime. No environment variable moves the IDE's MCP files, and Cursor has no managed MCP file.

**Nothing is merged and nothing is Shadowed by name.** Every Entry becomes its own server with a Scope-prefixed identifier: `user-<name>`, `project-<folderIndex>-<folderName>-<name>`, `plugin-<plugin>-<server>`, `user-<extensionId>-extension-<name>`, or `cursor-*` for built-ins. A user and a project server with the same name both exist. On this machine `cd-throwaway-user` exists twice, as `user-cd-throwaway-user` and `project-0-cursor-cd-throwaway-user`. The only collapsing is a feature-gated **dedupe of non-project remote servers by URL**. Its default pick order is user > project > extension > team > plugin > default > provider, and the user can override the pick.

**Project servers need approval before they run.** Approvals are stored in global `state.vscdb` as `cursor/approvedProjectMcpServers` = `["<identifier>:<configHash>"]`. Changing a server's config revokes its approval. That is why the hand-written project servers never appeared in the cache. Only `project-0-cursor-playwright` is approved here.

**Disable is native and lives in SQLite.** The Customize toggle writes `cursor/disabledMcpServers`, a JSON array of identifiers, in the **workspace** `state.vscdb`. A feature-gated alternative is `cursor/disabledGlobalMcpServers` at profile scope. Project plugins are enabled through `<project>\.cursor\settings.json` → `plugins.<name>.enabled`. User-scope Cursor plugins are installed and enabled **on the Cursor account (server side)**, with only a cache in `state.vscdb`. Claude Code plugins follow Claude's own `enabledPlugins`. One app-wide switch, **Include Third-Party Plugins, Skills, and Other Configs** (`cursor/thirdPartyExtensibilityEnabled`, on by default), turns off everything Cursor imports from Claude, Codex and Grok. No local per-skill Disable was found.

**Skills** are `SKILL.md` folders under `.cursor/skills`, `.claude/skills`, `.codex/skills`, `.grok/skills` and `.agents/skills`. Cursor reads each of these at user level (`~`) and in every workspace folder, including nested project subfolders. Built-ins live in `~/.cursor/skills-cursor`, and plugins add more. Skills are not overridden by name. Two folders collapse into one only when they share the same root and relative path. Then the first config folder wins, in the order `.cursor` > `.claude` > `.codex` > `.grok` > `.agents`.

**Cursor writes these files while running.** Its UI and deeplink code re-serialize the MCP files with `JSON.stringify(…, null, 2)`, which drops comments. The deeplink installer parses the user file with strict `JSON.parse`, so a user `mcp.json` that contains a comment would be replaced by a file holding only the new server. That comes from reading the code and was not observed. Cursor also keeps rewriting `~/.cursor/projects/*/mcps/`, `~/.cursor/skills-cursor/`, `~/.cursor/.gitignore` and `state.vscdb`. The process is `Cursor.exe`, and stdio MCP servers run as its children.

---

## 1. MCP server locations per Scope

### 1.1 Locations

| Scope (config-digger) | Windows path | Identifier prefix | Tag |
|---|---|---|---|
| user | `%USERPROFILE%\.cursor\mcp.json` | `user-` | [docs] [D-mcp]; [code] [W] `getUserConfigFilePath(){…joinPath(e,".cursor","mcp.json")}` with `userHome({preferLocal:!1})`; [disk] holds `cd-throwaway-user`, `cd-throwaway-http`, `context7` |
| project (versioned) | `<workspace folder>\.cursor\mcp.json`, one per root folder of the workspace | `project-<folder index>-<folder basename>-` | [docs] [D-mcp]; [code] [W] `joinPath(s.uri,".cursor","mcp.json")`, `computeIdentifierPrefix(e){return e?\`project-${e}-\`:"user-"}`, `folderToKey(e){return\`${e.index}-${e.name}\`}`; [disk] holds `cd-throwaway-project`, `cd-throwaway-user`, `playwright` |
| plugin (Cursor Marketplace) | `%USERPROFILE%\.cursor\plugins\cache\<marketplace>\<plugin>\<git sha>\mcp.json` (or the manifest's `mcpServers`) | `plugin-<plugin>-<server>` | [docs] [D-plugins-ref]; [disk] `plugin-mobbin-mobbin`, `plugin-posthog-mcp-posthog`, `plugin-google-calendar-google-calendar` |
| plugin (local) | `%USERPROFILE%\.cursor\plugins\local\<plugin>\` | `plugin-…` | [docs] [D-plugins]; folder exists and is empty here [disk] |
| plugin (Claude Code) | the Claude plugin's install path from `%USERPROFILE%\.claude\plugins\installed_plugins.json`, using its `.cursor-plugin/plugin.json` if present | `plugin-<plugin>-<server>` | [code] [X] `"plugins","installed_plugins.json"`, `loadClaudePlugin`; [disk] `supabase` is read from `~\.claude\plugins\cache\claude-plugins-official\supabase\0.1.15\agents\cursor\mcp.json`. See §5.3 |
| extension | not in a file: `vscode.cursor.mcp.registerServer()` at runtime | `user-<extensionId>-extension-<name>` | [docs] [D-mcp]; [code] [W] `registerMCPServer(e,t,n){const i=\`extension-${…}\`` |
| team | Cursor dashboard (Plugins & MCPs), delivered over the network | (`dashboardManaged`) | [docs] [D-mcp] "Team MCP distribution"; [code] [W] `teamConfiguredServers` |
| built-in | none: shipped with Cursor (`cursor-app-control`, `cursor-ide-browser`, `cursor-origin-readonly`) | `cursor-` | [disk] `~/.cursor/projects/*/mcps/` |
| managed | **none.** No file declares an MCP server. Enterprise "MCP Allowlist" and network controls are dashboard policies, not Entries | — | [docs] [D-mcp] "Enterprise admin controls" |

- A **local** Scope doesn't exist for Cursor MCP.
- The Customize page draws a "workspace" filter, but that is the project file.
- Only the **root** `.cursor\mcp.json` of each workspace folder is read. Nested folders and parent folders aren't [code] [W] (`refreshProjectManagedServers` loops over `contextService.getWorkspace().folders`).

### 1.2 Overrides

- **IDE**: no environment variable or setting moves `mcp.json`. The path is always `<user home>\.cursor\mcp.json`. In a remote window (WSL/SSH) it is the **remote** home, because the lookup uses `userHome({preferLocal:!1})` [code] [W]. Remote is out of scope here.
- The IDE's own user data sits in `%APPDATA%\Cursor` (`--user-data-dir` in the running command lines [disk]). Extensions sit in `%USERPROFILE%\.cursor\extensions` (`product.json` `dataFolderName: ".cursor"` [disk]).
- **Cursor CLI** (`cursor-agent`, a separate binary bundled under the agent-worker storage): `CURSOR_CONFIG_DIR` > `$XDG_CONFIG_HOME/cursor` > `~/.cursor` for `cli-config.json`, and `CURSOR_DATA_DIR` for `projects/` [code] [CLI] `process.env.CURSOR_CONFIG_DIR`, `process.env.CURSOR_DATA_DIR`; [docs] [D-cli-config]. The CLI still reads MCP from `homedir()/.cursor/mcp.json` [code] [CLI] `join((0,i.homedir)(),".cursor","mcp.json")`. Its approvals and Disables live in separate files (§6).
- None of these variables are set on this machine [disk].
- Policies: the shipped `policies\Cursor.admx` writes to `Software\Policies\Cursor\Cursor` and has no MCP, plugin or skill policy. Its policies include `AllowedExtensions`, `UpdateMode` and `WorkspaceTrustEnabled` [disk]. The key is absent in HKLM and HKCU [disk]. `C:\ProgramData\Cursor` is the enterprise directory for **hooks only** (`hooks.json`) [docs] [D-hooks], [code] [W] `getEnterpriseConfigDirectory()`. It is absent here [disk].

---

## 2. File format and secret-bearing fields

The root key is `mcpServers`, a map from name to server. The user and project files are parsed by a JSONC-tolerant reader. It strips comments and retries without trailing commas [code] [W] `function KZe(e){const t=TQf(e);try{return JSON.parse(t)}catch{…replace(/,\s*([}\]])/g,"$1")…}` (module `"jsonc.js"`). If `mcpServers` is not an object, the whole file is rejected with "Invalid config: mcpServers must be an object" [code] [W].

Fields Cursor reads from user/project files [code] [W] `parseMcpServersFromFile`:

| Transport | Fields read | May hold secrets |
|---|---|---|
| stdio (has `command`) | `command`, `args`, `env`, `envFile`, `enabledTools` | `env` values, `args` (tokens passed as flags), and `envFile` (path to a file of secrets) |
| remote (has `url`, no `command`) | `url`, `headers`, `auth` (`CLIENT_ID`, `CLIENT_SECRET`, `scopes`), `placement`, `enabledTools` | `headers` (for example `Authorization`), `auth.CLIENT_SECRET`, and the `url` query string |

- **Transport is chosen by `command` vs `url`, not by `type`.** A `url` entry always becomes `type:"streamableHttp"`. An entry with neither field is skipped with an error. **[contradicts docs]**: the docs list `type: "stdio"` as required ([D-mcp] "STDIO server configuration"). On this machine the `cd-throwaway-*` entries have no `type` and are accepted [disk].
- **`cwd` is not read** from the user or project file in 3.22.12. Only plugin servers carry `cwd` [code] [W] (the stdio object built in `parseMcpServersFromFile` has no `cwd`; `refreshPluginServers` does). The docs say nothing about `cwd` for `mcp.json`.
- `enabledTools` (a string array) and `placement` are undocumented [code] [W].
- Interpolation: `${env:NAME}`, `${userHome}`, `${workspaceFolder}`, `${workspaceFolderBasename}`, `${pathSeparator}`/`${/}` in `command`, `args`, `env`, `url`, `headers`, and `auth` [docs] [D-mcp]. Plugin `mcp.json` also expands `${CURSOR_PLUGIN_ROOT}` and `${CLAUDE_PLUGIN_ROOT}`, plus plugin `variables` set in the dashboard [docs] [D-plugins-ref]; [code] [CLI] `replace(/\$\{CLAUDE_PLUGIN_ROOT\}/g,…)`.
- **Real secret on disk:** the marketplace/Context7 entry in the user file carries a literal bearer token in `headers.Authorization` [disk] (value not reproduced).
- **OAuth state** for remote servers is kept outside the files. It lives in global `state.vscdb` keys `mcpOAuth.secret.<base64>`, `mcpOAuth.global.<base64>` and `secret://{"extensionId":"anysphere.cursor-mcp",…}`, and in `%APPDATA%\Cursor\User\globalStorage\mcp-oauth-attempts\*.json` [disk] (key names only). config-digger must never read these values.

---

## 3. Skills per Scope

Skill folders are defined as a single list [code] [X]: `Bt=[{configDir:".cursor",subdir:"skills",thirdParty:!1},{configDir:".claude",…,thirdParty:!0},{configDir:".codex",…,thirdParty:!0},{configDir:".grok",…,thirdParty:!0},{configDir:".agents",…,thirdParty:!1},Jt]` with `Jt={configDir:".cursor",subdir:"skills-cursor",…,builtin:!0}`. Each entry is joined to the user home (Scope user) and to every workspace folder (Scope project).

| Scope | Folder | Third-party (gated) | Tag |
|---|---|---|---|
| project | `<folder>\.cursor\skills\` | no | [docs] [D-skills]; [code] |
| project | `<folder>\.agents\skills\` | no | [docs]; [code] |
| project | `<folder>\.claude\skills\`, `<folder>\.codex\skills\` | yes | [docs] "for compatibility"; [code] |
| project | `<folder>\.grok\skills\` | yes | [code] only. **[contradicts docs]**: not listed in [D-skills] |
| project (nested) | any `.cursor/skills` or `.agents/skills` deeper in the repo, auto-scoped to that subtree | — | [docs] [D-skills] "Nested skill directories". **[contradicts docs]**: the five folders, and no subtree scope (below) |
| user | `~\.cursor\skills\`, `~\.agents\skills\` | no | [docs]; [disk] `~\.cursor\skills\context7-mcp` |
| user | `~\.claude\skills\`, `~\.codex\skills\`, `~\.grok\skills\` | yes | [docs] (not `.grok`); [disk]: Customize lists 16 skills from `~\.claude\skills`, including the Claude-Desktop-synced `~\.claude\skills\synced\<uuid>\…` |
| built-in | `~\.cursor\skills-cursor\` (26 folders here, plus `.sync-manifest.json`) | no | [code] `Jt`; [disk]. Managed by Cursor, see §7 |
| plugin | `skills\` inside a Cursor or Claude Code plugin (or the manifest `skills` path) | Claude plugins: yes | [docs] [D-plugins-ref]; [disk] superdesign, supabase@claude-plugins-official, mattpocock-skills@claude-plugins-official, material-3@material-3-skill |

- Folders are walked **recursively**. Identity comes from the folder that holds `SKILL.md` [docs] [D-skills].
- Paths with a dot-folder below `…/skills/` are skipped, so `~\.claude\skills\.trash\` is ignored here [disk]. So are Codex system skills under `.codex/skills` (`imagegen`, `openai-docs`, `plugin-creator`, `skill-creator`, `skill-installer`) [code] [X] `Dt=new Set(["imagegen","openai-docs",…])`.
- Frontmatter: `name` (must match the folder), `description`, `paths` (legacy `globs`), `disable-model-invocation`, `icon`, `color`, `metadata` [docs] [D-skills].
- The third-party folders load only while **Include Third-Party Plugins, Skills, and Other Configs** is on [code] [X] `function Ut(e){return Bt.filter(t=>!t.builtin&&(e||!t.thirdParty))}`.

#### Nested skills folders

Read in the 3.27.12 bundle, `NestedExtensibilityService.discover` and `AgentSkillsCursorRulesService.loadAll` [X], and tested with the release gate's probe, `uat/agents/cursor.mjs`, its `nested-skills` launches (**[test]**, see `docs/research/release-gate.md`):

- The IDE builds its skills service with nested skills on, in its code: `loadNestedSkills` is `true`, and no feature gate or setting turns it off [code] [X] `loadNestedSkills:s`.
- For each workspace folder, Cursor runs ripgrep from the folder with `--files --hidden --no-require-git --follow` and the include globs `*/**/.cursor/**`, `*/**/.agents/**`, and while third-party configs are on `*/**/.claude/**`, `*/**/.codex/**` and `*/**/.grok/**` [code] [X] `buildIncludeGlobs(e){const t=["*/**/.cursor/**","*/**/.agents/**","*/**/AGENTS.md"]`, `followSymlinks:!0,source:"nested_extensibility"`. The leading `*/` keeps the folder's own `.cursor` and the like out, which are read as root folders [code]. Skills one folder down and three are listed, in the `/` menu and in Customize, and one reached through a junction [test].
- A `SKILL.md` found this way is a skill when its path holds `/<configDir>/skills/` for one of the five folders, case aside; the third-party three count only while the switch is on [code] [X] `new RegExp(\`[/\\\\]${ny(e)}[/\\\\]${ny(t)}[/\\\\]\`,"i")`, `if(n&&!t)continue;return"skills"`. Nested `.claude\skills`, `.codex\skills` and `.grok\skills` skills are listed with the switch on and not with it off [test]. **[contradicts docs]**: [D-skills] names only `.cursor/skills` and `.agents/skills`.
- Excluded at any depth, below a skills folder too: `node_modules`, `.git`, `.turbo`, `.next`, `.cache`, `.pnpm`, `.yarn`, `dist`, `build`, `out`, `target`, `.vscode`, `.idea`, `venv`, `__pycache__`, `logs`, `tmp`, `temp` [code] [X] `excludeGlobs:Hr`. A skill below `node_modules` or `dist`, and one at `<skills>\build\<skill>`, are not listed [test]. There is no depth limit [code].
- Ignore files apply, those of the folders above the workspace folder too, as ripgrep reads them, since `--no-ignore-vcs` is not passed [code]. A skills folder below a folder that a `.gitignore` or an `.ignore` leaves out is not listed, nor one in a `.cursor` folder a `.gitignore` leaves out; one whose skills folder itself a `.gitignore` leaves out is listed, since the include globs match what lies below `.cursor` [test].
- No search runs when `git rev-parse --show-toplevel` fails in the workspace folder, so the folder must be in a git work tree, at its root or below it [code] [X] `if(!await Gr(e,this.gitExecutor,(0,v.resolve)(this.root)))`; nor, as written, when the folder is the user's home folder [code] [X] `if(s===o||wt(s,{platform:process.platform,homeDirectory:o}).length>0)`, the second test for macOS only. A Project in no git work tree lists no nested skill, but a git Project that is the home folder lists them [test]: on Windows the first test, which compares the two paths as spelled, did not skip it.
- Nested skills are parsed and filtered as the root ones, by the same code: frontmatter, dot folders below `skills`, Codex's reserved names [code] [X]. `loadAll` appends them after the root and user ones, leaving out only a skill whose real path was already read [code] [X] `u.has(r)||(u.add(r),m.push(t))`.
- **[contradicts docs]**: there is no subtree scope. `scopedTo` comes only from a frontmatter's `metadata.scopedTo` [code].
- A nested skill at the same path below its marker folder as the workspace folder's own skill or a user's is the one listed; of two nested ones, the one whose folder holding the marker comes first by path, whatever the markers, and of one folder's, the `.cursor` one over the `.agents` one [test].
- Cursor's other kinds are separate and not skills: rules (`.cursor/rules/*.mdc`, `~/.cursor/rules/`), subagents (`.cursor/agents`, `.claude/agents`, `.codex/agents`), commands and hooks [docs] [D-subagents], [D-hooks]. **[contradicts docs]**: [D-skills] says user rules "are not stored on the file system". Yet `~\.cursor\rules\context7.mdc` exists [disk], and Cursor's managed `.gitignore` lists `!rules/` as "User-level cursor rules" [disk].

---

## 4. Precedence and merge rules

### MCP servers

1. **No merge.** Each Entry becomes one server keyed by its prefixed identifier (§1.1). Same-named Entries in the user and project files are two independent servers. Neither is Shadowed. [code] [W] `computeIdentifier(e){…return e.extensionId?\`${t}${e.extensionId}-${e.name}\`:\`${t}${e.name}\`}`. [disk] global key `mcpService.knownServerIds` = `["user-cd-throwaway-user","user-cd-throwaway-http","project-0-cursor-cd-throwaway-project","project-0-cursor-cd-throwaway-user","user-context7","project-0-cursor-playwright"]`.
2. All sources are concatenated in this order: built-in/default, user, project, extension, team, plugin, provider. The list then passes through `mcpConfigService.resolveServers` [code] [W] `resolveEffectiveServers(){…n=[...this.defaultServers,...this.userConfiguredServers,...this.projectManagedServers,...this.extensionRegisteredServers,...this.teamConfiguredServers,...this.pluginServers,...t]`.
3. **URL dedupe (feature gate `dedupe_mcp_servers`).** Servers are grouped only if they are remote (`streamableHttp`), **not project-managed**, and share a normalized URL. A group exposes the user's saved selection, kept in storage key `mcpService.selectionPreferences` at profile scope. Without a saved selection, the first member by source kind wins, in the order `["user","project","extension","team","plugin","default","provider"]`. The others are Shadowed [code] [W] `function bJf(e){if(!(e.projectManaged||e.type!=="streamableHttp"||!e.url)…return\`streamableHttp:${…}\`}` and `R4d=["user","project",…]`. stdio servers and project servers are always singletons. Whether the gate is on here couldn't be read, because the Statsig gate names are hashed in `state.vscdb`.
4. **Approval gate (project only).** A project server is enabled only if `cursor/approvedProjectMcpServers` holds its legacy bare identifier or `<identifier>:<hash>`. The hash covers `command, args, env, envFile, url, headers, auth` [code] [W] `computeServerConfigHash(e){const t=["command","args","env","envFile","url","headers","auth"]…}` and `isServerEnabledByUser`. Any config change deletes the approval ("Server modified (hash changed), deleting old client and removing approval").
   - How the hash is made, from the same function and `parseMcpServersFromFile` [code] [W]. The hash is computed over the server object the parser builds, not over the file's entry. A server with a truthy `command` becomes `{command, args, env, envFile}`. Otherwise it becomes `{url, headers, auth}`, with the `url` passed through `ensureUrlHasProtocol`, which prepends `http://` unless the URL matches `/^[a-z]{1,8}:\/\//`. The hash is `ek(JSON.stringify(n)).toString(16).substring(0,16)`. Here `n` holds those of the seven fields the object has, in the listed order. `ek` is VS Code's `hash()` string hash: start at 149417, then `h = (h << 5) - h + charCode | 0` per UTF-16 unit. Its result is a signed 32-bit number, so a negative hash is written with a leading `-`.
   - [disk] This reproduces the one approval on this machine: `{"command":"npx","args":["@playwright/mcp@latest"]}` hashes to `62809ed2`.
5. **What this machine shows** [disk]:
   - The user-scope `cd-throwaway-user` is running. A single `npx -y @modelcontextprotocol/server-everything` child runs without `--shadow-test`.
   - The project-scope `cd-throwaway-user` and `cd-throwaway-project` are configured but unapproved, so they are not started.
   - `playwright` is approved (`project-0-cursor-playwright:62809ed2`) and running (`npx @playwright/mcp@latest`).
   - The name-clash test therefore reads as "two Entries, two servers, no precedence", not as Shadowing.
6. The docs don't describe precedence ([D-mcp] is silent). **[contradicts docs]** only in the sense that the behaviour is entirely undocumented.
7. **CLI difference.** The CLI has a helper that merges the two files **by name with project winning**: `{mcpServers:{...n.mcpServers,...s.mcpServers}}` [code] [CLI]. It also uses the same `project-<i>-<basename>-` / `user-` prefixes elsewhere. Where that merged view is used was not traced (Open questions).

### Skills

- No override by name. Entries are deduplicated by a **path key**, `<root>::skills/<relative path>`. The key is built by cutting the path at the first `/.cursor/`, `/.claude/`, `/.codex/`, `/.grok/` or `/.agents/` [code] [X] `function sr(e){…indexOf("/.cursor/")…indexOf("/.agents/")…}`, `function or(e,t){…r.get(e)…}`. The first occurrence of a key wins. Load order follows `Bt`: `.cursor` > `.claude` > `.codex` > `.grok` > `.agents`.
  - [disk]: `~\.claude\skills\computer-use` (a symlink) and `~\.agents\skills\computer-use` both exist. Customize lists only the `.claude` copy.
- Different roots never collapse. A user skill and a project skill with the same name both appear. `unslop` appears twice here, from `~\.claude\skills\unslop` and `~\.claude\skills\synced\<uuid>\unslop` [disk].
- The merged list is sorted by tier, Builtin (0) < UserStore (1) < UserHome (2) < Workspace (3) < Plugin (4) [code] [X] `e[e.Builtin=0]="Builtin",e[e.UserStore=1]…e[e.Plugin=4]="Plugin"`. Which of two same-named skills `/name` resolves to is not settled (Open questions).

### Plugins

- Local `~/.cursor/plugins/local` vs marketplace: "If a marketplace plugin with the same name is already installed, that install takes precedence over the local copy" [docs] [D-plugins]. **[contradicts docs]**: the loader never dedupes a `"user-local"` plugin by name, only by the real path of its folder [code] [X] `cd()` returns `undefined` for `"user-local"`.
- Local plugins: every folder or link in `~/.cursor/plugins/local` whose name does not start with `.`, with or without a manifest, named by the folder [code] [X] `if(r.name.startsWith("."))continue;`, `{source:"user-local",sourceInfo:{name:r.name,localPath:n}}`. A team admin can turn them all off (`allowUserLocalPluginImports`) [code] [W].
- A plugin's servers are named `plugin-<name>-<server>` after the name of its key or folder, not its manifest's `name` [code] [X] `` `plugin-${n}-${e}` `` with `n=r.identifier.sourceInfo.name`.
- A manifest's `skills`, a path or a list of paths, replaces the `skills` folder [code] [X] `void 0!==s?.skills?this.discoverFromManifestPaths(n,s.skills,!0,hu):i.has("skills")?this.discoverSkills(…)`.
- Neither a Feature gate nor the third-party switch holds back Cursor's own plugins [code] [X] `loadCursorFirstParty:!0`, `filter(l=>t&&n||l.pluginSource!=="claude-plugin")`.
- Project-scope plugin keys are deduplicated across workspace folders, and the first folder wins [code] [X] `function up(e){…r.has(t)||(r.add(t),n.push(…))}`. A key without `gitUrl` goes to Cursor's server, which picks the marketplace of a bare `<name>` [code] [X] `resolveProjectPlugins`. The commit is resolved over the network on each load, and the folder `cache\<marketplace>\<name>\<commit>` is used once it holds `.cache-complete`; the other commits are pruned [code] [X] `markCacheComplete`, `pruneOldVersions`. The release gate's probe saw a bare key resolve to `cursor-public` on an account in no team, and nothing resolve without an account (`docs/research/release-gate.md`).
- Claude Code plugins: one install per plugin id. A project/local install whose `projectPath` matches the workspace beats the user install [code] [X] `function Pl(e,t,r,n)`.

---

## 5. How plugins and extensions are recorded on disk (attribution)

### 5.1 Cursor Marketplace plugins

- **Cache (not install state):** `~\.cursor\plugins\cache\<marketplace>\<plugin>\<40-hex git sha>\`, with a `.cache-complete` marker [disk]. The marketplace folder is `cursor-public` here. Old versions are pruned (`pruneOldVersions:!0`) [code] [X].
  - **A cached plugin is not necessarily installed.** `vercel` is cached here, but its server ran only from 15:52:29 to 15:52:30 in the session logs. It is absent from the installed-ids cache and from `~/.cursor/projects/*/mcps` [disk].
  - The caches also lag behind each other. `plugin-firebase-firebase` connected at 16:54:56 and its `npx firebase-tools mcp` child was running at about 17:05. Yet firebase is in neither the installed-ids cache nor the `mcps` cache [disk].
- **Manifest lookup order:** `.cursor-plugin/plugin.json` > `.claude-plugin/plugin.json` > `plugin.json` [code] [X] `B=[".cursor-plugin/plugin.json",".claude-plugin/plugin.json","plugin.json"]`. The MCP file comes from the manifest `mcpServers` (a path, inline object or array) or from `mcp.json`/`.mcp.json` at the plugin root [docs] [D-plugins-ref]; [code] [CLI] `Ie=[".mcp.json","mcp.json"]`. Several plugins here ship manifests for many agents (`.claude-plugin`, `.codex-plugin`, `.grok-plugin`, `.kimi-plugin`, `gemini-extension.json`) [disk].
- **User-scope install state is on the Cursor account.** The client calls `aiserver.v1.InstallUserPluginRequest`, `UninstallUserPluginRequest`, `UpdateUserPluginInstallRequest` and `ListUserPluginInstallsRequest` [code] [W]. The local trace is a cache in global `state.vscdb`, key `cursor.plugins.installedIds.<team id or "no-team">|<workspace URI or "no-workspace">`. Its value is `[{"id":"<marketplace plugin id>","sources":["user"|"project"]}]` [code] [W] `iId="cursor.plugins.installedIds"`; [disk] `no-workspace` → ids `45893411`, `52632771` (user); the scratch folder adds `55647251`, `69591835` (project).
- **Project-scope install state is a versioned file.** `<project>\.cursor\settings.json` → `plugins.<key>` = `{ "enabled": true, "gitUrl"?, "gitRef"? }`. Key is `<name>` or `<marketplace>/<name>` [code] [X] `ip(e,t)` writes `["plugins",t],{enabled:!0}` and `ap` adds `gitUrl`/`gitRef`; [disk] `superdesign`, `posthog-mcp`.
- **Attribution map, id → name → files:** the workspace `state.vscdb` key `workbench.customize.primitiveSourceSnapshot.mcps.v3` (and `.skills.v3`) holds the Customize page's last snapshot. Each descriptor lists `{id, kind, name, filePath, scope ("user"|"project"|"plugin"), sourceLabel, pluginId?, marketplaceId?}` [disk]. Example: `mobbin` → `pluginId 52632771`, `marketplaceId 34`, file `…\cache\cursor-public\mobbin\d24b…\mcp.json`. It is a UI cache that is written while Cursor runs. It can be stale and exists only for workspaces that were opened. The built-ins (`cursor-app-control`, `cursor-ide-browser`, `Origin`) are mislabelled there with `filePath` = the user `mcp.json` [disk]. They are not in that file.

### 5.2 Per-project MCP cache (`~/.cursor/projects`)

- Path: `~\.cursor\projects\<slug>\mcps\<identifier>\` holds `SERVER_METADATA.json` (`{serverIdentifier, serverName}`), `INSTRUCTIONS.md` and `tools/` [disk].
  - The slug replaces every non-alphanumeric character with `-`, collapses runs and trims [code] [CLI] `function s(e){return e.replace(/[^a-zA-Z0-9]/g,"-").replace(/-+/g,"-").replace(/^-+|-+$/g,"")}`. It is lossy, and the drive letter shows lowercased (`e-scratch-config-digger-agents-cursor`).
  - `empty-window` holds the no-folder window.
- **The folder name is the server identifier**, so it encodes Provenance (`user-`, `project-<i>-<name>-`, `plugin-`, `cursor-`). In `project-0-cursor-playwright`, **`0` is the workspace-folder index and `cursor` is the folder's basename** (§1.1).
- It is written by the agent's "MCP FileSystem Writer". A server is written only after it connects and exposes tools, resources, prompts or instructions [disk] logs: `Server "user-context7" excluded from write: tools=0, resources=0, prompts=0, hasInstructions=false`. Unapproved, Disabled or failing servers are therefore missing. It is a derived cache, not configuration. config-digger should not treat it as an Entry source.

### 5.3 Claude Code plugins

- Cursor reads `%USERPROFILE%\.claude\plugins\installed_plugins.json` (`plugins.<id>[] = {scope, projectPath?, installPath}`). It also reads `enabledPlugins` from `~\.claude\settings.json`, and for each workspace from `<ws>\.claude\settings.json` and `<ws>\.claude\settings.local.json` [code] [X] `ku(e)` → `join(r,".claude")`, `xu`, `Eu`.
- Rules [code] [X] `function Pl(e,t,r,n)`:
  - a `user` install loads if user `enabledPlugins[id] === true`;
  - a `project`/`local` install loads if its `projectPath` equals the workspace and the workspace `enabledPlugins[id] === true`;
  - a workspace `enabledPlugins[id] === false` blocks the plugin in that workspace.
- Plugin source is tagged `"claude-plugin"`. Skills, rules and agents from it are dropped while the third-party switch is off [code] [X] `this.currentImportSettings.importThirdPartyPlugins||(e=e.filter(e=>"claude-plugin"!==e.identifier.source))`. Their MCP servers additionally need feature gate `enable_cc_plugin_import` [code] [W] `refreshPluginServers`.
- On this machine Cursor loads `supabase` (MCP plus 2 skills), `mattpocock-skills` and `material-3` (skills). `superpowers` is skipped (`enabledPlugins` false). `ponytail` is skipped (a `local` install for another project) [disk]. Logs show `loadClaudePlugin mattpocock-skills@claude-plugins-official loaded in 17.8ms` [disk].
- **[contradicts docs]**: [D-plugins] and [D-third-party-hooks] mention only Claude **hooks** behind the third-party switch. Neither says Claude Code **plugins** are imported.

### 5.4 VS Code extensions

- `~\.cursor\extensions\extensions.json` (VS Code format). Two Anysphere remote extensions are installed here [disk]. An extension can add MCP servers only at runtime through `vscode.cursor.mcp.registerServer()` [docs] [D-mcp]. No disk trace of those servers was found.

---

## 6. Native Disable mechanisms

| Item | How | Where it is stored | Tag |
|---|---|---|---|
| MCP server (default) | Customize → MCP → toggle | `cursor/disabledMcpServers` = JSON array of identifiers, **workspace scope**: `%APPDATA%\Cursor\User\workspaceStorage\<id>\state.vscdb`. Present here as `[]` | [code] [W] `disabledMcpServers:vke([],1,0)` (scope 1 = workspace), `isServerEnabledByUser`; [disk] |
| MCP server (gate `mcp_profile_scoped_global_disabled_state`) | same toggle, non-project servers | `cursor/disabledGlobalMcpServers`, **profile scope** (global `state.vscdb` for the default profile). Absent here | [code] [W] `disabledGlobalMcpServers:vke([],0,0)`, `getDisabledServersListFor` |
| project MCP server (trust) | approval prompt | `cursor/approvedProjectMcpServers`, **application scope**, global `state.vscdb` | [code] [W] `approvedProjectMcpServers:vke([],-1,0)`; [disk]. Approval is trust, not a config-digger Disable |
| MCP tool | Customize → per-tool toggle; `enabledTools` in `mcp.json` | `mcpDisabledTools` inside the reactive-storage blob `src.vs.platform.reactivestorage.browser.reactiveStorageServiceImpl.persistentStorage.applicationUser` (global DB; empty here) | [disk] key present; [code] `enabledTools` parsing |
| everything imported from Claude/Codex/Grok | Settings → Agents → Third-Party Imports → **Include Third-Party Plugins, Skills, and Other Configs** | `cursor/thirdPartyExtensibilityEnabled`, application scope, default `true` (key absent here) | [docs] [D-third-party-hooks]; [code] [W] `thirdPartyExtensibilityEnabled:Eh(!0)` |
| Cursor plugin, project Scope | uninstall from Customize removes the key; Cursor only loads keys whose `enabled === true` | `<project>\.cursor\settings.json` → `plugins.<key>.enabled` | [code] [X] `filter(([,e])=>!0===e.enabled)`, `lp(e,t)` removes the key |
| Cursor plugin, user Scope | Customize | Cursor account (server side). No local file | [code] [W] `UpdateUserPluginInstallRequest` |
| Claude Code plugin | Claude's own `enabledPlugins[id] = false` | `~\.claude\settings.json` / `<ws>\.claude\settings(.local).json`. Shared with Claude Code | [code] [X] `Pl`; [disk] `superpowers` |
| skill | **no local per-skill toggle found.** Frontmatter `disable-model-invocation: true` stops auto-invocation only, and editing `SKILL.md` changes a shared file. Managed/team skills have a server-side disabled list (`getDisabledManagedSkillPaths`) | — | [docs] [D-skills]; [code] [X] |
| all MCP / user MCP | enterprise MCP Allowlist (dashboard) | server side | [docs] [D-mcp] |

- **Cursor CLI** keeps its own state per project in `~\.cursor\projects\<slug>\`: `mcp-approvals.json`, `mcp-disabled.json`, `mcp-auth.json` and `mcp-group-selections.json` [code] [CLI] `approvalPath:…join(n,"mcp-approvals.json"),disabledPath:…"mcp-disabled.json"`. These files don't exist here, because the CLI has never been run [disk].
- Writing any `state.vscdb` key is safe only while `Cursor.exe` is stopped (§7).

---

## 7. Does Cursor rewrite files while running? Process names

| File | Who writes it, and how | Tag |
|---|---|---|
| user `~\.cursor\mcp.json` | **UI remove**: JSONC parse, then `JSON.stringify(r,null,2)`, which **drops comments and formatting** (`removeServerFromConfigFile`). **Deeplink install** (`cursor://anysphere.cursor-deeplink/mcp/install`): strict `JSON.parse`. On a parse error it starts from `{mcpServers:{}}` and still writes, **wiping existing servers** (`installProposedServer`). **"Add custom MCP"** opens the file in the editor and creates `{"mcpServers":{}}` if it is missing (`openConfigFile`). The file is watched and reloaded on change | [code] [W] |
| project `.cursor\mcp.json` | UI remove: same `JSON.stringify` rewrite. "Add" opens the file in the editor. Watched (`onDidFilesChange`) | [code] [W]; [disk] local history `%APPDATA%\Cursor\User\History\-76dbb9d5\` shows the project file saved from Cursor's editor at 16:51:57 |
| project `.cursor\settings.json` | plugin install/uninstall through jsonc-parser edits (`tabSize:2`). **Formatting is preserved** | [code] [X] `Zd={formattingOptions:{tabSize:2,insertSpaces:!0,…}}` |
| `~\.cursor\projects\<slug>\mcps\` | rewritten continuously ("Incremental write: 1 changed…") | [disk] logs |
| `~\.cursor\skills-cursor\` | built-in skills re-synced (`.sync-manifest.json`). Folder mtime 17:19 while running | [disk] |
| `~\.cursor\.gitignore` | `# >>> CURSOR MANAGED BLOCK >>>` | [disk] |
| `~\.cursor\plugins\cache\` | clones and prunes plugin versions | [code] [X]; [disk] logs `Plugin missing from cache, installing: google-calendar` |
| `state.vscdb` (global and per workspace) | SQLite in WAL mode. `-wal` was written at 17:07 while running | [disk] |

- **Lead from #11 (Context7 dropped a `//` comment).** Confirmed that Cursor's own writers drop comments. The specific Context7 change wrote `mcp.json` (16:48:49.779), `~\.cursor\rules\context7.mdc` (.980) and `~\.cursor\skills\context7-mcp\SKILL.md` (.982) within 0.2 s [disk]. The Cursor bundle contains no "context7" string [code]. The writer could have been Cursor's deeplink path or Context7's own installer (Open questions).
- **Process**: `Cursor.exe` from `%LOCALAPPDATA%\Programs\cursor\` for a user install, or `%ProgramFiles%\cursor\` for a system install [docs] [D-endpoint]. 18 `Cursor.exe` processes were running here, all with `--user-data-dir=…\AppData\Roaming\Cursor` [disk]. stdio MCP servers run as children, for example `npx.exe` [disk]. The CLI runs as `cursor-agent` (`cursor-agent.cmd`/`.ps1` in the agent-worker storage) [disk].

---

## 8. Where Cursor records the projects it has opened

- `%APPDATA%\Cursor\User\workspaceStorage\<id>\workspace.json` → `{"folder":"file:///e%3A/scratch/config-digger-agents/cursor"}`. An `empty-window` folder sits beside it [disk].
- Global `state.vscdb` [disk]:
  - `cursor/glass.additionalProjects`: the Agents Window project list (`displayPath`, `workspaceIdentifier.id`, `lastUsedAt`).
  - `workspaceMetadata.entries`: `workspaceId`, `folderUri` and worktree info.
  - `cursor.plugins.installedIds.*|<folder URI>`.
- `~\.cursor\projects\<slug>\`: lossy slug, so not reversible. Map it back through the two keys above [disk], [code] [CLI].
- `history.recentlyOpenedPathsList` is `{"entries":[]}`. `storage.json` → `profileAssociations` lists only an empty window [disk]. Cursor 3.x opens projects inside the Agents Window ("glass"), so the VS Code recent-list keys don't record them.

---

## 9. macOS / Linux differences (not verified)

| Item | Windows | macOS | Linux | Tag |
|---|---|---|---|---|
| MCP, skills, plugins, projects | `%USERPROFILE%\.cursor\…` | `~/.cursor/…` | `~/.cursor/…` | [docs] [D-mcp], [D-skills] |
| User data (`User\settings.json`, `state.vscdb`, `workspaceStorage`) | `%APPDATA%\Cursor` | `~/Library/Application Support/Cursor` | `~/.config/Cursor` | [docs] Windows only ([D-endpoint]); macOS/Linux inferred from the VS Code base. Not verified |
| Enterprise hooks dir (no MCP) | `C:\ProgramData\Cursor` | `/Library/Application Support/Cursor` | `/etc/cursor` | [docs] [D-hooks]; [code] [W] `getEnterpriseConfigDirectory` |
| CLI config | `%USERPROFILE%\.cursor\cli-config.json` | `~/.cursor/cli-config.json` | `$XDG_CONFIG_HOME/cursor/cli-config.json` if set | [docs] [D-cli-config]; [code] [CLI] |
| App binaries | `%LOCALAPPDATA%\Programs\cursor` / `%ProgramFiles%\cursor` | `/Applications/Cursor.app` | — | [docs] [D-endpoint] |

---

## Leads from #11, checked

| Lead | Verdict |
|---|---|
| Cursor rewrites `mcp.json` on UI edits and drops comments | **Confirmed for Cursor's code paths** (§7). The project file kept its formatting because the human edited it in Cursor's editor, not through a rewrite. Who wrote the Context7 change is unresolved |
| The per-project cache encodes Provenance; what is the `0`? | **Confirmed.** The `0` is the workspace-folder index, followed by the folder basename. The cache holds only connected servers that have tools (§5.2) |
| Hand-written project servers are missing from the cache: approval? | **Confirmed.** Approval is stored in `cursor/approvedProjectMcpServers` (global `state.vscdb`) as `identifier:hash`. The name-clash test is readable: no Shadowing, the project copy is just unapproved (§4) |
| Cursor loads Claude Code plugins | **Confirmed** from code and disk, including how it picks them (§5.3) |
| Plugin enablement and user-level location | Project: `.cursor/settings.json`. **User: server side**, cached in `cursor.plugins.installedIds.*`. `google-calendar` shows in both projects because it is installed at user Scope. `vercel` is cached but uninstalled, and firebase's state is ambiguous (§5.1) |
| Marketplace installs write real credentials inline | **Confirmed** (`headers.Authorization` in the user file). OAuth tokens also sit in `state.vscdb` (§2) |

---

## Product implications for config-digger

1. **Effective/Shadowed works differently for Cursor.** Entries are not Shadowed by name across Scopes. Both copies are live servers. config-digger should report same-named Entries as two Items, or as one Item with two Effective Entries, not as winner and loser. Shadowing applies only to the gated remote-URL dedupe.
2. **A new Entry state, "pending approval".** A project Entry that isn't in `cursor/approvedProjectMcpServers` (with the current hash) is configured but inert. Telling it apart from a Disabled Entry needs read access to global `state.vscdb`.
3. **Disable means writing SQLite in the workspace DB** (`cursor/disabledMcpServers`). The DB is located by matching `workspaceStorage\*\workspace.json` to the folder. The write is safe only while `Cursor.exe` is stopped. A user-Scope server is Disabled per workspace, not globally, unless the profile-scope gate is on.
4. **Cross-Agent Entries.** Cursor reads `~/.claude/skills`, `.claude/skills`, `~/.codex/skills`, `.agents/skills`, and Claude Code plugins with their `enabledPlugins`. One folder or plugin is an Entry for several Agents, and Remove affects all of them. Disabling a Claude plugin through `enabledPlugins` Disables it in both Claude Code and Cursor.
5. **User-scope Cursor plugins have no local source of truth.** config-digger can list them from the cache keys and the Customize snapshot, but it can't Disable or Remove them without the Cursor account. Its Provenance is "plugin, account-managed", and the only action to offer is "open Cursor".
6. **Write safety.** Cursor rewrites both MCP files with `JSON.stringify(…,2)`. Its deeplink installer treats an unparsable user file as empty. config-digger should keep the user `mcp.json` strict JSON, never add comments, and back it up before edits.
7. The `~/.cursor/projects/*/mcps` cache and the Customize snapshot are good **attribution aids**, but they are not Entry sources. Both are derived and can be stale.

---

## Sources

Docs (fetched 2026-10-01 as Markdown from `cursor.com/<path>.md`, listed in https://cursor.com/llms.txt):

- [D-mcp]: https://cursor.com/docs/mcp
- [D-skills]: https://cursor.com/docs/skills
- [D-plugins]: https://cursor.com/docs/plugins
- [D-plugins-ref]: https://cursor.com/docs/reference/plugins
- [D-customize]: https://cursor.com/docs/customize-cursor
- [D-hooks]: https://cursor.com/docs/hooks
- [D-third-party-hooks]: https://cursor.com/docs/reference/third-party-hooks
- [D-subagents]: https://cursor.com/docs/subagents
- [D-cli-config]: https://cursor.com/docs/cli/reference/configuration
- [D-cli-acp]: https://cursor.com/docs/cli/acp
- [D-endpoint]: https://cursor.com/docs/enterprise/endpoint-security
- Changelog, Customize (3.9, Jun 22 2026): https://cursor.com/changelog/customize

Shipped code (Cursor 3.22.12, read-only):

- [W] `%LOCALAPPDATA%\Programs\cursor\resources\app\out\vs\workbench\workbench.desktop.main.js`
- [X] `%LOCALAPPDATA%\Programs\cursor\resources\app\extensions\cursor-agent-exec\dist\main.js`
- [CLI] `%APPDATA%\Cursor\User\globalStorage\anysphere.cursor-agent-worker\agent-cli\.local\share\cursor-agent\versions\2026.09.28-64d2043\index.js`
- `%LOCALAPPDATA%\Programs\cursor\resources\app\product.json`, `policies\Cursor.admx`

On-disk evidence (read-only):

- `%USERPROFILE%\.cursor\`: `mcp.json` (structure only), `plugins\cache\`, `projects\*\mcps\*\SERVER_METADATA.json`, `skills\`, `skills-cursor\`, `rules\`, `.gitignore`.
- `E:\scratch\config-digger-agents\cursor\.cursor\{mcp.json,settings.json}`.
- `%APPDATA%\Cursor\User\`:
  - `settings.json`;
  - `globalStorage\{state.vscdb,storage.json}` (copied);
  - `workspaceStorage\{ebf8269ee7453051eebf6d3b33565220,empty-window}\` (copied);
  - `History\` (metadata only).
- `%APPDATA%\Cursor\logs\20261001T151939\` (MCP, plugin and file-writer logs).
- `%USERPROFILE%\.claude\plugins\installed_plugins.json` and `%USERPROFILE%\.claude\settings.json` (structure only).
- The running process list.

## Open questions

1. **Which same-named skill wins on `/name`?** The loader keeps both a user and a project skill, and both copies of `unslop` here. Whether `/unslop` resolves to the first by tier or prompts the user wasn't settled read-only.
2. **Feature gates.** `dedupe_mcp_servers`, `mcp_profile_scoped_global_disabled_state` and `enable_cc_plugin_import` decide Shadowing, the Disable key and Claude-plugin MCP import. Their values are hashed in the Statsig bootstrap and could not be read. `enable_cc_plugin_import` is evidently on, because supabase's MCP server loads. The other two decide whether the Agent definition has to support both variants.
3. **Who wrote the Context7 change** (a Cursor deeplink or Context7's installer), and whether that path dropped the `//` comment or it was already gone. Settling it means reproducing it on a disposable home.
4. **The census mismatch.** The last "Plugin MCP census" logged `projectServers: 2` while the project file now holds three servers. The census is logged only on plugin refresh, so it may predate the 16:51 save. That needs a fresh log after a restart.
5. **Shape of `cursor/disabledMcpServers` once a server is toggled.** It is presumably an identifier list, since the code compares `includes(e.identifier)`. No server is Disabled here, so the value is `[]`.
6. **User-scope plugin Disable.** Does Customize Disable (rather than uninstall) a user plugin through `UpdateUserPluginInstallRequest`? Is any local flag kept?
7. **Does `enabled: false` in `.cursor/settings.json` act as a Disable** that Cursor shows as "installed but off", or does Customize treat it as not installed? The code only shows that it isn't loaded.
8. **CLI merged view.** The CLI's `{...user, ...project}` merge (project wins by name) contradicts the IDE's model. Where the CLI uses it (listing only, or runtime) wasn't traced. The CLI is likely a separate Agent definition anyway.
9. **Remote windows** (WSL/SSH) read `mcp.json` from the remote home. Out of scope for v0.1.
