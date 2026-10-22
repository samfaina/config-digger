import { afterEach, describe, expect, test, vi } from "vitest";
import { commands } from "../bindings";
import { fixtures } from "./fixtures";

// The fake backend stands in for Tauri's `invoke`, as `npm run dev:fake` aliases it.
vi.mock("@tauri-apps/api/core", () => import("./core"));

const PROJECT = String.raw`E:\cd-throwaway\project`;

afterEach(() => {
  vi.unstubAllGlobals();
});

function ok<T>(result: { status: "ok"; data: T } | { status: "error"; error: unknown }): T {
  if (result.status === "error") throw new Error(`a backend error: ${JSON.stringify(result.error)}`);
  return result.data;
}

describe("the fake backend", () => {
  test("answers every command without a backend error", async () => {
    const operation = fixtures.history[0].id;
    const entry = fixtures.inventories[0].inventory.items[0].entries[0];
    const calls: Record<keyof typeof commands, () => Promise<unknown>> = {
      readInventory: () => commands.readInventory(null),
      switchProject: () => commands.switchProject(PROJECT),
      backupsOf: () => commands.backupsOf(entry.path),
      openLocation: () => commands.openLocation(entry.path),
      revealLocation: () => commands.revealLocation(entry.path),
      planAction: () => commands.planAction({ kind: "entry", verb: "remove", locator: entry.locator, project: null }),
      executePlan: () => commands.executePlan("plan-1", []),
      cancelPlan: () => commands.cancelPlan("plan-1"),
      readHistory: () => commands.readHistory(),
      historyDetail: () => commands.historyDetail(operation),
      dismissOperation: () => commands.dismissOperation(operation),
      readPlaces: () => commands.readPlaces(),
      changeList: () => commands.changeList({ kind: "hide_project", path: PROJECT }),
      checkHome: () => commands.checkHome(String.raw`E:\cd-throwaway\claude-desktop-home`, "claude-desktop"),
      checkProject: () => commands.checkProject(String.raw`E:\cd-throwaway\project-extra`),
      pickLocation: () => commands.pickLocation("folder"),
      readSettings: () => commands.readSettings(),
      saveSettings: () => commands.saveSettings({ check_updates_on_startup: true }),
      openDataFolder: () => commands.openDataFolder("backups"),
      readAbout: () => commands.readAbout(),
      openPage: () => commands.openPage("repository"),
      updateStatus: () => commands.updateStatus(),
      startupUpdateCheck: () => commands.startupUpdateCheck(),
      checkForUpdates: () => commands.checkForUpdates(),
      installUpdate: () => commands.installUpdate(),
      openUpdateNotes: () => commands.openUpdateNotes(),
    };

    for (const [name, call] of Object.entries(calls)) {
      const result = (await call()) as { status: string; error?: unknown };
      expect(result, name).toMatchObject({ status: "ok" });
    }
  });

  test("answers the inventory of the Project asked for", async () => {
    const none = ok(await commands.readInventory(null));
    const project = ok(await commands.switchProject(PROJECT));

    expect(none.project).toBeNull();
    expect(project.project).toBe(PROJECT);
    expect(project.items.map((item) => item.name)).toContain("cd-throwaway-project");
    expect(none.items.map((item) => item.name)).not.toContain("cd-throwaway-project");
  });

  test("answers a Project it has no inventory of with the inventory of no Project, in that Project", async () => {
    const many = String.raw`E:\cd-throwaway\many\project-07`;

    const inventory = ok(await commands.switchProject(many));

    expect(inventory.project).toBe(many);
    expect(inventory.items.map((item) => item.name)).toContain("cd-throwaway-codex");
  });

  test("previews the action asked for on the Entry asked for", async () => {
    const codex = fixtures.inventories[0].inventory.items.find((item) => item.name === "cd-throwaway-codex")!;
    const locator = codex.entries[0].locator;

    const preview = ok(await commands.planAction({ kind: "entry", verb: "remove", locator, project: null }));

    expect(preview.preview.action).toBe("remove");
    expect(JSON.stringify(preview.preview)).toContain("cd-throwaway-codex");
  });

  test("changes nothing it answers next when an action is executed", async () => {
    const before = ok(await commands.readHistory());

    const executed = ok(await commands.executePlan("plan-1", []));

    expect(executed.kind).toBe("applied");
    expect(ok(await commands.readHistory())).toEqual(before);
  });

  test("says a data folder holds nothing yet as the throwaway data has it", async () => {
    expect(ok(await commands.openDataFolder("backups"))).toBe(true);
    expect(ok(await commands.openDataFolder("logs"))).toBe(false);
  });

  test("answers an error for each command the page's `fail` parameter names", async () => {
    vi.stubGlobal("location", { search: "?fail=open_location,read_about" });

    const opened = await commands.openLocation(String.raw`E:\cd-throwaway\codex-home\config.toml`);
    const about = await commands.readAbout();

    expect(opened).toMatchObject({ status: "error", error: { code: "unexpected" } });
    expect(about.status).toBe("error");
    expect(ok(await commands.readHistory())).toEqual(fixtures.history);
  });
});
