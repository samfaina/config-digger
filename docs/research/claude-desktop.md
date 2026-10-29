# Research: Claude Desktop configuration on Windows

Issue: [#9](https://github.com/samfaina/config-digger/issues/9). Terms follow [`CONTEXT.md`](../../CONTEXT.md).

Verified against Claude Desktop **2.16120.0.0**, installed from the Microsoft Store as MSIX package `Claude_pzs8sxrjxfjjc`, on Windows 11 Pro 10.0.26200, user profile `C:\Users\kgon1`, on 2026-10-01. Each fact is tagged:

- **[disk]**: verified on disk, read-only.
- **[bundle]**: read from the app's own shipped JavaScript, `app\resources\app.asar` in the package install folder. Claude Desktop is closed source, so this is the nearest thing to its source code. It is also on disk, but it describes behaviour, not state. Minified names change between releases, so these facts should be re-checked when the app updates.
- **[docs]**: stated by a primary source and not observable on this machine.
- **[contradicts docs]**: what is on disk differs from, or goes beyond, what the docs say.

Secret values are never reproduced. Account and organization UUIDs are shortened to `<account>` and `<org>`. Tokens and encrypted values appear only as a key name or `***`.

Throughout, `<userData>` means Claude Desktop's data folder. Section 1 explains how it resolves. On this machine it is:

```
C:\Users\kgon1\AppData\Local\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Claude
```

## Summary

Claude Desktop is a single-Scope Agent. It has no project or local Scope. MCP servers come from five places:

1. **User file**: `<userData>\claude_desktop_config.json`, key `mcpServers`. Only stdio servers (`command`, `args`, `env`) are valid there.
2. **Desktop extensions** (`.mcpb`, formerly `.dxt`): unpacked into `<userData>\Claude Extensions\<id>\`, with on/off state in `<userData>\Claude Extensions Settings\<id>.json` and install records in `<userData>\extensions-installations.json`.
3. **Managed policy**: `HKLM\SOFTWARE\Policies\Claude` or else `HKCU\SOFTWARE\Policies\Claude`, key `managedMcpServers`, plus gates that switch user-added servers and extensions off.
4. **claude.ai connectors**: cloud-only. No authoritative copy on disk, only per-session snapshots.
5. **Org plugins / marketplaces**: cloud-delivered.

The documented path `%APPDATA%\Claude` **does not exist** for an MSIX install when viewed from an ordinary process such as config-digger. The Store package virtualises writes to `%APPDATA%` into `%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Claude`. The app's own code probes three candidates in order:

- `%LOCALAPPDATA%\Claude-Data`, used only when `%APPDATA%` is a UNC path;
- `%APPDATA%\Claude`;
- the `LocalCache\Roaming\Claude` path above.

A package can also move its data folder out of its `LocalCache`, to `%LOCALAPPDATA%\Claude-Data\<family>`, which it then reads before the other two (§1.1.1). config-digger probes the same folders. Third-party-inference deployments (Bedrock, Vertex and similar) use the separate folder `%LOCALAPPDATA%\Claude-3p`.

Precedence is per server name. A name in `claude_desktop_config.json` replaces an extension's server of the same name outright, with no field merge. Extensions are keyed by their display name.

Native Disables:

- one extension: `isEnabled: false` in its settings file;
- all extensions: managed key `isDesktopExtensionEnabled`;
- all user-file servers: managed key `isLocalDevMcpEnabled`;
- individual tools: `<userData>\mcp-user-tool-toggles.json`.

The user file has **no per-server disable flag**: its schema is closed, so Disabling one Entry there means moving it out of the file.

Desktop skills exist on disk only as a **read-only mirror of the claude.ai account's skills**. The mirror lives at `<userData>\local-agent-mode-sessions\skills-plugin\<org>\<account>\` and its `manifest.json` carries an `enabled` flag per skill. It is cloud-managed Provenance: Disable and Remove happen in the account, not on disk. The Desktop "Code" tab runs Claude Code, which reads `~/.claude/skills` and also loads the `claude_desktop_config.json` servers. That is Claude Code's configuration (see `research/claude-code`).

The app rewrites `claude_desktop_config.json` while it runs, because the same file holds UI `preferences`. It does so from an in-memory copy, so external edits made while it runs can be lost, and the docs require a full restart after editing. The process is `claude.exe` under `C:\Program Files\WindowsApps\Claude_*`, which has the same image name as the Claude Code CLI. The `CoworkVMService` service (`cowork-svc.exe`) also runs.

---

## 1. Where MCP servers are configured

### 1.1 The data folder (`<userData>`) and MSIX virtualisation

| Install | Path the app uses | Visible to other processes at | Tag |
|---|---|---|---|
| Classic installer (`.exe`) or unpackaged | `%APPDATA%\Claude\` | same | [docs] ([MCP quickstart](https://modelcontextprotocol.io/docs/develop/connect-local-servers): "**Windows**: `%APPDATA%\Claude\claude_desktop_config.json`"). Not present on this machine [disk] |
| MSIX, Store package `Claude_pzs8sxrjxfjjc` | `%APPDATA%\Claude\` as the app sees it | `%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Claude\` | [disk]. [contradicts docs]: Anthropic documents only `%APPDATA%\Claude` |
| MSIX, enterprise package `AnthropicPBC.Claude_fnn82j28hfe8t` | same scheme | `%LOCALAPPDATA%\Packages\AnthropicPBC.Claude_fnn82j28hfe8t\LocalCache\Roaming\Claude\` | [bundle]: the app lists both package family names. Not installed here [disk] |
| MSIX, either package, once it moved its data out of the package (3.6556 and 4.29) | `%LOCALAPPDATA%\Claude-Data\<family>`, the package family in lower case, such as `claude_pzs8sxrjxfjjc` | same: the package writes `Claude-Data` to its real path | [bundle], [disk], [test]. See [§1.1.1](#111-the-move-to-claude-datafamily) |
| `%APPDATA%` on a UNC share (redirected roaming profile) | `%LOCALAPPDATA%\Claude-Data\` | same | [bundle]: chosen only when `%APPDATA%` is a `\\server\…` path. The managed key `relocateUncUserData` also exists. The folder exists here but is empty [disk] |
| Third-party inference mode ("3P") | `%LOCALAPPDATA%\Claude-3p\`. Older releases used `%APPDATA%\Claude-3p\` and migrated it | same | [docs] ([3P data storage](https://claude.com/docs/third-party/claude-desktop/data-storage)). [bundle] confirms. Exists here but is empty [disk] |
| Override | `--user-data-dir=<folder>` on the command line. The `CLAUDE_USER_DATA_DIR` env var sets `userData` only in unsigned builds: signed builds ignore it and use the default data folder, so config-digger takes no home from it (#368) | same | [test, bundle]. `CLAUDE_E2E_1P_USERDATA` and `CLAUDE_E2E_3P_USERDATA` are test-only absolute-path overrides [bundle] |

How MSIX virtualisation works, per Microsoft. On Windows 10 1903 and later, new files a packaged app writes under `AppData` "are written to a private per-user, per-app location; but merged at runtime to appear in the real `AppData` location" ([MSIX behind the scenes](https://learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-behind-the-scenes)), and "Files in the virtualized location are visible only to the app" ([flexible virtualization](https://learn.microsoft.com/en-us/windows/msix/desktop/flexible-virtualization)). The redirect target is `%LocalAppData%\Packages\<PackageFamilyName>\LocalCache\` ([MSIX troubleshooting](https://learn.microsoft.com/en-us/windows/msix/msix-troubleshooting-guide)). [docs]

Verified here:

- `Test-Path $env:APPDATA\Claude` is `False` from an unpackaged shell, while `LocalCache\Roaming\Claude` holds `claude_desktop_config.json` and the rest of the app's state. [disk]
- The package manifest (`C:\Program Files\WindowsApps\Claude_2.16120.0.0_x64__pzs8sxrjxfjjc\AppxManifest.xml`) declares `virtualization:FileSystemWriteVirtualization` with `ExcludedDirectories` for `$(KnownFolder:LocalAppData)\Claude\logs`, `\Claude-3p` and `\Claude-Data`. Only those three Desktop folders are written to their real path. `%APPDATA%\Claude` is not excluded, so it is virtualised. [disk] The element and its `unvirtualizedResources` capability are documented in [the FileSystemWriteVirtualization schema](https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-virtualization-filesystemwritevirtualization). [docs]

The resolution order, as coded [bundle]. On Windows, the first-party data folder candidates are, in order:

1. `%LOCALAPPDATA%\Claude-Data` (only when `%APPDATA%\Claude` is UNC);
2. for a package, `%LOCALAPPDATA%\Claude-Data\<family>` once it moved there (§1.1.1, read in 3.6556 and 4.29);
3. `%APPDATA%\Claude`;
4. `%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Claude`.

The first one that exists wins, with a fallback to `%APPDATA%\Claude`. config-digger, which is unpackaged, should probe the same list and add the `AnthropicPBC.Claude_fnn82j28hfe8t` LocalCache path.

**Classic and MSIX side by side (inference, since tested: see below; how config-digger reads it: §1.1.2):** if a user has both a classic install and an MSIX install, both `%APPDATA%\Claude` and `LocalCache\Roaming\Claude` can exist. Microsoft says a packaged app reads "from the per-user, per-package location first", so the MSIX app sees its LocalCache copy of a file, and the classic file where it has none.

**Tested on 3.6556.2 in Windows Sandbox** (release gate, #80; [`release-gate.md`](release-gate.md)) [test]. The classic installer claude.ai serves (`claude.ai/api/desktop/win32/x64/exe/latest/redirect`) is Squirrel's: it installs for the user in `%LOCALAPPDATA%\AnthropicClaude` (a launcher `claude.exe`, `Update.exe`, `app-<version>\claude.exe`), with the uninstall key `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\AnthropicClaude`, and the classic app reads `%APPDATA%\Claude`. The packaged app's data folder is `%APPDATA%\Claude` too, as its `--user-data-dir` shows, with the package's `LocalCache` laid over it file by file, as Microsoft says: beside a classic folder it reads the classic files and creates no `LocalCache`, and a file its `LocalCache` holds wins over the classic one. Neither app starts an MCP server before someone signs in. On the reference machine, the package has moved its data to `%LOCALAPPDATA%\Claude-Data\claude_pzs8sxrjxfjjc` (§1.1.1).

**A data folder outside the lookup order, tested on 3.6556.2 in Windows Sandbox** (#211) [test]. The signed app ignores `CLAUDE_USER_DATA_DIR`: started with it set to a throwaway folder, it reads `%APPDATA%\Claude`, and each of its Chromium children names `%APPDATA%\Claude` in `--user-data-dir` while the main process names none. Its bundle has the warning "CLAUDE_USER_DATA_DIR is set and signed builds ignore it" [bundle]. Started with `--user-data-dir=<folder>` on its command line instead, the classic app from its `app-<version>` folder and the packaged app from its package folder alike, it reads that folder, and every one of its processes names it in `--user-data-dir`, the main one too. With a space in the folder, the switch is quoted, the whole argument on the main process and one child (`"--user-data-dir=C:\x y"`) and the value on the other children (`--user-data-dir="C:\x y"`); without one, some children quote the value and one does not. So the switch tells which folder a running Claude Desktop reads, and config-digger reads it from the processes to tell whether one reads an Agent home the user added.

### 1.1.1 The move to `Claude-Data\<family>`

Issue: [#305](https://github.com/samfaina/config-digger/issues/305). A packaged app can move its data folder out of its package's `LocalCache`, to `%LOCALAPPDATA%\Claude-Data\<family>`. Read in the bundle of 3.6556.2.0 (`claude.ai`'s download on 2026-10-29) and of 4.29.0.0 (the Store package on the reference machine), `.vite/build/index.pre.js`, which runs before the app picks `<userData>` [bundle]; seen on the reference machine's disk [disk]; and tested on 3.6556.2 in Windows Sandbox, `uat/agents/claude-desktop.mjs` [test].

Where each install keeps its data folder:

| Install | Data folder | Tag |
|---|---|---|
| Classic app | `%APPDATA%\Claude`. It never moves: the move is only for a packaged app | [test], [bundle] |
| A package, before it moves | `%APPDATA%\Claude` as it sees it: its `LocalCache\Roaming\Claude` laid over any classic folder (§1.1) | [test] |
| A package, once it moved | `%LOCALAPPDATA%\Claude-Data\<family>`, `<family>` being the package family in lower case: `claude_pzs8sxrjxfjjc` or `anthropicpbc.claude_fnn82j28hfe8t`. The manifest's `FileSystemWriteVirtualization` excludes `Claude-Data`, so the folder is at its real path | [bundle], [disk], [test] |
| `%APPDATA%` on a UNC share | `%LOCALAPPDATA%\Claude-Data` itself. A package never moves then | [bundle] |
| `--user-data-dir`, or a third-party deployment | the folder the switch names, or `%LOCALAPPDATA%\Claude-3p`. Neither moves | [bundle] |

When it moves. The decision is one function (`AQ` in 4.29, `CQ` in 3.6556) [bundle]. At a start, a packaged app in first-party mode, with no UNC `%APPDATA%` and no `--user-data-dir`, moves when:

- `Claude-Data\<family>` does not exist yet, nor anything else by that name;
- Windows is build 19041 or later;
- its data lives in its package alone: with data in a real `%APPDATA%\Claude`, beside the package's or not, it stays (`data_in_roaming`, `data_in_roaming_and_package`). So a package installed beside a classic app does not move while the classic folder holds data;
- its config's `preferences.userDataOutsidePackageAnswer` is `true`, or `preferences.userDataFolderAnswer` before Windows 11 (build 22000). Otherwise it stays, `stay_rollout_off`. The main bundle latches a rollout flag into that preference (flag `2177080227` on Windows 11, `2363431316` before), whenever a fresh payload of flags says on; it latches off only for a signed-in account. Before Windows 11 the move also waits on the age of the `Claude-Data` folder, which a helper app of the package creates (3.6556 names it `MakeDataFolder`);
- no other instance of the app is running.

It copies the data folder to `Claude-Data\~copying.<family>`, renames that to `Claude-Data\<family>`, writes `Claude-Data\README.txt`, which tells the user not to move or delete the folder, and empties the package's `LocalCache\Roaming\Claude` down to markers named `.user-data-<state>.<family>`, the states being `left`, `blocked`, `back`, `copied`, `uncopied`, `parent` and, in 4.29, `whole`. `.user-data-leaving.<family>` sits beside that folder while it leaves [bundle]. On the reference machine the `LocalCache` holds only `.user-data-left.claude_pzs8sxrjxfjjc`, empty, and `Claude-Data\claude_pzs8sxrjxfjjc` holds the whole data folder: `claude_desktop_config.json` with `preferences.userDataOutsidePackageAnswer: true`, `config.json`, `claude-code`, `claude-code-vm`, `vm_bundles`, `local-agent-mode-sessions` and the rest [disk].

Once `Claude-Data\<family>` is a folder, or a link to one, the app reads it, whether a classic `%APPDATA%\Claude` exists or not and whatever the marker says (`use_moved`). Only while the answer reads `false`, and the `LocalCache` is clear, does it move the folder back into the package (`move_back`); 4.29 also moves it back on Windows builds before 19044 [bundle]. Moved, every process names the moved folder in `--user-data-dir` [test].

The Sandbox run (3.6556.2, Windows 11 build 26300, nobody signed in) [test]:

- A fresh install wrote no `Claude-Data\<family>` over six launches: two beside a classic folder, one with `--user-data-dir`, and three with the classic folder gone, which kept its data in `LocalCache`.
- With `preferences.userDataOutsidePackageAnswer: true` written into its `LocalCache` config by the probe, the next start moved the data folder to `Claude-Data\claude_pzs8sxrjxfjjc`, wrote `README.txt` beside it and left `.user-data-left.claude_pzs8sxrjxfjjc` in the `LocalCache`. The app then read the moved config and named the moved folder in `--user-data-dir`, also with a classic `%APPDATA%\Claude` written beside it.
- Not tried, since writing the latched answer reached the move: updating from an older package, signing in, and using Cowork. Which of these turns the rollout flag on for a real account is not known. The flag needs a fresh payload of flags, which a signed-out app in the Sandbox never latched.

### 1.1.2 A package beside a classic folder, as config-digger reads it

Issue: [#306](https://github.com/samfaina/config-digger/issues/306), ADR 0010. A package that has not moved its data folder sees `%APPDATA%\Claude` with its `LocalCache\Roaming\Claude` laid over it, path by path (§1.1). config-digger reads it so: the package's home (`msix`, `msix-anthropicpbc`) is an Overlay home over the classic home, `over = "classic"` in `claude-desktop.toml`.

- **A file**, such as `claude_desktop_config.json` or `extensions-installations.json`: the package reads its `LocalCache` copy when it holds one, and else the classic file [test]. A classic file the package reads is one Entry in both homes, with an Item and a State in each.
- **A folder**, `Claude Extensions`, `Claude Extensions Settings` and the skills mirror `local-agent-mode-sessions\skills-plugin`: the package lists the entries of both folders by name, its `LocalCache`'s winning where both hold one. So it sees an extension held in the classic folder alone, one held in its `LocalCache` alone, and its own copy of one held in both, and an extension's settings file is read where the package finds it. This is inferred from Microsoft's docs: the definition marks it `unverified` until the `homes[msix]` check of `uat/agents/claude-desktop.mjs` passes. Run signed out on 3.6556.2, the check gave no verdict, since the app's logs named no extension; it runs with `--signed-in`. An extension's settings file found in neither folder is written in the classic folder while that folder exists, so config-digger never creates one in the `LocalCache`.
- **The lookup orders**: the classic app's holds the classic home alone; a package's holds its own home, which it picks while its `LocalCache` folder or the classic folder exists. A package's Installation reads its own home, not the classic one.
- **Writes**: Disable, Enable and Remove write an Entry where it lives, so a server the package reads from the classic file is parked from the classic file, and the preview lists the classic home among the homes whose State changes. config-digger never creates or copies a file in a package's `LocalCache`. A parked server's origin is the home whose folder held its file, the classic one here, so Enable puts it back in the classic file even once the package holds a config of its own; it is listed as Disabled in each home that reads that file.

### 1.2 MCP sources per Scope

| Scope (config-digger) | Source | Location | Tag |
|---|---|---|---|
| managed | Device policy | Registry `HKLM\SOFTWARE\Policies\Claude`, else `HKCU\SOFTWARE\Policies\Claude`. Value `managedMcpServers` (JSON). Gates: `isLocalDevMcpEnabled`, `isDesktopExtensionEnabled`, `isDesktopExtensionDirectoryEnabled`, `isDesktopExtensionSignatureRequired`, `allowedPluginMcpServers`, `deniedPluginMcpServers` | [docs] ([Enterprise configuration](https://support.claude.com/en/articles/12622667-enterprise-configuration-for-claude-desktop), [3P configuration](https://claude.com/docs/third-party/claude-desktop/configuration)). [bundle]: key path is `SOFTWARE\Policies\<app name>` with app name `Claude`, and if HKLM holds any value, HKCU is ignored. Neither key exists here [disk] |
| managed (cloud) | Org "bootstrap" server or the Anthropic-hosted control plane | network only. Some keys are accepted only from there, never from the registry | [bundle] |
| managed (cloud) | Org extension allowlist and blocklist | allowlist cached encrypted in `<userData>\config.json` as `dxt:allowlistCache:<org>` = `***`, blocklist in `<userData>\extensions-blocklist.json` | [disk]. Allowlist feature: [docs] ([allowlist article](https://support.claude.com/en/articles/12592343-enabling-and-using-the-desktop-extension-allowlist)) |
| user | User-added servers | `<userData>\claude_desktop_config.json` → `mcpServers` | [docs] (MCP quickstart). This machine's file has no `mcpServers` key [disk] |
| user (extension Provenance) | Desktop extensions | `<userData>\Claude Extensions\<id>\manifest.json` (see §5) | [bundle]. No extensions installed here, so the folder is absent [disk] |
| user (cloud Provenance) | claude.ai connectors | account, cloud. See §1.3 | [docs] |
| project / local | — | **Not applicable.** Desktop chat has no per-project MCP config. Code-tab sessions use Claude Code's project and local Scopes | [docs] ([Claude Code desktop](https://code.claude.com/docs/en/desktop)) |

### 1.3 claude.ai connectors: what is visible on disk

Connectors are added in the account (Customize > Connectors, or Organization settings > Connectors). "Claude connects to your remote MCP server from Anthropic's cloud infrastructure, rather than from your local device" ([custom connectors](https://support.claude.com/en/articles/11175166-get-started-with-custom-connectors-using-remote-mcp)). They are "available everywhere without extra setup" ([desktop vs web connectors](https://support.claude.com/en/articles/11725091-when-to-use-desktop-and-web-connectors)). [docs]

On disk there is **no authoritative list** of connectors. Two artefacts exist [disk]:

- Each Code-tab session file `<userData>\claude-code-sessions\<account>\<org>\local_<uuid>.json` carries `remoteMcpServersConfig`, an array of `{uuid, name, url, tools}`. It is a snapshot of the connectors offered to that session. Across 172 session files here it names `Linear` (97 times) and `visualize`. `enabledMcpTools` keys per-session tool toggles as `<connector uuid>:<tool>-<hash>`.
- `<userData>\mcp-user-tool-toggles.json` (`{"v":3,"owners":{"<account>":[…disabled tool keys…]}}`) records per-tool Disables for the account [disk], [bundle].

Treat connectors as cloud Provenance and read-only. A snapshot shows that a connector *was* offered, not that it is still configured.

## 2. File format, root key, per-server fields, secrets

`claude_desktop_config.json` is plain JSON. A leading BOM is stripped and comments are not allowed: the parser is `JSON.parse` [bundle]. The documented example is `{"mcpServers": {"<name>": {"command": …, "args": […]}}}`, with `env` for variables ([MCP quickstart](https://modelcontextprotocol.io/docs/develop/connect-local-servers)). [docs]

The schema in 2.16120 [bundle]:

- Top level: `mcpServers`, `globalShortcut`, `features` (`isDxtEnabled`, `isDxtDirectoryEnabled`, `isLocalDevMcpEnabled`, `isUvSystemPythonEnabled`, …), `isUsingBuiltInNodeForMcp`, `isDxtAutoUpdatesEnabled`, `dxtMaxTotalSizeMB`, `deploymentMode` (`"1p"`/`"3p"`), `managedConfig`, `coworkUserFilesPath`, `preferences`, and a few others.
- Per server: `{ command: string (required), args?: string[], env?: {string: string}, extensionId?: string }`.
- **There is no `url`, `type` or `transport`.** A remote server entry fails validation. The app skips it and shows "Some MCP servers couldn't be loaded … not valid MCP server configurations and were skipped". [bundle] [contradicts docs]: the docs never say this outright; they only route remote servers through Connectors ([remote servers](https://modelcontextprotocol.io/docs/develop/connect-remote-servers)).
- Unknown per-server keys such as `disabled` are not part of the schema. With zod's default behaviour they would be dropped silently (inference).
- If the whole file fails to parse, the app shows "Couldn't load app settings" and **refuses to write the file back**, so a bad hand edit is not overwritten [bundle].

On this machine the file is about 33 KB with top-level keys `coworkUserFilesPath` and `preferences`. Most of its size is `preferences.epitaxyPrefs`, which is UI state for the Code tab. [disk]

**Fields that may hold secrets:**

- `mcpServers.<name>.env.*`: plaintext API keys, for example the docs' own `"BRAVE_API_KEY"` example [docs].
- `args`: may contain tokens.
- Policy `managedMcpServers` "May embed bearer tokens" [bundle].
- `<userData>\config.json` holds `oauth:tokenCache` / `oauth:tokenCacheV2` (encrypted, `***`) [disk].
- Extension `user_config` values marked `sensitive` [docs] (see §5).

config-digger must mask all of these.

## 3. Where skills are read from

| Scope | Location | Tag |
|---|---|---|
| user (cloud) | Account skills, uploaded as ZIP via Customize > Skills, each toggled on or off ([Use skills in Claude](https://support.claude.com/en/articles/12512180-use-skills-in-claude)) | [docs] |
| user (cloud mirror on disk) | `<userData>\local-agent-mode-sessions\skills-plugin\<org>\<account>\` holds `.claude-plugin\plugin.json` (`name: "anthropic-skills"`, "Anthropic-managed skills for Claude Desktop"), `manifest.json` and `skills\<name>\SKILL.md` | [disk]: 15 skill folders here. [contradicts docs]: no doc mentions a local copy |
| managed | No device-policy skill list for Desktop. The `skillBundles` key says "Claude Desktop ignores this list; it gets skills from plugin marketplaces". Related keys: `disableBundledSkills`, `skillCreationEnabled` | [bundle] |
| project / local | None for Desktop chat or Cowork. "Cowork sessions … don't read `~/.claude/skills/`" ([Claude Code skills](https://code.claude.com/docs/en/skills)). The Code tab's "Local sessions load your personal skills from `~/.claude/skills/`" ([Claude Code desktop](https://code.claude.com/docs/en/desktop)). That is Claude Code's configuration | [docs] |

The `manifest.json` structure [disk], [bundle]: `{lastUpdated, skills: [{skillId, name, description, creatorType: "user"|"anthropic", updatedAt, enabled, backingPluginId?, source?}]}`. The fields come straight from the account API response (`id`, `creator_type`, `enabled`, `backing_plugin_id`). On this machine 1 skill is `creatorType: "user"` and 14 are `"anthropic"`, all `enabled: true`. The manifest was rewritten at app start, at the same time as `config.json`.

For config-digger this mirror is a good way to *list* Desktop skills offline with an Effective flag (`enabled`). Its Provenance is cloud: editing it would be overwritten on the next sync.

## 4. Precedence and merge rules

From the code [bundle]:

1. Extension servers are collected first, keyed by `manifest.display_name || manifest.name`. If two extensions share a display name, the second is keyed by its extension id. Only extensions with `isEnabled: true` and no `orgBlockedReason` are included.
2. User servers from `claude_desktop_config.json` are collected only when policy `isLocalDevMcpEnabled` is not false **and** `features.isLocalDevMcpEnabled` in the file is not `false`.
3. The result is `Object.assign({}, extensionServers, userServers)`. It is **per name and whole-Entry**: a user-file server with the same name Shadows the extension's server completely, with no field merge.
4. Managed `managedMcpServers` and org-plugin servers are handled in a separate pool. In that pool, a plugin server whose name collides with a managed server is dropped ("name collides with existing server"). How managed servers rank against user-file servers in the Chat surface could not be fully traced; see Open questions.
5. Policy tiers: if `HKLM\SOFTWARE\Policies\Claude` has any value, HKCU is ignored entirely. This is per tier, not per key. Docs: "when machine policy is present under `HKLM\SOFTWARE\Policies\Claude`, the app ignores `HKCU\SOFTWARE\Policies\Claude` entirely" ([3P configuration](https://claude.com/docs/third-party/claude-desktop/configuration)). [docs] and [bundle] agree.

The Code tab loads `claude_desktop_config.json` servers "alongside servers from `~/.claude.json` and `.mcp.json`" ([Claude Code desktop](https://code.claude.com/docs/en/desktop)) [docs]. How names clash across those sources there is Claude Code's rule set (see `research/claude-code`).

## 5. How extensions are recorded on disk

The format is documented by the MCPB repo. `.mcpb` (formerly `.dxt`) is a zip with `manifest.json`. Required fields: `manifest_version`, `name`, `version`, `description`, `author` and `server`. `server.type` is `node`, `python`, `binary` or `uv`, and `server.mcp_config` holds `command`, `args` and `env`. `${__dirname}` expands to the extension folder, and `user_config` fields can be `sensitive` ([MANIFEST.md](https://github.com/modelcontextprotocol/mcpb/blob/main/MANIFEST.md), [mcpb README](https://github.com/modelcontextprotocol/mcpb)). [docs] The repo moved from `anthropics/dxt` / `anthropics/mcpb` to `modelcontextprotocol/mcpb` [docs].

On disk [bundle]. No extension is installed on this machine, so none of these files exist yet [disk].

| Path | Content |
|---|---|
| `<userData>\Claude Extensions\<id>\` | Unpacked bundle. `manifest.json` at the root. Any directory with a readable `manifest.json` counts as installed |
| `<userData>\Claude Extensions Settings\<id>.json` | `{isEnabled: bool, userConfig?: {…}, orgBlockedReason?, enabledBeforeOrgBlock?, orgBlockKind?, configDegraded?}`. **A missing file means `isEnabled: false`** |
| `<userData>\extensions-installations.json` | `{"extensions": {"<id>": {id, version, hash, installedAt, certificateFingerprint, manifest, signatureInfo, source}}}`. This is the install record; it is what attributes an Entry to an extension |
| `<userData>\Extensions Update Cache\`, `dxt-staging\`, `dxt-install-*`, `dxt-unpacked-*` | Transient update and install staging |
| `<userData>\extensions-blocklist.json` | Org blocklist cache. Here: 2 entries with ids `ant.dir.gh.tomtom.tomtom-mcp*`, from `https://claude.ai/api/organizations/<org>/dxt/blocklist` [disk] |

**Extension ids** [bundle] are `<prefix>.<author>.<name>`, sanitised:

- `local.mcpb.…` for a `.mcpb` file;
- `local.dxt.…` for a `.dxt` file;
- `local.unpacked.…` for a developer folder install;
- `ant.dir.<…>` for directory installs (`source: "registry"`), as in the blocklist above.

**Attribution rule for config-digger.** An MCP server Entry produced by an extension is not written to `claude_desktop_config.json`. It is synthesised at runtime from the manifest's `server.mcp_config`, keyed by display name, and tagged internally with `extensionId` (which the app strips from user-file servers). So:

- extension Entries are enumerated by walking `Claude Extensions\*\manifest.json`;
- `extensions-installations.json` gives Provenance (source, version, signature);
- `Claude Extensions Settings\<id>.json` gives the enabled state.

**Secrets in extension settings.** Docs: sensitive values are encrypted "using the operating system's secure storage (… Credential Manager on Windows …)" ([local MCP servers on Claude Desktop](https://support.claude.com/en/articles/10949351-getting-started-with-local-mcp-servers-on-claude-desktop)). The code stores them **inline** in `userConfig` as strings prefixed `__encrypted__:`. If they can't be decrypted, the settings are marked `configDegraded` [bundle]. [contradicts docs] in wording: the ciphertext sits in the JSON file, not in Credential Manager. The encryption key source (DPAPI/safeStorage or Credential Manager) was not traced. config-digger must treat any `__encrypted__:` value as `***`.

**Installed Claude Code binaries.** `<userData>\claude-code\<version>\claude.exe` (2.1.284 here) and `<userData>\claude-code-vm\<version>\claude` are the Claude Code runtimes Desktop downloads for the Code tab and Cowork [disk]. They are not Entries.

## 6. Native Disable without Remove

| Target | Mechanism | Tag |
|---|---|---|
| One extension | Settings > Extensions toggle → `Claude Extensions Settings\<id>.json` `isEnabled: false`. Changing it triggers a reconnect of MCP servers, with no restart needed | [bundle]. Docs describe the Settings > Extensions UI ([support 10949351](https://support.claude.com/en/articles/10949351-getting-started-with-local-mcp-servers-on-claude-desktop)) but not the file |
| All extensions | Policy `isDesktopExtensionEnabled = false` (legacy name `isDxtEnabled`, accepted until 2026-10-07 12:00 PT, then "unreadable: extensions are disabled"), or file `features.isDxtEnabled: false` | [docs] ([configuration changelog](https://claude.com/docs/third-party/claude-desktop/configuration-changelog)). [bundle] |
| Extension directory | Policy `isDesktopExtensionDirectoryEnabled`, or file `features.isDxtDirectoryEnabled: false` | [docs], [bundle] |
| One user-file server | **No native flag.** The schema has no `disabled` field. Disable means moving the Entry out of `mcpServers` and keeping it elsewhere so it can come back, or removing it | [bundle] |
| All user-file servers | Policy `isLocalDevMcpEnabled = false` ("Allow user-added MCP servers"), or file `features.isLocalDevMcpEnabled: false` | [docs] (Enterprise configuration), [bundle] |
| One tool of a server or connector | UI tool toggle → `<userData>\mcp-user-tool-toggles.json` (per account). Managed `toolPolicy` (`"blocked"`) per managed server | [disk], [bundle] |
| Connector | Per-conversation toggle in the UI; account-level management in the cloud. Nothing on disk to edit | [docs] ([use connectors](https://support.claude.com/en/articles/11176164-use-connectors-to-extend-claude-s-capabilities)) |
| Skill | Customize > Skills toggle (cloud). Mirrored as `enabled` in `skills-plugin\…\manifest.json`. Do not edit the mirror | [docs], [disk] |
| Org-blocked extension | Blocklist sets `orgBlockedReason` and remembers `enabledBeforeOrgBlock` | [bundle] |

A known docs conflict: the 3P configuration changelog says that in v1.19367.0 the default of `isDesktopExtensionEnabled` changed from `true` to `false`. The support article still shows `true`. The code says "1P builds default to enabled at runtime unless this is e[xplicitly set]…" [bundle], so the `false` default probably applies to 3P deployments only.

## 7. Does the Agent rewrite these files while running? Which processes show it is running?

**Rewrites:**

- `claude_desktop_config.json` holds `preferences` (UI state), so the app writes it during normal use. The file here was last written 2026-09-28. The app keeps the parsed file in memory and writes the whole object back. It does reload on some paths, but an external edit made while it runs can be overwritten by the next preferences write. It refuses to write if the load was degraded. [bundle], [disk]
- The docs require a restart: "completely quit Claude Desktop and restart it. The application needs to restart to load the new configuration" ([MCP quickstart](https://modelcontextprotocol.io/docs/develop/connect-local-servers)) [docs].
- `config.json`, `extensions-blocklist.json`, `bridge-state.json` and the skills `manifest.json` were all rewritten at the last app start (2026-10-01 14:03:21). `git-worktrees.json` was rewritten a minute later. [disk]

config-digger should warn before writing while Desktop runs, or require Desktop to be closed.

**Processes** [disk]:

- `claude.exe` from `C:\Program Files\WindowsApps\Claude_<version>_x64__pzs8sxrjxfjjc\app\` (14 processes here: Electron main, renderers, helpers). The manifest names it `app\Claude.exe`, and the App Execution Alias is `claude-desktop.exe`.
- The **Claude Code CLI is also `claude.exe`** (`%USERPROFILE%\.local\bin\claude.exe`), and so is the Desktop-managed copy under `<userData>\claude-code\<ver>\`. Tell them apart by executable path.
- Service `CoworkVMService` (display name "Claude", `app\resources\cowork-svc.exe`), plus `chrome-native-host.exe` and the `claude-ssh-*.exe` helpers.
- `<userData>\lockfile` is the Electron single-instance lock.

## 8. Where the Agent records projects it has opened

Desktop chat has no projects on disk; claude.ai Projects live in the cloud. Folder-related records [disk]:

- `<userData>\claude-code-sessions\<account>\<org>\local_<uuid>.json`: one file per Code-tab session, with `cwd` and `originCwd`. Here: 172 sessions over 4 distinct folders.
- `<userData>\git-worktrees.json`: `untrackedDirGc.cwds` maps folder paths to last-seen timestamps (4 project folders here) and lists `worktrees`.
- `claude_desktop_config.json` → `preferences.localAgentModeTrustedFolders` (1 entry) and `preferences.remoteSessionFolderGrants` (per-session folder grants for Cowork).
- `<userData>\local-agent-mode-sessions\<account>\<org>\` holds Cowork `spaces.json`, `remote-session-spaces.json` and `scheduled-tasks.json`.

For project discovery, the `cwd` values of the session files are the most complete list.

## 9. macOS and Linux differences (from sources, not verified)

- macOS config: `~/Library/Application Support/Claude/claude_desktop_config.json`. Logs: `~/Library/Logs/Claude` ([MCP quickstart](https://modelcontextprotocol.io/docs/develop/connect-local-servers)) [docs]. Managed policy: the `com.anthropic.claudefordesktop` preference domain via MDM ([Enterprise configuration](https://support.claude.com/en/articles/12622667-enterprise-configuration-for-claude-desktop)) [docs], [bundle].
- Linux: Desktop is in beta ([desktop on Linux](https://code.claude.com/docs/en/desktop-linux)). No primary source gives its config path. The code falls back to Electron's `appData/Claude`, normally `~/.config/Claude` (inference from Electron defaults, not verified).
- Windows logs: docs say `%APPDATA%\Claude\logs` with `mcp.log` and `mcp-server-<name>.log` [docs]. On this MSIX install, current logs are in the **real** `%LOCALAPPDATA%\Claude\logs` (excluded from virtualisation; `main.log`, `mcp.log`, …). Older logs up to 2026-08-22 are in `<userData>\logs`. [disk] [contradicts docs]

## Sources

- MCP, Connect to local MCP servers: https://modelcontextprotocol.io/docs/develop/connect-local-servers
- MCP, Connect to remote MCP servers: https://modelcontextprotocol.io/docs/develop/connect-remote-servers
- MCPB repo and manifest spec: https://github.com/modelcontextprotocol/mcpb , https://github.com/modelcontextprotocol/mcpb/blob/main/MANIFEST.md
- Anthropic Engineering, Desktop Extensions: https://www.anthropic.com/engineering/desktop-extensions
- Anthropic support, Getting started with local MCP servers on Claude Desktop: https://support.claude.com/en/articles/10949351-getting-started-with-local-mcp-servers-on-claude-desktop
- Anthropic support, Enterprise configuration for Claude Desktop: https://support.claude.com/en/articles/12622667-enterprise-configuration-for-claude-desktop
- Anthropic support, Desktop extension allowlist: https://support.claude.com/en/articles/12592343-enabling-and-using-the-desktop-extension-allowlist
- Anthropic support, Deploy Claude Desktop for Windows: https://support.claude.com/en/articles/12622703-deploy-claude-desktop-for-windows
- Anthropic support, Custom connectors using remote MCP: https://support.claude.com/en/articles/11175166-get-started-with-custom-connectors-using-remote-mcp
- Anthropic support, When to use desktop and web connectors: https://support.claude.com/en/articles/11725091-when-to-use-desktop-and-web-connectors
- Anthropic support, Use connectors: https://support.claude.com/en/articles/11176164-use-connectors-to-extend-claude-s-capabilities
- Anthropic support, Use skills in Claude: https://support.claude.com/en/articles/12512180-use-skills-in-claude
- Claude Code docs, Desktop: https://code.claude.com/docs/en/desktop ; Skills: https://code.claude.com/docs/en/skills ; Desktop on Linux: https://code.claude.com/docs/en/desktop-linux
- Claude Desktop 3P docs: installation https://claude.com/docs/third-party/claude-desktop/installation ; configuration https://claude.com/docs/third-party/claude-desktop/configuration ; changelog https://claude.com/docs/third-party/claude-desktop/configuration-changelog ; data storage https://claude.com/docs/third-party/claude-desktop/data-storage
- Microsoft, MSIX behind the scenes: https://learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-behind-the-scenes
- Microsoft, Flexible virtualization: https://learn.microsoft.com/en-us/windows/msix/desktop/flexible-virtualization
- Microsoft, FileSystemWriteVirtualization element: https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-virtualization-filesystemwritevirtualization
- Microsoft, MSIX troubleshooting guide: https://learn.microsoft.com/en-us/windows/msix/msix-troubleshooting-guide
- On-disk evidence: `C:\Program Files\WindowsApps\Claude_2.16120.0.0_x64__pzs8sxrjxfjjc\AppxManifest.xml` and `app\resources\app.asar` (Claude Desktop 2.16120.0.0); `%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Claude\`

## Open questions

1. **Managed vs user-file precedence in Chat.** The code shows `managedMcpServers` in a separate pool. It also shows that org-plugin servers colliding with managed names are dropped. Whether a managed server Shadows a same-named `claude_desktop_config.json` server in Chat was not traced. This needs a test with a policy set.
2. **Extension secrets.** What encrypts `__encrypted__:` values: Electron safeStorage (DPAPI) or Windows Credential Manager, as the docs say? Can config-digger detect "configured" without decrypting? It probably can, by presence of the key.
3. **Live extension install.** No extension is installed on this machine, so the §5 layout comes from code only. Install one test `.mcpb` (in a throwaway profile via `--user-data-dir`, since signed builds ignore `CLAUDE_USER_DATA_DIR`) to confirm file names and the `source` values in `extensions-installations.json`.
4. **Classic and MSIX side by side.** Which folder does each build use when both `%APPDATA%\Claude` and `LocalCache\Roaming\Claude` exist? Should config-digger report both or pick one?
5. **Unknown per-server keys.** Is a `disabled: true` key in a user-file server silently stripped (zod default), or does it reject the Entry? This decides whether config-digger may use an in-file marker for Disable or must move the Entry out.
6. **Connector visibility.** Is there any non-session local cache of the account's connector list, perhaps in IndexedDB or Local Storage (not inspected)? Session snapshots are not authoritative.
7. **Linux config path.** No primary source states it.
8. **`isDesktopExtensionEnabled` default.** Is the `false` default from changelog v1.19367.0 3P-only? The support article still says `true`.
