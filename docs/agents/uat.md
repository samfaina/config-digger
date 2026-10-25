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

Keys are in English. The text is in the tester's language, Spanish today. The runner's own labels are Spanish too.

### Adding or changing steps when a feature lands

1. Add a step for each acceptance criterion a person must see in the running app or installer. Code that tests already cover needs no step. The look of a UI change is checked only here, never by screenshots in the PR (see `docs/agents/issue-tracker.md`).
2. Put it in the section of the screen or flow it walks. Start a new section when none fits.
3. Give it the next free number in its section. **Never renumber a step.** Results and tickets point at step ids, so a step keeps its id when steps are added, moved or removed around it. A removed step's id is not reused. The order of the steps in the file is the order of the walk, so `3.7` may come before `3.4`. Sections work the same way: a new section takes the next free number wherever it goes in the walk, so section 9 (Instalar) comes before section 1 (Datos de prueba).
4. Write one action per instruction line, and an `expect` the tester can check by looking: names, counts, the exact message. Give each thing to see a check of its own, and put the exact UI text between « and »: the messages, titles, labels and paths the tester must see, not the buttons an action presses. When to mark the step KO or Blocked is an instruction, not a check.
5. Use only throwaway data: `cd-throwaway-*` Entries and the fake Secret `not-a-real-secret`. When a step needs data the setup script does not create yet, add it to the script, and use `setup` when the step needs it fresh.
6. List the issue in `covers`.
7. Run `cargo test -p xtask --test uat_plan`. It checks the plan against the schema, that no step id is used twice, that no step asks for a window smaller than the app's smallest (`minWidth` and `minHeight` in `crates/config-digger-app/tauri.conf.json`), that a step opens the Agent homes tab before it presses Add Agent home, and that each check is one sentence whose « and » pair up.

When a change to the app makes a step wrong, change the step's text and keep its id.

## The test data

```powershell
.\uat\data\cd-throwaway-setup.ps1          # creates E:\cd-throwaway
.\uat\data\cd-throwaway-setup.ps1 -Reset   # deletes it and creates it again
.\uat\data\cd-throwaway-setup.ps1 -Root D:\tmp\cd-throwaway -Reset
.\uat\data\cd-throwaway-setup.ps1 -Reset -Symlink   # adds a skill behind a relative folder symlink
.\uat\data\cd-throwaway-setup.ps1 -Reset -VsCode    # adds VS Code profiles in vscode-appdata
```

`-VsCode` writes VS Code's files as they would be in an `%APPDATA%` of their own, `vscode-appdata`: the default profile, a profile Work with its own `mcp.json`, and a profile Shared that shares the default profile's MCP servers, with the Project last opened in Work and the extra Project in Shared. A step that needs them starts the app with `APPDATA` set to that folder, as step 16.1 does, so that the app never reads the tester's own VS Code profiles.

`-Symlink` needs Developer Mode or admin rights, as creating a symlink does on Windows, so the default data has no symlink. A step whose `setup` adds a switch to `-Reset`, such as step 11.12, gets it from the WebDriver walk too.

The script refuses any `-Root` not named `cd-throwaway`, so `-Reset` can never delete another folder. It refuses an existing folder without `-Reset`. `cargo test -p xtask --test uat_data` checks both, on Windows.

## Automated steps

Most steps run without a tester, from `uat/auto/`, before the run is published. Each tool writes JSON results in the runner's shape (see below), with two more fields the runner ignores: `by`, which names what wrote the verdict (`local` for these tools, `agent-judge` for the agent that judged screenshots), and `evidence`. The WebDriver walk's results also carry `checks`, `looks` and `manual`, which stay on the machine: only the runner's fields, `by` and `evidence` reach a run. Screenshots stay on the machine that took them (`docs/agents/screenshots.md`). `docs/research/uat-automation.md` is the research behind them.

| Tool | Steps | Run |
| --- | --- | --- |
| `uat/auto/webdriver/` | The app's screens: sections 1, 15, 10, 2, 3, 4, 5, 6, 7, 11 and 14, 12.1, 13.1 and 13.5 | `.\uat\auto\webdriver\run.ps1 -App <config-digger-app.exe>` |
| `uat/auto/sandbox/` | A clean PC, install, update and uninstall in Windows Sandbox: 8.1, 9.1 to 9.5, 12.2, 13.2 to 13.4. SmartScreen in 9.2 stays with a person: a file from a mapped folder has no mark of the web. Needs Windows Sandbox, so Windows Pro, Enterprise or Education | `.\uat\auto\sandbox\build-update.ps1 -Commit v<version>`, then `.\uat\auto\sandbox\run.ps1 -Version <version>` |

The WebDriver walk drives the release build through `tauri-driver` and the `msedgedriver` of the installed WebView2, which `run.ps1` downloads. It runs the app on a throwaway Windows profile, a folder named `cd-throwaway-profile` that stands for `USERPROFILE`, `APPDATA` and `LOCALAPPDATA`, so the app never sees the tester's Agent homes or data, and on the throwaway data of `uat/data/cd-throwaway-setup.ps1`. It walks the steps in the plan's order, one module per section in `uat/auto/webdriver/steps/`, and answers the Windows folder picker with UI Automation (`picker.ps1`). Steps 7.2 and 13.1 open Claude Desktop, and close it again if it was closed; without Claude Desktop installed they are Blocked. Step 14.2 measures the scans of the tester's own homes with the CLI next to the app, `list -v` and `disable --dry-run -v`, which write nothing. Step 5.4 emulates Windows' animation effects turned off with `prefers-reduced-motion`, the setting WebView2 takes from them, and never changes the tester's setting. At the end the walk checks that config-digger's real data folders and the real Codex and Claude Desktop configuration are byte for byte as before, and exits with 2 when one changed. To see a busy state that lasts less than a frame, a step holds a command's answer: Tauri's `invoke` can't be wrapped, so `driver.mjs` holds the `fetch` it sends to `http://ipc.localhost/<command>`.

Each step's checks are the plan's `expect`, one by one. What a style can't show, such as a colour seen as a whole or a shadow, is listed in `looks`, and in the result's notes under "A juzgar en capturas", with the screenshot to look at. The walk's `ok` then covers its checks only: before the step reaches a run, the agent that ran the walk reads those screenshots and sets the step's verdict, `ko` with what it saw when a picture breaks the `expect`, and `by: "agent-judge"`. A step whose `manual` field in `steps/` says what a person must still see, such as how a notice moves, stays in the tester's walk. A step that fails before any of its own checks, on its setup or on what an earlier step left, is `blocked`.

When a step leaves the window blank, the walk notes what the page logged and its last commands, opens the app again and runs the step once more.

When a step of the plan changes, change its module in `steps/` with it. `node --test uat/auto/webdriver/walk.test.mjs` checks that every step the walk knows is a step of the plan, that every step of a section it walks has a module, but those the Sandbox walks, and the parts of the walk that need no app.

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

4. Write the automated results with one `ArtifactData` `batch` of `set results/<step id>`, each `{ id, status, notes, time, version, by, evidence }` and nothing else. Leave out the steps that still need a person (`manual`), unless their automated verdict is already KO, which a ticket fixes first; and leave out any step that already has a tester's result, one with no `by`: a tester's verdict is never overwritten. The runner shows each step with a result as done and starts the tester on the first step without one, so the automated steps drop out of the walk.

5. Give the tester the link, and post it on the walk's issue with the steps left to walk.

The tester works in the page. The checks under "Resultado esperado" have boxes to tick while looking; a tick only guides the eye, is never stored and is gone when the step changes. Each saved verdict writes `results/<step id>` at once, so a reload loses nothing. KO and Blocked need notes. A KO or Blocked clicked before the notes are written is the step's pending verdict: its button is outlined, nothing is stored, and a "Guardar KO" or "Guardar bloqueado" button saves it once the notes have text. Clicking OK or leaving the step drops it. Notes on an OK step are optional, and the notes box says they are read too and go in before OK is pressed: pressing OK saves them, and pressing it again saves notes added after. The Export button ("Exportar pasos con notas") copies every step with notes as a Markdown table, whatever its verdict.

## Turning a run's results into tickets

1. Read the results: `ArtifactData` with the run's `url`, `action: "list"`, `collection: "results"`. Each document is:

   ```json
   { "id": "3.2", "status": "ko", "notes": "…", "time": "2026-10-18T09:30:00.000Z", "version": "0.1.0-rc.2" }
   ```

   `status` is `ok`, `ko` or `blocked`: the tester's verdict on the step. It is not an Entry's State, so a ticket says "KO" or "Blocked", never "status". Match each `id` to its step in `uat/plan.json` at the commit the run was published from.

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
