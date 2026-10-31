# Research: Claude Code configuration on Windows

Issue: [#8](https://github.com/samfaina/config-digger/issues/8). Terms follow [`CONTEXT.md`](../../CONTEXT.md).

Verified against Claude Code **2.1.286** (native install, `installMethod: "native"`) on Windows 11, home `C:\Users\kgon1`, on 2026-10-01, and re-checked against **2.1.302** on 2026-10-17 ([§10](#10-re-check-against-21302)). Each fact is tagged:

- **[disk]**: verified on disk, read-only
- **[docs]**: stated by the docs and not observable on this machine
- **[contradicts docs]**: what's on disk differs from or goes beyond what the docs say
- **[binary]**: read from the strings of the installed `claude.exe`

Secret values are never reproduced. Where one would appear, the text shows only the key name or `***`.

## Summary

Claude Code keeps MCP server Entries in these places. User and local Scope both live in `%USERPROFILE%\.claude.json`: user Scope at the top-level `mcpServers` key, local Scope at `projects["<abs path>"].mcpServers`. Project Scope is `<project>\.mcp.json`. Managed Scope is `C:\Program Files\ClaudeCode\managed-mcp.json`, plus the `managedMcpServers` key in any managed settings source (file, registry or remote). Plugins add servers from files in their install directory, and claude.ai connectors are fetched over the network with nothing on disk. Every file uses the root key `mcpServers`.

For a given server name the **whole Entry from the highest-precedence source wins and fields are never merged**. The order is managed > local > project > user > plugin > claude.ai connector. Scopes compete by name. Plugins and connectors compete by endpoint (URL or command). A `managed-mcp.json` file takes exclusive control: every other Entry stops loading, which is a block and not a Shadowing.

Skills are folders named `<name>/SKILL.md`. They are read from the managed directory, `%USERPROFILE%\.claude\skills`, `.claude\skills` in the project and in its parents up to the repo root, from plugins, and from claude.ai sync. On a name clash the order is enterprise > personal > project. Plugin skills are namespaced as `plugin:skill`, so they never Shadow.

Every Item kind has a native Disable that leaves the Entry in place:

- MCP servers: the `/mcp` toggle, which writes `projects[<path>].disabledMcpServers` in `.claude.json`, plus `disabledMcpjsonServers` and `deniedMcpServers` in settings.
- Skills: `skillOverrides: {"name": "off"}` in settings. This does not apply to plugin skills.
- Plugins: `enabledPlugins["name@marketplace"]: false` in settings.

The Agent rewrites `.claude.json` constantly while it runs: on this machine it changed several times in a few minutes, and the Agent keeps five rotating backups. The Agent also watches and reloads its settings files. A running Agent shows up as `claude.exe` started from `%USERPROFILE%\.local\bin\` (native install), with one `~/.claude/sessions/<pid>.json` file per session. **Claude Desktop is also named `claude.exe`**, so the executable's path is what tells them apart, not the process name.

---

## 1. MCP server locations per Scope

| Scope | Windows path | Key path | Tag |
|---|---|---|---|
| managed (exclusive) | `C:\Program Files\ClaudeCode\managed-mcp.json` | `mcpServers` | [docs]. Absent on this machine [disk] |
| managed (provided) | `managedMcpServers` key in any managed settings source (see below) | `managedMcpServers.<name>` | [docs] |
| local | `%USERPROFILE%\.claude.json` | `projects["<abs project path>"].mcpServers.<name>` | [disk] e.g. `projects["E:/projects/petme-app"].mcpServers.dart` |
| project | `<project root>\.mcp.json` | `mcpServers.<name>` | [disk] e.g. `E:\projects\claudio\stint\.mcp.json` |
| user | `%USERPROFILE%\.claude.json` | top-level `mcpServers.<name>` | [docs]. This machine has no top-level `mcpServers` key [disk] |
| plugin | `<installPath>\.mcp.json` and/or whatever `mcpServers` points to in `<installPath>\.claude-plugin\plugin.json` | `mcpServers.<name>` | [disk] (see §5) |
| claude.ai connectors | Not on disk. Fetched from claude.ai when signed in | n/a | [docs]. On disk only as names in `disabledMcpServers`, e.g. `claude.ai Linear` [disk]. With no config on disk, config-digger cannot see a connector's endpoint, so in v0.1 connectors are no Entries and do not compete by endpoint with plugin servers; the definition carries the Agent note `cloud_connectors_not_on_disk`, as Claude Desktop's does (#113) |
| session only | `--mcp-config <file>` flag | `mcpServers` | [docs] |

Sources: [MCP scopes](https://code.claude.com/docs/en/mcp), [managed MCP](https://code.claude.com/docs/en/managed-mcp).

**Managed settings sources.** These carry `managedMcpServers`, `allowedMcpServers`, `deniedMcpServers` and the other policy keys. Listed highest priority first ([managed settings](https://code.claude.com/docs/en/managed-settings)):

1. Server-managed (remote) settings from claude.ai or a gateway. The local cache is `%USERPROFILE%\.claude\remote-settings.json` ([claude-directory](https://code.claude.com/docs/en/claude-directory)). Absent here [disk].
2. MDM/OS policy: registry value `Settings` (REG_SZ/REG_EXPAND_SZ holding JSON) under `HKLM\SOFTWARE\Policies\ClaudeCode`. Absent here [disk].
3. Files: `C:\Program Files\ClaudeCode\managed-settings.json` plus `C:\Program Files\ClaudeCode\managed-settings.d\*.json`, merged together. Absent here [disk].
4. `HKCU\SOFTWARE\Policies\ClaudeCode` `Settings`. This one is user-writable and applies only when no admin source above it is present. Absent here [disk].

By default (`managedSourcesBehavior: "first-wins"`) only the highest-ranked source that has a policy key applies. With `"merge"`, every admin source is combined. [docs]

**Legacy path:** "Claude Code doesn't read the legacy Windows path `C:\ProgramData\ClaudeCode\managed-settings.json`" ([managed settings](https://code.claude.com/docs/en/managed-settings)) [docs]. That directory does not exist on this machine [disk].

**`CLAUDE_CONFIG_DIR`:** overrides `~/.claude`. Settings, plugins and transcripts then live under it, and `settings.json` is read from `CLAUDE_CONFIG_DIR\settings.json`. The Agent reads the variable only from the launching environment, never from a settings `env` block. Requires v2.1.150+ ([env-vars](https://code.claude.com/docs/en/env-vars), [settings](https://code.claude.com/docs/en/settings)) [docs]. The variable is unset on this machine [disk]. **No source says whether `.claude.json` moves with it** (see Open questions). `CLAUDE_CODE_PLUGIN_CACHE_DIR` separately overrides the plugins root, which defaults to `~/.claude/plugins` ([env-vars](https://code.claude.com/docs/en/env-vars)) [docs].

On Windows `~` is `%USERPROFILE%` ([settings](https://code.claude.com/docs/en/settings)) [docs, disk].

**Watch out:** "local" means two different things. Local Scope for MCP is `~/.claude.json`. Local Scope for settings is `.claude/settings.local.json` ([MCP](https://code.claude.com/docs/en/mcp)) [docs].

## 2. File format and per-server fields

Every MCP file shares the root shape `{"mcpServers": {"<name>": {...}}}`. In `.claude.json` that object sits at the top level (user Scope) or under `projects[path]` (local Scope) ([MCP](https://code.claude.com/docs/en/mcp)) [docs, disk].

| Field | Transport | Notes | May hold secrets |
|---|---|---|---|
| `type` | all | `stdio`, `http`, `sse`, `ws`. `streamable-http` is accepted as an alias of `http` ([managed MCP](https://code.claude.com/docs/en/managed-mcp)). The docs' plugin examples leave it out for stdio entries that have `command` | no |
| `command`, `args` | stdio | `args` is an array | **yes** (tokens passed as args) |
| `env` | stdio | object | **yes** |
| `url` | http/sse/ws | | **yes** (query-string keys) |
| `headers` | http/sse/ws | object | **yes** (`Authorization`, API keys) |
| `headersHelper` | http/sse/ws | path of a script that emits headers | no (it is a path) |
| `oauth` | http/sse | `clientId`, `callbackPort`, `scopes`, `authServerMetadataUrl` | low |
| `timeout`, `alwaysLoad` | all | per-server tool timeout (ms) and eager connect | no |

On this machine [disk]:

- The local-Scope `dart` Entry has fields `type, command, args, env`, with `type=stdio`.
- The project `.mcp.json` Entries `supabase` and `webstorm` have `type, url`, with `type=http`.
- The plugin Entry `supabase` (from the supabase plugin) has `type, url, headers`, with `type=http`.

Variable expansion: `${VAR}` and `${VAR:-default}` work in `command`, `args`, `env`, `url` and `headers`. An unset variable stays literal and the Agent shows a warning. Credential variables such as `ANTHROPIC_API_KEY` read as empty in a remote `url` or in `headers` ([MCP](https://code.claude.com/docs/en/mcp)) [docs]. Plugin entries can also use `${CLAUDE_PLUGIN_ROOT}`, `${CLAUDE_PLUGIN_DATA}`, `${CLAUDE_PROJECT_DIR}` and `${user_config.KEY}` ([plugins reference](https://code.claude.com/docs/en/plugins-reference)) [docs]. **Product implication:** an Entry can show a `${VAR}` reference rather than the real secret value, and config-digger should display it unexpanded. `${CLAUDE_PLUGIN_ROOT}` is the exception: it holds no Secret, Claude Code puts the plugin's folder in its place with forward slashes on Windows ([plugins reference](https://code.claude.com/docs/en/plugins-reference)) [docs], and config-digger shows the folder too, so that two plugins' servers written alike have two endpoints (#115).

Other places where secrets live [disk, names only, never read]:

- `%USERPROFILE%\.claude\.credentials.json`: the sign-in credential file.
- `%USERPROFILE%\.claude\sessions\<pid>.json`: contains a `peerToken` key.
- `%USERPROFILE%\.claude\sessions\<pid>.<hash>.key`.

A plugin `userConfig` value marked `sensitive` goes to OS secure storage. Non-sensitive values are stored in `pluginConfigs` in user `settings.json` ([plugins reference](https://code.claude.com/docs/en/plugins-reference)) [docs].

## 3. Skill locations per Scope

| Scope | Windows path | Tag |
|---|---|---|
| managed ("Enterprise") | `.claude\skills\<name>\SKILL.md` inside the managed settings directory, which works out to `C:\Program Files\ClaudeCode\.claude\skills\<name>\SKILL.md` | [docs]. The full Windows path is derived by joining two doc statements. Absent here [disk] |
| user ("Personal") | `%USERPROFILE%\.claude\skills\<name>\SKILL.md` | [disk] |
| project | `<cwd>\.claude\skills\<name>\SKILL.md` and the same path in every parent up to the repo root. For a linked git worktree, see "Project skills in a linked worktree" below | [docs] |
| nested | `<subdir>\.claude\skills\<name>\SKILL.md` in any folder below the start folder, loaded the first time the session reads or edits a file in `<subdir>` (see "Nested skills" below). Named `<name>`, or `<subdir>:<name>` when another skill has that name | [docs, code, test] |
| `--add-dir` | `.claude\skills\` inside each added directory | [docs] |
| plugin | `<installPath>\skills\<name>\SKILL.md`, plus any directories listed in the manifest's `skills` key | [disk] (supabase plugin: `skills: "./skills/"`) |
| claude.ai synced | `%USERPROFILE%\.claude\skills\synced\` | [docs]. **[contradicts docs]** on disk the skills sit one level deeper, at `skills\synced\<uuid>_<uuid>\<name>\` (here: `docs, docx, pdf, pptx, xlsx, skill-creator, unslop, …`), next to `manifest.json`, `.last-complete-round` and `.staging\` |
| legacy commands | `.claude\commands\<name>.md` (user and project) | [docs] |

Sources: [skills](https://code.claude.com/docs/en/skills), [claude-directory](https://code.claude.com/docs/en/claude-directory).

More observations [disk]:

- Some Entries in `%USERPROFILE%\.claude\skills\` are **NTFS junctions** into `%USERPROFILE%\.agents\skills\<name>`: `computer-use`, `orca-cli`, `orchestration`. That directory also holds a `.skill-lock.json`. This is the **skill manager** Provenance, and config-digger has to detect junctions/symlinks so that a Remove does not delete the target.
- `skills\.trash\<epoch>-<pid>-<rand>\` holds skills removed by sync. The docs say "Removes synced skills from `~/.claude/skills/.trash/`" ([skills](https://code.claude.com/docs/en/skills)).
- A folder under `skills\` that contains `.claude-plugin\plugin.json` is a plugin, `<name>@skills-dir`, not a plain skill ([plugin loading](https://code.claude.com/docs/en/plugins/loading)) [docs].

### Project skills in a linked worktree

Facts from the [skills](https://code.claude.com/docs/en/skills) and [worktrees](https://code.claude.com/docs/en/worktrees) docs pages as of 2026-10-29, and `uat/agents/claude-code.mjs` on a throwaway main checkout and its `git worktree add` worktrees with the local model stand-in, on 2.1.316 (#474).

- In a session in a linked worktree, made with `--worktree` or `git worktree add`, Claude Code searches parent folders for `.claude\skills` only up to the worktree root. A skill in a folder above the worktree is not read, nor is one in the main checkout's folders when the worktree is inside it (`--worktree`'s default, `.claude\worktrees\<name>`) [docs, test].
- From v2.1.277, when the worktree root has no `.claude\skills`, for example because `.claude\skills` is gitignored, it loads the main checkout's project skills instead. In a worktree whose root has its own `.claude\skills`, only that copy loads [docs, test].
- An empty `.claude\skills` folder at the worktree root keeps the main checkout's skills out; a `.claude` folder without `skills` does not [test].
- The main checkout's project skills are those of its root's `.claude\skills` only: with the Project in `<worktree>\sub`, `<main>\sub\.claude\skills` is not read [test].
- They add to the skills of a `.claude\skills` between the Project and the worktree root, and lose a shared name to them, as a folder farther up loses to a nearer one [test].
- A worktree inside the main checkout reads the same way as one outside it [test].
- The same read-through covers `.claude\agents` and `.claude\commands`, which config-digger does not read [docs].
- For a CLI session, the docs do not say that `.mcp.json`, settings or `CLAUDE.md` come from the main checkout, and in a worktree outside the main checkout the main checkout's `.mcp.json` is not read [test]. In one inside it, the walk of `.mcp.json` up to below the drive's root reaches it anyway [test]. For a worktree session started from the desktop app, the docs say Claude Code reads settings, hooks, skills, agents, commands and `.mcp.json` from the main checkout's root, while `CLAUDE.md` and `.claude\rules` still load from the worktree [docs]. config-digger does not read those sessions apart.

config-digger reads this through `linked_worktree = "main-checkout-stands-in"` on the `project-skills` anchor: the main checkout root's `.claude\skills` is read as one more folder of the walk, below the worktree root, when the worktree root does not hold `.claude\skills`. Its skills are `project` Entries of the worktree's Project, at their real path in the main checkout.

### Nested skills

Facts from 2.1.312: the skills docs page, its bundled code, and `uat/agents/claude-code.mjs` on throwaway folders with the local model stand-in answering with a Read tool call (#352).

- Skills in a `.claude\skills` below the folder Claude Code starts in are not loaded at start. They load the first time Claude reads or edits a file in that subfolder, and stay loaded for the rest of the session. `/add-dir` on the subfolder loads them sooner [docs, test].
- From the folder of each file it reads or edits (Read, Edit, Write), Claude Code walks up while the folder is strictly below the start folder, and checks `<folder>\.claude\skills` in each. There is no depth limit: one read loads the skills of a folder 1 and 3 levels down. A sibling folder no touched file is under is never read [code, test].
- It skips a skills folder whose holding folder git ignores, asking `git check-ignore -- <folder>` (debug log: `Skipped gitignored skills dir`): a `.gitignore` leaving out a folder on the way hides the skills below it, and one leaving out only the `.claude` folder hides nothing. Outside a git repo the check fails and the skills load [code, test].
- It also skips Claude Code's own claude.ai sync folders, and on Windows a path holding an 8.3 short name (`~1`). It runs no discovery at all when the `projectSettings` source is turned off or a policy allows only plugin customization (debug log: `Dynamic skill discovery skipped`) [code].
- It loads these skills as project-settings skills, with the same parsing as `project-skills` [code].
- Claude Code keys these skills by their folder and their name, so a nested skill never replaces another. With `deploy` at the repo root and in `apps\web\.claude\skills`, the model is told about `deploy` (the root one) and `apps/web:deploy`; a user skill or another nested skill of that name does the same, both nested ones then prefixed, and a Project skill's frontmatter alias of that name does not. A nested skill whose name is unique keeps it, and its description says which folder it applies to [docs, code, test].
- `skillOverrides` turns a nested skill off by the name it is listed under, and one listed as `<subdir>:<name>` by its bare name too, which turns off a same-named Project skill as well [test].
- Not tested: a nested skill against a same-named managed skill or built-in command, which the code counts as a clash [code]; a nested skills folder reached through a link; `name-only` and `user-invocable-only` on a nested skill.

## 4. Precedence and merge rules

### MCP servers: per name, whole Entry, no field merge [docs]

> "When the same server is defined in multiple places, Claude Code connects once using the highest-precedence source. The entire server entry from that source is used; fields are not merged across scopes." ([MCP](https://code.claude.com/docs/en/mcp))

The order is: managed (`managedMcpServers` / `managed-mcp.json`) > local > project > user > plugin-provided > claude.ai connectors. Managed, local, project and user compete **by name**. Plugins and connectors compete **by endpoint**: the same URL, ignoring host case, the default port and a trailing slash, or the same command. When `managed-mcp.json` and `managedMcpServers` both define a name, the file's Entry wins ([managed MCP](https://code.claude.com/docs/en/managed-mcp)).

Exclusive control: when `managed-mcp.json` is present, only its servers, `managedMcpServers` and a few built-ins load. User, local, project, plugin, `--mcp-config` and claude.ai Entries are all suppressed ([managed MCP](https://code.claude.com/docs/en/managed-mcp)). config-digger should label these Entries **blocked by policy**, distinct from Shadowed.

Policy filters apply after precedence:

- `deniedMcpServers` merges from every Scope, can match on `serverName`, `serverUrl` or `serverCommand`, and always wins.
- `allowedMcpServers` merges from every Scope unless `allowManagedMcpServersOnly` is set.

Inside one plugin, the default `.mcp.json` loads first, followed by each `mcpServers` source declared in the manifest. "A server name declared later replaces an earlier one" ([plugins reference](https://code.claude.com/docs/en/plugins-reference)).

### Skills: per name [docs]

Enterprise > personal > project > bundled. A skill beats a legacy command of the same name. A root skill beats a nested one, though both stay invocable. Local and plugin skills beat synced skills, the docs say; in fact a synced skill is `anthropic-skills:<folder>` and never competes, only the bare `/<folder>` going to a local skill that takes it ([release gate](release-gate.md), 2.1.310). Plugin skills are namespaced `/<plugin>:<skill>` and answer to the bare name only when nothing else claims it ([skills](https://code.claude.com/docs/en/skills)).

### Settings files: per key, lists concatenate, objects deep-merge [docs]

Order: managed > CLI (`--settings`) > `.claude\settings.local.json` > `.claude\settings.json` > `~\.claude\settings.json`. "When you set the same list key … in more than one file, Claude Code combines the lists" (exceptions: `fallbackModel`, `modelPicker`, `availableModels`, `modelSettings`) ([settings](https://code.claude.com/docs/en/settings)).

Within `managed-settings.d`, scalars are last-wins, lists are unioned with duplicates removed, nested blocks merge key by key, and `managedMcpServers` and `extraKnownMarketplaces` are replaced whole per name ([managed settings](https://code.claude.com/docs/en/managed-settings)).

`enabledPlugins` is merged **key by key** across `--add-dir` < user < project < local < flag < managed. A managed `true` force-enables a plugin and a managed `false` blocks it ([plugin loading](https://code.claude.com/docs/en/plugins/loading)).

### Plugins with the same manifest name [docs]

Managed-locked > `--plugin-dir`/`--plugin-url`/`CLAUDE_CODE_PLUGIN_DIRS` > installed marketplace plugin > skills-dir plugin (the user copy beats the project copy) > synced ([plugin loading](https://code.claude.com/docs/en/plugins/loading)).

## 5. Plugins on disk, and attributing an Entry to a plugin

The plugins root is `%USERPROFILE%\.claude\plugins`, overridable with `CLAUDE_CODE_PLUGIN_CACHE_DIR` ([plugin loading](https://code.claude.com/docs/en/plugins/loading)). Contents on this machine [disk]:

| Path | Content |
|---|---|
| `installed_plugins.json` | `{"version": 2, "plugins": {"<name>@<marketplace>": [ {scope, installPath, version, installedAt, lastUpdated, gitCommitSha?, projectPath?} ]}}`. The value is an **array**, with one element per install Scope. `projectPath` is present for `local`/`project` Scope (e.g. `ponytail@ponytail`, `scope=local`, `projectPath=E:\projects\claudio\stint`). Paths use Windows backslashes |
| `known_marketplaces.json` | `{"<marketplace>": {source, installLocation, lastUpdated}}` |
| `cache\<marketplace>\<plugin>\<version>\` | the plugin files that `${CLAUDE_PLUGIN_ROOT}` points to |
| `cache\…\<old version>\.orphaned_at` | marks a superseded version, deleted 14 days later. Seen at `superpowers\6.3.0` while 6.4.1 is current. **Ignore these** |
| `cache\…\<version>\.in_use\<pid>` | **undocumented**. JSON `{pid, procStartFt}` for every running Agent process that has the version loaded |
| `data\<id>\` | `${CLAUDE_PLUGIN_DATA}`, named `<name>-<marketplace>`, e.g. `superpowers-claude-plugins-official`, `ponytail-inline` |
| `marketplaces\<name>\` | marketplace clones |
| `synced\<uuid>_<uuid>\<plugin>~g<n>\` | plugins synced from claude.ai, with `<plugin>~g<n>.meta.json`, `manifest.json`, `.marketplaces.json`; see "Synced plugins" below |
| `store\`, `plugin-catalog-cache.json`, `.last_inuse_sweep` | undocumented internals |

Enabled state is recorded only in the settings files' `enabledPlugins` map, whose keys are `name@origin`. The origin is a marketplace name, or one of `inline`, `skills-dir`, `synced` ([plugin loading](https://code.claude.com/docs/en/plugins/loading)) [docs, disk]. User `settings.json` here contains `superpowers@claude-plugins-official: false` and four `true` values. `stint\.claude\settings.local.json` contains `ponytail@ponytail: true` [disk]. **An entry in `installed_plugins.json` does not mean the plugin is enabled.** Always cross-check `enabledPlugins`.

Plugin structure ([plugins reference](https://code.claude.com/docs/en/plugins-reference)) [docs, disk]:

- The manifest is `.claude-plugin\plugin.json` and is optional.
- `mcpServers` merges with the default `.mcp.json`. It can be a path string, an inline object, a `.mcpb`/`.dxt` bundle, or an array of these. The supabase plugin sets `"mcpServers": "./agents/claude/.mcp.json"` [disk], so a parser has to resolve paths and can't simply read `<root>\.mcp.json`. A value of another shape, such as a number or a list holding one, fails the manifest's validation, and the plugin fails to load whole (2.1.299 to 2.1.309 and 2.1.312) [test].
- `skills` adds to the default `skills\` directory. A plugin skill's name is its folder's, or a declared skill folder's frontmatter `name`, with each UTF-16 unit other than `A`-`Z`, `a`-`z`, `0`-`9`, `_` and `-` written `-`. The name rule of local skills does not apply to either. Claude Code applies it to the whole `<plugin>:<name>` instead, behind `tengu_slate_wicket`, so it skips only a plugin skill whose `<plugin>:<name>` is over 256 characters (2.1.299 to 2.1.309 and 2.1.312) [test].

**Attribution algorithm** (derived from the facts above):

1. Read `installed_plugins.json` to get each `id → installPath(s)`.
2. Merge `enabledPlugins` across Scopes to get the Effective enabled state.
3. For each enabled plugin, parse the manifest and the default files to list its MCP server names and skill folders. Attribute them as:
   - MCP: `plugin:<plugin-name>:<server>`. This exact string is what `/mcp` writes into `disabledMcpServers`, e.g. `plugin:supabase:supabase` [disk].
   - skill: `<plugin-name>:<skill>`.
4. Add synced plugins (`plugins\synced\…`) and skills-dir plugins (`skills\<x>\.claude-plugin\plugin.json`). `--plugin-dir` plugins exist only for their session and leave no trace on disk.

### Synced plugins

Claude Code downloads the plugins of the signed-in claude.ai account into `plugins\synced\` (Claude Code 2.1.310, Max account) [disk]:

```
plugins\synced\
  .bucket-<org>_<account>                    empty file, not a folder
  <org>_<account>\
    manifest.json                            {lastUpdated, plugins: [{pluginId, name, description, version, updatedAt, marketplaceName, installationPreference, generation}]}
    .marketplaces.json                       claude.ai marketplaces (etag, rows, parserVersion)
    .last-complete-round                     a round id
    <name>~g<generation>.meta.json           {server_plugin_id, marketplace_name, installation_preference}
    <name>~g<generation>\                    a normal plugin root
      .claude-plugin\plugin.json             {name, version, description, author}
      skills\<skill>\SKILL.md
      .in_use\<pid>
```

How a synced plugin differs from an installed one:

- **Name.** It is named by the `name` of its `.claude-plugin\plugin.json`, not by its folder: `cowork-plugin-management~g2` loads as `cowork-plugin-management`, its skills offered as `cowork-plugin-management:<skill>` [disk]. The `~g<n>` suffix is a generation, not part of the name. What Claude Code names a synced folder with no manifest `name` is not known; config-digger takes the folder's name up to `~g`.
- **No registry.** No `installed_plugins.json` record lists it: the folder is all there is [disk].
- **On by default.** No settings file names it in `enabledPlugins`, and it loads anyway [disk], where an installed plugin no settings file names is off [test]. Its `enabledPlugins` key is `<name>@synced` ([plugin loading](https://code.claude.com/docs/en/plugins/loading)) [docs]; `false` there is taken to turn it off, merged as any plugin key is.
- **`syncClaudeAiPlugins: false`** in user, local or managed settings turns off every synced plugin ([plugin loading](https://code.claude.com/docs/en/plugins/loading)) [docs].
- **Same-named plugins.** Of plugins with one manifest name, an installed marketplace plugin beats a skills-dir plugin, which beats a synced one ([plugin loading](https://code.claude.com/docs/en/plugins/loading)) [docs]. While an installed plugin of its name applies, config-digger Shadows every server and skill of the synced plugin, those the installed one lacks too, and none of them competes with another (#354). Which installed plugins keep a synced one out is not known beyond the docs: config-digger takes one that is off, or that no settings file names, to keep nothing out, one bound to a Project to keep it out in that Project alone, and one that fails to load to keep it out still.
- **`installation_preference`.** `"available"` in the `.meta.json`, and the plugin loads [disk]. What other values do is not known.
- **The sync gate.** `tengu_claudeai_plugins_sync_enabled` is `true` in `cachedGrowthBookFeatures` here [disk]. Unlike the skill sync gate, it decides whether Claude Code reads the synced plugins at all: held off, by its cached value, `DISABLE_GROWTHBOOK` or a third-party provider, none loads ([release gate](release-gate.md), 2.1.309).
- **The account's `manifest.json`.** Claude Code 2.1.309 loads only a plugin the manifest lists, and the sync round it runs at each start while the gate is on rewrites the manifest and deletes a plugin claude.ai does not list. It names the folder `<name>`, without `~g<generation>`. config-digger reads every folder ([#494](https://github.com/samfaina/config-digger/issues/494)).
- **The account.** As for synced skills, Claude Code reads them only while it runs on the signed-in claude.ai account, by the tokens it keeps rather than by `oauthAccount` ([#491](https://github.com/samfaina/config-digger/issues/491)): `apiKeyHelper` and a non-empty `ANTHROPIC_AUTH_TOKEN` or `CLAUDE_CODE_OAUTH_TOKEN` take it off, a third-party provider, `ANTHROPIC_API_KEY` and `primaryApiKey` leave it on ([release gate](release-gate.md), 2.1.309). `CLAUDE_CODE_OAUTH_TOKEN` in a Project's settings takes off the synced skills alone ([#493](https://github.com/samfaina/config-digger/issues/493)).

config-digger reads each folder of the account's, `<name>~g<n>` or `<name>`, as a plugin with `cloud-mirror` Provenance at user Scope, read-only: its servers and skills, as for an installed plugin, and nothing else of `synced\` (#113).

"Extensions": Claude Code has no separate extension concept. The nearest thing is `.mcpb`/`.dxt` bundles referenced from a plugin, which are extracted to `.mcpb-cache\` inside the plugin root ([plugins reference](https://code.claude.com/docs/en/plugins-reference)) [docs]. In 2.1.310's code, `.mcpb-cache\<hash>.metadata.json` records each bundle it extracted: `source`, the value that names it, `extractedPath`, `.mcpb-cache\<content hash>`, which holds the bundle's `manifest.json`, and when it was cached. The bundle's one server is named by the manifest's `name`, from its `server.mcp_config` with `${__dirname}` standing for that folder [binary]; the release gate's probe saw all of it on 2.1.299 to 2.1.309 and 2.1.312 ([release gate](release-gate.md)).

## 6. Native Disable mechanisms (no removal)

| Item | Mechanism | Where it is written | Tag |
|---|---|---|---|
| MCP server (any source) | `/mcp` toggle, per project | `.claude.json` → `projects["<path>"].disabledMcpServers: [names]`. Default-off built-ins use `enabledMcpServers` instead | [docs] [disk]: `["plugin:supabase:supabase","claude.ai Linear","claude-in-chrome"]` |
| MCP server from `.mcp.json` | `disabledMcpjsonServers: [names]` | any settings file. Wins even over approvals | [docs] |
| MCP server from `.mcp.json` | not approving it: leave it out of `enabledMcpjsonServers` and don't set `enableAllProjectMcpServers` | any settings file (`stint\.claude\settings.local.json` holds `enabledMcpjsonServers: ["webstorm"]`) | [docs] [disk] |
| MCP server (any) | `deniedMcpServers: [{serverName}/{serverUrl}/{serverCommand}]` | any settings file. Merged from all Scopes, so it can block managed servers too | [docs] |
| claude.ai connectors | `disableClaudeAiConnectors: true`, or env var `ENABLE_CLAUDEAI_MCP_SERVERS=false` | settings / environment | [docs] |
| skill | `skillOverrides: {"<name>": "off" \| "name-only" \| "user-invocable-only" \| "on"}` | any settings file. The `/skills` menu writes it to `.claude\settings.local.json`. **Doesn't apply to plugin skills** | [docs] [disk]: `{"graphify":"off","jetpack-compose":"off"}` in `stint\.claude\settings.local.json` |
| skill | permission deny rule `Skill(<name>)` | `permissions.deny` | [docs] |
| bundled / synced skills | `disableBundledSkills: true` / `syncClaudeAiSkills: false` | settings | [docs] |
| plugin (and every skill and MCP server it carries) | `enabledPlugins["<name>@<origin>"]: false`, or `claude plugin disable <id> --scope user\|project\|local` | the settings file of that Scope | [docs] [disk] |
| synced plugins (all) | `syncClaudeAiPlugins: false` | user/local/managed settings | [docs] |

Sources: [MCP: disable a server without removing it](https://code.claude.com/docs/en/mcp), [settings reference](https://code.claude.com/docs/en/settings-reference), [skills](https://code.claude.com/docs/en/skills), [plugin CLI](https://code.claude.com/docs/en/plugins/cli-reference), [plugin loading](https://code.claude.com/docs/en/plugins/loading).

**[contradicts docs]** The docs list the servers covered by `disabledMcpServers` as "user-configured servers, plugin servers, servers your organization provides …, the claude.ai connectors …, and built-in servers". Project `.mcp.json` servers are absent from that list. On disk, `projects["E:/projects/claudio/stint"].disabledMcpServers` contains `webstorm`, which is defined only in `stint\.mcp.json`. So the `/mcp` toggle also writes project servers into this list.

For Remove, the native commands are `claude mcp remove`, `claude plugin uninstall` (also deletes `data\<id>\` unless `--keep-data` is passed) and `claude plugin marketplace remove`. A `managedMcpServers` Entry can't be removed locally. It can only be Disabled through `/mcp` or `deniedMcpServers` ([managed MCP](https://code.claude.com/docs/en/managed-mcp), [plugin CLI](https://code.claude.com/docs/en/plugins/cli-reference)) [docs].

## 7. Does the Agent rewrite these files while running? Process names

**Yes.**

- `~/.claude.json` is rewritten by the Agent itself. The docs say "Claude Code writes back to it when you change settings in `/config` or approve trust prompts" ([claude-directory](https://code.claude.com/docs/en/claude-directory)). In practice it is rewritten much more often, since it also holds per-project session metrics, caches and counters.
  - [disk]: during this session its mtime moved from 14:15:26 to 14:16:04.
  - `~/.claude/backups/` holds five rotating `.claude.json.backup.<epoch-ms>` files, and two were created within 12 minutes. The docs: "Earlier versions of `~/.claude.json`, copied when Claude Code rewrites the file. Claude Code keeps the five newest" ([claude-directory](https://code.claude.com/docs/en/claude-directory)).
  - If the file can't be parsed, the Agent copies it to `backups/.claude.json.corrupted.<ts>` and prompts for a reset ([settings](https://code.claude.com/docs/en/settings)) [docs].
- The Agent writes settings files as well:
  - `/config` writes `~/.claude/settings.json`.
  - Permission approvals and `/skills` write `.claude/settings.local.json`.
  - `claude plugin …` writes `enabledPlugins` and `extraKnownMarketplaces`.
  - `claude mcp add --scope project` writes `.mcp.json`.

  Sources: [settings](https://code.claude.com/docs/en/settings), [skills](https://code.claude.com/docs/en/skills), [plugin loading](https://code.claude.com/docs/en/plugins/loading) [docs].
- The Agent **watches** user, project, local and managed settings files and hot-reloads them, firing a `ConfigChange` hook ([settings](https://code.claude.com/docs/en/settings)). Skills directories are also watched and reload on change ([skills](https://code.claude.com/docs/en/skills)). Plugin changes take effect only after `/reload-plugins` or a restart ([plugin loading](https://code.claude.com/docs/en/plugins/loading)) [docs].
- **Product implication:** an edit config-digger makes to `.claude.json` can be overwritten by a running Agent. Write `.claude.json` atomically and re-read it to verify, or warn/refuse while the Agent is running (see Open questions). Edits to settings files are less risky because the Agent re-reads them.

**Process detection** [disk]:

- The native CLI runs as **`claude.exe` from `%USERPROFILE%\.local\bin\claude.exe`**. Versions live in `%USERPROFILE%\.local\share\claude\versions\<ver>`; 2.1.269, 2.1.283 and 2.1.286 are present. This layout is documented in [setup](https://code.claude.com/docs/en/setup). Four such processes were running.
- **Claude Desktop is also `claude.exe`**, from `C:\Program Files\WindowsApps\Claude_<ver>_x64__pzs8sxrjxfjjc\app\claude.exe`. Twelve such processes were running. **Telling them apart by process name alone gives false positives.** Match on the image path instead.
- npm installs pull in the same native binary through an optional dependency, so the image path then sits inside the npm global `node_modules` ([setup](https://code.claude.com/docs/en/setup)) [docs]. The WinGet install path is not documented.
- `%USERPROFILE%\.claude\sessions\<pid>.json` is "one small file per running session, used to detect concurrent sessions and crashes", removed on exit and cleaned on the next launch ([claude-directory](https://code.claude.com/docs/en/claude-directory)) [docs]. On disk the file's keys are `pidDomain`, `procStartFt` and `peerToken` (a secret: don't read or log it), and the four files matched the four running CLI PIDs [disk]. The reliable test: the PID is alive **and** its process start time equals `procStartFt` (a Windows FILETIME).
- Per-version markers `plugins\cache\…\.in_use\<pid>` (`{pid, procStartFt}`) pointed at the same four PIDs [disk, undocumented].

## 8. Where the Agent records opened projects

- **`~/.claude.json` → `projects`** is a map keyed by absolute project path. Each value holds trust state (`hasTrustDialogAccepted`), local-Scope `mcpServers`, the MCP toggles (`disabledMcpServers`, `enabledMcpjsonServers`, `disabledMcpjsonServers`), `allowedTools` and last-session metrics. [docs] ([claude-directory](https://code.claude.com/docs/en/claude-directory)), [disk]: 53 entries.
  - **[contradicts docs]** The docs only show POSIX-style keys. On this machine the keys come in several spellings for the same folder: 49 use forward slashes (`E:/projects/config-digger`), 4 use backslashes (`E:\projects\claudio\stint`) and 4 have a lowercase drive letter (`e:/projects/claudio/stint`). They hold **different data**. For `stint`, trust is `true` only on the backslash key, while the MCP toggles exist only on the `E:/` key. Changelog 2.1.285: "Windows: Fixed `claude --bg` and the agents view refusing a folder that `claude` already trusts when its trust record was saved with different letter case" ([changelog](https://code.claude.com/docs/en/changelog)).
  - config-digger has to group these keys case-insensitively and separator-insensitively, and must not assume a single key per project.
- **`~/.claude/projects/<sanitized-path>/`** holds transcripts (`<session>.jsonl`) and auto-memory. [docs] ([claude-directory](https://code.claude.com/docs/en/claude-directory)) gives the example `/home/user/work/my-repo → -home-user-work-my-repo`. [disk]: every non-alphanumeric character becomes `-`, so `E:\projects\config-digger → E--projects-config-digger`. **The mapping is lossy and can't be reversed reliably**, so use `.claude.json` keys or transcript `cwd` fields for the real path. `CLAUDE_CODE_PROJECT_DIR_NAME` overrides the name when `CLAUDE_CONFIG_DIR` is set ([env-vars](https://code.claude.com/docs/en/env-vars)).
- **`~/.claude/history.jsonl`** has one line per prompt, "with timestamp and project path" ([claude-directory](https://code.claude.com/docs/en/claude-directory)) [docs].
- **`~/.claude.json` → `githubRepoPaths`** maps `owner/repo` to an array of local paths, e.g. `samfaina/petme-app → ["E:\projects\petme-app"]` [disk, undocumented].

## 9. macOS / Linux differences (stated in docs, not verified, deferred to v0.2)

| Thing | macOS | Linux / WSL | Source |
|---|---|---|---|
| managed directory (`managed-settings.json`, `managed-settings.d/`, `managed-mcp.json`, `.claude/skills/`) | `/Library/Application Support/ClaudeCode/` | `/etc/claude-code/` | [managed settings](https://code.claude.com/docs/en/managed-settings), [managed MCP](https://code.claude.com/docs/en/managed-mcp) |
| OS policy | plist domain `com.anthropic.claudecode` (configuration profile) | none. On WSL the Windows HKLM/`C:\Program Files\ClaudeCode` policy chain applies when `wslInheritsWindowsSettings` is set | [managed settings](https://code.claude.com/docs/en/managed-settings) |
| user files | `~/.claude/`, `~/.claude.json` | same | [settings](https://code.claude.com/docs/en/settings) |
| CLI binary | `~/.local/bin/claude` → symlink into `~/.local/share/claude/versions/` | same | [setup](https://code.claude.com/docs/en/setup) |
| path-list separator (`CLAUDE_CODE_PLUGIN_DIRS`, `CLAUDE_CODE_PLUGIN_SEED_DIR`) | `:` | `:` (Windows: `;`) | [env-vars](https://code.claude.com/docs/en/env-vars) |
| plugin component paths containing `\` | rejected | rejected (they load on Windows only) | [plugin loading](https://code.claude.com/docs/en/plugins/loading) |

## 10. Re-check against 2.1.302

Issue: [#101](https://github.com/samfaina/config-digger/issues/101). On 2026-10-17 the facts in `claude-code.toml` were re-checked against the [changelog](https://code.claude.com/docs/en/changelog) entries for 2.1.287 to 2.1.302, the 2.1.302 binary's settings schema and messages, and this machine's files [disk].

**Unchanged.** The native Installation still runs as `%USERPROFILE%\.local\bin\claude.exe`, with 2.1.296, 2.1.301 and 2.1.302 under `versions\`. Claude Desktop's MSIX copy of 2.1.299 sits at `claude-code\2.1.299\<hash>\claude.exe`, as 2.1.286 did. `lastReleaseNotesSeen` reads `2.1.302`. `projects` still holds `mcpServers`, `disabledMcpServers`, `enabledMcpjsonServers` and `disabledMcpjsonServers` per Project. `installed_plugins.json` is still `version: 2` with the same record fields, and superseded versions still carry `.orphaned_at`. Synced skills still sit at `skills\synced\<uuid>_<uuid>\<name>\`, next to `manifest.json`, `.last-complete-round` and `.staging\`. An empty `.bucket-<uuid>_<uuid>` file now sits beside the `<uuid>_<uuid>` folder; it is not a folder, so it is never read as one. `cachedGrowthBookFeatures` and `cachedGrowthBookFeaturesAt` are still in `~/.claude.json`, and `tengu_claudeai_skills_sync_enabled` is `true` here [disk]. The settings schema still types `enabledMcpjsonServers` and `disabledMcpjsonServers` as lists of strings [binary].

**Changed:**

- **Skill names (2.1.299).** "Changed skill and command loading: one whose file or folder name has hidden, direction-changing or blank-like characters, `<` or `>` is no longer offered until renamed; such a `name:` is ignored" ([changelog](https://code.claude.com/docs/en/changelog)). 2.1.292 already ignored a skill's `name` over 256 characters ([changelog](https://code.claude.com/docs/en/changelog)) [docs]. In the binary, the check warns "a name may not hold hidden, direction-changing or blank-like characters, < or >, or be over 256 characters". It also refuses a space at either end, a space next to a `:`, and a leading or trailing `:`. The strict check applies while the GrowthBook gate `tengu_slate_wicket` is on, which it is by default; the gate is absent from this machine's cache [binary, disk]. The definition declares it as the name rule `visible-chars` on every skill location, under that gate (`name_rule_when`), and the gate as on without a value: a skill whose folder name breaks it is Invalid, and a frontmatter `name` that breaks it counts as no `name`. The binary's 2.1.302 classes go beyond what the release gate tested, and the definition's marks say which ([#171](https://github.com/samfaina/config-digger/issues/171)).
- **Plain-string MCP approval lists (2.1.299).** "Fixed a settings file being ignored when `enabledMcpjsonServers` or `disabledMcpjsonServers` held a single server name (letters, digits, - or _) as a plain string" ([changelog](https://code.claude.com/docs/en/changelog)). The release gate tested what it does (§11). The definition's `plain_string_name` reads it so on the settings files' `enabledMcpjsonServers` sources and `disabledMcpjsonServers` switches, and not in the Project's state; Disable over one writes a list of both names, and Enable of its name deletes the key ([#172](https://github.com/samfaina/config-digger/issues/172)).
- **Session files.** `sessions\<pid>.json` no longer holds `peerToken` or `procStartFt`. Its keys now include `pid`, `procStart`, `sessionId`, `cwd`, `kind`, `status` and `version`, and a `<pid>.<hash>.key` file still sits beside it [disk]. config-digger reads only the file name, so nothing changes.

2.1.290 "Fixed skills not being found when asked for by the name in SKILL.md when their folder has a different name …: the skill listing now shows both names" ([changelog](https://code.claude.com/docs/en/changelog)) [docs]. That is how a skill is invoked, not which of two same-named skills wins, so the definition still names a skill by its frontmatter `name`. Whether the folder name also competes belongs to [#105](https://github.com/samfaina/config-digger/issues/105).

Nothing else in the range touches a location, format, Disable switch or skill rule the definition reads. Other changes include `disableClaudeAiConnectors` and `allowedMcpServers` URL rules now applying to `.mcp.json` and plugin servers (2.1.290), which the definition already assumed, and plugin installs at project or local Scope no longer adding a second record when only the drive letter's case differs (2.1.295). `verified` is raised to `2.1.286`–`2.1.302`.

## 11. Release gate tests (2.1.299 to 2.1.309)

The release gate (#80) ran Claude Code on throwaway homes with `uat/agents/claude-code.mjs`; [`release-gate.md`](release-gate.md) has the method and every result. What it changes here [test]:

- **Skills are named by their folder**, as written, and compete by it. The frontmatter `name` is an alias the skill also runs by, unless the 2.1.299 name rule refuses it. `skillOverrides` names the folder. A plugin's skill is `<plugin>:<folder>`. This replaces §3's and §4's naming by frontmatter, and answers #105: names are not rewritten, and of same-named skills along the walk to the repo root the nearest folder's wins.
- **`.mcp.json` is read in every folder above the launch folder**, past the repo root, the nearest winning by name (open question 4).
- **`CLAUDE_CONFIG_DIR` moves `.claude.json`** into that folder: user servers are read from `<CLAUDE_CONFIG_DIR>\.claude.json` (open question 1).
- **Approvals in a Project's own files need trust**: the Project's state, `.claude/settings.json` and `.claude/settings.local.json` approve only while the state, under the Project's path with forward slashes, holds `hasTrustDialogAccepted: true`. A key with backslashes is not read (open question 3, for approvals).
- **A plain-string list** approves or skips that one server in a settings file, and approves nothing in the Project's state. The definition reads it so ([#172](https://github.com/samfaina/config-digger/issues/172)).
- **A plugin no settings file names is off.** Of two plugins' servers with one endpoint, the plugin first in the merged `enabledPlugins` wins. A plugin whose `plugin.json` does not parse fails to load whole; one whose `.mcp.json` does not parse loads without those servers. A plugin of a `directory` marketplace is read from the marketplace's folder, not from the cache.
- **Managed configuration**, tested in Windows Sandbox with `uat/agents/claude-code-machine.mjs`: a `managed-mcp.json` that does not parse is skipped, `claude mcp list` reports it, and it keeps its exclusive control, so only `managedMcpServers` load. An HKLM value, `managed-settings.json` or `managed-settings.d` file that does not parse stops Claude Code ("Managed settings document could not be parsed as a JSON object"), even beside a source that parses; an HKCU value that does not parse is skipped. Of HKLM and the file, only HKLM's settings are read. `managedMcpServers` is never read from HKCU, and an entry with a command or at an `http://` URL is dropped.

## Sources

- MCP: https://code.claude.com/docs/en/mcp
- Managed MCP: https://code.claude.com/docs/en/managed-mcp
- Settings and precedence: https://code.claude.com/docs/en/settings
- Settings reference: https://code.claude.com/docs/en/settings-reference
- Managed settings: https://code.claude.com/docs/en/managed-settings
- .claude directory: https://code.claude.com/docs/en/claude-directory
- Skills: https://code.claude.com/docs/en/skills
- Plugin manifest reference: https://code.claude.com/docs/en/plugins-reference
- Plugin loading reference: https://code.claude.com/docs/en/plugins/loading
- Plugin CLI reference: https://code.claude.com/docs/en/plugins/cli-reference
- Environment variables: https://code.claude.com/docs/en/env-vars
- Setup / install / uninstall: https://code.claude.com/docs/en/setup
- Changelog: https://code.claude.com/docs/en/changelog

## Open questions

1. **Does `CLAUDE_CONFIG_DIR` move `.claude.json`?** The docs say it relocates "configuration, cache, and session transcripts" and `settings.json`, but never name `.claude.json`. That needs a test with the variable set, which is out of bounds here because nothing may be written outside the worktree.
2. **Does a running Agent re-read `.claude.json` before writing it, or write back its in-memory copy?** This decides whether config-digger can safely edit `.claude.json` (local/user MCP Entries, `disabledMcpServers`) while the Agent runs. Not documented. The docs only say the file is "read at session start" for MCP servers.
3. **How does the Agent pick among duplicate `projects` keys** (case and separator variants), and which spelling does it write for a new session? Changelog 2.1.285 suggests it now matches case-insensitively for trust, but the rule for `mcpServers` and `disabledMcpServers` lookups is undocumented.
4. **Is `.mcp.json` looked up in parent directories** the way skills are, or only at the launch directory or repo root? The docs only say "project root".
5. **Sanitization rule for `~/.claude/projects/<name>`** with long paths. Some Agents truncate and hash; this wasn't observed here.
6. **The WinGet install's binary path.** Not documented and not present on this machine.
7. **The exact secure-storage backend on Windows** for OAuth tokens and `sensitive` plugin `userConfig` values. `.credentials.json` exists, but the docs don't say whether Windows Credential Manager is used as well.
8. **The meaning of `plugins\store\` and `plugins\synced\<uuid>_<uuid>`**: is the pair account and organization? Undocumented. For `skills\synced\` it is `<organizationUuid>_<accountUuid>`, the ids of `oauthAccount` in `~/.claude.json`, and Claude Code reads only the signed-in account's folder ([release gate](release-gate.md)). **Partly answered (#113):** `plugins\synced\<uuid>_<uuid>\` holds the claude.ai account's synced plugins, one `<name>~g<generation>` folder each (see "Synced plugins" in §5), and its pair is presumably the same `<organizationUuid>_<accountUuid>` as for `skills\synced\`; config-digger reads only the Signed-in account's folder for both, another pair's Unread (#308). `plugins\store\` is still unexplained.
