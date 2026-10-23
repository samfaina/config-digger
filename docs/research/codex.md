# Research: Codex configuration on Windows

Issue: [#12](https://github.com/samfaina/config-digger/issues/12). Terms follow [`CONTEXT.md`](../../CONTEXT.md).

Verified against **Codex CLI 0.159.3** (`npm i -g @openai/codex` under Volta, Node 22.23.1) on Windows 11, home `C:\Users\kgon1`, on 2026-10-01. Source citations point at `openai/codex` tag [`rust-v0.159.3`](https://github.com/openai/codex/tree/rust-v0.159.3) (commit `01fc69f`), which matches the installed binary. All `developers.openai.com/codex/*` doc URLs now answer `308` and redirect to `learn.chatgpt.com/docs/*`, so the docs are cited at their new address.

Each fact is tagged:

- **[disk]**: verified read-only on this machine, either by reading files or by running `codex mcp list` / `codex debug prompt-input`.
- **[sandbox]**: verified by running the installed 0.159.3 binary against a throwaway `CODEX_HOME` and throwaway projects inside the session scratchpad. No real config was touched.
- **[source]**: read in the `rust-v0.159.3` source and not observable here.
- **[docs]**: stated by OpenAI's docs.
- **[contradicts docs]**: the source or the binary does something the docs don't say, or say differently.

No secret values appear here. `~/.codex/auth.json` was listed but not opened. `CD_FAKE_TOKEN = "not-a-real-secret"` is a deliberate fake from issue #7.

## Summary

Codex has one config format, TOML, with one root key for MCP servers: `[mcp_servers.<name>]`. It reads it from a stack of layers: packaged defaults, `%ProgramData%\OpenAI\Codex\config.toml` (system), cloud-managed bundles, `%CODEX_HOME%\config.toml` (user, default `%USERPROFILE%\.codex`), an optional profile file `%CODEX_HOME%\<name>.config.toml`, one `.codex\config.toml` per folder from the project root down to the cwd, and `-c` flags. Admin restrictions live in a separate file, `%ProgramData%\OpenAI\Codex\requirements.toml`, which can only allowlist servers, not declare them.

**Layers are deep-merged, not replaced per name.** A project file that sets `[mcp_servers.foo] args = [...]` changes only the `args` of the user's `foo` and keeps its `command` and `env` [sandbox]. A project file that sets only `enabled = false` disables a user server. If two layers give the same server different transports (one `command`, one `url`), Codex refuses to load any config and exits [sandbox]. So for Codex, Effective and Shadowed apply per field, not per Entry.

**Project layers need trust.** Trust is a `[projects.'<lower-cased path>'] trust_level = "trusted"` entry in the user `config.toml`. An untrusted project's `.codex\config.toml` is loaded but disabled. **Its `.codex\skills` folder is still read, though.** A skill placed there showed up in the model prompt of an untrusted project [sandbox], although the docs say untrusted projects skip all project `.codex/` layers [contradicts docs].

**Skills** come from `<project>\.codex\skills` and `<dir>\.agents\skills` for every folder from the project root to the cwd, `%CODEX_HOME%\skills` (deprecated), `%USERPROFILE%\.agents\skills`, `%CODEX_HOME%\skills\.system` (bundled, rewritten by Codex), `%ProgramData%\OpenAI\Codex\skills` (admin), and plugins. Codex dedupes only by `SKILL.md` path. Two skills with the same name both stay listed, so a skill is never Shadowed. `~/.agents/skills` is the same folder VS Code reads.

**Disable is native and lives in the user `config.toml`.** MCP servers have `enabled = false`. Skills have `[[skills.config]] path|name, enabled = false`, honoured only from the user layer and `-c` flags. Plugins have `[plugins."<name>@<marketplace>"] enabled = false`, and plugin MCP servers have `[plugins."<id>".mcp_servers.<name>] enabled = false`. Plugins installed from the account's remote catalog take their enabled state from the server, not from the file.

**A machine can have several Codex homes.** `CODEX_HOME` moves everything (config, plugins, skills, state DBs, credentials). Orca sets it to `%APPDATA%\orca\codex-runtime-home\home`, which holds a copy of the user `config.toml`, Orca's hooks trust state, and directory links back to `~/.codex\plugins` and `~/.codex\skills` [disk].

---

## 1. MCP server locations per Scope

### 1.1 Layers

From [`loader/mod.rs`][S-loader] L107–L138 (doc comment) and L145–L501 (code), with precedence numbers from [`config_layer_source.rs`][S-layer-src] L33–L52 (higher wins):

| Scope (config-digger) | Windows path | Precedence | Tag |
|---|---|---|---|
| (built-in) | packaged defaults embedded in the binary (`config/defaults.toml`) | −10 | [source] L186–L200 |
| managed (MDM) | macOS only (`com.openai.codex` managed preferences) | 0 | [source] [S-macos] L27–L29 |
| managed (system defaults) | `%ProgramData%\OpenAI\Codex\config.toml`, path from `SHGetKnownFolderPath(FOLDERID_ProgramData)` with fallback `C:\ProgramData` | 10 | [source] L797–L819. **Absent here**: no `C:\ProgramData\OpenAI` [disk] |
| managed (cloud) | enterprise cloud config bundle delivered on ChatGPT sign-in, no file | 15 | [source] L209–L224; [docs] [D-managed] |
| user | `%CODEX_HOME%\config.toml`, default `%USERPROFILE%\.codex\config.toml` | 20 | [source] L289–L321; [disk] |
| user (profile) | `%CODEX_HOME%\<profile>.config.toml`, only with `--profile <name>` | 21 | [source] L323–L334, [S-config] L2017–L2025; [docs] [D-config-adv] |
| project | `<dir>\.codex\config.toml` for each `<dir>` from the project root down to the cwd, deeper wins | 25 | [source] L1586–L1770; [disk] |
| session | `-c key=value` flags, or UI choices | 30 | [source] L415–L421 |
| managed (legacy) | `managed_config.toml`: **ignored on Windows** unless a test override sets a path | 40 / 50 | [source] [S-layer-io] L65–L101 |

`requirements.toml` is a separate admin channel, not a layer: `%ProgramData%\OpenAI\Codex\requirements.toml`, plus cloud bundle requirements ([`loader/mod.rs`][S-loader] L107–L120, L724–L740, L809–L813). It can restrict MCP servers but never adds one (see §6). Absent here [disk].

**[contradicts docs]** The managed-configuration page says Windows managed defaults live at `~/.codex/managed_config.toml` ([D-managed]). In 0.159.3 that file is ignored on Windows with the startup warning "CODEX_HOME/managed_config.toml is no longer supported on Windows. Use %ProgramData%\OpenAI\Codex\requirements.toml for enforced settings or config.toml for defaults" ([S-layer-io] L74–L101). The basic config page lists the system file only for Unix ([D-config-basic]). The source reads `%ProgramData%\OpenAI\Codex\config.toml` on Windows too.

**[contradicts docs]** The two docs pages disagree on profile vs project order. [D-config-basic] puts project above profile, [D-config-adv] puts profile above project. The source matches config-basic: profile 21 < project 25.

### 1.2 `CODEX_HOME`

[`utils/home-dir/src/lib.rs`][S-home] L13–L63:

- `CODEX_HOME`, when set and non-empty, must name an existing directory. Codex canonicalizes it and fails with "CODEX_HOME points to …, but that path does not exist" otherwise.
- When unset, the home is `<home_dir()>\.codex`, which need not exist yet.
- `allow_symlinked_codex_home = true` in the user config keeps a symlinked alias instead of the canonical path ([S-symlink] L8–L28).

What moves with `CODEX_HOME`: the user `config.toml` and profile files, `skills\` (deprecated user skills and `.system`), `plugins\cache\`, `.tmp\marketplaces\`, `auth.json`, the MCP OAuth fallback file `.credentials.json` ([S-oauth] L17, L819, L1062), `hooks.json`, `sessions\`, `history.jsonl` and the `*.sqlite` state files. What does **not** move: `%USERPROFILE%\.agents\skills`, which is resolved from the OS home ([S-skill-roots] L35–L36, L103–L108). The state DBs can be moved separately with `CODEX_SQLITE_HOME` or `sqlite_home` ([S-config] L270–L271, [S-state] L128).

**Two homes on this machine** [disk]:

| Home | Who uses it | Contents |
|---|---|---|
| `C:\Users\kgon1\.codex` | any Codex started without `CODEX_HOME` | `config.toml` with `cd-throwaway-user`, `cd-throwaway-http`, one trusted project; `skills\.system\`; `plugins\cache\openai-curated-remote\{plugin-management,work-pets}\`; `state_5.sqlite` and other DBs; `auth.json` |
| `%APPDATA%\orca\codex-runtime-home\home` | every Codex started from an Orca terminal (`CODEX_HOME` set by Orca) | its own `config.toml` holding the same two servers and trusted project plus Orca's `[hooks.state]` trust hashes; `hooks.json`; `config.toml.bak`; `.orca-config-settings-baseline.json` (lists `mcpServers: [cd-throwaway-user, cd-throwaway-http]`); its own `auth.json` and DBs; `plugins` and `skills` are **directory links to `~/.codex\plugins` and `~/.codex\skills`** |

The Orca home looks like a mirror Orca maintains from `~/.codex\config.toml`. That is Orca's behaviour, not Codex's, and its sync rules are unknown (see Open questions). For config-digger it means the same Entry can appear in two user files, and the plugin and skill folders must be deduplicated by resolved path.

`codex mcp list` run from the scratch project gives the same result in both homes [disk].

### 1.3 Project layer and trust

- **Project root.** Codex walks up from the cwd looking for a `project_root_markers` entry, default `[".git"]`. A `.git` directory counts only if it has a `HEAD`. With no marker, the cwd is the root and only the cwd's `.codex` is read. `project_root_markers = []` disables the walk ([S-loader] L1490–L1526, [S-markers] L5–L50; [docs] [D-config-adv]).
- **Which folders.** Every `<dir>\.codex\` between the root and the cwd, inclusive, becomes a project layer, ordered root first, cwd last, so the deepest wins. A `.codex` that resolves to `CODEX_HOME` is skipped, so `~/.codex` is never read as a project layer ([S-loader] L1638–L1685). A `.codex` folder without `config.toml` still yields an (empty) layer ([S-loader] L1743–L1754).
- **Trust lookup.** For each folder Codex looks up `projects` in the merged non-project config: first the folder itself, then the project root, then the Git repository root (the main checkout, for linked worktrees). On Windows the keys are compared ASCII-lower-cased, canonical spelling first, then original ([S-loader] L1041–L1084, L1370–L1416; [S-trust] L74–L104). So trusting a repo root trusts every `.codex` inside it and its linked worktrees.
- **Recorded as** `[projects.'<path>'] trust_level = "trusted" | "untrusted"` in the user `config.toml` ([S-cfgtoml] L466, L596–L606; [S-protocol] L642–L645). On this machine: `[projects.'e:\scratch\config-digger-agents\codex'] trust_level = "trusted"`, written by the TUI's trust prompt with a lower-cased path [disk]. `projects` is read from every non-project layer, so managed config or `-c` can also grant or deny trust ([S-loader] L337–L398).
- **Untrusted project.** The layer is kept with a `disabled_reason` ("To load project-local config, hooks, and exec policies, add <key> as a trusted project in <user config.toml>") and is left out of the effective config ([S-loader] L1086–L1104, L1119–L1135). Verified: `cd-throwaway-project` appears in `codex mcp list` only when the cwd is the trusted scratch folder [disk], and an untrusted project's server never appears [sandbox].
- **Keys a project can't set** even when trusted: `openai_base_url`, `chatgpt_base_url`, `model_provider(s)`, `notify`, `profile(s)`, `otel` and a few more. They are stripped with a startup warning ([S-loader] L84–L101, L1137–L1247). `mcp_servers` is not on that list, so a trusted repo can add or change MCP servers.

### 1.4 How each CLI command sees Scopes

- `codex mcp add` and `codex mcp remove` have no Scope flag. They load the **merged** servers with no cwd (packaged defaults, system, cloud, user, legacy managed) and write that whole set back to `%CODEX_HOME%\config.toml` ([S-mcp-cmd] L293–L461, L506–L537; [S-config] L2278–L2312). A server declared only in `%ProgramData%\OpenAI\Codex\config.toml` would therefore be copied into the user file on the next add or remove [source].
- `codex mcp list` and `codex mcp get` load the full config for the cwd, so they include project servers when the cwd is in a trusted project [disk], and they mask `env` values as `KEY=*****` ([S-mcp-cmd] L664–L700, L1056).
- `codex mcp list` left `config.toml` and every state file untouched (mtime snapshot before and after). The only change under `~/.codex` was the removal of a stale `tmp\arg0\codex-arg0*` folder [disk].

---

## 2. File format and secret-bearing fields

TOML, root table `mcp_servers`, one sub-table per server name. Names accepted by `codex mcp add`: ASCII letters, digits, `- _ : @ / .` ([S-mcp-cmd] L1115–L1126). The raw shape is [`mcp_types.rs`][S-mcp-types] L377–L444. The transport is chosen by which key is present: `command` makes it stdio, `url` makes it streamable HTTP, neither is an error ([S-mcp-types] L499–L554). There is no `type` key. Unknown keys are accepted silently unless strict config validation is on (the struct has `#[schemars(deny_unknown_fields)]` but not serde's).

| Transport | Fields | May hold secrets |
|---|---|---|
| stdio | `command`, `args`, `env` (map), `env_vars` (names or `{name, source = "local"\|"remote"}` to forward), `cwd` | `env` values, `args` |
| streamable HTTP | `url`, `bearer_token_env_var`, `http_headers` (map), `env_http_headers` (header → env var name), `http_headers_helper` (a shell command that prints headers), `auth` (`"oauth"`, `"chatgpt"`, `"ema_auth"`), `oauth = {client_id, client_secret, callback_url, callback_port, authorization_server_issuer}`, `oauth_resource`, `scopes` | `http_headers` values (e.g. `Authorization`), `oauth.client_secret`, the query string of `url`, and the helper command line |
| both | `enabled` (default `true`), `required`, `environment_id` (default `"local"`), `startup_timeout_sec` or `startup_timeout_ms`, `tool_timeout_sec`, `startup_readiness`, `supports_parallel_tool_calls`, `tool_input_schema_max_bytes`, `omit_tools_from`, `default_tools_approval_mode`, `enabled_tools`, `disabled_tools`, `tools.<tool>.{approval_mode, output_token_limit}` | no |

- Inline `bearer_token` is rejected: "mcp_servers.<name> uses unsupported `bearer_token`; set `bearer_token_env_var`" ([S-config] L2314–L2333; [S-mcp-types] L529).
- Mixing transports in one server (`command` plus `url`, `http_headers`, `oauth`, …) is an error ([S-mcp-types] L492–L529).
- OAuth tokens obtained by `codex mcp login` go to the OS keyring, or to `%CODEX_HOME%\.credentials.json` when the keyring is unavailable or `mcp_oauth_credentials_store = "file"` ([S-types] L130–L142). `auth.json` holds the Codex sign-in itself ([D-config-adv]). Neither file is MCP config, but both are secrets that config-digger must never read.
- **[contradicts docs]** The MCP page lists a stdio field `experimental_environment` ([D-mcp]). 0.159.3 has no such key. Its equivalent is `environment_id`.

On disk [disk]: `cd-throwaway-user` is stdio with `command`, `args` and `env.CD_FAKE_TOKEN`. `cd-throwaway-http` has `url` and `bearer_token_env_var = "CD_FAKE_BEARER"`. `cd-throwaway-project` is stdio.

---

## 3. Skills per Scope

Roots come from [`ext/skills/src/host_roots.rs`][S-skill-roots] L28–L131 and L137–L185. The test at [S-skill-roots-tests] L243–L290 pins the order:

| Scope | Folder | `SkillScope` | Note |
|---|---|---|---|
| project | `<dir>\.codex\skills\` for each project layer, **including disabled (untrusted) ones** | Repo | uses `all_layers_high_to_low` ([S-state] L560–L574) |
| project | `<dir>\.agents\skills\` for each folder from the project root to the cwd | Repo | only if the folder exists |
| user | `%CODEX_HOME%\skills\` | User | "Deprecated user skills location … kept for backward compatibility" |
| user | `%USERPROFILE%\.agents\skills\` | User | resolved from the OS home, not `CODEX_HOME` |
| system | `%CODEX_HOME%\skills\.system\` | System | bundled skills, written by Codex ([S-skills-lib] L63–L105) |
| admin | `%ProgramData%\OpenAI\Codex\skills\` (the folder of the system `config.toml`) | Admin | |
| plugin | `<plugin root>\skills\`, or the manifest's `skills` path | plugin | [S-plugin-loader] L1079–L1108 |

A skill is a folder with `SKILL.md` whose frontmatter has `name` and `description` ([D-skills]).

**[contradicts docs]** The skills page lists `$CWD/.agents/skills`, `$CWD/../.agents/skills`, `$REPO_ROOT/.agents/skills`, `$HOME/.agents/skills`, `/etc/codex/skills` and the bundled set ([D-skills]). It doesn't mention `.codex/skills` or `$CODEX_HOME/skills`, both of which the source reads.

**[contradicts docs]** Untrusted projects still contribute `.codex\skills`. A probe skill in an untrusted project's `.codex\skills` appeared in `codex debug prompt-input` output [sandbox]. The docs say untrusted projects skip "project-scoped `.codex/` layers" ([D-config-basic], [D-config-adv]).

On this machine [disk]: `~/.codex\skills\` holds only `.system\` (`imagegen`, `openai-docs`, `review-agent`, `skill-creator`, `skill-installer`, plus a `.codex-system-skills.marker`). `~/.agents\skills\` holds `computer-use`, `orca-cli` and `orchestration`, and Codex lists them in its prompt [sandbox]. The two remote plugins ship `plugin-management` and `create-pet`, `pets`, `update-pet`.

---

## 4. Precedence and merge rules

### 4.1 Config keys, including `mcp_servers`

All enabled layers are folded low to high with [`merge_toml_values`][S-merge] (L56–L61, L139–L150). Tables merge recursively key by key, and anything that isn't a table (strings, numbers, **arrays**) is replaced whole. `mcp_servers` gets no special case. So:

- Same server name in two layers: one server whose fields come from both, higher layer winning per field. Verified with user `dup = {command="user-cmd", args=["u"], env.FROM_USER}` and project `dup = {args=["p"]}`: the result is `command = "user-cmd"`, `args = ["p"]`, `env.FROM_USER` kept [sandbox].
- `args`, `enabled_tools`, `disabled_tools`, `scopes` are arrays and replace wholesale. `env`, `http_headers`, `env_http_headers`, `tools` are tables and merge per key.
- A partial override in a higher layer can disable a lower Entry (`enabled = false`) without repeating its transport [sandbox].
- Conflicting transports across layers (user `command`, project `url`) make the merged server invalid, and **Codex refuses to start**: "Error: failed to load bootstrap configuration … url is not supported for stdio in `mcp_servers.mixed`" [sandbox].
- The loader keeps per-key origins (`ConfigLayerStack::origins()`, [S-loader-readme]), which is the closest Codex gets to "which layer won".

For config-digger this means "Effective vs Shadowed" for Codex is per field: the Effective server is the merge, and each Entry contributes some fields and has others Shadowed. Dropping a lower Entry can change the Effective server even when a higher Entry with the same name exists.

### 4.2 MCP servers from different sources

After the merge, servers go into a catalog with a source tier ([`codex-mcp/src/catalog.rs`][S-catalog] L118–L137, L476–L558). Sorted low to high, last one wins per name:

1. plugin servers (the plugin sorted first by `<name>@<marketplace>` wins among plugins; [S-plugin-loader] L162–L198 logs "skipping duplicate plugin MCP server name");
2. thread-selected plugins;
3. `mcp_servers` from config;
4. compatibility (host) servers;
5. extensions.

So a `[mcp_servers.<name>]` Entry Shadows a plugin server with the same name, whole Entry, no field merge. A server disabled at a higher tier stays disabled ([S-catalog] L516–L535).

### 4.3 Skills

No Shadowing. Roots are deduped by path ([S-skill-roots] L271–L274), skills by `SKILL.md` path, and the list is sorted by scope rank, then name, then path ([S-skill-merge] L232–L249). Same-named skills from different roots all remain. The test is literally named `resolved_config_and_repo_roots_preserve_order_and_dedupe_paths_not_names` ([S-skill-roots-tests] L596–L677). The docs agree: "If two skills share the same `name`, Codex doesn't merge them; both can appear in skill selectors" ([D-skills]). Plugin skills are namespaced `<plugin>:<skill>`.

### 4.4 Plugins

`[plugins]` is read from the merged config of enabled layers, so a trusted project can enable or disable a plugin ([S-plugin-loader] L162–L176; test `configured_plugins_from_stack_merges_enabled_effective_layers` in `loader_tests.rs` L413).

---

## 5. Plugins and other attributions on disk

- **Installed plugins** live in `%CODEX_HOME%\plugins\cache\<marketplace>\<plugin>\<version>\`. The highest valid version folder is the active one ([S-installed] L10–L12, L47–L60; [S-store] L25, L108–L116). Per-plugin data goes to `%CODEX_HOME%\plugins\data\` ([S-store] L25–L26).
- **Manifest**: `<version>\.codex-plugin\plugin.json` (`name`, `version`, `skills`, `mcpServers`, `apps`, `hooks`, `interface`). Codex also recognises `.claude-plugin\plugin.json`, `.cursor-plugin\plugin.json` and a bare `plugin.json` ("agent plugins") ([S-manifest-paths] L48–L53; [S-plugin-ns] L12).
- **Plugin MCP servers** come from the manifest's `mcpServers` (inline object or path), else `<root>\.mcp.json`. Their transport is owned by the plugin. Host config may only add policy under `[plugins."<id>".mcp_servers.<name>]` (`enabled`, approval mode, tool lists) ([S-plugin-loader] L69, L1109–L1130, L1513–L1580; [S-types] L1002–L1030). The catalog attributes each such server to its plugin id and root ([S-config] L1772–L1799), which is how config-digger can attribute an Entry: a server that lives in a plugin's `.mcp.json` under `plugins\cache\<marketplace>\<plugin>\` has plugin Provenance.
- **Enablement record**: `[plugins."<plugin>@<marketplace>"] enabled = true|false` in config ([S-cfgtoml] L494; [S-plugin-loader] L731–L744, L859).
- **Remote-installed plugins** (account catalog, marketplace `openai-curated-remote`) are not listed in `config.toml`. Each has `<plugin>\.codex-remote-plugin-install.json` (`{schema_version, remote_plugin_id}`) next to its version folders. Codex takes their `enabled` from the server-side install list and keeps only the local `mcp_servers` policy ([S-plugin-loader] L233–L372). On this machine `plugin-management 0.1.0` and `work-pets 0.1.6` are installed this way, with no `[plugins]` table in `config.toml` [disk]. Their Provenance is cloud.
- **Marketplaces**: `[marketplaces.<name>]` in config, cloned into `%CODEX_HOME%\.tmp\marketplaces\<name>`, plus the OpenAI curated clone at `%CODEX_HOME%\.tmp\plugins\` (present here with `plugins.sha`) and `~/.agents/plugins/marketplace.json`. Marketplace manifests may be `.agents/plugins/marketplace.json`, `.claude-plugin/marketplace.json` or `.cursor-plugin/marketplace.json` ([S-marketplace] L20–L25; [S-installed-mkt] L12–L40; [S-plugin-mgr] L3557–L3594). These are install sources, not Entries.
- **Imports from other Agents.** Codex has an "external agent config migration" that copies Claude Code and Cursor MCP servers into a Codex `config.toml` (missing names only) and skills into `.agents\skills`, at home or repo scope ([S-ext-migr-service] L467, L633, L692–L712; [S-ext-migr-mcp] L10–L11; [S-ext-migr-scope]). Each completed import is a row in `state_5.sqlite` table `external_agent_config_imports`, empty here [disk]. A copied Entry is an ordinary file Entry afterwards. Nothing marks it as imported in the TOML.

---

## 6. Native Disable mechanisms

| Item | How | Where | Tag |
|---|---|---|---|
| MCP server (config) | `enabled = false` in its table. "Set `false` to disable a server without deleting it." | any layer. Thanks to the deep merge, a higher layer can disable a lower Entry with just `[mcp_servers.<name>] enabled = false` | [docs] [D-mcp]; [source] [S-mcp-types] L245–L246; [sandbox] |
| MCP server tools | `enabled_tools` / `disabled_tools` / `tools.<tool>` | same table | [source] |
| plugin MCP server | `[plugins."<id>".mcp_servers.<name>] enabled = false` | config | [source] [S-plugin-loader] L966–L975, L1431–L1452 |
| skill | `[[skills.config]]` with `path = "<abs path to SKILL.md>"` **or** `name = "<name>"`, and `enabled = false`. Later rules win, and a name rule hits every skill with that name | **user layer (incl. profile) and `-c` only**. Project and managed layers are ignored | [docs] [D-skills]; [source] [S-skills-cfg] L90–L126, L150–L211; [sandbox]: a project-level rule had no effect, the same rule via `-c` hid the skill |
| bundled skills | `[skills.bundled] enabled = false` | merged config | [source] [S-skills-cfg] L128–L147 |
| plugin | `[plugins."<name>@<marketplace>"] enabled = false`, or `/plugins` in the TUI | merged config | [source] [S-plugin-loader] L859; [docs] [D-plugins] |
| remote plugin | toggled in the account, mirrored locally on load | server | [source] [S-plugin-loader] L339–L372 |
| admin restriction | `requirements.toml` `[mcp_servers.<name>] identity = { command = "…" }` (whatever the args) or `{ url = "…" }`, or a matcher form: `identity = { command = { executable = "…", args = [<matcher>, …] } }`, as many args each matching, or `identity = { url = <matcher> }`, a matcher being `{ match = "exact" \| "prefix", value }` or `{ match = "regex", expression }` on the whole value. A regex that does not compile stops Codex. A server not matching any entry is forced `enabled = false` with reason `requirements (<source>)`. `[plugins."<id>".mcp_servers]` does the same for plugin servers | `%ProgramData%\OpenAI\Codex\requirements.toml` or cloud | [docs] [D-managed]; [source] [S-config] L2166–L2240, `codex-rs/config/src/mcp_requirements.rs`; [test] `uat/agents/codex-machine.mjs` in Windows Sandbox, 0.159.3 |

The TUI writes `[[skills.config]]` entries itself (`SetSkillConfig`, `SetSkillConfigByName`) ([S-edit] L61–L64). It writes skill paths after normalising them ([S-edit] L480–L489), and the loader canonicalises path selectors before matching ([S-skills-cfg] L188–L192). config-digger should write absolute, canonical `SKILL.md` paths to match.

---

## 7. Does Codex rewrite files while running? Process names

**Writers of `config.toml`.** All go through `ConfigEditsBuilder`: read the whole file, edit it with `toml_edit` (format-preserving), then write a temp file in the same folder and rename it over the original ([S-edit] L726–L770; [S-path-utils] L144–L156). There is no lock, so a concurrent writer loses its change. Edits that happen at runtime ([S-edit] L31–L75):

- trust prompt → `[projects.'<path>'] trust_level` ([S-config] L2335+; TUI `config_update.rs` L81);
- model and reasoning choice, service tier, `[notice]` acknowledgements, `[tui]` settings such as `screen_reader_detection_done` (present here [disk]);
- `[windows] sandbox = "elevated"` from Windows sandbox setup (present here [disk]);
- skill toggles → `[[skills.config]]`;
- `codex mcp add/remove` → replaces the whole `[mcp_servers]` table. Existing non-inline server tables are re-serialised from the parsed config, so comments inside them are lost and fields are normalised (for example `startup_timeout_ms` becomes `startup_timeout_sec`) ([S-edit] L425–L478);
- plugin and marketplace installs → `[plugins]`, `[marketplaces]`.

**Other files Codex writes on its own:**

- `%CODEX_HOME%\skills\.system\` is deleted and re-extracted whenever the embedded fingerprint differs from `.codex-system-skills.marker` ([S-skills-lib] L69–L105). A fresh home got `installation_id` and `skills\.system\` on the first `codex debug prompt-input` [sandbox].
- `plugins\cache\` (remote plugin sync), `.tmp\plugins\` (curated marketplace clone), `cache\`, `models_cache.json`, `version.json`, `*.sqlite` with WAL files, `tmp\arg0\` (stale folders are cleaned up) [disk].
- I found no file watcher on `config.toml`. The app server watches the skill roots and emits "skills changed", throttled to every 10 s ([S-skills-watcher] L25–L28, L76–L130). The skills doc says to restart Codex after editing `[[skills.config]]` ([D-skills]).

**Processes** [disk], while a Codex TUI was running on this machine:

| Process | Path / command line |
|---|---|
| `node.exe` | `…\Volta\tools\image\node\22.23.1\node.exe …\@openai\codex\bin\codex.js` (npm shim) |
| `codex.exe` | `…\@openai\codex\node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex.exe` (the TUI) |
| `codex.exe` | `%USERPROFILE%\.codex\packages\app-server-daemon\releases\0.159.3-x86_64-pc-windows-msvc\bin\codex.exe app-server --listen unix:// --managed-daemon` |
| `codex.exe` | same binary, `app-server daemon pid-update-loop` |

The app-server daemon is a long-lived shared process with pid and lock files in `%CODEX_HOME%\app-server-daemon\` (`daemon.pid`, `daemon.lock`, …) [disk]. A config-digger "is Codex running?" check can look for any `codex.exe`, or for a live `daemon.pid` in each Codex home. The desktop app (Microsoft Store package `OpenAI.Codex`, [D-win-deploy]) isn't installed here, so its process name wasn't observed.

### 7.1 Desktop app and IDE extension

Per the docs, "the ChatGPT desktop app, Codex CLI, and IDE extension share this configuration" ([D-mcp]), and "the CLI and IDE extension share the same configuration layers" ([D-config-basic]). The Windows app stores config and sessions in `%USERPROFILE%\.codex` in native mode. In WSL mode it uses the Linux `~/.codex` unless `CODEX_HOME` points at `/mnt/c/Users/<user>/.codex` ([D-win-app]). So the app and extension don't add new Scopes on Windows. WSL is a separate home. `codex app` launches or installs the desktop app. Neither the app nor the IDE extension is installed here (no `OpenAI.Codex` package, no `openai.chatgpt` VS Code extension) [disk], so this is [docs] only.

---

## 8. Where Codex records the projects it has opened

| Record | What it holds | Here |
|---|---|---|
| `config.toml` `[projects.'<path>']` | every folder the user answered the trust prompt for, lower-cased on Windows | one entry, `e:\scratch\config-digger-agents\codex` [disk] |
| `%CODEX_HOME%\sessions\YYYY\MM\DD\rollout-<ts>-<uuid>.jsonl` (+ `archived_sessions\`) | one file per session. The first `session_meta` line has `cwd` | folder absent: no session was saved [disk]; layout from [S-rollout-list] L438, [S-rollout] L86–L87, [S-session-meta] L3123–L3142 |
| `state_5.sqlite` `threads.cwd` (+ `git_origin_url`, `git_branch`, `project_id`) | indexed thread list | table empty in both homes [disk] |
| `state_5.sqlite` `projects` / `project_roots(path)` | named projects with root folders (migration `0049_projects.sql`), used by the app's project list | empty [disk] |
| `[notice.external_config_migration_prompts.projects]` | projects where the import prompt was shown or dismissed | absent |

The DBs were copied to the scratchpad and opened with `sqlite3 -readonly` [disk]. `state_5.sqlite` is the 0.159.3 file name ([S-sqlite] L33). The number is a schema generation and will change.

---

## 9. macOS / Linux differences (not verified)

| Item | Windows | macOS / Linux | Tag |
|---|---|---|---|
| user home | `%USERPROFILE%\.codex` | `~/.codex` | [source] [S-home] |
| system config | `%ProgramData%\OpenAI\Codex\config.toml` | `/etc/codex/config.toml` | [source] [S-loader] L79, L778–L786 |
| requirements | `%ProgramData%\OpenAI\Codex\requirements.toml` | `/etc/codex/requirements.toml` | [source] L723–L731 |
| legacy managed config | ignored | `/etc/codex/managed_config.toml`, still loaded as the top layer and as requirements | [source] [S-layer-io] L21–L22, L222–L233 |
| MDM | none | macOS managed preferences domain `com.openai.codex`, keys `config_toml_base64`, `requirements_toml_base64` | [source] [S-macos] L27–L29 |
| admin skills | `%ProgramData%\OpenAI\Codex\skills` | `/etc/codex/skills` | [source]; [docs] [D-skills] |
| trust keys | ASCII-lower-cased | case-sensitive | [source] [S-loader] L1393–L1399 |

---

## Product implications for config-digger

1. **The Codex Agent definition needs a "deep merge" precedence mode.** Per-name "first wins" is wrong here. Effective state is per field, a partial Entry is legal, and conflicting partial Entries crash Codex.
2. **Discovery must enumerate Codex homes.** `CODEX_HOME` from the environment config-digger runs in is only one of them. `%USERPROFILE%\.codex` is always a candidate, and Orca's home is another on this machine. Linked `plugins`/`skills` folders must be deduplicated by resolved path.
3. **Trust is part of Effective.** A project Entry is Effective only if its folder, the project root or the repo root is trusted in the merged non-project config. Skills under `.codex\skills` ignore trust.
4. **Disable for MCP servers can be a one-line partial table in a higher layer**, but the safe default is to write `enabled = false` into the Entry's own table. Removing a server whose name exists elsewhere changes the merge result.
5. **Skill Disable has to go in the user `config.toml`**, never in a project file, and should use canonical `SKILL.md` paths. There is no Shadowing between same-named skills to show.
6. **Write safety.** Codex rewrites `config.toml` atomically with temp-and-rename and no lock, and `codex mcp add/remove` reflows the whole `[mcp_servers]` table. config-digger must re-read right before writing and keep a backup.
7. **Shared folders with other Agents.** `~/.agents/skills` is read by Codex and VS Code. Codex also reads `.claude-plugin`/`.cursor-plugin` manifests inside its own plugin cache, and its import feature copies Claude Code and Cursor Entries.

---

## Sources

Docs (the `developers.openai.com/codex/*` URLs redirect here):

- [D-config-basic](https://learn.chatgpt.com/docs/config-file/config-basic): https://learn.chatgpt.com/docs/config-file/config-basic
- [D-config-adv](https://learn.chatgpt.com/docs/config-file/config-advanced): https://learn.chatgpt.com/docs/config-file/config-advanced
- [D-mcp](https://learn.chatgpt.com/docs/extend/mcp?surface=cli): https://learn.chatgpt.com/docs/extend/mcp?surface=cli
- [D-skills](https://learn.chatgpt.com/docs/build-skills): https://learn.chatgpt.com/docs/build-skills
- [D-plugins](https://learn.chatgpt.com/docs/plugins): https://learn.chatgpt.com/docs/plugins
- [D-managed](https://learn.chatgpt.com/docs/enterprise/managed-configuration): https://learn.chatgpt.com/docs/enterprise/managed-configuration
- [D-win-app](https://learn.chatgpt.com/docs/windows/windows-app): https://learn.chatgpt.com/docs/windows/windows-app
- [D-win-deploy](https://learn.chatgpt.com/docs/enterprise/windows-deployment): https://learn.chatgpt.com/docs/enterprise/windows-deployment

Source (`openai/codex` @ `rust-v0.159.3`, paths under `codex-rs/`):

- [S-loader](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/loader/mod.rs): `config/src/loader/mod.rs`
- [S-loader-readme](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/loader/README.md): `config/src/loader/README.md`
- [S-layer-io](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/loader/layer_io.rs): `config/src/loader/layer_io.rs`
- [S-macos](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/loader/macos.rs): `config/src/loader/macos.rs`
- [S-layer-src](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/config_layer_source.rs): `config/src/config_layer_source.rs`
- [S-merge](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/merge.rs): `config/src/merge.rs`
- [S-state](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/state.rs): `config/src/state.rs`
- [S-trust](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/project_trust.rs): `config/src/project_trust.rs`
- [S-markers](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/project_root_markers.rs): `config/src/project_root_markers.rs`
- [S-cfgtoml](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/config_toml.rs): `config/src/config_toml.rs`
- [S-mcp-types](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/mcp_types.rs): `config/src/mcp_types.rs`
- [S-skills-cfg](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/skills_config.rs): `config/src/skills_config.rs`
- [S-types](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/types.rs): `config/src/types.rs`
- [S-symlink](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/codex_home_symlink.rs): `config/src/codex_home_symlink.rs`
- [S-home](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/utils/home-dir/src/lib.rs): `utils/home-dir/src/lib.rs`
- [S-path-utils](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/utils/path-utils/src/lib.rs): `utils/path-utils/src/lib.rs`
- [S-config](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/core/src/config/mod.rs): `core/src/config/mod.rs`
- [S-edit](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/core/src/config/edit.rs): `core/src/config/edit.rs`
- [S-mcp-cmd](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/cli/src/mcp_cmd.rs): `cli/src/mcp_cmd.rs`
- [S-catalog](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/codex-mcp/src/catalog.rs): `codex-mcp/src/catalog.rs`
- [S-oauth](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/rmcp-client/src/oauth.rs): `rmcp-client/src/oauth.rs`
- [S-skill-roots](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/ext/skills/src/host_roots.rs): `ext/skills/src/host_roots.rs`
- [S-skill-roots-tests](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/ext/skills/src/host_roots_tests.rs): `ext/skills/src/host_roots_tests.rs`
- [S-skill-merge](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/ext/skills/src/loader/host_merge.rs): `ext/skills/src/loader/host_merge.rs`
- [S-skills-lib](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/skills/src/lib.rs): `skills/src/lib.rs`
- [S-skills-watcher](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/app-server/src/skills_watcher.rs): `app-server/src/skills_watcher.rs`
- [S-plugin-loader](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/core-plugins/src/loader.rs): `core-plugins/src/loader.rs`
- [S-plugin-mgr](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/core-plugins/src/manager.rs): `core-plugins/src/manager.rs`
- [S-store](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/core-plugins/src/store.rs): `core-plugins/src/store.rs`
- [S-manifest-paths](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/exec-server-protocol/src/protocol.rs): `exec-server-protocol/src/protocol.rs`
- [S-marketplace](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/core-plugins/src/marketplace.rs): `core-plugins/src/marketplace.rs`
- [S-installed-mkt](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/core-plugins/src/installed_marketplaces.rs): `core-plugins/src/installed_marketplaces.rs`
- [S-installed](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/core-plugin-common/src/installed.rs): `core-plugin-common/src/installed.rs`
- [S-plugin-ns](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/utils/plugins/src/plugin_namespace.rs): `utils/plugins/src/plugin_namespace.rs`
- [S-ext-migr-service](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/external-agent-migration/src/service.rs): `external-agent-migration/src/service.rs`
- [S-ext-migr-mcp](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/external-agent-migration/src/mcp.rs): `external-agent-migration/src/mcp.rs`
- [S-ext-migr-scope](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/external-agent-migration/src/scope.rs): `external-agent-migration/src/scope.rs`
- [S-sqlite](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/state/src/sqlite.rs): `state/src/sqlite.rs`
- [S-rollout](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/rollout/src/lib.rs): `rollout/src/lib.rs`
- [S-rollout-list](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/rollout/src/list.rs): `rollout/src/list.rs`
- [S-protocol](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/protocol/src/config_types.rs): `protocol/src/config_types.rs`
- [S-session-meta](https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/protocol/src/protocol.rs): `protocol/src/protocol.rs`

On-disk evidence (read-only): `%USERPROFILE%\.codex\` (`config.toml`, `skills\`, `plugins\cache\`, `.tmp\`, `app-server-daemon\`, `packages\`, a read-only copy of `state_5.sqlite`), `%APPDATA%\orca\codex-runtime-home\home\` (`config.toml`, `.orca-config-settings-baseline.json`, link targets of `plugins` and `skills`, a copy of `state_5.sqlite`), `E:\scratch\config-digger-agents\codex\.codex\config.toml`, `%USERPROFILE%\.agents\skills\`, running processes via `Win32_Process`. Sandbox runs used `CODEX_HOME=<scratchpad>\sbx\home` and throwaway projects marked with `project_root_markers = [".projroot"]`.

## Open questions

1. **Orca's mirror.** How and when does Orca copy `~/.codex\config.toml` into its home, and in which direction? `.orca-config-settings-baseline.json` suggests a three-way sync of selected keys and `mcpServers`. If config-digger Disables an Entry in `~/.codex`, does Orca propagate it, or revert it? This needs Orca's source or a controlled test, not read-only inspection.
2. **Remote plugin enablement offline.** Where, if anywhere, is the server-side enabled state of `openai-curated-remote` plugins cached locally? Without it, config-digger can't tell whether `plugin-management` and `work-pets` are enabled without calling the account API.
3. **Untrusted `.codex\skills`.** Is loading skills from disabled project layers intended or a bug? It contradicts the docs. If OpenAI fixes it, the Agent definition's trust rule for skills will change with the version.
4. **Desktop app on Windows.** Its process name, whether it starts its own app-server or reuses the CLI daemon in `~/.codex\packages\app-server-daemon`, and whether it writes `projects`/`project_roots` in `state_5.sqlite`. The app isn't installed here.
5. **Cloud-managed config.** The shape and local caching (if any) of enterprise cloud bundles. Nothing on this personal account exercises it.
6. **Thread-scoped config loaders** (`thread_config_loader`) insert layers by precedence from the app server. Which clients use them, and can they carry `mcp_servers`?
7. **Docs vs binary drift.** `experimental_environment` vs `environment_id`, Windows `managed_config.toml`, profile order. The Agent definition may need a minimum version or per-version rules.

<!-- link reference definitions -->
[D-config-basic]: https://learn.chatgpt.com/docs/config-file/config-basic
[D-config-adv]: https://learn.chatgpt.com/docs/config-file/config-advanced
[D-mcp]: https://learn.chatgpt.com/docs/extend/mcp?surface=cli
[D-skills]: https://learn.chatgpt.com/docs/build-skills
[D-plugins]: https://learn.chatgpt.com/docs/plugins
[D-managed]: https://learn.chatgpt.com/docs/enterprise/managed-configuration
[D-win-app]: https://learn.chatgpt.com/docs/windows/windows-app
[D-win-deploy]: https://learn.chatgpt.com/docs/enterprise/windows-deployment
[S-loader]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/loader/mod.rs
[S-loader-readme]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/loader/README.md
[S-layer-io]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/loader/layer_io.rs
[S-macos]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/loader/macos.rs
[S-layer-src]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/config_layer_source.rs
[S-merge]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/merge.rs
[S-state]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/state.rs
[S-trust]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/project_trust.rs
[S-markers]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/project_root_markers.rs
[S-cfgtoml]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/config_toml.rs
[S-mcp-types]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/mcp_types.rs
[S-skills-cfg]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/skills_config.rs
[S-types]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/types.rs
[S-symlink]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/config/src/codex_home_symlink.rs
[S-home]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/utils/home-dir/src/lib.rs
[S-path-utils]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/utils/path-utils/src/lib.rs
[S-config]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/core/src/config/mod.rs
[S-edit]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/core/src/config/edit.rs
[S-mcp-cmd]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/cli/src/mcp_cmd.rs
[S-catalog]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/codex-mcp/src/catalog.rs
[S-oauth]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/rmcp-client/src/oauth.rs
[S-skill-roots]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/ext/skills/src/host_roots.rs
[S-skill-roots-tests]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/ext/skills/src/host_roots_tests.rs
[S-skill-merge]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/ext/skills/src/loader/host_merge.rs
[S-skills-lib]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/skills/src/lib.rs
[S-skills-watcher]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/app-server/src/skills_watcher.rs
[S-plugin-loader]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/core-plugins/src/loader.rs
[S-plugin-mgr]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/core-plugins/src/manager.rs
[S-store]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/core-plugins/src/store.rs
[S-manifest-paths]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/exec-server-protocol/src/protocol.rs
[S-marketplace]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/core-plugins/src/marketplace.rs
[S-installed-mkt]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/core-plugins/src/installed_marketplaces.rs
[S-installed]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/core-plugin-common/src/installed.rs
[S-plugin-ns]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/utils/plugins/src/plugin_namespace.rs
[S-ext-migr-service]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/external-agent-migration/src/service.rs
[S-ext-migr-mcp]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/external-agent-migration/src/mcp.rs
[S-ext-migr-scope]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/external-agent-migration/src/scope.rs
[S-sqlite]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/state/src/sqlite.rs
[S-rollout]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/rollout/src/lib.rs
[S-rollout-list]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/rollout/src/list.rs
[S-protocol]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/protocol/src/config_types.rs
[S-session-meta]: https://github.com/openai/codex/blob/rust-v0.159.3/codex-rs/protocol/src/protocol.rs
