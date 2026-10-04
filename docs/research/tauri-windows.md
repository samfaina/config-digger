# Research: Tauri 2 and Windows constraints

Issue: #16. Researched 2026-10-01 against Tauri 2.12 (`tauri` crate 2.12.1, `tauri-plugin-fs` 2.6.0, `tauri-plugin-updater` 2.13.1) and `sysinfo` 0.39.6. Terms in **bold** come from `CONTEXT.md`.

## Summary and recommendations

1. **Do file I/O in custom Rust commands, not the fs plugin.** Put the reading, parsing, backup and writing of **Entries** in the core library and expose a handful of task-level commands (`scan`, `disable_entry`, `remove_entry`, ...). Do not install `tauri-plugin-fs`. To give the webview access to the whole home directory, the fs plugin would need a scope such as `$HOME/**/*`. That scope would hand a compromised frontend arbitrary read/write over the user's profile. Custom commands only expose the operations config-digger actually performs.
2. **Lock the commands down with `AppManifest::commands`.** By default every registered command is callable from every window. List the commands in `build.rs`; Tauri then generates `allow-<command>`/`deny-<command>` permissions, and one capability file grants them to the `main` window. Don't grant the webview any `fs:`, `shell:` or `updater:` permissions.
3. **MSIX-virtualised AppData is readable and writable.** Files that a Store app such as Claude Desktop creates under AppData physically live at `%LOCALAPPDATA%\Packages\<PackageFamilyName>\LocalCache\{Roaming,Local}\...`. That is an ordinary folder owned by the user. An unpackaged process (config-digger's Rust core) can open it by its real path; I checked this on this machine. What an unpackaged process cannot do is see those files at the *virtual* path (`%APPDATA%\Claude\...`), so **Agent definitions** must list the `LocalCache` path explicitly. If a file exists in both places, the packaged app reads the `LocalCache` copy first, so that copy is the **Effective** one.
4. **Ship an NSIS installer in `currentUser` mode.** It needs no admin rights, installs to `%LOCALAPPDATA%`, works with winget and with the updater, and matches a tool that only touches the current user's files. Tauri's MSI is per-machine only. Keep it as an optional extra if enterprise users ask for it. For WebView2, keep the default `downloadBootstrapper` or use `embedBootstrapper` (+~1.8 MB). Windows 11 ships the Evergreen runtime, and most Windows 10 machines already have it.
5. **Code signing: release v0.1 unsigned, then apply to SignPath Foundation.** SignPath Foundation is free for OSI-licensed projects, but the project must already be released. Signing then runs from GitHub Actions on GitHub-hosted runners, and the publisher shown is "SignPath Foundation". Azure Artifact Signing costs US$9.99/month, but individuals must be in the US or Canada; outside those two countries you need a registered organisation in an eligible country. Until the project is signed, publish SHA-256 checksums and GitHub build-provenance attestations, and document the SmartScreen "More info → Run anyway" step. Unsigned builds are **blocked outright** on PCs where Smart App Control is enforcing.
6. **Make the update check opt-in.** The updater never checks for updates on its own. The app must call `check()`, so a settings toggle that defaults to off is enough. Call it from Rust, not the webview. The updater needs a minisign keypair: the public key goes in `tauri.conf.json` and the private key is a CI secret. It also needs an HTTPS endpoint; a GitHub Releases `latest.json` produced by `tauri-action` is enough. This signature is separate from Authenticode and is required even for unsigned builds. Opt-in also matches SignPath's privacy rule: no network transfer unless the user asks for it.
7. **Detect running Agents with `sysinfo` and refresh only processes.** Process names come from a Toolhelp snapshot and need no special rights. Executable paths work for the user's own processes, including elevated ones on Windows 10+. Command lines (needed to tell `node.exe` running Claude Code apart from other Node processes) need `PROCESS_VM_READ`, which fails for elevated or protected processes. Treat "running" as a best-effort warning, never as a lock.

## 1. Tauri 2 security model: capabilities, permissions, scopes

### How it works

- Capabilities are JSON/TOML files in `src-tauri/capabilities/`. Each one grants a list of permissions to specific windows/webviews (by label), optionally per platform, and optionally to remote URLs ([capabilities](https://v2.tauri.app/security/capabilities/)).
- Permissions are named privileges for commands: `<plugin>:<permission>` for plugins, no prefix for app-defined ones. They can carry allow/deny scopes ([permissions](https://v2.tauri.app/security/permissions/)).
- Scopes are data passed to the command. The command implementation has to enforce them: "The scope is passed to the command and handling or properly enforcing is implemented by the command itself", and the same applies to app commands: "the scope type needs to be defined in the application and then enforced in the command implementation" ([command scopes](https://v2.tauri.app/security/scope/)).

### App commands are open by default

The capabilities guide says: "By default, all commands that you registered in your app (using the `tauri::Builder::invoke_handler` function) are allowed to be used by all the windows and webviews of the app. To change that, consider using `AppManifest::commands`" ([capabilities source](https://github.com/tauri-apps/tauri-docs/blob/v2/src/content/docs/security/capabilities.mdx)). `AppManifest::commands` defines "a list of commands that gets permissions autogenerated in the format of `allow-$command` and `deny-$command`" ([docs.rs](https://docs.rs/tauri-build/latest/tauri_build/struct.AppManifest.html)). Tauri's own API example lists app commands in `build.rs` and then grants them in a capability as `"allow-perform-request"`, `"allow-echo"` and so on ([build.rs](https://github.com/tauri-apps/tauri/blob/dev/examples/api/src-tauri/build.rs), [run-app.json](https://github.com/tauri-apps/tauri/blob/dev/examples/api/src-tauri/capabilities/run-app.json)).

Proposed config (names illustrative):

```rust
// src-tauri/build.rs
fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&[
                "scan", "get_entry", "disable_entry", "enable_entry",
                "remove_entry", "list_running_agents", "check_for_update",
            ]),
        ),
    )
    .expect("failed to run tauri-build");
}
```

```json
// src-tauri/capabilities/main.json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "main",
  "windows": ["main"],
  "platforms": ["windows", "macOS", "linux"],
  "permissions": [
    "core:default",
    "allow-scan", "allow-get-entry", "allow-disable-entry", "allow-enable-entry",
    "allow-remove-entry", "allow-list-running-agents", "allow-check-for-update"
  ]
}
```

### fs plugin compared with custom commands

The fs plugin can reach the home directory, but only with a broad scope. "Permissions alone do not grant a scope": `fs:allow-exists` with no `allow` scope fails with `forbidden path`. The documented way to open up home is a scope like `{ "path": "$HOME/**/*" }` ([fs plugin](https://v2.tauri.app/plugin/file-system/), [source](https://github.com/tauri-apps/tauri-docs/blob/v2/src/content/docs/plugin/file-system.mdx)). Other relevant details:

- It rejects `..` path components and wants paths relative to a base directory such as `$HOME`, `$APPDATA` or `$LOCALDATA` ([fs plugin](https://v2.tauri.app/plugin/file-system/)).
- `stat` follows symlinks and fails if the target is out of scope ([source](https://github.com/tauri-apps/tauri-docs/blob/v2/src/content/docs/plugin/file-system.mdx)).
- On Windows, globs match dot-folders like `.claude` by default: `require_literal_leading_dot` "Defaults to `true` on Unix systems and `false` on Windows" ([config.rs](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/fs/src/config.rs)).

Why custom commands are the better fit for config-digger:

| Concern | fs plugin from the webview | Custom Rust commands |
|---|---|---|
| Blast radius if the webview is compromised (XSS through a rendered config value, a malicious skill README, ...) | Arbitrary read/write inside the scope, which would have to be `$HOME/**` plus `$LOCALDATA/Packages/**` | Only the operations config-digger defines, e.g. "disable Entry X", always with backup |
| CLI reuse | Logic would live in TypeScript; the Rust CLI would need a copy | One Rust core library shared by CLI and GUI |
| Backups, atomic writes, format-preserving edits (JSONC/TOML) | Hand-rolled in JS | Done in the core library once |
| Running-Agent check before writing | Needs a separate round trip | Enforced inside the write command |
| Path handling | Must express everything as base-dir-relative paths; no `..` | Plain `std::path`, resolved by **Agent definitions** |

Custom commands have their own rules. Use owned argument types (`String`, not `&str`) in async commands, and return a `serde::Serialize` error type, which the guide suggests building with `thiserror` ([calling Rust](https://v2.tauri.app/develop/calling-rust/)). Never accept a raw path from the frontend and write to it. Accept an Entry identifier from a previous `scan` and resolve the path in Rust. This replaces the scope check the fs plugin would otherwise do.

## 2. Reading another app's MSIX-virtualised AppData

### What Windows does

- On Windows 10 1903 and later, for packaged desktop apps that are virtualised: "All newly created files and folders in the user's `AppData` folder ... are written to a private per-user, per-app location; but merged at runtime to appear in the real `AppData` location." Changes to files that already exist go to the real files. On open, "the OS will open the file from the per-user, per-package location first. If that location doesn't exist, then the OS will attempt to open the file from the real `AppData` location" ([behind the scenes](https://learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-behind-the-scenes)).
- "Files in the virtualized location are visible only to the app" ([flexible virtualization](https://learn.microsoft.com/en-us/windows/msix/desktop/flexible-virtualization)). This refers to the merged view at the original path. Another process looking at `%APPDATA%\Claude` won't see them.
- The physical location can be inspected from outside: "Inspect the VFS container to see redirected files: `%LocalAppData%\Packages\<PackageFamilyName>\LocalCache\`" ([MSIX troubleshooting](https://learn.microsoft.com/en-us/windows/msix/msix-troubleshooting-guide)).
- Apps can opt out with the `unvirtualizedResources` restricted capability. Windows 11 also allows per-folder `ExcludedDirectories` ([flexible virtualization](https://learn.microsoft.com/en-us/windows/msix/desktop/flexible-virtualization)). Whether a given Store app opts out is up to its publisher, so Agent definitions should probe both locations.

### Check on this machine (2026-10-01, Windows 11 26200)

- `%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Claude\claude_desktop_config.json` exists, and `%APPDATA%\Claude\claude_desktop_config.json` does **not**. So for the Store build of Claude Desktop the `LocalCache` copy is the only one.
- An unpackaged PowerShell process opened that file for reading without problems. The folder's ACL gives the interactive user `FullControl`, inherited, alongside the package SID, SYSTEM and Administrators.

### Implications

- A Tauri app installed with NSIS or MSI is not packaged, so it can read and write `LocalCache\...` paths directly with `std::fs`. Nothing in Tauri gets in the way, because the Rust side has no sandbox. If we ever *did* use the fs plugin, the path would need a `$LOCALDATA/Packages/**` scope.
- Agent definitions should search `%LOCALAPPDATA%\Packages\<PackageName>_*\LocalCache\Roaming\<dir>\...`. The suffix after `_` is the publisher hash, so glob on it rather than hard-coding it. If both copies exist, the packaged app reads the `LocalCache` copy, so mark the real-AppData copy as **Shadowed** for that Agent.
- **Don't ship config-digger itself as MSIX** without opting out of virtualisation. A virtualised config-digger that *creates* a file under AppData (a new config, a backup, a disabled-Entries store) would have that file redirected into *config-digger's own* package folder, where Agents can't see it. This is one more reason the Store's MSIX route is a poor fit (see §4).

## 3. Detecting running Agents on Windows

- `sysinfo` 0.39.6 is current ([docs.rs](https://docs.rs/sysinfo/latest/sysinfo/)). Use `default-features = false, features = ["system"]` and refresh only what you need: `System::new()` plus `refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet))`. `ProcessRefreshKind::nothing()` turns off every expensive field ([ProcessRefreshKind](https://docs.rs/sysinfo/latest/sysinfo/struct.ProcessRefreshKind.html)). The second argument is `remove_dead_processes` ([system.rs](https://github.com/GuillaumeGomez/sysinfo/blob/main/src/common/system.rs)). This API has changed between 0.x minor versions, so pin the version.
- On Windows, sysinfo enumerates processes with `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)`, so names and PIDs need no process handle. To get more detail it opens each process with `PROCESS_QUERY_INFORMATION | PROCESS_VM_READ` and falls back to `PROCESS_QUERY_LIMITED_INFORMATION` ([system.rs](https://github.com/GuillaumeGomez/sysinfo/blob/main/src/windows/system.rs), [process.rs](https://github.com/GuillaumeGomez/sysinfo/blob/main/src/windows/process.rs)).
- The executable path comes from `GetModuleFileNameExW`. On Windows 10 and later, with a NULL module, that "requires only `PROCESS_QUERY_LIMITED_INFORMATION`" ([GetModuleFileNameExW](https://learn.microsoft.com/en-us/windows/win32/api/psapi/nf-psapi-getmodulefilenameexw)). Protected processes allow only that limited right ([process access rights](https://learn.microsoft.com/en-us/windows/win32/procthread/process-security-and-access-rights)). Exe paths should therefore be available for the user's Agents, elevated or not.
- Command lines are read from the target's memory (`PROCESS_VM_READ`). sysinfo's docs warn that on Windows "you might need to use `administrator` privileges" for `cmd()` ([Process](https://docs.rs/sysinfo/latest/sysinfo/struct.Process.html)). Expect `cmd()` to be empty for elevated processes when config-digger runs unelevated (inferred from the access-rights table above).
- What this means for config-digger:
  - Match on exe path or name first: `Code.exe`, `Cursor.exe`, `claude.exe`, and Store apps under `C:\Program Files\WindowsApps\<pkg>\`.
  - Use the command line only to tell apart Node-hosted Agents (`node.exe ...\@anthropic-ai\claude-code\...`, `codex`).
  - Report "possibly running" when the information is incomplete.
  - Re-check right before each write. Never treat the check as a lock.
- Alternative if `sysinfo`'s extra machinery is unwanted: call `CreateToolhelp32Snapshot` + `QueryFullProcessImageNameW` directly through the `windows` crate. Both need only `PROCESS_QUERY_LIMITED_INFORMATION` ([process access rights](https://learn.microsoft.com/en-us/windows/win32/procthread/process-security-and-access-rights)). sysinfo stays the simpler cross-platform choice.

## 4. Installers: MSI vs NSIS, per-user vs per-machine, WebView2

### MSI vs NSIS ([Windows installer](https://v2.tauri.app/distribute/windows-installer/))

| | NSIS (`-setup.exe`) | MSI (WiX v3) |
|---|---|---|
| Build host | Windows; cross-compiling from Linux/macOS is possible but "not tested as much" | Windows only |
| Install scope | `bundle.windows.nsis.installMode`: `currentUser` (default, no admin, installs to `%LOCALAPPDATA%`), `perMachine` (admin), `both` (user chooses; needs admin) | Per-machine only |
| Customisation | Hooks; several languages in one binary | WiX fragments/templates; one MSI per language |
| Updater | Supported | Supported |

The fs plugin docs add that "Apps installed via MSI or NSIS in `perMachine` and `both` mode require admin permissions for write access in `$RESOURCES` folder" ([fs plugin](https://v2.tauri.app/plugin/file-system/)). That doesn't affect us, because we never write to our own resources.

**Recommendation: NSIS, `currentUser`.** config-digger only edits the current user's files, so elevating at install time buys nothing. The updater's `quiet` mode needs "admin or user-wide install" ([updater](https://v2.tauri.app/plugin/updater/)), and per-user satisfies that. winget expects installers to work "for both administrators and non-administrators" and to support non-interactive mode ([winget submission](https://learn.microsoft.com/en-us/windows/package-manager/package/repository)). NSIS `/S` covers silent installs ([Microsoft Store guide](https://v2.tauri.app/distribute/microsoft-store/)). Build the MSI only if an enterprise/GPO user asks.

### WebView2

- Tauri's options (`bundle.windows.webviewInstallMode.type`, [Windows installer](https://v2.tauri.app/distribute/windows-installer/)):
  - `downloadBootstrapper`: default, 0 MB, needs internet only if WebView2 is missing.
  - `embedBootstrapper`: about 1.8 MB.
  - `offlineInstaller`: about 127 MB.
  - `fixedRuntime`: about 180 MB.
  - `skip`: not recommended.
- Microsoft: "The Evergreen WebView2 Runtime will be included as part of the Windows 11 operating system", and "The vast majority of Windows 10 devices have the WebView2 Runtime installed already". When the bootstrapper runs unelevated, as a `currentUser` NSIS install does, it installs WebView2 per-user ([WebView2 distribution](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution)).
- **Recommendation:** keep `downloadBootstrapper` (the installer stays small and the download almost never happens), or `embedBootstrapper` if the goal is "the installer itself makes no network calls". Only a Microsoft Store listing needs `offlineInstaller` ([Microsoft Store guide](https://v2.tauri.app/distribute/microsoft-store/)).

### Package managers

- **winget:**
  - Submit a manifest PR to `microsoft/winget-pkgs`.
  - Automated validation includes antivirus/Defender scans, URL reputation, silent install and clean uninstall checks.
  - The installer URL must come "directly from the publisher's website"; GitHub Releases is the usual source.
  - Signing isn't listed as a requirement, but AV/PUA false positives block a submission ([winget submission](https://learn.microsoft.com/en-us/windows/package-manager/package/repository)).
- **Scoop:** the main bucket accepts only "non-GUI" tools from GitHub projects with at least "500 stars and 150 forks" ([Scoop criteria](https://github.com/ScoopInstaller/Scoop/wiki/Criteria-for-including-apps-in-the-main-bucket)). The GUI app would go to the Extras bucket or a self-hosted bucket. The CLI could go to main once it is popular enough.
- **Microsoft Store:**
  - Developer accounts are now free ([Partner Center](https://learn.microsoft.com/en-us/windows/apps/publish/partner-center/open-a-developer-account)).
  - Tauri produces only EXE/MSI ([Microsoft Store guide](https://v2.tauri.app/distribute/microsoft-store/)).
  - MSI/EXE Store submissions must be "digitally signed with a code signing certificate that chains up to a certificate issued by a Certificate Authority (CA) that is part of the Microsoft Trusted Root Program" ([MSI/EXE requirements](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msi/app-package-requirements)), so the Store doesn't solve signing.
  - The MSIX route ("Free Microsoft code signing") would require hand-packaging and would bring the AppData virtualisation problem from §2.

## 5. Code signing and SmartScreen

### What happens unsigned

- SmartScreen shows "Windows protected your PC"; the user must click "Run anyway". "Enterprise policy can prevent continuation entirely." Self-signed certificates behave the same as unsigned ([SmartScreen reputation](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation)).
- Unsigned files "must build reputation anew with every update", so every release starts at zero ([SmartScreen reputation](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation)).
- **Smart App Control** (Windows 11 clean installs, in enforcement mode): "Apps cannot be run unless they are recognized by Microsoft's app intelligence services, or they are signed with a trusted certificate". Unsigned unknown code "is blocked by default" ([Smart App Control](https://learn.microsoft.com/en-us/windows/apps/develop/smart-app-control/overview)). This is a hard block, not a warning, so some users cannot run an unsigned build at all.
- Signing doesn't remove the first warnings either. EV and OV certificates build reputation the same way, "EV certificates no longer bypass SmartScreen", and reputation "can take several weeks and hundreds of clean installs" ([SmartScreen reputation](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation), [Tauri Windows signing](https://v2.tauri.app/distribute/sign/windows/)). What signing does: it shows a verified publisher, carries reputation across releases, and satisfies Smart App Control.

### Options for a solo OSS maintainer

| Option | Cost | Eligibility / constraints | Publisher shown | Fit |
|---|---|---|---|---|
| **SignPath Foundation** ([terms](https://signpath.org/terms), [site](https://signpath.org/)) | Free for OSS | OSI licence "without commercial dual-licensing" (MIT OR Apache-2.0 qualifies). "The project must already be released in the form that should be signed." MFA for all team members. Authors/Reviewers/Approvers roles. A code signing policy on the project home page. A privacy statement ("will not transfer any information to other networked systems unless specifically requested by the user"), or a disclosed, disable-able data flow. "Software must not modify the user's system configuration without proper warnings." Builds must run on GitHub-hosted runners and be uploaded with `actions/upload-artifact` ([SignPath GitHub](https://docs.signpath.io/trusted-build-systems/github)) | "SignPath Foundation" ("SignPath Foundation is the publisher of the OSS project") | **Best fit.** config-digger already shows confirmations before writes and makes no default network calls |
| **Azure Artifact Signing** (formerly Trusted Signing) | Basic US$9.99/month for 5,000 signatures; Premium US$99.99/month ([product page](https://azure.microsoft.com/en-us/products/artifact-signing)); needs a paid Azure subscription ([FAQ](https://learn.microsoft.com/en-us/azure/artifact-signing/faq)) | "Individual developers must be located in the United States or Canada." Organisations: US, Canada, EU, UK, Australia, NZ, Japan, South Korea, Singapore, Switzerland, Norway, Israel ([quickstart](https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart)). Short-lived certificates; Tauri supports it through `signCommand` + `artifact-signing-cli` ([Tauri Windows signing](https://v2.tauri.app/distribute/sign/windows/)) | Maintainer's legal name (or company name) | Good if eligible; no hardware token. **Not available to an individual in the EU** |
| OV certificate from a CA | Usually a few hundred US$/year (not verified here) | Hardware token or cloud HSM required for new certificates; configured with `certificateThumbprint`/`digestAlgorithm`/`timestampUrl` or `signCommand` ([Tauri Windows signing](https://v2.tauri.app/distribute/sign/windows/)) | Maintainer's name | Most expensive, most hassle |
| Microsoft Store (MSIX) | Free account | Tauri has no MSIX output; virtualisation hazard (§2) | Microsoft re-signs | Not recommended |

**Recommendation:**

1. Release v0.1 unsigned on GitHub Releases. Publish SHA-256 checksums and GitHub artifact attestations so users can run `gh attestation verify` ([artifact attestations](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations)). The README should explain the SmartScreen "More info → Run anyway" step and say that Smart App Control will block the build.
2. Then apply to SignPath Foundation. The CI pipeline already has to run on GitHub-hosted runners for this.
3. Fall back to Artifact Signing if the maintainer qualifies (an individual in the US or Canada, or a company in an eligible country).

## 6. Updater

- **Keys:**
  - Generate them with `tauri signer generate -w ~/.tauri/myapp.key`.
  - The private key is passed as `TAURI_SIGNING_PRIVATE_KEY`, plus `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` if it has one. It is a CI secret and must never be lost.
  - The public key goes inline in `plugins.updater.pubkey`; it cannot be a file path.
  - `bundle.createUpdaterArtifacts: true` produces the `.sig` files.

  This signature is separate from Authenticode and is required whether or not the installer is Authenticode-signed ([updater](https://v2.tauri.app/plugin/updater/)).
- **Endpoints:**
  - Set them in `plugins.updater.endpoints`, with `{{current_version}}`, `{{target}}` and `{{arch}}` variables. The endpoint can be a static JSON file or a dynamic server (204 means no update).
  - TLS is required unless `dangerousInsecureTransportProtocol` is set.
  - Endpoints and pubkey can also be set at runtime through the updater builder ([updater](https://v2.tauri.app/plugin/updater/)).
  - `tauri-action` uploads `latest.json` by default (`uploadUpdaterJson: true`) for the endpoint `https://github.com/<user>/<repo>/releases/latest/download/latest.json` ([tauri-action](https://github.com/tauri-apps/tauri-action)). Don't publish releases without updater bundles, because `releases/latest` would point at them.
- **Windows behaviour:** `plugins.updater.windows.installMode` is `passive` (default), `basicUi` or `quiet`. The app exits while the installer runs ([updater](https://v2.tauri.app/plugin/updater/)).
- **Opt-in:** "Not automatic — must be explicitly called" via JS `check()` or Rust `app.updater()?.check().await?` ([updater](https://v2.tauri.app/plugin/updater/)). Plan:
  - Register the plugin, but call `check()` only from a Rust command, and only when the user has turned on "Check for updates", which defaults to off. Also offer a manual "Check now" button.
  - Don't grant `updater:default` to the webview. The `check_for_update` app command covers what it needs.
  - This keeps the "no network calls unless the user asks" promise, and with it SignPath's privacy statement.
  - Builds installed through winget may want the check disabled entirely so the two update paths don't compete (open question).

## Sources

Tauri
- https://v2.tauri.app/security/capabilities/ and source https://github.com/tauri-apps/tauri-docs/blob/v2/src/content/docs/security/capabilities.mdx
- https://v2.tauri.app/security/permissions/
- https://v2.tauri.app/security/scope/
- https://v2.tauri.app/learn/security/writing-plugin-permissions/
- https://docs.rs/tauri-build/latest/tauri_build/struct.AppManifest.html
- https://github.com/tauri-apps/tauri/blob/dev/examples/api/src-tauri/build.rs
- https://github.com/tauri-apps/tauri/blob/dev/examples/api/src-tauri/capabilities/run-app.json
- https://v2.tauri.app/develop/calling-rust/
- https://v2.tauri.app/plugin/file-system/ and source https://github.com/tauri-apps/tauri-docs/blob/v2/src/content/docs/plugin/file-system.mdx
- https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/fs/src/config.rs
- https://v2.tauri.app/distribute/windows-installer/
- https://v2.tauri.app/distribute/sign/windows/
- https://v2.tauri.app/distribute/microsoft-store/
- https://v2.tauri.app/plugin/updater/
- https://github.com/tauri-apps/tauri-action

Microsoft
- https://learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-behind-the-scenes
- https://learn.microsoft.com/en-us/windows/msix/desktop/flexible-virtualization
- https://learn.microsoft.com/en-us/windows/msix/msix-troubleshooting-guide
- https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution
- https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation
- https://learn.microsoft.com/en-us/windows/apps/develop/smart-app-control/overview
- https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart
- https://learn.microsoft.com/en-us/azure/artifact-signing/faq
- https://azure.microsoft.com/en-us/products/artifact-signing
- https://learn.microsoft.com/en-us/windows/apps/publish/partner-center/open-a-developer-account
- https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msi/app-package-requirements
- https://learn.microsoft.com/en-us/windows/package-manager/package/repository
- https://learn.microsoft.com/en-us/windows/win32/procthread/process-security-and-access-rights
- https://learn.microsoft.com/en-us/windows/win32/api/psapi/nf-psapi-getmodulefilenameexw

Signing, packaging, processes
- https://signpath.org/terms
- https://signpath.org/
- https://docs.signpath.io/trusted-build-systems/github
- https://github.com/ScoopInstaller/Scoop/wiki/Criteria-for-including-apps-in-the-main-bucket
- https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations
- https://docs.rs/sysinfo/latest/sysinfo/
- https://docs.rs/sysinfo/latest/sysinfo/struct.ProcessRefreshKind.html
- https://docs.rs/sysinfo/latest/sysinfo/struct.Process.html
- https://github.com/GuillaumeGomez/sysinfo/blob/main/src/common/system.rs
- https://github.com/GuillaumeGomez/sysinfo/blob/main/src/windows/system.rs
- https://github.com/GuillaumeGomez/sysinfo/blob/main/src/windows/process.rs

## Open questions

1. **Where is the maintainer legally based, and is there a company?** This decides whether Artifact Signing is an option (individuals: US/Canada only). If neither applies, SignPath Foundation is the only cheap credible path.
2. **SignPath acceptance timing.** They require an existing release, so v0.1 will be unsigned. How long their review takes, and whether a young project with few users is accepted, is not documented. Apply right after v0.1.
3. **Do other Store-packaged Agents virtualise?** Only Claude Desktop (Store) was checked by hand. Each Agent definition for a Store app (e.g. ChatGPT, future Store builds of Cursor/VS Code) needs the same `LocalCache` check, and some may declare `unvirtualizedResources`.
4. **Writing into `LocalCache` while the packaged app runs.** Reads and writes work by ACL. Whether the packaged app picks up external edits without a restart is Agent-specific and needs testing. The running-Agent warning covers it for now.
5. **`sysinfo` on real Agents.** The claims that `exe()` works and `cmd()` is empty for elevated processes come from reading the source and the Win32 docs, not from a test. A short spike against Claude Code, VS Code and Cursor, elevated and not, should confirm them, or lead us to use the `windows` crate directly.
6. **Updater and package managers.** Should winget/Scoop builds compile the updater out (a Cargo feature) to avoid two competing update paths?
7. **MSI at all?** Ship NSIS only for v0.1 unless someone needs GPO/Intune deployment.
