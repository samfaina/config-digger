# config-digger

config-digger shows which MCP servers and skills each AI coding Agent on a Windows machine actually uses, where each one comes from, and why one Entry wins over another. It can Disable, Enable and Remove Entries with a preview, a backup and Undo. It comes as a desktop app and a matching `config-digger` CLI.

It reads your configuration locally and sends nothing anywhere.

## Install

Windows 11, x64. Windows 10 22H2 should work but is not tested.

### Installer

Download `config-digger_<version>_x64-setup.exe` from [GitHub Releases](https://github.com/samfaina/config-digger/releases) and run it. It installs for your user only, without admin rights, into `%LOCALAPPDATA%\Programs\config-digger`, with the app and the `config-digger` CLI in the same folder. You can pick another folder, but not one of config-digger's data folders (see [Uninstall](#uninstall)) or a folder inside or above one.

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

## Updates

The app updates only when you ask. It makes no network call unless you click Check now in About, or turn on "Check for updates on startup" in Settings (off by default). Even then, it only fetches `latest.json` from the latest GitHub release, which release candidates never reach. When it finds an update, a notice shows the version and a link to its release notes. The update installs only when you click Install and restart, and never while an Operation is writing. The app downloads the installer, checks its signature, and runs it in passive mode, which replaces both the app and the `config-digger` CLI, then starts the app again. The CLI never checks for updates: update it with the installer, winget or a new CLI zip.

## Uninstall

Uninstall config-digger from Settings > Apps, or with `winget uninstall samfaina.config-digger`.

By default the uninstaller removes only the app's install folder, `%LOCALAPPDATA%\Programs\config-digger` unless you picked another, and keeps config-digger's data:

- `%LOCALAPPDATA%\config-digger`: backups, the History of Operations behind Undo, logs, the parked-Entry store and when the app last checked for updates;
- `%APPDATA%\config-digger`: the homes and Projects you added or hid, and the app's settings.

If you tick "Delete config-digger data", it deletes both folders. It warns you first that every backup and with them the Undo history will be lost, and names any parked Entries.

**Enable parked Entries before uninstalling.** When an Agent has no switch of its own, Disable parks the Entry: it moves the Entry out of the Agent's file into config-digger's parked-Entry store. The uninstaller never puts parked Entries back. If you delete the data, they are gone. If you keep it, they stay parked until you install config-digger again and Enable them.

## Releasing

Maintainers: set the workspace version in `Cargo.toml`, merge, then push a tag `v<version>`, such as `v0.1.0` or `v0.1.0-rc.1`. The [release workflow](.github/workflows/release.yml) checks that the tag is the workspace version, builds the installer and the CLI zip on GitHub-hosted runners, signs the installer for the updater, and publishes them with the installer's `.sig`, `latest.json`, `SHA256SUMS` and attestations. If the signature or `latest.json` is missing, the run fails before it creates the release. `-rc.N` tags become prereleases and are never marked latest, so the updater, which reads `releases/latest`, never offers them. For a final release, submit the `winget-manifest` artifact of the workflow run to [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) under `manifests/s/samfaina/config-digger/<version>/`.

To build the installer locally, sign it with the updater key, as the release does:

```powershell
cd crates\config-digger-app\ui
npm ci
$env:TAURI_SIGNING_PRIVATE_KEY = "$HOME\.tauri\config-digger.key"
npm run tauri -- build --config tauri.bundle.conf.json
```

### The updater signing key

The updater accepts only installers signed with config-digger's minisign key. The public key is inlined in `crates/config-digger-app/tauri.conf.json` (`plugins.updater.pubkey`). The private key is the repository secret `TAURI_SIGNING_PRIVATE_KEY`, which holds the contents of the key file. The key has no password. If a future key has one, put the password in the secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

**Back up the private key offline**, for example in a password manager and on a USB drive kept away from the machine. GitHub never shows a secret again, so the backup is the only other copy. If the key is lost, installed copies of config-digger can no longer update themselves: a new key needs a new public key in the app, and users must install that version by hand.

To make a new key pair, run `npm run tauri -- signer generate -w "$HOME\.tauri\config-digger.key"` in `crates\config-digger-app\ui`. Put the contents of the `.pub` file in `plugins.updater.pubkey` and the contents of the key file in the secret.

## License

MIT OR Apache-2.0.
