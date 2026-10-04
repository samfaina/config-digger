# Research: Gemini CLI configuration on Windows

Issue: [#13](https://github.com/samfaina/config-digger/issues/13). Terms follow [`CONTEXT.md`](../../CONTEXT.md).

Verified against **Gemini CLI 0.62.0** (`npm i -g @google/gemini-cli` under Volta, package at `%LOCALAPPDATA%\Volta\tools\image\packages\@google\gemini-cli\node_modules\@google\gemini-cli\`, a single esbuild `bundle\`), on Windows 11, home `C:\Users\kgon1`, on 2026-10-01. Gemini CLI was not running during the checks. No `gemini` command was run for this research, because `gemini mcp list` spawns stdio servers in a trusted folder (see §6) and every CLI start writes `projects.json` (see §7). Source citations point at `google-gemini/gemini-cli` tag [`v0.62.0`](https://github.com/google-gemini/gemini-cli/tree/v0.62.0) (commit `b460678`). Key strings and defaults quoted below were also found in the installed bundle, so the tag matches the install. Docs citations point at the `docs/` folder of the same tag, which is the source of geminicli.com/docs.

Each fact is tagged:

- **[disk]**: verified read-only on this machine (files under `~/.gemini`, the scratch folders, or the installed bundle).
- **[docs]**: stated by a primary source (the official docs or the `google-gemini/gemini-cli` source) and not observable on this machine.
- **[contradicts docs]**: the source or the disk differs from what the docs say.

Secret values are never reproduced. `~/.gemini/oauth_creds.json` and `google_accounts.json` were not opened. Only directory listings and JSON key names were read under `~/.gemini/antigravity*` and `~/.gemini/config`.

## Summary

Gemini CLI keeps MCP servers in the `mcpServers` key of up to four `settings.json` files: system defaults (`C:\ProgramData\gemini-cli\system-defaults.json`), user (`~/.gemini/settings.json`), project (`<cwd>\.gemini\settings.json`) and system override (`C:\ProgramData\gemini-cli\settings.json`). Each is relocatable by an environment variable. Extensions under `~/.gemini/extensions/<name>/` add more servers from their `gemini-extension.json`. Gemini CLI has no local Scope.

**Merge is per server name, whole object.** `mcpServers` uses a shallow merge, so for a name defined in several files the whole object from the highest-precedence file wins: system override > project > user > system defaults. Fields are not merged across settings files. A settings Entry with the same name as an extension server is different: the two are merged field by field, with settings winning for scalars, `env` merged, `excludeTools` unioned and `includeTools` intersected.

**Folder trust gates everything.** Folder trust is on by default (the docs say off). In a folder that is not trusted, the project `settings.json` is ignored, workspace skills are skipped, and **no MCP server starts at all, user-level ones included**. `gemini mcp list` then reports every server as "Disabled". That is why both `cd-throwaway-*` servers showed "Disabled" in #7: the scratch folder was trusted only later. Trust lives in `~/.gemini/trustedFolders.json`, matched by longest prefix. On this machine `c:/users/kgon1` is `DO_NOT_TRUST`, so every project under the home folder without its own rule is untrusted.

**Disable is native and stored outside the Entry.** MCP servers: `~/.gemini/mcp-server-enablement.json`, `{ "<lowercased name>": { "enabled": false } }`, global and by name only. Skills: `skills.disabled` (a list of names) in the user or project `settings.json`. Extensions: path-glob rules in `~/.gemini/extensions/extension-enablement.json`. A "user scope" extension rule is `/<home>/*`, so **on Windows it does not cover projects on another drive** such as `E:\`. `mcp.allowed`/`mcp.excluded` block servers by name, and system policy can turn MCP, extensions or skills off.

**Skills** are `SKILL.md` files one level deep in `~/.gemini/skills`, `~/.agents/skills`, `<cwd>\.gemini\skills` and `<cwd>\.agents\skills`, plus extension `skills/` folders and two built-ins. The skill name comes from the frontmatter `name`, not the folder (here the folder `~/.gemini/skills/jetpack-compose` is the skill `android-jetpack-compose`). The last one loaded wins: built-in < extension < `~/.gemini/skills` < `~/.agents/skills` < `.gemini/skills` < `.agents/skills`. `~/.agents/skills` is shared with VS Code and other Agents.

**Antigravity shares `~/.gemini`.** Gemini CLI owns a fixed set of top-level names (§1.5). Antigravity owns `antigravity*/` and `config/`, and `config/mcp_config.json` uses the same `mcpServers` root key. Gemini CLI never reads it. Attribution must therefore go by path, not by format.

---

## 1. MCP server locations per Scope

### 1.1 Locations

| Scope (config-digger) | Windows path | Override | Tag |
|---|---|---|---|
| managed (defaults, lowest) | `C:\ProgramData\gemini-cli\system-defaults.json` | `GEMINI_CLI_SYSTEM_DEFAULTS_PATH`; otherwise the folder of the system settings file | [docs] [S-settings] L119–127, [D-config] L36–43. `C:\ProgramData\gemini-cli` absent here [disk] |
| user | `%USERPROFILE%\.gemini\settings.json` | `GEMINI_CLI_HOME` replaces the home folder for everything (`<GEMINI_CLI_HOME>\.gemini\...`) | [docs] [S-storage] L54–60, L78–80; [S-paths] L22–28. Present, holds `cd-throwaway-user` [disk] |
| project | `<cwd>\.gemini\settings.json`. The **current directory**, not a git root, and no upward search. Not read when `<cwd>` is the home folder | none | [docs] [S-settings] L778–779, L888–890; [S-storage] L203–205, L211–224, L344–346. Present in `E:\scratch\config-digger-agents\gemini`, holds `cd-throwaway-project` [disk] |
| managed (override, highest) | `C:\ProgramData\gemini-cli\settings.json` | `GEMINI_CLI_SYSTEM_SETTINGS_PATH` | [docs] [S-settings] L106–117, [D-config] L52–60. Absent here [disk] |
| extension | `~/.gemini/extensions/<name>/gemini-extension.json` → `mcpServers`, or for `link` installs the manifest in the linked source folder | follows `GEMINI_CLI_HOME` | [docs] [S-extmgr] L708–760, L828–860; [S-extstorage] L38–40. Two linked extensions here (§5) [disk] |
| managed (cloud) | Code Assist admin controls fetched at sign-in: `admin.mcp.enabled`, `admin.mcp.config` (allowlist), `admin.mcp.requiredConfig` (servers injected for everyone) | not a file | [docs] [S-settings] L394–409, L487–513; [S-cliconfig] L878–923 |
| local | none. Gemini CLI has no per-user, per-project file | | [docs] |

Notes:

- **System files are skipped unless admin-owned.** Before loading either system file, Gemini CLI checks the file and its parent folders. On Windows it runs `Get-Acl` and requires an owner such as Administrators, SYSTEM or TrustedInstaller, and no write access for ordinary users. Otherwise it skips the file with a "Security Warning" ([S-settings] L851–876, [S-security] L90–130, L591) [docs]. config-digger should report such a file as present but ignored.
- **`admin.*` in any file is ignored.** The merged `admin` block is rebuilt from schema defaults plus remote admin settings only ([S-settings] L394–409) [docs]. The managed controls for MCP, extensions and skills therefore come only from the cloud.
- **No other MCP sources.** Gemini CLI does not read `.mcp.json`, `.vscode/mcp.json`, Claude or Cursor files, or Antigravity's `mcp_config.json`. The string `mcp_config.json` does not occur in `packages/core/src` or `packages/cli/src` at v0.62.0 [docs].
- No `GEMINI_*` or `GOOGLE_*` environment variable is set on this machine [disk].

### 1.2 Other environment variables that change what is read

| Variable | Effect | Source |
|---|---|---|
| `GEMINI_CLI_HOME` | replaces `os.homedir()` everywhere: user settings, extensions, skills, `~/.agents`, trust file, projects registry | [S-paths] L22–28 |
| `GEMINI_CLI_SYSTEM_SETTINGS_PATH`, `GEMINI_CLI_SYSTEM_DEFAULTS_PATH` | move the two managed files | [S-settings] L106–127 |
| `GEMINI_CLI_TRUSTED_FOLDERS_PATH` | moves `trustedFolders.json` | [S-storage] L131–136, [D-trust] L124–129 |
| `GEMINI_CLI_TRUST_WORKSPACE=true/false`, `GEMINI_RESTRICTED_MODE=true` | force trust on or off for the session | [S-trust] L47–56 |
| `SANDBOX=sandbox-exec` (macOS) | moves runtime state (trust file, projects, tmp) to `~/.cache/.gemini` | [S-storage] L92–108 |
| `--allowed-mcp-server-names` (flag) | replaces the allowlist and ignores `mcp.excluded` for the session | [S-cliconfig] L816–819, L994–1006 |
| `-e/--extensions <names>` (flag) | only the named extensions are active; `-e none` disables all | [S-extenable] L146–163 |

### 1.3 How the effective settings are computed

`loadSettings(cwd)` reads the four files, expands `$VAR`, `${VAR}` and `${VAR:-default}` in every string, and validates them ([S-settings] L781–847) [docs]. JSON comments are allowed (`strip-json-comments`) [docs]. It decides trust from system defaults + user + system only, then drops the project file entirely if the folder is not trusted ([S-settings] L919–938, L255–281, L329–331) [docs].

### 1.4 Telling Gemini CLI files apart from Antigravity files in `~/.gemini`

Antigravity (Google's newer agent suite) writes into the same `~/.gemini` folder. On this machine [disk]:

| Path under `~/.gemini` | Owner | Evidence |
|---|---|---|
| `settings.json`, `trustedFolders.json`, `projects.json`, `state.json`, `installation_id`, `google_accounts.json`, `oauth_creds.json`, `extension_integrity.json`, `extensions/`, `skills/`, `history/`, `tmp/` | Gemini CLI | each name is built in Gemini CLI source: [S-storage] L22–23, L54–160, L255–256, L289–296; [S-state] L11; [S-integrity-const] L40–42 [docs] |
| `GEMINI.md` (0 bytes, 2025-11-19) | **shared**: Gemini CLI loads it as global memory ([S-memory] L318); its date matches Antigravity's first files (`antigravity/installation_id`, `browserAllowlist.txt`, 2025-11-19) | [disk]. Not an MCP/skill Entry, so v0.1 can ignore it |
| `antigravity/`, `antigravity-backup/`, `antigravity-browser-profile/`, `antigravity-cli/`, `antigravity-ide/` | Antigravity | names; contents are `.pb`/`.pbtxt` state, `brain/`, `conversations/`, `mcp/`, `skills/` [disk] |
| `config/` (`config.json`, `mcp_config.json`, `hooks.json`, `plugins/`, `projects/`, `sidecars/`, `.migrated`) | Antigravity | `antigravity/mcp_config.json` is a symlink to `config/mcp_config.json` [disk] |

Rules for the Agent definition:

1. Treat only the Gemini CLI names above as Gemini CLI. Everything matching `antigravity*` or `config/` belongs to another Agent (out of scope for v0.1).
2. **Do not detect Gemini CLI MCP files by format.** `config/mcp_config.json` has root key `mcpServers` (here one server, `dart-mcp-server`, keys `command`/`args`) [disk], and `antigravity-cli/settings.json` (keys `colorScheme`, `enableTelemetry`, `trustedWorkspaces`) [disk] is a different file from `~/.gemini/settings.json`.
3. The same skill can be installed for both: the skill manager skillfish wrote `jetpack-compose` to both `~/.gemini/skills/` (Gemini CLI) and `~/.gemini/antigravity/skills/` (Antigravity) at the same second on 2026-07-11 [disk]. Those are two Entries for two Agents.
4. The Jul 24 Gemini CLI files (`installation_id` 09:08, `history/kgon1`, `history/petme`) predate Antigravity CLI's files (`antigravity-cli/installation_id`, `config/projects/default-cli-project.json`, both 09:11) by three minutes [disk]. Gemini CLI 0.62.0 ships a built-in `antigravity-support` skill about migrating to Antigravity CLI ([S-builtin]) [docs], which may explain the sequence. It does not change attribution.

### 1.5 Gemini CLI file names (for attribution)

From source, Gemini CLI may create these under `~/.gemini` [docs]: `settings.json`, `trustedFolders.json`, `projects.json`, `state.json`, `installation_id`, `google_accounts.json`, `oauth_creds.json`, `mcp-oauth-tokens.json`, `a2a-oauth-tokens.json`, `mcp-server-enablement.json`, `extension_integrity.json`, `integrity.key` (fallback only), `policy_integrity.json`, `keybindings.json`, `GEMINI.md`, `.env`, and the folders `extensions/`, `skills/`, `commands/`, `policies/`, `agents/`, `acknowledgments/`, `history/`, `tmp/` ([S-storage] L22–160, [S-integrity-const] L40–42). Of the optional files, `mcp-server-enablement.json`, `integrity.key` and `.env` are absent here [disk]. The missing `integrity.key` means the extension-integrity key is in the Windows Credential Manager under `gemini-cli-extension-integrity` ([S-integrity] L44–82) [docs].

---

## 2. File format and secret-bearing fields

Root key `mcpServers`, an object keyed by server name, in every settings file and in `gemini-extension.json` ([S-schema] L161–174, [D-config] L2487–2537) [docs]. Fields come from `MCPServerConfig` ([S-coreconfig] L484–521) [docs]:

| Transport | Fields | May hold secrets |
|---|---|---|
| stdio | `command`, `args`, `env`, `cwd` | `env` values, `args` |
| SSE | `url` with `type: "sse"` (or `url` alone, transport auto-detected) | `headers`, query string of `url` |
| streamable HTTP | `url` with `type: "http"`, or the deprecated `httpUrl` | `headers`, query string |
| websocket | `tcp` | — |
| all | `timeout`, `trust` (skip confirmations), `description`, `includeTools`, `excludeTools`, `oauth{enabled, clientId, clientSecret, authorizationUrl, issuer, tokenUrl, scopes, audiences, redirectUri, tokenParamName, registrationUrl}`, `authProviderType`, `targetAudience`, `targetServiceAccount` | **`oauth.clientSecret`** ([S-oauth] L44–56) |

- If several transports are given, `httpUrl` wins over `url`, which wins over `command` ([D-config] L2496–2498) [docs].
- `$VAR` references keep the secret out of the file. When Gemini CLI writes `settings.json` it writes the raw, unexpanded values back ([S-settings] L1240–1246) [docs].
- An extension server cannot set `trust`: it is stripped on load ([S-extmgr] L1245–1249, [D-ext] L152) [docs]. Extension settings marked `sensitive` go to the OS keychain; others go to `~/.gemini/extensions/<name>/.env` (user) or `<cwd>\.env` (workspace) ([S-extmgr] L767–826, [S-extvars] L24) [docs].
- MCP OAuth tokens are stored apart from the config, in the keychain or in `~/.gemini/mcp-oauth-tokens.json` ([S-storage] L70–72) [docs]. Not present here [disk].
- `.env` files can feed `$VAR` values: `<dir>\.gemini\.env` (trusted folders only) or `<dir>\.env`, searching upward from the cwd, then `~/.gemini/.env` and `~/.env` ([S-settings] L561–598) [docs].
- On disk [disk]: user `cd-throwaway-user` is stdio (`command: npx`, `args`, `env.CD_FAKE_TOKEN = not-a-real-secret`); project `cd-throwaway-project` is stdio; extension `mcp-server-example` declares `nodeServer` with `args: ["${extensionPath}${/}example.js"]` and `cwd: "${extensionPath}"`. `${extensionPath}`, `${workspacePath}` and `${/}` are extension-only variables ([S-extmgr] L867–873) [docs].

---

## 3. Skills per Scope

From [S-skillmgr] L54–99 [docs], and in the installed bundle [disk]:

| Order (later wins) | Scope | Folder | Condition |
|---|---|---|---|
| 1 | built-in | inside the bundle: `antigravity-support`, `skill-creator` | always [disk] (`bundle\builtin\`) |
| 2 | extension | `<extension path>\skills\` (the linked source folder for `link` installs) | extension active |
| 3 | user | `~\.gemini\skills\` | always |
| 4 | user | `~\.agents\skills\` | always |
| 5 | project | `<cwd>\.gemini\skills\` | folder trusted |
| 6 | project | `<cwd>\.agents\skills\` | folder trusted |

- Discovery globs `SKILL.md` and `*/SKILL.md` only (the folder itself or one level down), ignoring `node_modules` and `.git` ([S-skillloader] L115–134) [docs].
- **The skill name is the frontmatter `name`**, with `: \ / < > * ? " |` replaced by `-`, not the folder name ([S-skillloader] L164–188) [docs]. On disk: `~/.gemini/skills/jetpack-compose/SKILL.md` declares `name: android-jetpack-compose` [disk]. Disabling and Shadowing go by that name.
- On disk [disk]: `~/.gemini/skills/jetpack-compose` (skillfish marker `.skillfish.json`, so Provenance = skill manager); `~/.agents/skills/{computer-use, orca-cli, orchestration}` (the same folders VS Code reads); the extension skill `greeter` in `E:\scratch\config-digger-agents\gemini-extensions\cd-throwaway-skills\skills\greeter\`. No project skills in the scratch folder.
- `gemini skills install` copies into `~/.gemini/skills` or `.gemini/skills` (default user). `gemini skills link` creates a **junction** on Windows (`dir` symlink elsewhere) ([S-skillutils] L242–282) [docs]. A junction under a skills folder therefore marks a linked skill. None exist here [disk].
- `skills.enabled: false` turns skills off; the cloud `admin.skills.enabled` does too ([S-schema] L2516–2541, [S-coreconfig] L1536–1545) [docs].

---

## 4. Precedence and merge rules

**Settings files.** `customDeepMerge(schemaDefaults, systemDefaults, user, project, system)`; later wins ([S-settings] L255–281, [S-deepmerge] L24–96) [docs]. Per-path strategies come from the schema ([S-settings] L59–80):

- `mcpServers`: `SHALLOW_MERGE` ([S-schema] L169). At the top level, `{...lower, ...higher}`: **one Effective Entry per name, chosen whole**. System override > project > user > system defaults. The other Entries with that name are Shadowed. This matches the enterprise docs ("System > Workspace > User", [D-enterprise] L319–333) [docs]. The docs' phrase "the values are merged" ([D-enterprise] L36) means the names are combined, not the fields.
- Server names are case-sensitive keys here, but the enablement file and `mcp.allowed`/`mcp.excluded` compare lowercased, trimmed names ([S-mcpenable] L57–59) [docs]. `My-Server` and `my-server` are two Entries that share one Disable switch.
- `mcp.allowed`: the **intersection** of every file that defines it. `mcp.excluded`: the **union** of every file ([S-settings] L518–558) [docs]. A user can narrow a system allowlist but not widen it.
- `skills.disabled` and `extensions.disabled`: `UNION` ([S-schema] L2490–2500, L2534–2544) [docs].
- An untrusted project file contributes nothing, including to the three lists above ([S-settings] L329–331) [docs].

**Settings vs extensions (MCP).** At runtime settings servers are registered first, then each active extension's servers ([S-mcpmgr] L375–411, L575–578) [docs]:

- Settings Entry + extension Entry with the same name: **merged field by field**. The settings Entry wins scalars (`command`, `url`, `timeout`...), `env` is merged with settings values winning, `excludeTools` is the union, `includeTools` the intersection ([S-mcpmgr] L327–373, [D-mcp] L814–838) [docs]. Both Entries are partly Effective. A settings Entry with only `excludeTools` is a valid override of an extension server.
- Two extensions with the same server name: the first one loaded is kept, the other skipped with a warning ([S-mcpmgr] L384–397) [docs]. Load order is `readdir` order of `~/.gemini/extensions`.
- `gemini mcp list` shows it differently: the settings Entry wins and the extension Entry is not listed ([S-mcplist] L50–62) [docs].
- Two extensions with the same manifest `name`: loading fails ([S-extmgr] L641–649) [docs].

**Skills.** Last one wins by name in the order of §3, with a "Skill conflict detected" warning ([S-skillmgr] L124–148) [docs]. Within a tier, `.agents/skills` beats `.gemini/skills` ([D-skills] L49–53) [docs].

**Gating, in the order applied to each server** ([S-mcpenable] L100–179, [S-mcpmgr] L444–466) [docs]: `admin.mcp.enabled` (cloud) → `mcp.allowed` → `mcp.excluded` → session disable → `mcp-server-enablement.json` → **folder trusted** → extension active. Any failure means the server does not start.

---

## 5. How extensions are recorded on disk (attribution)

- Each extension is a folder `~/.gemini/extensions/<manifest name>/` holding `.gemini-extension-install.json` = `{source, type, ref?, releaseTag?, autoUpdate?, allowPreRelease?}` with `type` one of `git`, `github-release`, `local`, `link` ([S-coreconfig] L445–452, [S-extvars] L21–23) [docs]. For `local`, `git` and `github-release` the extension files are copied into that folder. For `link` only the metadata file is there and Gemini CLI reads the manifest, MCP servers and skills from `source` ([S-extmgr] L429–442, L758–760) [docs].
- On disk [disk]: `extensions/mcp-server-example/` and `extensions/skills-example/` each contain only `.gemini-extension-install.json`, `{"source": "E:\\scratch\\config-digger-agents\\gemini-extensions\\cd-throwaway-mcp" | "...\\cd-throwaway-skills", "type": "link"}`. The folder name comes from the manifest `name`, not the source folder, as the docs expect ([D-ext] L134–139).
- Attribution: an MCP server or skill whose definition sits in an extension's manifest or `skills/` is an extension Entry. Its Provenance is "plugin or extension" and it is taken away by uninstalling or disabling the extension, never by editing the manifest. `type: link` points at a folder the user owns, and Remove must not delete that source.
- The extension id is a SHA-256 of the GitHub URL, or of `source`, or of the name ([S-extmgr] L1452–1478) [docs]. `~/.gemini/extension_integrity.json` stores, per extension name, a hash of the install metadata and an HMAC signature, checked only on update ([S-integrity], [S-update] L45–70) [docs]. It has entries for both extensions here [disk]. config-digger should not touch it.
- Extensions are user-level only. There is no project extensions folder; per-project control is the enablement file (§6).
- Extension settings declared in the manifest are stored in `.env` files and the keychain (§2).

---

## 6. Native Disable mechanisms

| Item | How | Where it is stored | Tag |
|---|---|---|---|
| MCP server | `gemini mcp disable <name>` / `/mcp disable <name>`; `enable` deletes the key | `~/.gemini/mcp-server-enablement.json` = `{ "<lowercased name>": { "enabled": false } }`. **Global and by name**: it disables every Entry with that name, in every Scope and extension | [docs] [S-mcpenable] L181–272, [D-mcp] L1259–1280. File absent here [disk] |
| MCP server (session) | `--session` flag | memory only | [docs] |
| MCP server (block) | `mcp.excluded: ["name"]` in a settings file, or an `mcp.allowed` list without the name | user or project `settings.json`. A blocked server is reported "Blocked", not "Disabled" | [docs] [S-mcplist] L175–184 |
| all MCP | cloud `admin.mcp.enabled: false` | not a file | [docs] |
| skill | `gemini skills disable <name> [--scope user\|workspace]` / `/skills disable` | `skills.disabled` (list of names, case-insensitive) in the user or project `settings.json` | [docs] [S-skillsettings] L28–60, [S-skillmgr] L181–187 |
| all skills | `skills.enabled: false`; cloud `admin.skills.enabled` | settings / cloud | [docs] |
| extension | `gemini extensions disable <name> [--scope user\|workspace]` | `~/.gemini/extensions/extension-enablement.json` = `{ "<name>": { "overrides": ["/C:/Users/kgon1/*", "!/E:/proj/*", ...] } }`. The last matching rule wins; `!` disables; a trailing `*` includes subfolders; no rule means enabled | [docs] [S-extenable] L13–108, L146–178; [S-extmgr] L1176–1240 |
| all extensions | cloud `admin.extensions.enabled`; `-e none` | cloud / flag | [docs] |

**`--scope user` for extensions means "under the home folder".** User scope writes a rule for `homedir()` with `*`, workspace scope a rule for the cwd with `*` ([S-extmgr] L1191–1195, L1226–1230) [docs]. Installing or linking writes the user rule ([S-extmgr] L492–496). On disk both extensions have exactly `"/C:/Users/kgon1/*"` [disk]. The match is a case-sensitive regular expression on `/<cwd with forward slashes>/` ([S-extenable] L83–108, L173). So **a user-scope disable has no effect in `E:\...` projects**, where no rule matches and the default is enabled. config-digger's Effective/Disabled computation for extensions must evaluate the rules against each project path, and a config-digger "Disable everywhere" must not rely on the user-scope rule on Windows. See Open questions.

**The `extensions.disabled` setting** is still in the schema but nothing in v0.62.0 reads it [docs] (no reference outside the schema). Do not use it.

### Why `gemini mcp list` showed both servers "Disabled" (#7)

- `listMcpServers` uses `getMergedSettingsAsIfTrusted()` when the folder is untrusted, so project servers are listed, prints "MCP servers are configured but disabled because this folder is untrusted. User-level servers are also suppressed in untrusted folders", and then returns `DISABLED` for every server that passed the allow/exclude/enablement checks ([S-mcplist] L186–188, L200–232) [docs]. The message is in the installed bundle [disk].
- A folder with no matching rule in `trustedFolders.json` counts as untrusted (`isTrusted ?? false`, [S-settings] L927–929) [docs]. `trustedFolders.json` was last written at 15:07:38, after both `mcp add` runs (project file 14:47:15), when the first interactive `gemini` run trusted the folder [disk]. The `mcp list` in #7 ran before that, so the folder was untrusted and both servers were reported Disabled. No `mcp-server-enablement.json` exists, which rules out an explicit Disable [disk].
- The runtime agrees: `startConfiguredMcpServers` and `maybeDiscoverMcpServer` return early when the folder is not trusted ([S-mcpmgr] L461–463, L575–578) [docs].
- **[contradicts docs]**: the docs say untrusted stdio servers show as "Disconnected" ([D-mcp] L1205–1208). The code returns "Disabled" for all transports.
- **Re-running `gemini mcp list` is not read-only in a trusted folder**: it connects to each server, which spawns stdio commands such as `npx -y @modelcontextprotocol/server-everything` ([S-mcplist] L72–155) [docs]. It also starts extensions and touches the projects registry. It was not run for this research.

**Folder trust is on by default.** `security.folderTrust.enabled` defaults to `true` in the schema ([S-schema] L1915–1922, [S-trustcli] L32–35) [docs]. **[contradicts docs]**: [D-trust] L10 says the feature is "disabled by default". The disk agrees with the source: this machine has no `folderTrust` key in `settings.json`, yet Gemini CLI prompted for trust and wrote `trustedFolders.json` [disk].

---

## 7. Does the Agent rewrite files while running? Process names

| File | When Gemini CLI writes it | How | Tag |
|---|---|---|---|
| `~/.gemini/settings.json` (and project `.gemini/settings.json`) | `mcp add/remove`, `skills enable/disable`, auth method selection (`security.auth.selectedType`), `/model` change, IDE nudge, and at every load when deprecated keys are migrated | `comment-json` parse + stringify (comments kept, 2-space indent), whole file rewritten; raw `$VAR` values preserved | [docs] [S-commentjson] L19–47, [S-settings] L461–485, L1001–1255, [S-app] L813 |
| `~/.gemini/trustedFolders.json` | trust dialog, `/permissions`, extension install/link from an untrusted folder | lock file (`proper-lockfile`), re-read, write to `.tmp.<uuid>`, rename; keys normalised | [docs] [S-trust] L199–271, L333–363 |
| `~/.gemini/projects.json`, `history/<slug>/.project_root`, `tmp/<slug>/.project_root` | **every start in a new folder** (also non-interactive commands that initialise storage) | lock + temp file + rename with retries | [docs] [S-registry] L95–125, L163–230; [S-storage] L277–304 |
| `extension-enablement.json`, `.gemini-extension-install.json`, `extension_integrity.json` | `extensions install/link/enable/disable/uninstall/update` | plain `writeFileSync` | [docs] [S-extenable] L206–209 |
| `mcp-server-enablement.json` | `mcp enable/disable` | plain `writeFile` | [docs] [S-mcpenable] L383–386 |

- On this machine `settings.json` was rewritten at 15:07:44, between the trust write (15:07:38) and the auth files (15:07:52), with unchanged content [disk] (#7). That is consistent with the auth dialog calling `setValue(scope, 'security.auth.selectedType', ...)`, which rewrites the file even when the value is the same ([S-app] L801–813) [docs].
- Gemini CLI does not watch `settings.json`. MCP settings are marked `requiresRestart` ([S-schema] L161–174) and no file watcher on settings exists in the source [docs]. An edit by config-digger takes effect at the next start, but a running session can overwrite it when it saves a setting, because `setValue` writes back its in-memory copy of the file.
- **Process names.** The Volta shim `%LOCALAPPDATA%\Volta\bin\gemini.cmd` runs `volta run gemini` [disk], which starts `node.exe` on `...\@google\gemini-cli\bundle\gemini.js` (`bin` in the package's `package.json`) [disk]. That process normally re-spawns itself as a child `node.exe` with the same script ([S-relaunch] L38–70) [docs], unless `GEMINI_CLI_NO_RELAUNCH` is set. There is no `gemini.exe`. Detect a running Gemini CLI as `node.exe` whose command line contains `gemini-cli` and `gemini.js`. A plain npm install (without Volta) uses `%APPDATA%\npm\gemini.cmd` → `node.exe ...\node_modules\@google\gemini-cli\bundle\gemini.js` [docs] (not verified). No such process was running during the checks [disk].

---

## 8. Where Gemini CLI records opened projects

- `~/.gemini/projects.json` = `{ "projects": { "<path>": "<slug>" } }`, one entry per folder Gemini CLI has started in, **including the home folder** ([S-registry] L163–230) [docs]. Keys are `path.resolve(p).toLowerCase()` on Windows, so lower-cased with backslashes ([S-registry] L95–100) [docs]. Here: `c:\users\kgon1` → `kgon1`, `e:\projects\claudio\petme` → `petme`, `e:\scratch\config-digger-agents\gemini` → `gemini`, `...\gemini-extensions` → `gemini-extensions` [disk].
- The slug is the folder name slugified (`[^a-z0-9]` → `-`), with collisions resolved by ownership markers ([S-registry] L304–423) [docs]. `~/.gemini/history/<slug>/.project_root` and `~/.gemini/tmp/<slug>/.project_root` hold the same lower-cased backslash path [disk]. `tmp/<slug>/` also holds `chats/`, `logs/` and `logs.json`.
- `~/.gemini/trustedFolders.json` lists trusted and distrusted folders. Keys go through `normalizePath`: absolute, **forward slashes**, lower-cased on Windows and macOS ([S-paths] L331–352, [S-trust] L229–258) [docs]. The file is re-normalised on load, so hand-written backslash keys still match ([S-trust] L305–308). Here: `c:/users/kgon1` `DO_NOT_TRUST`, and `e:/projects/claudio/petme`, `e:/scratch/config-digger-agents/gemini`, `e:/scratch/config-digger-agents/gemini-extensions` `TRUST_FOLDER` [disk]. `gemini-extensions` was trusted by `gemini extensions link`, run from that folder, not by an interactive session ([S-extmgr] L219–233) [docs].
- **Three path spellings for one folder.** `projects.json` uses `e:\scratch\...`, `trustedFolders.json` uses `e:/scratch/...`, and `extension-enablement.json` uses `/E:/scratch/.../*` with the original case [disk]. config-digger must normalise all three (resolve, lower-case on Windows, unify separators) before joining them.
- `state.json` holds UI counters only (`startupWarningCounts`, `tipsShown`) [disk].

---

## 9. macOS / Linux differences (from source, not verified)

| Item | Windows | macOS | Linux | Source |
|---|---|---|---|---|
| system override | `C:\ProgramData\gemini-cli\settings.json` | `/Library/Application Support/GeminiCli/settings.json` | `/etc/gemini-cli/settings.json` | [S-settings] L106–117 |
| system defaults | `...\system-defaults.json` next to it | same folder | same folder | [S-settings] L119–127 |
| system file security check | `Get-Acl` via PowerShell | owner/mode check | owner/mode check | [S-security] L90–130, L591 |
| user folder | `%USERPROFILE%\.gemini` | `~/.gemini` | `~/.gemini` | [S-storage] L54–60 |
| runtime state under `sandbox-exec` | n/a | `~/.cache/.gemini` | n/a | [S-storage] L92–108 |
| path comparison | case-insensitive | case-insensitive | case-sensitive | [S-paths] L347–352 |
| `projects.json` keys | lower-cased | original case | original case | [S-registry] L95–100 |
| `skills link` | junction | dir symlink | dir symlink | [S-skillutils] L276–282 |

---

## 10. Authentication modes (brief, does not affect config reading)

`security.auth.selectedType` holds one of `oauth-personal`, `gemini-api-key`, `vertex-ai`, `compute-default-credentials`, `gateway`, or the legacy `cloud-shell` ([S-authtype] L63–70) [docs]. Here it is `oauth-personal` [disk].

| Mode | Status per source |
|---|---|
| `oauth-personal` with a free individual account | **Refused by the server** on this machine: "This client is no longer supported for Gemini Code Assist for individuals ... migrate to the Antigravity suite" (#7). The string is not in the 0.62.0 bundle [disk], so the refusal is server-side. **[contradicts docs]**: the 0.62.0 docs still recommend "Sign in with Google" for individual accounts ([D-auth] L21–36) |
| `oauth-personal` with a Workspace / organisation account | documented ([D-auth] L24, L310–330) [docs]; not tested |
| `gemini-api-key` (AI Studio key) | documented ([D-auth] L76–121) [docs]; not tested |
| `vertex-ai` (ADC, service account, or Cloud API key) | documented ([D-auth] L123–308) [docs]; not tested |

config-digger reads files only, so none of this blocks it. Note that the cloud "managed" Scope (§1.1) is only fetched for signed-in Code Assist users.

---

## Product implications for config-digger

1. **Agent definition.** Four settings files with path overrides by environment variable, plus extensions. One Effective Entry per name, chosen whole by file precedence, except that a settings Entry and an extension Entry with the same name are merged field by field. The definition format needs to express "merge with extension" and an ACL gate on managed files.
2. **Effective state depends on the project.** Folder trust decides whether a project's servers, its skills, and even the user's servers apply. The "Effective" column has to be computed per project, with "inactive: folder not trusted" as a reason. The same holds for extension enablement rules.
3. **Disable writes a Gemini-owned side file.** MCP: `mcp-server-enablement.json` (global, by lowercased name). Skills: `skills.disabled` in a settings file. Extensions: `extension-enablement.json` path rules, written for the specific project paths (or for the drive roots) rather than the home-folder rule.
4. **Attribution by path.** Only the names in §1.5 are Gemini CLI. Antigravity's `config/mcp_config.json` looks identical and must be ignored by this Agent definition.
5. **Shared folders.** `~/.agents/skills` and `<project>\.agents\skills` are also read by VS Code and other Agents. One folder is an Entry for several Agents.
6. **Write safety.** Gemini CLI rewrites `settings.json` on ordinary actions and keeps an in-memory copy, so config-digger should write only while no `node.exe ... gemini-cli ... gemini.js` process is running, and keep comments (Gemini CLI itself uses `comment-json`).
7. **Project discovery.** `projects.json` lists every folder Gemini CLI has run in. Its keys and `trustedFolders.json` keys use different separators.

---

## Sources

Docs (`google-gemini/gemini-cli` @ `v0.62.0`, published at geminicli.com/docs):

- [D-config]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/docs/reference/configuration.md
- [D-enterprise]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/docs/cli/enterprise.md
- [D-trust]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/docs/cli/trusted-folders.md
- [D-mcp]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/docs/tools/mcp-server.md
- [D-ext]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/docs/extensions/reference.md
- [D-skills]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/docs/cli/skills.md
- [D-auth]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/docs/get-started/authentication.mdx

Source (`google-gemini/gemini-cli` @ `v0.62.0`):

- [S-settings]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/cli/src/config/settings.ts
- [S-schema]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/cli/src/config/settingsSchema.ts
- [S-deepmerge]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/cli/src/utils/deepMerge.ts
- [S-commentjson]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/cli/src/utils/commentJson.ts
- [S-cliconfig]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/cli/src/config/config.ts
- [S-trustcli]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/cli/src/config/trustedFolders.ts
- [S-mcpenable]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/cli/src/config/mcp/mcpServerEnablement.ts
- [S-mcplist]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/cli/src/commands/mcp/list.ts
- [S-extmgr]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/cli/src/config/extension-manager.ts
- [S-extenable]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/cli/src/config/extensions/extensionEnablement.ts
- [S-extstorage]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/cli/src/config/extensions/storage.ts
- [S-extvars]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/cli/src/config/extensions/variables.ts
- [S-update]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/cli/src/config/extensions/update.ts
- [S-skillsettings]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/cli/src/utils/skillSettings.ts
- [S-skillutils]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/cli/src/utils/skillUtils.ts
- [S-skilldisable]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/cli/src/commands/skills/disable.ts
- [S-app]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/cli/src/ui/AppContainer.tsx
- [S-relaunch]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/cli/src/utils/relaunch.ts
- [S-state]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/cli/src/utils/persistentState.ts
- [S-storage]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/core/src/config/storage.ts
- [S-registry]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/core/src/config/projectRegistry.ts
- [S-coreconfig]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/core/src/config/config.ts
- [S-integrity]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/core/src/config/extensions/integrity.ts
- [S-integrity-const]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/core/src/config/constants.ts
- [S-paths]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/core/src/utils/paths.ts
- [S-trust]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/core/src/utils/trust.ts
- [S-security]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/core/src/utils/security.ts
- [S-memory]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/core/src/utils/memoryDiscovery.ts
- [S-mcpmgr]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/core/src/tools/mcp-client-manager.ts
- [S-oauth]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/core/src/mcp/oauth-provider.ts
- [S-skillmgr]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/core/src/skills/skillManager.ts
- [S-skillloader]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/core/src/skills/skillLoader.ts
- [S-builtin]: https://github.com/google-gemini/gemini-cli/tree/v0.62.0/packages/core/src/skills/builtin
- [S-authtype]: https://github.com/google-gemini/gemini-cli/blob/v0.62.0/packages/core/src/core/contentGenerator.ts

On-disk evidence (read-only): `~/.gemini/{settings.json, projects.json, trustedFolders.json, state.json, extension_integrity.json, installation_id}`, `~/.gemini/extensions/{extension-enablement.json, */.gemini-extension-install.json}`, `~/.gemini/{history,tmp}/*/.project_root`, `~/.gemini/skills/jetpack-compose/{SKILL.md,.skillfish.json}`, `~/.agents/skills/*/SKILL.md`, directory listings of `~/.gemini/antigravity*` and `~/.gemini/config`, key names only of `~/.gemini/config/{mcp_config.json,config.json}` and `~/.gemini/antigravity-cli/settings.json`, `E:\scratch\config-digger-agents\gemini\.gemini\settings.json`, `E:\scratch\config-digger-agents\gemini-extensions\*`, the installed bundle under `%LOCALAPPDATA%\Volta\tools\image\packages\@google\gemini-cli\`, and `%LOCALAPPDATA%\Volta\bin\gemini{,.cmd}`. Modification times of everything under `~/.gemini` (except `antigravity*`) and the two scratch folders were recorded before and after the checks and did not change.

## Open questions

1. **User-scope extension Disable on another drive.** The source says `gemini extensions disable <name>` (user scope) writes `!/C:/Users/<me>/*` and therefore leaves the extension enabled in `E:\` projects. This should be confirmed by toggling a throwaway extension and running Gemini CLI in `E:\scratch\...`, which needs writes outside this research's read-only rule. If confirmed, config-digger needs its own rule-writing strategy (per project, or one rule per drive root such as `!/E:/*`).
2. **`gemini skills disable` default Scope.** The CLI command defaults to `--scope workspace` (source and bundle). The docs say both `/skills disable` and the CLI default to `user` ([D-skills] L74–76, L120–121). The default of the slash command was not traced.
3. **Running `gemini mcp list` read-only.** It was not run, because in a trusted folder it spawns the configured stdio servers and every start writes `projects.json`. Is there a safe probe for config-digger tests, for example `GEMINI_CLI_HOME` pointed at a copy?
4. **Does Antigravity read Gemini CLI's files?** Antigravity was out of scope. Whether it reads `~/.gemini/skills`, `~/.gemini/GEMINI.md` or `~/.gemini/settings.json` matters for Remove (one file could be an Entry for both Agents). Its own Agent definition, if one is ever written, should answer this.
5. **Cloud admin controls.** `admin.mcp.config`/`requiredConfig` come from the Code Assist backend and exist only in memory. Is there any local cache config-digger could read to show managed (cloud) Entries? None was found in the source paths read here.
6. **`mcp.allowed` with no file defining it vs an empty list.** An empty `mcp.allowed: []` in any file makes the intersection empty and blocks every server. This follows from [S-settings] L534–558 but was not tested.
7. **Load order of extensions with the same server name** is `readdir` order. On NTFS that is usually alphabetical, but Node does not guarantee it.
