# config-digger

config-digger shows which MCP servers and skills each AI coding Agent on a Windows machine actually uses, where each one comes from, and why one Entry wins over another. It can Disable, Enable and Remove Entries with a preview, a backup and Undo. It comes as a desktop app and a matching `config-digger` CLI.

It reads your configuration locally and sends nothing anywhere.

## Install

Windows 11, x64. Windows 10 22H2 should work but is not tested.

### Installer

Download `config-digger_<version>_x64-setup.exe` from [GitHub Releases](https://github.com/samfaina/config-digger/releases) and run it. It installs for your user only, without admin rights, into `%LOCALAPPDATA%\config-digger`, with the app and the `config-digger` CLI in the same folder. That is also the folder where config-digger keeps its data (see [Uninstall](#uninstall)).

The box "Add the config-digger command to my PATH", checked by default, adds that folder to your user PATH, so `config-digger` works in any new terminal. The uninstaller takes it off again. If you untick the box, later installs and upgrades remember that. The installer never edits your PowerShell `$PROFILE`.

The app needs the WebView2 runtime. Windows 11 ships with it. If it is missing, the installer downloads it.

### winget

Once a release's manifest is accepted into the winget repository:

```powershell
winget install samfaina.config-digger
```

winget runs the same installer.

### CLI only

For CI, or if you don't want the app, download `config-digger_<version>_x64-cli.zip` from the same release. It holds `config-digger.exe` and nothing else. Unzip it to any folder and add that folder to your PATH yourself.

### Windows warnings

The builds are not code-signed yet.

- **SmartScreen** says "Windows protected your PC" the first time you run the installer. Click **More info**, then **Run anyway**.
- **Smart App Control**, when it is on (enforcement mode, on some clean Windows 11 installs), blocks unsigned apps with no way to allow them. config-digger can't run there until it is signed. Signing through SignPath Foundation is planned after v0.1.

### Checking a download

Each release has a `SHA256SUMS` file and GitHub artifact attestations for the installer and the CLI zip. Compare the hash `Get-FileHash` prints with the file's line in `SHA256SUMS`, or let `gh` check the attestation:

```powershell
Get-FileHash .\config-digger_0.1.0_x64-setup.exe -Algorithm SHA256
gh attestation verify .\config-digger_0.1.0_x64-setup.exe --repo samfaina/config-digger
```

## PowerShell completions

The installer doesn't set up completions. To have them in every session, add this line to your `$PROFILE`:

```powershell
config-digger completions powershell | Out-String | Invoke-Expression
```

`bash`, `zsh` and `fish` work the same way.

## Scripting with `--json`

With `--json`, every command prints exactly one JSON document with a `schema_version` field, errors included, and exits with a stable code from 0 to 10. The first release is `schema_version` 1. A breaking change to a document (a field renamed, removed or retyped) bumps `schema_version`. A new field does not.

## Uninstall

Uninstall config-digger from Settings > Apps, or with `winget uninstall samfaina.config-digger`.

By default the uninstaller removes only the app's own files and keeps config-digger's data:

- `%LOCALAPPDATA%\config-digger`: backups, the History of Operations behind Undo, logs and the parked-Entry store;
- `%APPDATA%\config-digger`: the homes and Projects you added or hid, and the app's settings.

If you tick "Delete config-digger data", it deletes both folders. It warns you first that every backup and with them the Undo history will be lost, and names any parked Entries.

**Enable parked Entries before uninstalling.** When an Agent has no switch of its own, Disable parks the Entry: it moves the Entry out of the Agent's file into config-digger's parked-Entry store. The uninstaller never puts parked Entries back. If you delete the data, they are gone. If you keep it, they stay parked until you install config-digger again and Enable them.

## Releasing

Maintainers: set the workspace version in `Cargo.toml`, merge, then push a tag `v<version>`, such as `v0.1.0` or `v0.1.0-rc.1`. The [release workflow](.github/workflows/release.yml) checks that the tag is the workspace version, builds the installer and the CLI zip on GitHub-hosted runners, and publishes them with `SHA256SUMS` and attestations. `-rc.N` tags become prereleases. For a final release, submit the `winget-manifest` artifact of the workflow run to [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) under `manifests/s/samfaina/config-digger/<version>/`.

To build the installer locally:

```powershell
cd crates\config-digger-app\ui
npm ci
npm run tauri -- build --config tauri.bundle.conf.json
```

## License

MIT OR Apache-2.0.
