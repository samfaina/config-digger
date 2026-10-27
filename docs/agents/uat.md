# UAT kit

A manual UAT walk of config-digger runs from a kit in `uat/`. The plan lives in the repo, a runner page shows it one step at a time, and the results land in a database an agent can read.

| File | What it is |
| --- | --- |
| `uat/plan.json` | The walk: ordered sections of steps, from a PC without config-digger through install, every screen, update and uninstall. |
| `uat/plan.schema.json` | The plan's JSON Schema. |
| `uat/runner.html` | The runner page, published as a private claude.ai Artifact for each run. |
| `uat/runner.test.mjs` | Tests of the runner's logic and the words of its page. Run them with `node --test uat/runner.test.mjs`. |
| `uat/data/cd-throwaway-setup.ps1` | Creates the throwaway Agent homes and Projects the steps use. |
| `uat/auto/` | The tools that walk most steps without a tester, before a run is published (see [Automated steps](#automated-steps)). |
| `uat/agents/` | Probes that test the built-in definitions' facts against the real Agents, for the release gate. They test the Agents, not config-digger: see `docs/research/release-gate.md`. |

## The plan

`uat/plan.json` has a `title`, the `language` of its text and a list of `sections`. A section has an `id` (`"3"`), a `title` and its `steps`. A step has:

- `id`, such as `"3.2"`: the section's id, a dot and a number.
- `title` and `goal`. The goal says what the step proves.
- `instructions`: one action per line.
- `commands`: `{label, shell, text}`, where `shell` is `powershell`, `cmd` or `bash` and `text` is pasted as is. It may be empty.
- `expect`: what the tester must see for OK, as a list of short checks: one thing to see per check, in one sentence. Exact UI text, such as a message, a title, a label or a path, goes between « and », as in `Sale el aviso «Copied the path.».` The runner shows the checks as a checklist, with that text set apart.
- `setup` (optional): a command, `{label, shell, text}`, that resets the data before the step. The runner shows it first, under "Antes de empezar".
- `covers`: the issues or spec sections the step checks, such as `"#75"` or `"#38 story 12"`.
- `automation` (optional): the tools that walk the step without a tester, each `{tool, where, test}`. `tool` is `fake-ui` (the fake UI's tests), `webdriver` (the WebDriver walk), `sandbox` (the Windows Sandbox walk), `local` (the local script), `cli` (a CLI test), `visual` (a screenshot compared with its reference) or `agent-judge` (the agent that judges the local walk's screenshots). `where` is `ci` or `local`. `test` names the check that covers the step:
  - `webdriver`: `<file> > <step id>`, the step module in `uat/auto/webdriver/steps/`, as in `uat/auto/webdriver/steps/notices.mjs > 5.1`.
  - `sandbox`: `<file> > <step id>`, the `Step` block in a script of `uat/auto/sandbox/`, as in `uat/auto/sandbox/in-sandbox.ps1 > 9.2`, or `uat/auto/sandbox/run.ps1 > 9.1` for the verdict `run.ps1` writes itself.
  - `visual`: a bare shot name of `SHOTS` in `crates/config-digger-app/ui/scripts/screenshots.ts`, as in `inventory`.
  - `agent-judge`: the same as `webdriver`, the step module whose screenshots the judge reads. A step that lists `agent-judge` lists `webdriver` too.
  - The other tools: the same `<file> > <step id>`. Nothing checks them yet: each one's ticket adds its check when it lands.
- `manual` (optional): what a person still checks on the step, and why no tool can, such as how a notice moves. The plan is the only place it lives: the WebDriver walk reads it from here.

A step with `automation` and no `manual` is automated: the runner leaves it out of the tester's queue, whether or not it has a result. A step with `manual` stays in the queue, whatever its `automation`.

Keys are in English. The text is in the tester's language, Spanish today. The runner's own labels are Spanish too.

### Adding or changing steps when a feature lands

1. Add a step for each acceptance criterion a person must see in the running app or installer. Code that tests already cover needs no step. The look of a UI change is checked only here, never by screenshots in the PR (see `docs/agents/issue-tracker.md`).
2. Put it in the section of the screen or flow it walks. Start a new section when none fits.
3. Give it the next free number in its section. **Never renumber a step.** Results and tickets point at step ids, so a step keeps its id when steps are added, moved or removed around it. A removed step's id is not reused. The order of the steps in the file is the order of the walk, so `3.7` may come before `3.4`. Sections work the same way: a new section takes the next free number wherever it goes in the walk, so section 9 (Instalar) comes before section 1 (Datos de prueba).
4. Write one action per instruction line, and an `expect` the tester can check by looking: names, counts, the exact message. Give each thing to see a check of its own, and put the exact UI text between « and »: the messages, titles, labels and paths the tester must see, not the buttons an action presses. When to mark the step KO or Blocked is an instruction, not a check.
5. Use only throwaway data: `cd-throwaway-*` Entries and the fake Secret `not-a-real-secret`. When a step needs data the setup script does not create yet, add it to the script, and use `setup` when the step needs it fresh.
6. List the issue in `covers`.
7. Fill `automation` with each tool that walks the step, and its `test`: when a step module, a `Step` block or a test of the fake UI covers it, list it. Fill `manual` with what a person must still see, when a tool walks the step but can't see all of it. A step no tool walks needs neither.
8. Run `cargo test -p xtask --test uat_plan`. It checks the plan against the schema, that no step id is used twice, that no step asks for a window smaller than the app's smallest (`minWidth` and `minHeight` in `crates/config-digger-app/tauri.conf.json`), that a step opens the Agent homes tab before it presses Add Agent home, that each check is one sentence whose « and » pair up, that each `webdriver`, `agent-judge`, `sandbox` or `visual` test names a step module, a `Step` block or a shot that exists, and that a step that lists `agent-judge` lists `webdriver` too.

When a change to the app makes a step wrong, change the step's text and keep its id.

## The test data

```powershell
.\uat\data\cd-throwaway-setup.ps1          # creates E:\cd-throwaway
.\uat\data\cd-throwaway-setup.ps1 -Reset   # deletes it and creates it again
.\uat\data\cd-throwaway-setup.ps1 -Root D:\tmp\cd-throwaway -Reset
.\uat\data\cd-throwaway-setup.ps1 -Reset -Symlink   # adds a skill behind a relative folder symlink
.\uat\data\cd-throwaway-setup.ps1 -Reset -VsCode    # adds VS Code profiles in vscode-appdata
.\uat\data\cd-throwaway-setup.ps1 -Reset -Cursor    # adds Cursor in cursor-appdata, and a git Project with a nested skill
.\uat\data\cd-throwaway-setup.ps1 -Reset -NameRule  # adds Claude Code skills whose folder names break its name rule
.\uat\data\cd-throwaway-setup.ps1 -Reset -PlainString  # adds a Project whose Claude Code settings hold plain-string MCP lists
```

`-NameRule` adds three skills to the Claude Code home: one whose folder name starts with a space, one with a zero-width space in it, and one with a space inside it, which the rule keeps, all named `cd-throwaway-name…`. They stay out of the default data, so that no other step counts them; step 19.1 walks them.

`-PlainString` adds `project-plain`, whose `.mcp.json` declares four `cd-throwaway-plain-*` servers and whose `.claude\settings.local.json` holds `enabledMcpjsonServers` and `disabledMcpjsonServers` as one plain-string name each, as Claude Code reads them since 2.1.299. Claude Code's state does not record the Project, so that Disable writes its local settings rather than `~/.claude.json`; steps 20.1 and 20.2 add it by hand and remove it at the end.

`-Cursor` writes Cursor's files as they would be in an `%APPDATA%` of their own, `cursor-appdata`: Cursor has run and opened `cursor-project`, a git repo, made with `git init`, whose `packages\app\.cursor\skills` holds a skill. A step that needs them starts the app with `APPDATA` set to that folder, as step 17.1 does.

`-VsCode` writes VS Code's files as they would be in an `%APPDATA%` of their own, `vscode-appdata`: the default profile, a profile Work with its own `mcp.json`, and a profile Shared that shares the default profile's MCP servers, with the Project last opened in Work and the extra Project in Shared. A step that needs them starts the app with `APPDATA` set to that folder, as step 16.1 does, so that the app never reads the tester's own VS Code profiles.

`claude-code-profile`, in the default data, stands for a user folder: its `.claude.json` holds a second leftover of `project-gone`. Step 18.1 starts the app with `USERPROFILE` set to it, so that it is Claude Code's default home, which the app discovers and the tester can hide; a home the user added can't be hidden.

`paths-that-wrap\in-the-smallest-window\of-config-digger`, in the default data, holds `project-long`, a Project Claude Code knows, and the Claude Desktop home, which step 10.3 adds. Under even the shortest root their paths are too long for one line of Projects & homes in the smallest window, so step 10.10 sees where a path wraps.

`-Symlink` needs Developer Mode or admin rights, as creating a symlink does on Windows, so the default data has no symlink. A step whose `setup` adds a switch to `-Reset`, such as step 11.12, gets it from the WebDriver walk too.

The script refuses any `-Root` not named `cd-throwaway`, so `-Reset` can never delete another folder. It refuses an existing folder without `-Reset`. `cargo test -p xtask --test uat_data` checks both, on Windows.

## Automated steps

Most steps run without a tester, from `uat/auto/`, before the run is published. Each tool writes JSON results in the runner's shape (see below), with two more fields: `by`, which names what wrote the verdict (`local` for these tools, `ci` for the fake UI's tests and the WebDriver walk on CI, `agent-judge` for the agent that judged screenshots), and `evidence`. The runner shows `by` as a chip and lists the agent judge's KOs, with their `evidence`, in Revisar. Which steps a tool walks, and what a person still checks on them, is in the plan's `automation` and `manual`. The WebDriver walk's results also carry `checks`, `looks` and `manual`, which stay on the machine: only the runner's fields, `by` and `evidence` reach a run. Screenshots stay on the machine that took them (`docs/agents/screenshots.md`). `docs/research/uat-automation.md` is the research behind them.

| Tool | Steps | Run |
| --- | --- | --- |
| `uat/auto/webdriver/` | The app's screens: sections 1, 15, 10, 2, 3, 4, 5, 6, 7, 11 and 14, 12.1, 13.1 and 13.5 | `.\uat\auto\webdriver\run.ps1 -App <config-digger-app.exe>`; CI runs it on each PR, with no screenshot (see [The WebDriver walk on CI](#the-webdriver-walk-on-ci)) |
| `crates/config-digger-app/ui/uat/` | The steps the UI on the fake backend can show, the "Fake UI" steps of section 8 of `docs/research/uat-automation.md`, on every PR. They catch regressions; they remove no step from the WebDriver walk, whose verdict counts when both have one, as it ran on the release build | `npm run uat:fake` in `crates/config-digger-app/ui`; CI runs it on each PR |
| `uat/auto/sandbox/` | A clean PC, install, update and uninstall in Windows Sandbox: 8.1, 9.1 to 9.5, 12.2, 13.2 to 13.4. SmartScreen in 9.2 stays with a person: a file from a mapped folder has no mark of the web. Needs Windows Sandbox, so Windows Pro, Enterprise or Education | `.\uat\auto\sandbox\build-update.ps1 -Commit v<version>`, then `.\uat\auto\sandbox\run.ps1 -Version <version>` |

The WebDriver walk drives the release build through `tauri-driver` and the `msedgedriver` of the installed WebView2, which `run.ps1` downloads. It runs the app on a throwaway Windows profile, a folder named `cd-throwaway-profile` that stands for `USERPROFILE`, `APPDATA` and `LOCALAPPDATA`, so the app never sees the tester's Agent homes or data, and on the throwaway data of `uat/data/cd-throwaway-setup.ps1`. It walks the steps in the plan's order, one module per section in `uat/auto/webdriver/steps/`, and answers the Windows folder picker with UI Automation (`picker.ps1`). Steps 7.2 and 13.1 open Claude Desktop, and close it again if it was closed. Step 7.6 opens a second Claude Desktop with `--user-data-dir` on the throwaway Claude Desktop home, and closes only that one. Without Claude Desktop installed, these steps are Blocked. Step 14.2 measures the scans of the tester's own homes with the CLI next to the app, `list -v` and `disable --dry-run -v`, which write nothing. Step 14.3 adds to its notes the `plan finished` line of the same `disable --dry-run -v` on the tester's own homes. Step 5.4 emulates Windows' animation effects turned off with `prefers-reduced-motion`, the setting WebView2 takes from them, and never changes the tester's setting. At the end the walk checks that config-digger's real data folders and the real Codex and Claude Desktop configuration are byte for byte as before, and exits with 2 when one changed. To see a busy state that lasts less than a frame, a step holds a command's answer: Tauri's `invoke` can't be wrapped, so `driver.mjs` holds the `fetch` it sends to `http://ipc.localhost/<command>`.

Each step's checks are the plan's `expect`, one by one. What a style can't show, such as a colour seen as a whole or a shadow, is listed in `looks`, and in the result's notes under "A juzgar en capturas", with the screenshot to look at. The walk's `ok` then covers its checks only: before the step reaches a run, [the agent judge](#the-agent-judge) reads those screenshots and gives the step its verdict. A step whose `manual` in the plan says what a person must still see, such as how a notice moves, stays in the tester's walk, and the walk ends its notes with that line, "Queda para una persona: …". A step module never holds `manual`. A step that fails before any of its own checks, on its setup or on what an earlier step left, is `blocked`.

When a step leaves the window blank, the walk notes what the page logged and its last commands, opens the app again and runs the step once more.

When a step of the plan changes, change its module in `steps/` with it. `node --test uat/auto/webdriver/walk.test.mjs` checks that every step the walk knows is a step of the plan, that every step of a section it walks has a module, but those the Sandbox walks, that each step it walks lists `webdriver` in its `automation` with a `test` ending in its id, that no step module holds `manual`, and the parts of the walk that need no app.

### The agent judge

The agent judge reads the screenshots of the local WebDriver walk and writes each step's verdict on its look into the walk's results. It runs after the walk, on the PC that ran it, and before [Publishing a run](#publishing-a-run). It never runs on CI, which takes no screenshot.

It takes every result of the walk whose `looks` is not empty, and every step whose plan `automation` lists `agent-judge`, but a result it already judged. A step that lists `agent-judge` lists `webdriver` too, since its screenshots come from that walk: `uat_plan` checks it.

`uat/auto/webdriver/judge.mjs` does the mechanical half, and the model only does the looking. `--work` is the `-Work` of `run.ps1`, `$env:TEMP\config-digger-uat` by default, whose `webdriver.json` holds the walk's results and `shots` its screenshots.

- `node uat/auto/webdriver/judge.mjs list [--work <dir>]` prints the steps to judge, as JSON. Each has its step's `title`, `goal`, `instructions` and `expect` from `uat/plan.json` in the checkout, which is the commit under test; `walk`, the walk's own `status`, `checks` and `notes`; and `looks`, each `{ what, file }` with the PNG's absolute path, and `missing: true` when it is not on disk. A step the judge can't judge has `unjudged`, saying why: the walk left it Blocked, took no screenshot of it, or one of its screenshots is missing.
- `node uat/auto/webdriver/judge.mjs merge --verdicts <file> [--work <dir>]` writes the judge's verdicts into `webdriver.json` and prints how many are OK, KO and Blocked. The file holds one entry per listed step, `{ id, status, notes, evidence }`. `node --test uat/auto/webdriver/judge.test.mjs` tests both modes.

The verdict on a step:

- `ok` when the walk's status was `ok`, and every `looks` entry and every part of `expect` a picture can show holds in the screenshots.
- `ko` when a screenshot breaks one. The notes name the part of `expect`, what the picture shows instead, and the screenshot's file name, as in "Delete backup en History es violeta, no rojo: expect dice que los cuatro botones comparten un rojo. Captura 7.5-2-botones.png."
- `blocked` when a screenshot a `looks` entry names is missing on disk, the walk took none of a step that lists `agent-judge`, or the walk's status was `blocked`. The judge also writes `blocked` when a screenshot is there but doesn't show what its `looks` entry names, such as when a notice covers it or the list is scrolled away from it. Its notes say what the shot shows instead, so that the step module takes the shot again.
- A walk `ko` stays `ko`, never `ok` or `blocked`, with the judge's findings on the look added to its notes.
- What a picture can't show, such as timing, motion, or text a check already asserted, the judge leaves alone. One line of its notes says so; it never guesses.

The merge replaces each listed step's result with the judge's `status`, `by: "agent-judge"` and `evidence`: the absolute path of the screenshot that decided the verdict, or of those the judge looked at for an `ok`, joined by `; `. The walk's notes stay, and the judge's lines follow under a line of their own, "Juez:". The other fields stay as the walk wrote them, so step 4 of Publishing a run imports them unchanged. The merge enforces the rules it can: a walk `ko` stays `ko`, and a step listed with `unjudged` merges as `blocked` whatever the judge said, or `ko` after a walk `ko`, with a line "Bloqueado: …" or "Sin juzgar: …" saying why, and the missing screenshot as its `evidence`, or the walk's own when none is missing. It refuses the whole file, and writes nothing, on a verdict for a step it didn't list, a listed step with no verdict or with two, a status other than `ok`, `ko` or `blocked`, a `ko` or `blocked` without notes on a step the judge could judge, or an `evidence` that names anything but the step's own screenshots. It takes a screenshot's path with either slash and in any case, and writes it as the listing does.

To judge a walk, the agent that ran it follows this prompt, or gives it to a subagent:

```text
Judge the look of the UAT steps of the local WebDriver walk in <work folder>.
1. Run `node uat/auto/webdriver/judge.mjs list --work <work folder>` from the repo root.
2. For each step it lists:
   - With `unjudged`: open nothing, and write the verdict `blocked`, with "" as its notes and evidence. The merge
     writes why, and keeps a walk `ko` as `ko`.
   - Otherwise, open each PNG of its `looks` with the Read tool. Check each `looks` entry's `what`, and each part of
     `expect` a picture can show, against the screenshots. Read `walk` for what the walk's checks already asserted.
   - When a screenshot doesn't show what its `looks` entry names, write `blocked`, saying what it shows instead.
   - Write `ok` when everything a picture can show holds, else `ko`, naming in the notes, in Spanish, the part of
     `expect`, what the picture shows instead and the screenshot's file name. Add one line naming what no picture can
     show, such as timing or motion, and do not guess it.
   - `evidence` is the absolute path of the screenshot that decided a `ko`, or of those you looked at for an `ok`,
     joined by "; ". Never an image.
3. Write the verdicts, one `{ id, status, notes, evidence }` per listed step, as a JSON array to a file in your
   scratchpad, and run `node uat/auto/webdriver/judge.mjs merge --work <work folder> --verdicts <that file>`.
4. Report the line merge prints. Never commit, attach, upload or post a screenshot.
```

The screenshots are read only by the session's model, through the Read tool. They are never committed, attached to an issue or a PR, uploaded to an Artifact, or sent to any other service, as `docs/agents/screenshots.md` asks of every screenshot. `evidence` is a path on the PC of the walk, never an image, and the run's "Revisar" shows that path. A PR that reports a judged walk gives the counts, never a screenshot.

### The WebDriver walk on CI

The `uat-webdriver` job in `.github/workflows/ci.yml` runs the same walk, over the same step modules, on `windows-latest`, on every PR and push to `main`. It builds the UI and the release app as a release does, `cargo app-release`, with the CLI next to it, installs `tauri-driver`, and maps `E:` with `subst` so that the plan's `E:\cd-throwaway` paths hold. `run.ps1 -Ci` picks the `msedgedriver` of the runner's WebView2 as a local run does, from the EdgeUpdate registry key, prints the version it took, and fails naming that version when no driver is published for it. A runner runs elevated, and an elevated app ignores the `WEBVIEW2_*` variables in which `msedgedriver` hands WebView2 its debugging port and the user data folder where it reads `DevToolsActivePort`, so the session would fail with `DevToolsActivePort file doesn't exist`. When it runs elevated, `run.ps1` puts a fixed debugging port, `--remote-debugging-port=9222`, in the HKLM policy `SOFTWARE\Policies\Microsoft\Edge\WebView2\AdditionalBrowserArguments` for the app's executable alone, which an elevated app honours, and puts back what was there when the walk ends. The walk then starts the app itself, and `msedgedriver` attaches to that port with `debuggerAddress`; closing the session closes the app. The app runs on the throwaway profile and data, and the byte-for-byte check runs at the end.

With `-Ci` (`walk.mjs --ci --evidence <run URL>`) the walk never calls WebDriver's screenshot endpoint, not even when a step fails. A step with looks gets its verdict from its checks alone, and its notes list the looks under "Sin juzgar en CI": the tester judges them in their walk. A step whose module sets `ciLacks` is Blocked without running, with the notes naming what the runner lacks: 14.2, which measures a tester's own homes. Steps 7.2, 7.6 and 13.1 are Blocked as on any PC without Claude Desktop. Steps 5.1 and 11.10 keep the `manual` part the plan gives them for a person.

The job uploads `uat-results.json` as the `uat-webdriver-results` artifact, and nothing else: one document per step, `{ id, status, notes, time, version, by: "ci", evidence }`, with the run's URL as `evidence` and no `checks`, `looks` or `manual`. The job summary has one row per step with its verdict and, for a KO or a Blocked, the first line of its notes. The verdicts are a report: a step's KO or Blocked leaves the job green. The job turns red only when the walk itself breaks: the build, the setup or the driver failing, the app's session not opening, which still writes `uat-results.json` with every step Blocked and why, `tauri-driver` stopping during the walk, the walk stopping before its last step, or the byte-for-byte check finding a change outside the throwaway root and profile.

### The fake UI's tests

`npm run uat:fake` serves the UI on the fake backend (`docs/agents/screenshots.md`) and runs one Playwright test per step in `crates/config-digger-app/ui/uat/`, in a headless Edge, or Chrome, at the app's window size. A test is named after its step's id, `uatStep("15.1", …)`, and an id that is no step of `uat/plan.json` fails the run. Each `check` names one check of the step's `expect` by its text and asserts it: the quoted text, the focus after a key press, the boxes at the smallest window where the step asks for it, the 4 s notice and the 10 s Undo snackbar on `page.clock`. A test fails unless it checks every check of its step or `leave`s it to a person with the reason, such as a motion, or the state after a write, which the fake backend never changes. A busy state is held with the page's `?delay=` on the command the step waits on. When a step of the plan changes, change its test with it: a check whose text changed fails the test.

The run writes `crates/config-digger-app/ui/uat-results.json`, one document per step, `{ id, status, notes, time, version, by: "ci", evidence }`: `ok`, with the checks left to a person in the notes; `ko`, with the failed check and the assertion's message; or `blocked` when the test failed before its first check, such as on loading the page. `version` is the workspace version, and `evidence` is the CI run's URL, or the test's name outside CI. The CI job uploads the file as the `uat-results` artifact, and nothing else: no test takes a screenshot.

## Publishing a run

A run is one private Artifact, with its own database of results. Publish a new one for each release candidate walked.

1. Write `run.json` in your scratchpad, with the run's name and the app version under test:

   ```json
   { "name": "config-digger 0.1.0-rc.2 UAT", "version": "0.1.0-rc.2" }
   ```

   The runner shows the name and stores the version in every result.

2. Publish `uat/runner.html` with the Artifact tool:
   - `files`: `{"plan.json": "uat/plan.json", "run.json": "<scratchpad>/run.json"}`.
   - `capabilities`: `{"db": {}, "user": {}}`.
   - `description`: one sentence naming the release candidate.
   - `icon`: `checklist`.

   A publish without `url` makes a new Artifact. To fix the page or the plan of the same run, publish again to its `url`, so its results stay.

3. Check the empty database once with `ArtifactData`, `action: "list"`, `collection: "results"`.

4. Import the automated results: one `ArtifactData` `batch` of `set results/<step id>`, each `{ id, status, notes, time, version, by, evidence }` and nothing else. This import is the only way an automated verdict reaches the database: no tool writes to a run. The `local` verdicts are the files of the local walks, the WebDriver walk's after [the agent judge](#the-agent-judge) merged its verdicts, and the `ci` verdicts are the `uat-results.json` the CI jobs of the release candidate's commit upload (see above). Write a result only for a step id in the plan at that commit.
   - Leave out the steps that still need a person, those with `manual` in the plan, unless their automated verdict is already KO, which a ticket fixes first.
   - Leave out any step that already has a tester's result, one with no `by`: a tester's verdict is never overwritten.
   - When two tools have a verdict for the same step, write one. The order is a tester's verdict, then `agent-judge`, then `local`, then `ci`: the local walk ran on the release build, and the agent judge's verdict replaces that step's `local` one. When the written verdict is `local` or `agent-judge` and the `ci` one has another status, add a line to the notes naming it and its notes, `CI: <status>: <its notes>`, with each line break of those notes a ` / `.

   For example, with a `local` verdict and no tester's result:
   - `local` `ok` and `ci` `ok` on 10.5: write the `local` one as it is.
   - Only `ci` `ok` on 15.1, the local walk having stopped before it: write the `ci` one.
   - `local` `ok` and `ci` `ko` "KO: sale «3 Projects»: «2 Projects», no «3 Projects»" on 2.2: write the `local` `ok`, its notes ending in `CI: ko: KO: sale «3 Projects»: «2 Projects», no «3 Projects»`, so the tester sees that a CI test failed on a step the release build passed.

   The runner leaves each automated step, one with `automation` and no `manual`, out of the tester's queue, with or without a result, and starts the tester on the first step left to walk.

5. Give the tester the link, and post it on the walk's issue with the steps left to walk.

The tester works in the page. The checks under "Resultado esperado" have boxes to tick while looking; a tick only guides the eye, is never stored and is gone when the step changes. Each saved verdict writes `results/<step id>` at once, so a reload loses nothing. KO and Blocked need notes. A KO or Blocked clicked before the notes are written is the step's pending verdict: its button is outlined, nothing is stored, and a "Guardar KO" or "Guardar bloqueado" button saves it once the notes have text. Clicking OK or leaving the step drops it. Notes on an OK step are optional, and the notes box says they are read too and go in before OK is pressed: pressing OK saves them, and pressing it again saves notes added after. The Export button ("Exportar pasos con notas") copies every step with notes as a Markdown table, whatever its verdict, with a column "Por" naming what wrote a tool's verdict, empty for a tester's.

An automated step is marked "Automático" on its page and in the list, "Auto" there once it has a result. The tester can still open it from the list, walk it and save a verdict, which replaces the tool's. A tool's verdict and its notes show on the step apart from the tester's notes, which start empty. The counts tell the steps left to walk ("Pendientes") apart from the automated steps with no result ("Automáticos sin resultado"): a tool that did not run shows there, rather than dropping out. A result a tool wrote shows a chip naming it: "CI", "Local" or "Agente juez". A tester's shows none.

"Revisar KO del agente juez" lists every result with `status: "ko"` and `by: "agent-judge"`, in plan order: the step, the judge's notes, and the screenshot's path on the PC of the local walk, to copy, never the picture. A verdict saved there, with the same rule that KO and Blocked need notes, is a tester's result, with no `by`, that keeps the judge's verdict as `judged: { status, notes }`, and the step leaves the list. A judge's OK is not listed.

## Turning a run's results into tickets

1. Read the results: `ArtifactData` with the run's `url`, `action: "list"`, `collection: "results"`. Each document is:

   ```json
   { "id": "3.2", "status": "ko", "notes": "…", "time": "2026-10-18T09:30:00.000Z", "version": "0.1.0-rc.2" }
   ```

   An automated verdict also has `by` and `evidence`, and a tester's verdict saved over the agent judge's has `judged`, the judge's `{ status, notes }`.

   `status` is `ok`, `ko` or `blocked`: the verdict on the step, a tester's when there is no `by`. It is not an Entry's State, so a ticket says "KO" or "Blocked", never "status". Match each `id` to its step in `uat/plan.json` at the commit the run was published from.

2. Read every result with notes: each `ko` and `blocked` one, and each `ok` one whose `notes` are not empty. Notes on an OK step are findings as often as not: the step passed, but the tester saw something worth fixing on the way. Each finding, whatever the step's verdict, goes through steps 3 and 4. A note that reports nothing to fix, such as "todo bien", needs neither a ticket nor a comment.

3. For each finding, check for a duplicate first, as `CLAUDE.md` asks for any reported bug:

   ```sh
   gh issue list --state open --search "<words from the notes>"
   gh pr list --state open --search "<words from the notes>"
   ```

   A finding already filed gets a comment on that issue instead: the step, the notes and the run.

4. File one issue per finding, in the format of `docs/agents/issue-tracker.md` and the vocabulary of `CONTEXT.md`. Start the body with the Parent of the walk's issue, then the line naming the run and step. For the 0.1 walk (#154) the Parent is #38:

   ```markdown
   ## Parent

   #38

   Found in UAT `config-digger 0.1.0-rc.2 UAT`, step `3.2`.

   ## What to build
   …
   ```

   Say what the tester saw (the notes), what the step expected, and what the step's `covers` point at. For a note on an OK step, say the step passed and the note is what is left to fix. A Blocked step is filed only when the blocker is a bug. When another step blocked it, say so in that step's issue.

5. Comment on the walk's issue with the list of new issues, grouped by kind (blocks the walk, behaviour, look and feel, and so on), as on #154.
