import { describe, expect, test } from "vitest";
import type { ListChange, ListChanged, Places, Project } from "./bindings";
import { createBusy, type BusyAction } from "./busy";
import { applyChange } from "./changing";
import type { CommandResult } from "./desktop";
import type { Notice } from "./Notice";

const PATH = String.raw`E:\cd-throwaway\project-hide`;
const HIDE: ListChange = { kind: "hide_project", path: PATH };

const project: Project = {
  path: PATH,
  missing: false,
  workspace_file: false,
  members: [],
  registered_by: ["claude-code"],
  added: false,
  hidden: true,
  entries: 0,
  last_opened: [],
};
const places: Places = { read_at: "2026-10-20T09:00:00Z", projects: [project], leftovers: [], stranded: [], store_read_failures: [], agents: [] };

/** A screen whose list changes answer when the test tells them to, with what it showed. */
function screen() {
  let answer: (result: CommandResult<ListChanged>) => void = () => {};
  const asked: ListChange[] = [];
  const shown: BusyAction[][] = [];
  const notices: (Notice | null)[] = [];
  const changing = {
    change: (change: ListChange) => {
      asked.push(change);
      return new Promise<CommandResult<ListChanged>>((resolve) => (answer = resolve));
    },
    busy: createBusy((now) => shown.push(now)),
    notify: (notice: Notice | null) => notices.push(notice),
    places,
  };
  return { changing, asked, notices, busy: () => shown.at(-1) ?? [], answer: (result: CommandResult<ListChanged>) => answer(result) };
}

describe("a list change from the Projects & homes screen", () => {
  test("shows as busy on its row, with no notice left over, as soon as it starts", () => {
    const { changing, asked, notices, busy } = screen();

    void applyChange(HIDE, changing);

    expect(asked).toEqual([HIDE]);
    expect(busy()).toEqual([{ text: `Hiding ${PATH}…`, row: PATH }]);
    expect(notices).toEqual([null]);
  });

  test("stays busy until its command answers with the new lists, then says what it did", async () => {
    const { changing, notices, busy, answer } = screen();

    const done = applyChange(HIDE, changing);
    await Promise.resolve();
    expect(busy()).toHaveLength(1);

    answer({ status: "ok", data: { listed: { kind: "project", project }, places } });

    expect(await done).toBeNull();
    expect(busy()).toEqual([]);
    expect(notices.at(-1)).toEqual({ kind: "success", text: `Hid ${PATH}. Picking it in the Inventory still evaluates it.` });
  });

  test("stops being busy when refused, and hands back why", async () => {
    const { changing, notices, busy, answer } = screen();

    const done = applyChange(HIDE, changing);
    answer({
      status: "error",
      error: { code: "unexpected", message: "The list couldn't be written.", candidates: [], backup: null, diffs: [], failed: null },
    });

    expect(await done).toBe("The list couldn't be written.");
    expect(busy()).toEqual([]);
    expect(notices).toEqual([null]);
  });
});
