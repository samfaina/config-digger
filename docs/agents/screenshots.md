# Screenshots of the UI

The app's UI only runs inside the Tauri window, which an agent can't drive. To see a screen it changed, an agent runs the real UI in a browser on a fake backend and takes screenshots there.

## The fake backend

The UI reaches the app only through `ui/src/bindings.ts`, which calls `invoke` from `@tauri-apps/api/core`. In Vite's `fake` mode, that module is aliased to `ui/src/fake/core.ts`, whose `invoke` answers each command from `ui/src/fake/fixtures.ts`. `bindings.ts` stays generated and untouched, and no production bundle holds the fake backend (`ui/build/fakeBackend.test.ts` checks it).

Write commands, such as `plan_action`, `execute_plan` or `save_settings`, answer with a plausible fixture and change nothing: the next read answers as before. `pick_location` picks the throwaway data's extra Project, whose check is recorded.

`?fail=open_location,read_about` in the page's address makes the commands it names answer an error, to see how a screen reports one.

## Run it

From `crates/config-digger-app/ui`:

| Command | What it does |
| --- | --- |
| `npm run dev:fake` | Serves the UI on the fake backend, at the address Vite prints (port 1430, or the next free one). |
| `npm run screenshots` | Writes a PNG of each screen and state to `crates/config-digger-app/ui/screenshots/`, which git ignores. |

`npm run screenshots` opens the UI in a headless Edge, or Chrome when Edge is missing, in a window the size of the app's, as `tauri.conf.json` sets it. It shoots the Inventory with and without an Item selected, the open Project dropdown with and without a search, a tooltip, the action dialog with What changes, and with many changes in the smallest window the app allows, Projects & homes on both tabs and with a search, History with an Operation open, Settings, About, and a notice of each kind. To shoot another state, add a shot to `SHOTS` in `ui/scripts/screenshots.ts`. Open a PNG to check the change by eye.

## Screenshots stay local

The repo is public. Never commit a screenshot, attach one to a PR or an issue, or post one anywhere. The look of a change is signed off in the UAT walk (see `docs/agents/issue-tracker.md` and `docs/agents/uat.md`); screenshots only let an agent check its own work.

## The fixtures

`cargo xtask fake-backend` records `ui/src/fake/fixtures.ts`. It creates the UAT's throwaway data with `uat/data/cd-throwaway-setup.ps1` in a temporary folder, then reads it through the app's own commands, so the DTOs keep their real shape. The core sees only that folder: its environment names no variable, home or registry key of this machine. The recording does what the UAT does by hand: it adds the Claude Desktop home, the second Claude Code home and the extra Project, hides a Project, and disables two servers, so that History holds Operations and Settings a parked Entry. Every path is written as under `E:\cd-throwaway`, the script's default, so the fixtures hold only `cd-throwaway-*` Entries and fake Secrets.

The fixtures are typed by `Fixtures` in `ui/src/fake/core.ts`, so `npm run typecheck` checks them against `bindings.ts`. Record them again after changing a command or an api DTO, after `cargo xtask bindings`, or after changing the throwaway data.
