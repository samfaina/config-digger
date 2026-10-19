# UAT kit

A manual UAT walk of config-digger runs from a kit in `uat/`. The plan lives in the repo, a runner page shows it one step at a time, and the results land in a database an agent can read.

| File | What it is |
| --- | --- |
| `uat/plan.json` | The walk: ordered sections of steps. |
| `uat/plan.schema.json` | The plan's JSON Schema. |
| `uat/runner.html` | The runner page, published as a private claude.ai Artifact for each run. |
| `uat/runner.test.mjs` | Tests of the runner's logic. Run them with `node --test uat/runner.test.mjs`. |
| `uat/data/cd-throwaway-setup.ps1` | Creates the throwaway Agent homes and Projects the steps use. |

## The plan

`uat/plan.json` has a `title`, the `language` of its text and a list of `sections`. A section has an `id` (`"3"`), a `title` and its `steps`. A step has:

- `id`, such as `"3.2"`: the section's id, a dot and a number.
- `title` and `goal`. The goal says what the step proves.
- `instructions`: one action per line.
- `commands`: `{label, shell, text}`, where `shell` is `powershell`, `cmd` or `bash` and `text` is pasted as is. It may be empty.
- `expect`: what the tester must see for OK.
- `setup` (optional): a command, `{label, shell, text}`, that resets the data before the step. The runner shows it first, under "Antes de empezar".
- `covers`: the issues or spec sections the step checks, such as `"#75"` or `"#38 story 12"`.

Keys are in English. The text is in the tester's language, Spanish today. The runner's own labels are Spanish too.

### Adding or changing steps when a feature lands

1. Add a step for each acceptance criterion a person must see in the running app or installer. Code that tests already cover needs no step. The look of a UI change is checked only here, never by screenshots in the PR (see `docs/agents/issue-tracker.md`).
2. Put it in the section of the screen or flow it walks. Start a new section when none fits.
3. Give it the next free number in its section. **Never renumber a step.** Results and tickets point at step ids, so a step keeps its id when steps are added, moved or removed around it. A removed step's id is not reused. The order of the steps in the file is the order of the walk, so `3.7` may come before `3.4`.
4. Write one action per instruction line, and an `expect` the tester can check by looking: names, counts, the exact message.
5. Use only throwaway data: `cd-throwaway-*` Entries and the fake Secret `not-a-real-secret`. When a step needs data the setup script does not create yet, add it to the script, and use `setup` when the step needs it fresh.
6. List the issue in `covers`.
7. Run `cargo test -p xtask --test uat_plan`. It checks the plan against the schema, that no step id is used twice, that no step asks for a window smaller than the app's smallest (`minWidth` and `minHeight` in `crates/config-digger-app/tauri.conf.json`), and that a step opens the Agent homes tab before it presses Add Agent home.

When a change to the app makes a step wrong, change the step's text and keep its id.

## The test data

```powershell
.\uat\data\cd-throwaway-setup.ps1          # creates E:\cd-throwaway
.\uat\data\cd-throwaway-setup.ps1 -Reset   # deletes it and creates it again
.\uat\data\cd-throwaway-setup.ps1 -Root D:\tmp\cd-throwaway -Reset
```

The script refuses any `-Root` not named `cd-throwaway`, so `-Reset` can never delete another folder. It refuses an existing folder without `-Reset`. `cargo test -p xtask --test uat_data` checks both, on Windows.

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

3. Check the empty database once with `ArtifactData`, `action: "list"`, `collection: "results"`, and give the tester the link. Post it on the walk's issue too.

The tester works in the page. Each saved verdict writes `results/<step id>` at once, so a reload loses nothing. KO and Blocked need notes. A KO or Blocked clicked before the notes are written is the step's pending verdict: its button is outlined, nothing is stored, and a "Guardar KO" or "Guardar bloqueado" button saves it once the notes have text. Clicking OK or leaving the step drops it. The Export button copies the KO and Blocked rows as a Markdown table.

## Turning a run's results into tickets

1. Read the results: `ArtifactData` with the run's `url`, `action: "list"`, `collection: "results"`. Each document is:

   ```json
   { "id": "3.2", "status": "ko", "notes": "…", "time": "2026-10-18T09:30:00.000Z", "version": "0.1.0-rc.2" }
   ```

   `status` is `ok`, `ko` or `blocked`: the tester's verdict on the step. It is not an Entry's State, so a ticket says "KO" or "Blocked", never "status". Match each `id` to its step in `uat/plan.json` at the commit the run was published from.

2. For each `ko` and `blocked` result, check for a duplicate first, as `CLAUDE.md` asks for any reported bug:

   ```sh
   gh issue list --state open --search "<words from the notes>"
   gh pr list --state open --search "<words from the notes>"
   ```

   A finding already filed gets a comment on that issue instead: the step, the notes and the run.

3. File one issue per finding, in the format of `docs/agents/issue-tracker.md` and the vocabulary of `CONTEXT.md`. Start the body with the Parent of the walk's issue, then the line naming the run and step. For the 0.1 walk (#154) the Parent is #38:

   ```markdown
   ## Parent

   #38

   Found in UAT `config-digger 0.1.0-rc.2 UAT`, step `3.2`.

   ## What to build
   …
   ```

   Say what the tester saw (the notes), what the step expected, and what the step's `covers` point at. A Blocked step is filed only when the blocker is a bug. When another step blocked it, say so in that step's issue.

4. Comment on the walk's issue with the list of new issues, grouped by kind (blocks the walk, behaviour, look and feel, and so on), as on #154.
