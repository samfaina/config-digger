# Screenshots of the UI

The app's UI only runs inside the Tauri window, which an agent can't drive. To see a screen it changed, an agent runs the real UI in a browser on a fake backend and takes screenshots there.

## The fake backend

The UI reaches the app only through `ui/src/bindings.ts`, which calls `invoke` from `@tauri-apps/api/core`. In Vite's `fake` mode, that module is aliased to `ui/src/fake/core.ts`, whose `invoke` answers each command from `ui/src/fake/fixtures.ts`. `bindings.ts` stays generated and untouched, and no production bundle holds the fake backend (`ui/build/fakeBackend.test.ts` checks it).

Write commands, such as `plan_action`, `execute_plan` or `save_settings`, answer with a plausible fixture and change nothing: the next read answers as before. `pick_location` picks the throwaway data's extra Project, whose check is recorded.

`?fail=open_location,read_about` in the page's address makes the commands it names answer an error, to see how a screen reports one.

`?delay=read_inventory:2000,read_places:500` holds the answer of each command it names for that many milliseconds, then answers as usual, the error of `?fail=` too, to see a busy state that would last less than a frame. An entry that is not `<command>:<whole milliseconds>` throws an error naming it, so a typo never runs with no delay. Both go in the same address: `?fail=read_about&delay=read_about:500`.

`?untouched=40` makes `execute_plan` fail on Codex's `config.toml`, the first of 40 files, and leave them all untouched, to see a result whose list is taller than the window: no action on the throwaway data touches that many files. Anything but a whole number above 0 throws an error naming it.

## Run it

From `crates/config-digger-app/ui`:

| Command | What it does |
| --- | --- |
| `npm run dev:fake` | Serves the UI on the fake backend, at the address Vite prints (port 1430, or the next free one). |
| `npm run screenshots` | Writes a PNG of each screen and state to `crates/config-digger-app/ui/screenshots/`, which git ignores. |
| `npm run screenshots:compare` | Writes the same PNGs, then compares each with its reference (see "Compare with references"). |
| `npm run screenshots:accept` | Copies the last run's PNGs into `screenshots/reference/`: all of them, or those it names, as in `npm run screenshots:accept -- about settings`. |
| `npm run uat:fake` | Runs the tests of the UAT steps the fake backend can show, with no screenshot (see "The fake UI's tests" in `docs/agents/uat.md`). |

`npm run screenshots` opens the UI in a headless Edge, or Chrome when Edge is missing, in a window the size of the app's, as `tauri.conf.json` sets it. It shoots the Inventory with and without an Item selected, the open Project dropdown with and without a search, a tooltip, the action dialog with What changes, and with many changes in the smallest window the app allows, Projects & homes on both tabs, with a search, scrolled down to its missing Projects, at its Stranded Entries and with its Hidden group open, the action dialog of their Remove all, the result of an action that left 40 files untouched in the smallest window, History with an Operation open, filtered in the smallest window with its status dropdown open, and filtered to nothing, Settings, About, and a notice of each kind, then the Inventory with an Item selected, both tabs of Projects & homes, History with an Operation open, Settings and About again in the smallest window (listed in `SMALLEST_SHOTS`). It also shoots the look of each UAT step that research #236 (section 8 of `docs/research/uat-automation.md`) marks "visual": the loading states, held with `?delay=`, the notices, the dialogs, dropdowns, tooltips and scrollbars of those steps. Each new shot in the smallest window is named `smallest-*`. A spinner or a pulse is shot at its first frame, so a busy state shoots the same each run. To shoot another state, add a shot to `SHOTS` in `ui/scripts/screenshots.ts`. Open a PNG to check the change by eye.

A shot whose state shows a UAT step's look names that step's id in its `uat` field, as in `uat: ["15.1"]`, so a step's look and its shots can be matched. The script fails on an id that is no step of `uat/plan.json`. A shot that covers no step leaves `uat` out. A shot shows what the fake backend can show. The scrollbar of 4.1 as the real WebView2 window draws it, and the other shots of the real window, are the WebDriver walk's, which the agent judge reads ("The agent judge" in `docs/agents/uat.md`).

## Compare with references

`npm run screenshots:compare` shoots every shot, then compares each PNG with the reference of the same name in `screenshots/reference/`, with `pixelmatch`. For each shot it prints one of:

- `match`: no more pixels differ than the threshold.
- `differs`: more pixels differ, with the shot's name and their count. It writes `screenshots/<name>.diff.png`: the reference, faded, with each pixel that differs in red.
- `no reference`: the shot has none yet.
- `size mismatch`: the shot and its reference differ in width or height.

It also lists the references that match no shot, which can be deleted. It exits non-zero when any shot differs, has no reference or has a size mismatch.

The threshold is `THRESHOLD` in `ui/scripts/compare.ts`: 20 pixels, at pixelmatch's default colour distance. It is small because the references are shot on the same machine and browser that compare them, where an unchanged screen matches to the pixel.

The references live in `screenshots/reference/`, one set per machine: a reference shot on another machine doesn't match, since DPI and font smoothing differ. Git ignores them with the rest of `screenshots/`, and they are never committed.

Compare mode never writes a reference. Only `npm run screenshots:accept` does, from the last run's PNGs. That is how a first baseline, or a changed look, becomes a reference. Look at each new or changed shot by eye before accepting it: the agent judge reads only the WebDriver walk's screenshots. A diff says the look changed, not that the new look is right.

## Screenshots stay local

The repo is public. Never commit a screenshot, attach one to a PR or an issue, or post one anywhere. The look of a change is signed off in the UAT walk (see `docs/agents/issue-tracker.md` and `docs/agents/uat.md`); screenshots only let an agent check its own work.

## The fixtures

`cargo xtask fake-backend` records `ui/src/fake/fixtures.ts`. It creates the UAT's throwaway data with `uat/data/cd-throwaway-setup.ps1` in a temporary folder, then reads it through the app's own commands, so the DTOs keep their real shape. The core sees only that folder: its environment names no variable, home or registry key of this machine. The recording does what the UAT does by hand: it adds the Claude Desktop home, the second Claude Code home and the extra Project, hides a Project, and disables two servers, so that History holds Operations and Settings a parked Entry. It then disables the server of a second Claude Desktop home and removes that home from the list, so that Projects & homes lists a Stranded Entry. Every path is written as under `E:\cd-throwaway`, the script's default, so the fixtures hold only `cd-throwaway-*` Entries and fake Secrets.

The fixtures are typed by `Fixtures` in `ui/src/fake/core.ts`, so `npm run typecheck` checks them against `bindings.ts`. Record them again after changing a command or an api DTO, after `cargo xtask bindings`, or after changing the throwaway data.
