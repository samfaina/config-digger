# Research: automating the UAT walk

Issue: #236. Researched 2026-10-22 against `uat/plan.json` at commit `ffc08ac`, which has 69 steps in 15 sections. Tried on the maintainer's machine, Windows 11 Pro build 26300, with Tauri 2.12 (`tauri-cli` 2.12.1, `tauri-plugin-updater` 2.13.2), `tauri-driver` 2.1.0, WebView2 and `msedgedriver` 155.0.4283.62, NSIS 3.11 and Windows Sandbox. Domain terms, such as Entry, Agent home and Operation, are used as `CONTEXT.md` defines them.

## Summary and recommendations

1. **The manual walk can shrink from 69 steps to parts of 3.** The motion halves of 5.1 and 11.10 stay: whether a notice and the Undo snackbar slide in and out without a jump. Tests already prove their durations, properties and the reduced-motion fade, but no tool here judges how motion looks over time. SmartScreen's "Windows protegió su PC" in 9.2 also stays, until the app is signed. Besides those, a person reviews each KO of the agent judge and approves each changed visual reference. Every other check goes to the tools the issue names (fake-backend UI tests, WebDriver on the real app, the Sandbox script, CLI tests, visual regression, the agent judge) or to the local script, step by step in [section 8](#8-every-step).
2. **WebDriver works on the real app.** `tauri-driver` with the `msedgedriver` that matches WebView2 opened a session on the release build in 0.3 s. It found the rail's buttons and the Inventory's Item rows, switched tabs and resized the window to 1100 × 640. I did not try it on `windows-latest`; the first follow-up starts there.
3. **The real app runs on the throwaway data alone.** Launch `tauri-driver` with `USERPROFILE`, `APPDATA` and `LOCALAPPDATA` pointing into `cd-throwaway`. The core reads every Agent home and writes its own data through those variables, and Windows resolves its known folders from `USERPROFILE`, so `USERPROFILE` must hold `AppData\Local` and `AppData\Roaming` too. Otherwise Tauri cannot find the app's data folder and WebDriver cannot open a session.
4. **Windows Sandbox runs the installer, the update and the uninstaller unattended.** A `.wsb` file maps the installers, the scripts and a results folder, and its logon command runs a PowerShell script. Silent installs and uninstalls passed every check: install folder, user `PATH` in `HKCU\Environment`, the remembered PATH choice, and both data folders.
5. **Check the installer's PATH page through UI Automation, not command-line options.** Windows PowerShell 5.1 inside the Sandbox drove the welcome page, the directory page, Already Installed, Finish and the uninstaller's Delete config-digger data warning, with nothing to install. The uninstaller needs it anyway: Tauri's NSIS script has no command-line switch for that box, and steps 13.3 and 13.4 depend on it. Steps 9.2, 9.3, 9.4 and 13.4 passed this way, and 13.3 passed except for its parked lines, since the run had no parked Entry.
6. **Check the update against a local endpoint, signed with a key made for the run.** Build installer A and installer B of the release candidate's commit with a test public key and `http://127.0.0.1:8765/latest.json`. Then serve B's signed `latest.json` inside the Sandbox. The updater found B, downloaded it, accepted its signature, installed it in passive mode and restarted on B's executable, keeping the PATH and the data. B's CLI still reported A's version, because I raised only Tauri's version, so B must be built with the workspace version raised. The key lives only for that run, and the release key never leaves its GitHub secret.
7. **A bug found on the way: the installer leaves config-digger off the PATH on a Windows without the VC++ runtime.** `config-digger.exe` links `vcruntime140.dll` dynamically. A clean Windows lacks it, so the CLI exits with `0xC0000135`, which means the DLL was not found. The installer's `add-to-path` runs that CLI, and so does the uninstaller's list of parked Entries. The app is unaffected, because `tauri-build` links the CRT statically. It is a follow-up of its own.
8. **Commit the visual references, as an exception to the screenshot rule, if the maintainer agrees.** They are shots of the fake backend, whose fixtures a test already holds to `cd-throwaway-*` Entries, fake Secrets and paths under `E:\cd-throwaway`. [Section 5](#5-the-look) proposes the rule change word for word. Without it, the references stay local and ignored, and only the local script compares them.
9. **Run the agent judge locally, in a Claude Code session.** It reads a step's `expect` and the screenshots the tools took, and writes OK or KO with `by: "agent-judge"`, which reach the run's `results` with the other verdicts. A person reviews its KOs in the runner. The screenshots are never committed, attached or uploaded: only the session's model reads them, as an agent reads its own screenshots today.
10. **Automated verdicts keep the runner's shape.** They go in as `{ id, status, notes, time, version }` documents with two more fields, `by` and `evidence`, which the runner ignores today. A step gets an `automation` field listing the tools that check it, and a `manual` field when a person must still look.

## 1. What exists today

- `npm run screenshots` (`ui/scripts/screenshots.ts`) runs the UI on the fake backend in a headless Edge with `playwright-core` 1.64 and writes PNGs of 18 states. It asserts nothing.
- Vitest on happy-dom covers the UI's logic: 29 test files in `ui/src` and `ui/build`. Many plan steps are already covered there, and section 8 names them.
- `cargo test` covers the core (47 test files in `crates/config-digger-core/tests`), the CLI's `installer` subcommands (`crates/config-digger-cli/tests/installer.rs`), the updater's config (`crates/config-digger-app/tests/updater_config.rs`), the fake backend's fixtures (`crates/config-digger-app/tests/fake_backend.rs`) and the plan and the data script (`xtask/tests/uat_plan.rs`, `uat_data.rs`).
- `cargo xtask installer` checks the installer's folders statically. Nothing installs it.

## 2. The real app through WebDriver

### What I ran

I installed `tauri-driver` with `cargo install tauri-driver --locked` (2.1.0) and downloaded `msedgedriver` 155.0.4283.62 from `https://msedgedriver.microsoft.com/155.0.4283.62/edgedriver_win64.zip`, the WebView2 version in `HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}`. I built the app with `cargo app-release`. Then I created the throwaway data with `uat/data/cd-throwaway-setup.ps1 -Root <scratch>\cd-throwaway` and started `tauri-driver --native-driver msedgedriver.exe` with:

```powershell
$env:USERPROFILE  = "<scratch>\cd-throwaway\claude-code-home"
$env:APPDATA      = "$env:USERPROFILE\AppData\Roaming"
$env:LOCALAPPDATA = "$env:USERPROFILE\AppData\Local"
```

A Node script spoke the W3C protocol with `fetch`, with no client library and no screenshot:

```json
POST /session
{ "capabilities": { "alwaysMatch": { "tauri:options": { "application": "E:\\projects\\config-digger\\target\\release\\config-digger-app.exe" } } } }
```

### What happened

| Check | Result |
|---|---|
| Session | Opened in 336 ms, `browserName` `webview2`, version 155.0.4283.62 |
| Title | `config-digger` |
| `button[aria-label='Projects & homes']` | Found and clicked |
| `.item-row` | 2 rows. The first reads `cd-throwaway-user`, its locator in the throwaway home, `1 Entry` and `Effective` |
| Projects tab | `Projects · 3` |
| `POST /window/rect` to 1100 × 640 | The viewport became 1100 × 640 at a device pixel ratio of 1.25 |

The app's log, written in the throwaway `LOCALAPPDATA`, said `inventory read homes=1 items=2`. Nothing outside the scratch folder changed.

The first attempt set `APPDATA` and `LOCALAPPDATA` into a folder outside `USERPROFILE`. The app still started and read the throwaway data, but its log said `failed to resolve the app local data directory, the webview falls back to its default data directory`. The session then failed with `session not created: DevToolsActivePort file doesn't exist`. Tauri resolves its data folder through the Windows known folders, and they follow `USERPROFILE`. With both `AppData` folders inside `USERPROFILE`, the session opened. The `tauri:options` capability takes only `application`, `args` and `webviewOptions` (`tauri-driver`'s `server.rs`), so the variables go on the `tauri-driver` process, which hands them down to the app.

Tauri's WebDriver guide supports `tauri-driver` on Windows and Linux only. Its WebdriverIO service keeps the Edge driver in step with WebView2 on its own. Without it, the tests must pick the `msedgedriver` whose version is the installed WebView2's. On `windows-latest`, the image's WebView2 version decides it.

### What it can reach, and what it can't

- Every write flow on real files: Add Agent home, Add Project, Hide, Remove leftovers, Disable, Undo, refused and failed Operations, Delete backup, Dismiss, the log lines, the updater's first check. The fake backend can't reach any of them, because its write commands change nothing.
- The Windows folder picker is a native dialog, outside the webview. Both Add dialogs have a text field (`aria-label="Folder"` and `"Path"`), so the tests type the path and check the picker separately. UI Automation can drive the common file dialog. I did not try it.
- Files the steps edit by hand in Notepad (11.4, 11.6) are edited by the tests directly, with the app closed when the step closes it.
- Plan text spells paths as `E:\cd-throwaway`. A runner has no `E:` drive, so the CI job runs `subst E: <workspace>\uat-drive` (no admin rights needed) before the setup script, and the assertions keep the plan's own text.
- Process detection reads the real processes. On CI no Agent is running. On the tester's machine, the local script runs Claude Desktop on purpose for 7.2.

### Where it runs

- **CI, on every PR,** with the throwaway profile. Nothing it uploads holds a screenshot: the job writes a results JSON and the console log, and it never calls the screenshot endpoint.
- **Locally, before each release candidate,** for 7.2 with Claude Desktop open. It uses the same throwaway profile, so the app adds and changes only throwaway homes. Claude Desktop keeps the tester's own configuration, and nothing touches it.

## 3. The UI on the fake backend

Playwright on the fake backend is the cheapest place for read-only screens, keyboard behaviour, layout boxes and the look. The screenshots script already holds the setup: Vite in `fake` mode, a headless Edge in the app's window size, and `SHOTS`. The tests reuse it and add assertions with `getByRole`, `boundingBox()`, `page.keyboard`, `page.clock` for the 4 s and 10 s notices, and `page.emulateMedia({ reducedMotion: "reduce" })`.

What it can't reach:

- **Writes.** `plan_action`, `execute_plan`, `save_settings` and the list changes answer a fixture and change nothing, so a step that checks a changed state after a write goes to WebDriver.
- **Busy states.** Fifteen steps check a spinner that "puede durar menos de un segundo". The fake backend answers at once, so the tests need a `?delay=<command>:<ms>` parameter beside `?fail=`, which holds a command's answer. With it, a test asserts the spinner, the disabled buttons and the text before the change, and the visual reference can freeze the state. The logic is already tested in `acting.test.ts`, `busy.test.ts`, `changing.test.ts` and `reads.test.ts`.
- **Long lists the fixtures lack.** History holds two Operations, so 4.3's twelve Disables go to WebDriver.
- **WebView2 itself.** Headless Edge and WebView2 are both Chromium 155 here, so CSS renders the same. What only the real window shows, such as its scrollbars, gets one WebDriver screenshot, taken locally and read by the agent judge.

## 4. Installer, update and uninstall in Windows Sandbox

### The setup

```xml
<Configuration>
  <Networking>Enable</Networking>
  <vGPU>Disable</vGPU>
  <ClipboardRedirection>Disable</ClipboardRedirection>
  <MemoryInMB>4096</MemoryInMB>
  <MappedFolders>
    <MappedFolder><HostFolder>…\installers</HostFolder><SandboxFolder>C:\uat\installers</SandboxFolder><ReadOnly>true</ReadOnly></MappedFolder>
    <MappedFolder><HostFolder>…\scripts</HostFolder><SandboxFolder>C:\uat\scripts</SandboxFolder><ReadOnly>true</ReadOnly></MappedFolder>
    <MappedFolder><HostFolder>…\results</HostFolder><SandboxFolder>C:\uat\results</SandboxFolder><ReadOnly>false</ReadOnly></MappedFolder>
  </MappedFolders>
  <LogonCommand><Command>powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\uat\scripts\in-sandbox.ps1</Command></LogonCommand>
</Configuration>
```

The host opens the `.wsb` file and waits for `results\done.txt`. The script appends one JSON line per check to `results\results.jsonl` and ends with `shutdown /s /t 5`, which closes the Sandbox. The `wsb` command-line tool is documented for Windows 11 24H2. It was not on this machine's PATH, and none of the runs needed it. The Sandbox user is `WDAGUtilityAccount`, an administrator, with culture `es-ES` and WebView2 155.0.4283.62. Each run took between 5 and 9 minutes. Installing the VC++ runtime took 1 to 4 of them.

### Silent install and uninstall

Run with `/S`, then `uninstall.exe /S`.

| Check | Result |
|---|---|
| `%LOCALAPPDATA%\Programs\config-digger\config-digger-app.exe` and `config-digger.exe` | Present |
| Nothing installed in `%LOCALAPPDATA%\config-digger` | OK |
| User `PATH` in `HKCU\Environment` holds the install folder | OK, once the VC++ runtime was there (see below) |
| `KeepPathAlone` under `HKCU\Software\samfaina\config-digger` | 0 |
| `config-digger --version` | `config-digger 0.1.0-rc.2` |
| Silent reinstall with `KeepPathAlone` set to 1 | Install folder off the PATH, choice still 1 |
| Silent uninstall | App gone, off the PATH, both data folders kept, PATH choice kept |

A silent uninstall never deletes data: Tauri's `$DeleteAppDataCheckboxState` is set only by the confirm page's box, and no command-line switch sets it (Tauri's `installer.nsi`).

### The VC++ runtime

The first Sandbox run installed both executables, but the PATH stayed unset and `config-digger --version` printed nothing. The Sandbox has no `vcruntime140.dll`, `vcruntime140_1.dll` or `msvcp140.dll` in `System32`. `config-digger.exe --version` exits with `0xC0000135`, STATUS_DLL_NOT_FOUND. `config-digger-app.exe` starts, because `tauri-build` links the CRT statically for the app, and nothing does it for the CLI. `hooks.nsh` edits the PATH through `config-digger installer add-to-path`, and its exit code only reaches the install log (`DetailPrint`). So on a PC without the VC++ runtime, the install succeeds and the PATH box does nothing. The uninstaller's `installer parked` fails the same way, and the warning falls back to "Any parked Entry is deleted too". Linking the CLI's CRT statically fixes both, and the Sandbox is how to check the fix. Until then, the script installs `vc_redist.x64.exe /install /quiet /norestart` first.

### The PATH page: two ways compared

| | Command-line options | UI Automation on the NSIS window |
|---|---|---|
| What it changes | `hooks.nsh` parses a new switch, such as `/PATH=0`, with `GetOptions` and sets `$KeepPathAlone` | Nothing in the installer |
| What it checks | The hook's logic, which `crates/config-digger-cli/tests/installer.rs` and the silent runs already cover | What the tester sees: the box's label, its starting state, Already Installed, the directory page's text and its disabled Next |
| The page itself | Still needs another check: a silent install skips it | Checked |
| The uninstaller | No switch exists for Delete config-digger data, so 13.3 and 13.4 still need the window | The same code drives it |
| Tools needed | None | `System.Windows.Automation` in Windows PowerShell 5.1, already in Windows. FlaUI or pywinauto would need .NET or Python inside the Sandbox |
| Cost | A switch that only tests use, shipped to users | About 200 lines of PowerShell |

**Recommendation: UI Automation.** It checks what the steps check, it needs nothing installed, and the uninstaller needs it either way. A command-line switch would add installer code that only the tests use and still leave the page unchecked.

What I learned driving it. The managed UIA client, without the client-side providers, shows every Win32 control of an NSIS page as a `Pane` with no patterns. The script finds controls by window class (`Button`, `Edit`, `Static`), name and AutomationId (Next is `1`, Cancel `2`). It drives them with window messages on their handles: `BM_GETCHECK`, `BM_CLICK`, `WM_SETTEXT`, `WM_GETTEXT`. The clicks must be posted, not sent. A sent `BM_CLICK` that ends a page waits for the next one, and the run hung there once. A new installer's window is found by its process id, because the previous one may still be closing.

| Step | Checks, all passed in the Sandbox |
|---|---|
| 9.2 | The box reads "Add the config-digger command to my PATH" and starts ticked. The directory page proposes `C:\Users\WDAGUtilityAccount\AppData\Local\Programs\config-digger` and says "It can't be config-digger's data folder, %LOCALAPPDATA%\config-digger or %APPDATA%\config-digger, nor a folder inside or above one.". Next is disabled on `%LOCALAPPDATA%\config-digger` and on a folder inside `%APPDATA%\config-digger`, and enabled again on the proposed folder. After Finish, with Run config-digger and Create desktop shortcut unticked, the folder is on the PATH and `KeepPathAlone` is 0 |
| 9.3 | The box starts ticked and is unticked. Already Installed reads "config-digger 0.1.0-rc.2 is already installed." and takes Add/Reinstall components. Afterwards the folder is off the PATH and `KeepPathAlone` is 1 |
| 9.4 | The box starts unticked. Ticked again, the folder is back on the PATH and `KeepPathAlone` is 0 |
| 13.3 | Delete config-digger data starts unticked. Ticked, Uninstall shows a warning with "Deleting config-digger data deletes every backup, and with them the Undo history." and "Click OK to delete the data, or Cancel to keep it and uninstall only the app.", and Cancel has the focus. After Cancel the app is gone, off the PATH, and the backups and the roaming folder are kept |
| 13.4 | After OK, the app, both data folders and `HKCU\Software\samfaina\config-digger` are gone, and the folder is off the PATH |

The 13.3 run had no parked Entry, so the warning had no parked lines. The script can park one inside the Sandbox with the CLI (`homes add` and `disable` on throwaway data) before uninstalling. I did not try it.

### Update

The updater reads only `releases/latest`, so a release candidate is never offered one from GitHub. I built two installers of the same commit with a key generated for the test (`tauri signer generate -w uat-test.key -p "" --ci`). Each build took an extra `--config` that sets the updater to it:

```json
{ "plugins": { "updater": {
  "pubkey": "<uat-test.key.pub>",
  "endpoints": ["http://127.0.0.1:8765/latest.json"],
  "dangerousInsecureTransportProtocol": true
} } }
```

A was `0.1.0-rc.2`. B also took `{"version": "0.1.0-rc.99"}`. `latest.json` named B's installer and the contents of its `.sig`, under `windows-x86_64` and `windows-x86_64-nsis`. Inside the Sandbox, a PowerShell `HttpListener` served the installers folder on `127.0.0.1:8765`. The script installed A silently, then pressed About, Check now and Install and restart through UI Automation, which also reads WebView2's accessibility tree.

| Check | Result |
|---|---|
| About before the check | `Last checked: never.` |
| Requests served | `GET /latest.json`, then `GET /config-digger_0.1.0-rc.99_x64-setup.exe`, both from `tauri-plugin-updater/2.13.2` |
| App log | `updates checked found="0.1.0-rc.99"`, `update installing version=0.1.0-rc.99`, then a new `app started` |
| Running app after the restart | `config-digger-app.exe`, product version `0.1.0-rc.99` |
| PATH and data | Still on the PATH, the backup kept |
| `config-digger --version` | Still `0.1.0-rc.2`, because a Tauri `version` override doesn't change `CARGO_PKG_VERSION`. B must be built with the workspace version raised, so that both executables report it |

WebDriver did not open a session inside the Sandbox, even with the installed app's data folder resolved. It failed after 61 s with the same `DevToolsActivePort file doesn't exist`. I did not find why and stopped there, since UI Automation reached the buttons. The CI tests are unaffected: they run on the host.

**Which key signs it.** A key made for the run, by the local script, deleted when it ends. The release key is a GitHub secret (`TAURI_SIGNING_PRIVATE_KEY`). Copying it to a tester's PC would put every user's update channel on that PC. A test key changes only the public key and the endpoint in A and B, which are the two values the Sandbox can't use anyway. What a test key can't check is that the shipped release candidate's public key matches the release secret. Nothing checks that today. The release workflow could verify `latest.json`'s signature against `tauri.conf.json`'s `pubkey` before it publishes. That is a separate follow-up.

### What Sandbox's limits block

- **No state after closing.** Nothing is lost: each run starts from a clean Windows, which 8.1 asks for, and writes its results to the mapped folder.
- **Restarts.** Since Windows 11 22H2, data survives a restart started inside the Sandbox. No step needs one.
- **One Sandbox at a time, Pro, Enterprise or Education only.** A tester on Windows Home can't run it. A second run waits until the first one's `WindowsSandboxServer` process ends.
- **No VC++ runtime and no GPU.** The runtime adds up to 4 minutes per run until the CLI links it statically. Without vGPU, WebView2 renders in software, which was fast enough.
- **SmartScreen.** Files from a mapped folder carry no Mark of the Web, so 9.2's "Windows protegió su PC" never shows in the Sandbox. No step's `expect` checks it, and it stays as it is until the app is signed.

## 5. The look

### Visual regression

The screenshots script grows a compare mode. It shoots each state, compares it with its reference using `pixelmatch` and `pngjs`, writes a diff image next to it in the ignored `screenshots/` folder, and exits non-zero when more pixels differ than a small threshold. The UI bundles its font (`@fontsource/outfit`), and the fixtures don't change between runs. So a reference shot on one machine and browser compares cleanly on that same machine and browser. A reference shot elsewhere does not, because DPI and font smoothing differ. References are therefore shot where they are compared.

Where the references live:

| | A. Committed | B. Local and ignored | C. A private store |
|---|---|---|---|
| Where | `crates/config-digger-app/ui/visual/*.png`, shot on `windows-latest` | `ui/screenshots/reference/`, per machine | A private repo |
| CI compares on every PR | Yes | No | Only with a token secret, which PRs from forks don't get |
| A change to the look | Shows in the PR as changed PNGs, which the maintainer approves | Needs a person to re-approve each machine's baseline | Updated outside the PR, so the PR doesn't show it |
| Rule change | Needed | None | None, but a second place to keep in step |

**Recommendation: A**, with the rule change below. The fixtures can't show the maintainer's machine: `fake_backend.rs`'s `paths_are_under_the_uat_root_and_name_no_folder_of_this_machine` and `every_item_is_a_throwaway_one` already check it. Those tests are why the exception can be this narrow. The references are made by a `workflow_dispatch` job on `windows-latest` that commits them on a branch. A PR that changes the look changes them too. A comparison that fails on CI prints the shot's name and the number of different pixels, and uploads nothing.

**Proposed rule change, for the maintainer to approve.** Neither file is edited by this research.

In `docs/agents/screenshots.md`, the section "Screenshots stay local" becomes:

> The repo is public. Never commit a screenshot, attach one to a PR or an issue, or post one anywhere, with one exception: the visual references in `crates/config-digger-app/ui/visual/`. They are shots of the fake backend only, made on `windows-latest` by the visual references workflow, and named after a shot in `SHOTS`. The fixtures hold only `cd-throwaway-*` Entries, fake Secrets and paths under `E:\cd-throwaway`, as `tests/fake_backend.rs` checks. A diff image from a failed comparison stays local like any other screenshot. The look of a change is signed off in the UAT walk (see `docs/agents/issue-tracker.md` and `docs/agents/uat.md`); screenshots only let an agent check its own work.

In `docs/agents/issue-tracker.md`, under "How the look of a UI change is checked", the first paragraph becomes:

> The repo is public, and screenshots of the real app show paths from the developer's machine. So no ticket asks for a screenshot in its PR, and no PR carries one, except the fake backend's visual references in `crates/config-digger-app/ui/visual/` (see `docs/agents/screenshots.md`).

If the maintainer declines, option B applies. The local script compares against references on the tester's machine. The agent judge looks at each new baseline before it is kept. CI checks the look only through `styles.test.ts`.

Visual regression says that the look changed, not whether it is right. A first reference, and every changed one, goes to the agent judge and then to the maintainer in the PR.

### The agent judge

A Claude Code session on the tester's machine, run by the local script after the other tools.

- **Input.** For each step whose `automation` lists `agent-judge`: the step's `title`, `goal`, `instructions` and `expect` from `uat/plan.json` at the commit under test, and the screenshots taken for it. The fake-UI tests write them to `ui/screenshots/uat/<step id>-<n>.png`, and the local WebDriver run writes real-window ones to the same place, all ignored by git. Each screenshot comes with a line saying what state it shows.
- **What it does.** It reads each PNG with its own vision and compares it with the parts of `expect` that a picture can show, such as colours, layout and what is visible. Parts it can't see, such as timing or text a test already asserted, it leaves to those tests and says so.
- **Output.** One result per step, in the runner's shape:

  ```json
  { "id": "7.5", "status": "ko", "notes": "Delete backup in History is violet, not red: expect says the four buttons share one red. Shot 7.5-2.", "time": "2026-11-02T10:00:00.000Z", "version": "0.1.0-rc.3", "by": "agent-judge", "evidence": "ui/screenshots/uat/7.5-2.png" }
  ```

  `ok` when every part of `expect` it can see holds, `ko` naming the part that doesn't, `blocked` when a screenshot is missing.
- **How it reaches the results.** The judge writes its results to the same local `uat-results.json` as the local script, and they reach the run with the others, as section 6 says.
- **How a person reviews it.** The runner gets a "Revisar" view that lists `ko` results with `by: "agent-judge"`, with their notes and the local path of the screenshot. The reviewer opens the PNG on their own machine or walks the step. Then they save their own verdict, which has no `by`. The runner copies the judge's `status` and `notes` into a `judged` field of that document, so the record of a disagreement stays. A judge's `ok` stands. For the first two release candidates, the reviewer also walks the judge's OK steps, to see how often it agrees, and the walk's issue records the result.
- **Telling verdicts apart.** `by` names what wrote the verdict: `"ci"` for the CI job, `"local"` for the local script and the Sandbox script it runs, `"agent-judge"` for the judge, and nothing for a tester. The runner shows a chip with it, and Export adds a column.

The judge doesn't run on CI. It would need a model key as a secret and would send screenshots off the machine.

## 6. How verdicts reach the results database

Every tool writes the runner's documents, `{ id, status, notes, time, version }`, plus `by` and `evidence`. `status` is `ok`, `ko` or `blocked`. A failed assertion gives `ko` with the assertion's message as notes. A failure before the step's own checks, such as a setup error or an earlier step it depends on, gives `blocked`. `version` is the workspace version under test. `evidence` is a CI run URL, a test name or a local path, never an image.

- The CI job uploads `uat-results.json`, verdicts only and no screenshot, as an Actions artifact of the release candidate's commit.
- The local script, the Sandbox script it runs and the agent judge write the same file locally.
- When an agent publishes a run, as `docs/agents/uat.md` describes, it downloads the commit's CI results with `gh run download`. It adds the local ones and writes them all with one `ArtifactData` batch (`set results/<id>`), before giving the tester the link. It writes a result only for a step id in the plan at that commit, and skips a step that already has a tester's result, one with no `by`. This is the only way automated verdicts reach the database.

The runner reads `status`, `notes`, `time` and `version` and ignores other fields, so the extra ones break nothing. A follow-up teaches it to show `by`, to skip automated steps in the walk's queue, and to offer the review view.

## 7. New plan fields

Two optional fields on a step, in `plan.schema.json`:

```json
"automation": [
  { "tool": "webdriver", "where": "ci", "test": "uat/webdriver/history.test.mjs > 11.3" },
  { "tool": "visual", "where": "ci", "test": "history-operation" }
],
"manual": "Whether the snackbar's slide looks smooth: tests check its duration and properties, not how it looks moving."
```

- `tool` is one of `fake-ui`, `webdriver`, `sandbox`, `local`, `cli`, `visual` or `agent-judge`. `where` is `ci` or `local`. `test` names the test, the shot or the script check that covers the step.
- `manual` says what a person still checks and why no tool can. A step with `automation` and no `manual` drops out of the manual walk.
- `uat_plan` checks that every `test` exists: a test named after the step id in the named file, a shot in `SHOTS`, or a check id in the Sandbox script. Step ids stay as they are.

## 8. Every step

"Covered today" names the tests that already check a part of the step. Each tool row is new work unless it says tried.

| Step | Covered today | Tool | Where | Why |
|---|---|---|---|---|
| 8.1 | None | Sandbox | local | A new Sandbox is a PC that never had config-digger. The script runs the step's first command and expects four False |
| 9.1 | `release.yml` runs `cargo xtask assets` | Local script | local | `gh release download <tag> --pattern '*-setup.exe'` checks the asset's name has the version, and puts it in the Sandbox's installers folder |
| 9.2 | `installer.rs` add-to-path tests, `cargo xtask installer` | Sandbox with UI Automation, **manual for SmartScreen** | local, manual | Tried, all checks passed. "New terminal" is read from `HKCU\Environment`. SmartScreen's prompt stays manual until the app is signed, see section 9 |
| 9.3 | `installer.rs` remove-from-path tests | Sandbox with UI Automation | local | Tried, passed |
| 9.4 | None | Sandbox with UI Automation | local | Tried, passed |
| 1.1 | `uat_data.rs` | WebDriver, setup | ci | The tests run the script and check its last line and file list before the session |
| 1.2 | `adding.test.ts`, `secret_masking.rs` | WebDriver, UI Automation for the picker | ci | Types the folder, checks the notice, the row's tags, the Items and the masked Secret. The picker check is untried |
| 15.1 | `Loading.test.tsx`, `reads.test.ts` | Fake UI with `?delay=read_inventory`, visual | ci | The delay holds the loading state long enough to assert and shoot |
| 15.2 | `reads.test.ts`, `Inventory.test.tsx` | Fake UI with a delay and focus events, visual | ci | Same, for the reading bar |
| 15.3 | `HistoryScreen.test.tsx`, `Loading.test.tsx` | Fake UI with a delay, visual | ci | Same, on three screens |
| 10.1 | `tooltips.test.ts` | Fake UI, visual | ci | Read only |
| 10.2 | `adding.test.ts` | WebDriver | ci | A real list change |
| 10.3 | `adding.test.ts` | WebDriver | ci | A real list change |
| 10.4 | `user_homes_and_projects.rs` | WebDriver | ci | Reads `homes-and-projects.json` in the throwaway `APPDATA` |
| 10.5 | `places.test.ts` | Fake UI | ci | Read only. The fixtures come from the same data |
| 10.6 | `places.test.ts` | Fake UI | ci | Navigation and filters are UI state |
| 10.14 | `Loading.test.tsx` | Fake UI with a delay, visual | ci | A busy state |
| 10.7 | `dialog.test.ts`, `missing_projects.rs` | WebDriver, visual for the diff's colours | ci | A real Remove. The tests also fail the step when Before you apply names a path outside the root, as the step does |
| 10.8 | `places.test.ts` | WebDriver, visual | ci | A real Hide. Visual for the grey Hidden row |
| 10.11 | `changing.test.ts`, `busy.test.ts` | Fake UI with a delay | ci | A busy state |
| 10.9 | `adding.test.ts` | WebDriver, UI Automation for the picker | ci | The error for a missing folder, then a real Add |
| 10.12 | `changing.test.ts` | Fake UI with a delay | ci | A busy state |
| 10.13 | `changing.test.ts`, `places.test.ts` | Fake UI with a delay | ci | A busy state |
| 10.10 | `styles.test.ts` | Fake UI at 1100 × 640, visual | ci | Line boxes and positions |
| 10.15 | `places.test.ts` | Fake UI | ci | A filter over fixtures |
| 2.1 | `dropdown.test.ts`, `Select.test.tsx` | Fake UI, visual, agent judge | ci, local | The panel's look |
| 2.2 | `dropdown.test.ts`, `Inventory.test.tsx` | Fake UI | ci | Real key presses |
| 2.5 | `dropdown.test.ts` | Fake UI | ci | Typing and filtering |
| 2.3 | `dropdown.test.ts` | Fake UI at 1100 × 640, visual | ci | The panel's box inside the viewport |
| 2.4 | `dropdown.test.ts` | Fake UI | ci | A click on a disabled option |
| 3.1 | `tooltips.test.ts` | Fake UI, visual | ci | Hover, 500 ms, and no `title` attribute anywhere, which is what would draw the system's tooltip |
| 3.2 | `tooltips.test.ts` | Fake UI | ci | Focus and Escape |
| 3.3 | `tooltips.test.ts` | Fake UI at 1100 × 640, visual | ci | The tooltip's box inside the viewport |
| 4.1 | `styles.test.ts` | Fake UI, visual, agent judge on a real-window shot | ci, local | The fake UI scrolls to `cd-throwaway-many-30`. WebView2's own scrollbar is seen only in the real window |
| 4.2 | `styles.test.ts` | Fake UI, visual | ci | `styles.test.ts` proves one scrollbar rule for every element |
| 4.3 | `styles.test.ts` | WebDriver, agent judge on a real-window shot | ci, local | The fixtures have two Operations. On CI, twelve real Disables, then 12 rows and an overflowing list. The scrollbar's look and hover need a picture, so the local WebDriver run shoots it for the judge |
| 4.4 | `styles.test.ts` | Fake UI, visual | ci | Overflow of both lists in the licenses dialog |
| 4.5 | `styles.test.ts` | Fake UI, visual | ci | Overflow of the panel |
| 4.6 | `styles.test.ts` | Fake UI at 1100 × 640, visual | ci | Boxes don't overlap, each block scrolls inside itself |
| 5.1 | `styles.test.ts`, `Notice.test.tsx`, `Snackbar.test.tsx` | Fake UI with `page.clock`, visual, **manual for the motion** | ci, manual | The 4 s, hover and colours are automated. See section 9 |
| 5.2 | `settings.test.ts` | Fake UI, visual | ci | Fixtures say which folders are empty. The test checks that `open_location` is not called |
| 5.3 | `Notice.test.tsx` | WebDriver, visual with `?fail=save_settings` | ci | A real write error from a read-only `settings.json` |
| 5.4 | `styles.test.ts` | Fake UI with reduced motion, WebDriver | ci | On the runner the tests turn Windows' animations off with `SystemParametersInfo(SPI_SETCLIENTAREAANIMATION)` and check `prefers-reduced-motion` in WebView2. Untried. It never runs locally, where it would change the tester's setting |
| 6.1 | `styles.test.ts`, `Inventory.test.tsx` | Fake UI, visual | ci | `aria-pressed` and the look |
| 7.1 | `styles.test.ts`, `ActionDialog.test.tsx`, `dialog.test.ts` | Fake UI, visual | ci | Labels, the locator's tooltip, the copy notice, wrapping at a narrow viewport |
| 7.2 | `running_agent.rs` | Local script with WebDriver | local | Needs Claude Desktop installed and open on the tester's PC. The app runs on the throwaway profile, so only the added home changes |
| 7.3 | `dialog.test.ts`, `delete_backup.rs` | WebDriver | ci | A real Delete backup |
| 7.4 | `ActionDialog.test.tsx`, `styles.test.ts` | Fake UI at 1100 × 640, visual | ci | `SHOTS` already has `action-dialog-many-changes` |
| 7.5 | `styles.test.ts`, `dialog.test.ts`, `HistoryScreen.test.tsx`, `ActionDialog.test.tsx` | Visual, agent judge | ci, local | Classes and contrast are tested. The rest is the look |
| 11.1 | `codex_disable.rs`, `secret_masking.rs` | WebDriver | ci | A real Disable |
| 11.2 | `history.test.ts` | WebDriver | ci | The backup folder is checked on disk. The Explorer window would need UI Automation, untried |
| 11.3 | `history_and_undo.rs` | WebDriver | ci | A real Undo |
| 11.4 | `history.test.ts`, `stale_previews_and_rollback.rs` | WebDriver | ci | The tests edit `config.toml` themselves |
| 11.5 | `stale_previews_and_rollback.rs`, `dialog.test.ts` | WebDriver | ci | `attrib +r` from the tests |
| 11.6 | `history.test.ts`, `acting.test.ts` | WebDriver, fake UI with a delay for the busy notice | ci | Closes the session, edits the manifest, opens a new one |
| 11.7 | `delete_backup.rs` | WebDriver | ci | A real Delete backup |
| 11.8 | `acting.test.ts` | Fake UI with a delay | ci | A busy state |
| 11.9 | `acting.test.ts` | Fake UI with a delay | ci | A busy state |
| 11.10 | `styles.test.ts`, `Snackbar.test.tsx` | Fake UI with `page.clock`, visual, **manual for the motion** | ci, manual | The 10 s, hover, focus ring and colours are automated. See section 9 |
| 11.11 | `history.test.ts` | WebDriver | ci | Uses the History the earlier WebDriver steps built |
| 14.1 | None | WebDriver | ci | Reads the log in the throwaway `LOCALAPPDATA` between sessions. The tests never move the focus |
| 14.2 | None | CLI test, run by the local script | local | Needs the tester's real homes, which CI doesn't have. `config-digger list -v` runs the startup's scan and `config-digger disable --dry-run -v cd-throwaway-codex` the preview's two, with `CODEX_HOME` set to the throwaway Codex home for that process only. Neither writes. Each `scan finished elapsed_ms` must be under 5000. Untried |
| 12.1 | `updates.test.ts`, `update_checks.rs`, `updater_config.rs` | WebDriver | ci | The runner reaches GitHub, so Check now gives the release candidate's real answer |
| 12.2 | `updater_config.rs` | Sandbox with a local endpoint | local | Tried, see section 4. The check of B's CLI version failed only because B kept A's workspace version. B needs it raised |
| 13.1 | `park.rs` | WebDriver, local script with Claude Desktop open, CLI test inside the Sandbox | ci, local | WebDriver on CI, with no Agent running. The local script repeats it with Claude Desktop open, as the step says either way must work. The Sandbox parks one with the CLI for 13.3's warning |
| 13.2 | `installer.rs` | Sandbox, CLI test | local | Tried: app gone, off the PATH, data and choice kept. `config-digger history --json` after the reinstall counts the Operations |
| 13.3 | `installer.rs` parked tests | Sandbox with UI Automation | local | Tried, passed except the parked lines: the run had no parked Entry, and those lines also need the VC++ runtime fix |
| 13.4 | None | Sandbox with UI Automation | local | Tried, passed |
| 13.5 | None | WebDriver and Sandbox, teardown | ci, local | Each run deletes its root and checks it is gone |

By tool: WebDriver checks 23 steps on CI and 7.2 locally, and tears down 13.5. The fake UI checks 35 steps, 12 of them with no other tool, because they have no look to check beyond what their tests assert. The Sandbox checks 9, the local script 4, CLI tests 3, visual regression 25 and the agent judge 4. 34 steps use two tools or more.

## 9. The manual walk that remains

**Target: parts of 3 steps, about 10 minutes**, plus the review of the agent judge's KOs and of changed references in PRs. Today's walk has 69 steps and took hours for rc.2.

| Step | What a person still checks | Why no tool here removes it |
|---|---|---|
| 9.2 | That Windows shows "Windows protegió su PC" for the installer downloaded with a browser, and that Más información and Ejecutar de todas formas get past it | The Sandbox's installer comes from a mapped folder with no Mark of the Web, so SmartScreen never shows there. The issue keeps it manual until the app is signed |
| 5.1 | That the notice slides in and out without a jump | Tests prove the duration, the transform and the fade. Smoothness is how motion looks over time on the real GPU, and stills can't show it to visual regression or the judge |
| 11.10 | That the Undo snackbar slides in and out without a jump | Same as 5.1 |

Neither step needs the rest of the walk first: both work on the throwaway data in the real app.

## 10. Follow-ups

Filed with Parent #38. They are ordered by how many steps each takes out of the walk. The second cuts none by itself: it is what lets the runner skip an automated step, so every other cut reaches the tester only once it lands, and it should land with the first.

1. The real app through WebDriver on CI: 23 steps, 1.2, 10.2 to 10.9, 11.1 to 11.7 among them. It starts by opening a session on `windows-latest`.
2. Automated verdicts in the runner: `automation` and `manual` in the plan, `by` and `evidence` in results, the import when publishing a run, and the review view.
3. The UI on the fake backend: Playwright assertions and `?delay=`. 35 steps, 12 of them with no other tool.
4. The installer, the update and the uninstaller in Windows Sandbox: 9 steps, already tried.
5. Visual regression of the fake backend's screens: the look in 25 steps, once the maintainer decides where the references live.
6. The agent judge: the look the fake backend can't show in 4.1, 4.3, 2.1 and 7.5, the judge's review of new references, and the review view's first users.
7. The local pre-release-candidate script: 7.2, 9.1, 13.1 with Claude Desktop open, and 14.2.
8. Bug: the installer leaves config-digger off the PATH without the VC++ runtime.
9. The release workflow checks the updater signature against the app's public key.

## Sources

- Tauri, WebDriver testing: https://v2.tauri.app/develop/tests/webdriver/
- `tauri-driver`'s capability mapping on Windows: https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-driver/src/server.rs
- Tauri's NSIS script, command-line options (`/P`, `/NS`, `/UPDATE`, `/R`, `/ARGS`) and `$DeleteAppDataCheckboxState`: https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi
- Tauri, Windows installer and NSIS hooks: https://v2.tauri.app/distribute/windows-installer/
- Tauri, updater plugin: endpoints, `dangerousInsecureTransportProtocol`, static JSON, `signer generate`: https://v2.tauri.app/plugin/updater/
- Microsoft, Windows Sandbox overview: restarts since 22H2, one instance, editions: https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/windows-sandbox-overview
- Microsoft, `.wsb` configuration: mapped folders, logon command, `WDAGUtilityAccount`: https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/windows-sandbox-configure-using-wsb-file
- Microsoft, Windows Sandbox command line (`wsb`, Windows 11 24H2): https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/windows-sandbox-cli
- Microsoft Edge WebDriver downloads: https://msedgedriver.microsoft.com/
- Playwright, clock and `emulateMedia`: https://playwright.dev/docs/clock, https://playwright.dev/docs/api/class-page#page-emulate-media
