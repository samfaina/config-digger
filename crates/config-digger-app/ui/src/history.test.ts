import { describe, expect, test } from "vitest";
import type { Error as ApiError, FileDiff, OperationDetail, OperationSummary } from "./bindings";
import {
  filterOperations,
  NO_FILTER,
  FILTER_STATUSES,
  FILTER_VERBS,
  operationActions,
  operationCount,
  type OperationFilter,
  operationRow,
  operationTime,
  recoveryBanner,
  selectedOperation,
  undoPress,
  undoRefusal,
} from "./history";

function summary(overrides: Partial<OperationSummary> = {}): OperationSummary {
  return {
    id: "20261002-095812-a3f",
    created_at: "2026-10-02T09:58:12Z",
    action: "remove",
    title: "Remove 1 Entry",
    status: "applied",
    status_text: "Applied",
    cause: null,
    needs_attention: false,
    undo_of: null,
    undone_by: null,
    backup_deleted: false,
    backup_deletable: true,
    agents: ["Claude Code"],
    entries: ["notes"],
    locations: [String.raw`C:\Users\me\.claude.json`],
    undoable: true,
    not_undoable: null,
    ...overrides,
  };
}

describe("an Operation's row", () => {
  test("shows its title, Agents, locations, number and the status the core renders", () => {
    const row = operationRow(
      summary({
        agents: ["Claude Code", "VS Code"],
        locations: [String.raw`C:\Users\me\.claude.json`, String.raw`E:\app\.mcp.json`],
      }),
    );

    expect(row).toMatchObject({
      title: "Remove 1 Entry",
      meta: "Claude Code, VS Code · 2 locations · Operation 20261002-095812-a3f",
      status: { label: "Applied", tone: "applied" },
    });
  });

  test("names one location in the singular, and no Agent when it changed no State", () => {
    const row = operationRow(summary({ action: "delete_backup", agents: [] }));

    expect(row.meta).toBe("No Agent · 1 location · Operation 20261002-095812-a3f");
  });

  test("tones its status by what became of it", () => {
    const tone = (overrides: Partial<OperationSummary>) => operationRow(summary(overrides)).status.tone;

    expect(tone({ undone_by: "20261002-101500-b10", status_text: "Undone by 20261002-101500-b10" })).toBe("undone");
    expect(tone({ undo_of: "20261001-080000-c20", status_text: "Undo of 20261001-080000-c20" })).toBe("undo");
    expect(tone({ status: "failed", status_text: "Failed, rolled back" })).toBe("failed");
    expect(tone({ status: "in_progress", status_text: "Incomplete" })).toBe("failed");
    expect(tone({ status: "needs_attention", needs_attention: true, status_text: "Needs attention" })).toBe(
      "attention",
    );
    expect(
      tone({ status: "needs_attention", needs_attention: false, status_text: "Rollback incomplete, dismissed" }),
    ).toBe("failed");
  });
});

describe("an Operation's time", () => {
  // Local times, as the user's clock shows them.
  const now = new Date(2026, 9, 2, 12, 0);
  const at = (...parts: [number, number, number, number, number]) => new Date(...parts).toISOString();

  test("is the time of day for today and yesterday", () => {
    expect(operationTime(at(2026, 9, 2, 9, 58), now)).toBe("Today 09:58");
    expect(operationTime(at(2026, 9, 1, 17, 20), now)).toBe("Yesterday 17:20");
  });

  test("is the date before that, with the year when it is another", () => {
    expect(operationTime(at(2026, 8, 30, 9, 12), now)).toBe("Sep 30 09:12");
    expect(operationTime(at(2025, 11, 31, 23, 5), now)).toBe("Dec 31 2025 23:05");
  });
});

describe("what can be done to an Operation", () => {
  test("an applied one offers Undo and Delete backup", () => {
    expect(operationActions(summary())).toEqual({
      undo: { offered: true, refusal: null },
      deleteBackup: true,
      dismiss: false,
    });
  });

  test("Undo stays offered when its files changed since, saying it would be refused", () => {
    const refused = String.raw`\`mcpServers.notes\` in C:\Users\me\.claude.json changed since the Operation`;

    expect(operationActions(summary({ undoable: false, not_undoable: refused })).undo).toEqual({
      offered: true,
      refusal: refused,
    });
  });

  test("a failed one offers no Undo, and says why", () => {
    const why = "It failed and was rolled back, so there is nothing to undo.";

    expect(
      operationActions(summary({ status: "failed", status_text: "Failed, rolled back", undoable: false, not_undoable: why }))
        .undo,
    ).toEqual({ offered: false, why });
  });

  test("one already undone, or whose backup was deleted, offers no Undo", () => {
    const undone = summary({
      undone_by: "20261002-101500-b10",
      undoable: false,
      not_undoable: "It was undone by 20261002-101500-b10; undo that Operation to apply it again.",
    });
    const deleted = summary({
      backup_deleted: true,
      backup_deletable: false,
      undoable: false,
      not_undoable: "Its backup was deleted, with the values its Undo would put back.",
    });

    expect(operationActions(undone).undo).toMatchObject({ offered: false });
    expect(operationActions(deleted)).toMatchObject({
      undo: { offered: false, why: "Its backup was deleted, with the values its Undo would put back." },
      deleteBackup: false,
    });
  });

  test("one whose backup deletion left copies behind offers Delete backup again", () => {
    const left = summary({
      backup_deleted: true,
      backup_deletable: true,
      undoable: false,
      not_undoable: "Its backup was deleted, with the values its Undo would put back.",
    });

    expect(operationActions(left)).toMatchObject({ undo: { offered: false }, deleteBackup: true });
  });

  test("one that needs attention offers Dismiss and Delete backup, but no Undo", () => {
    const attention = summary({
      status: "needs_attention",
      needs_attention: true,
      undoable: false,
      not_undoable: "Its Rollback is incomplete; recover its files from its backup by hand.",
    });

    expect(operationActions(attention)).toEqual({
      undo: { offered: false, why: "Its Rollback is incomplete; recover its files from its backup by hand." },
      deleteBackup: true,
      dismiss: true,
    });
  });
});

describe("the manual recovery banner", () => {
  const attention = (id: string) =>
    summary({ id, status: "needs_attention", needs_attention: true, status_text: "Needs attention" });

  test("shows while an Operation needs attention", () => {
    expect(recoveryBanner([summary(), attention("20261002-101500-b10")])).toBe("An Operation needs manual recovery");
    expect(recoveryBanner([attention("20261002-101500-b10"), attention("20261001-080000-c20")])).toBe(
      "2 Operations need manual recovery",
    );
  });

  test("goes once each is dismissed", () => {
    const dismissed = summary({
      status: "needs_attention",
      needs_attention: false,
      status_text: "Rollback incomplete, dismissed",
    });

    expect(recoveryBanner([summary(), dismissed])).toBeNull();
    expect(recoveryBanner([])).toBeNull();
  });
});

describe("a refused Undo", () => {
  const backup = String.raw`C:\Users\me\AppData\Local\config-digger\backups\20261002-095812-a3f`;
  const diff: FileDiff = {
    path: String.raw`C:\Users\me\.claude.json`,
    diff: null,
    diff_unavailable: "parse",
    diff_note: "Can't show a masked diff: the file as it is now does not parse as JSON.",
    changed_lines: 2,
  };
  const error = (overrides: Partial<ApiError>): ApiError => ({
    code: "unexpected",
    message: "",
    candidates: [],
    backup: null,
    diffs: [],
    failed: null,
    ...overrides,
  });

  test("shows the reason, the diffs from the backup and the backup folder", () => {
    const refused = error({
      code: "undo_refused",
      message: String.raw`Undo refused: \`mcpServers.notes\` in C:\Users\me\.claude.json changed since the Operation.`,
      backup,
      diffs: [diff],
    });

    expect(undoRefusal(refused)).toEqual({
      reason: String.raw`Undo refused: \`mcpServers.notes\` in C:\Users\me\.claude.json changed since the Operation.`,
      backup,
      diffs: [diff],
    });
  });

  test("is only an Undo the core refused for what changed, not another failure to plan", () => {
    expect(undoRefusal(error({ code: "blocked", message: "Operation x cannot be undone" }))).toBeNull();
    expect(undoRefusal(error({ code: "undo_refused", message: "refused", backup: null }))).toBeNull();
  });
});

describe("pressing Undo", () => {
  const backup = String.raw`C:\Users\me\AppData\Local\config-digger\backups\20261002-095812-a3f`;
  const reason = String.raw`Undo refused: \`mcpServers.notes\` in C:\Users\me\.claude.json changed since the Operation.`;
  const diff: FileDiff = {
    path: String.raw`C:\Users\me\.claude.json`,
    diff: "@@ -1 +1 @@\n-a\n+b\n",
    diff_unavailable: null,
    diff_note: null,
    changed_lines: 2,
  };
  const error = (overrides: Partial<ApiError>): ApiError => ({
    code: "unexpected",
    message: "",
    candidates: [],
    backup: null,
    diffs: [],
    failed: null,
    ...overrides,
  });
  const refused = summary({ undoable: false, not_undoable: reason });
  const detail = (overrides: Partial<OperationDetail> = {}): OperationDetail => ({
    summary: refused,
    project: null,
    changes: [],
    locations: [],
    backup,
    cause: null,
    rollback: [],
    verify: null,
    differences: [],
    undo_refusal: null,
    ...overrides,
  });

  test("shows the refusal its detail already holds at once, with the diffs from the backup, and opens no dialog", () => {
    const press = undoPress(refused, detail({ undo_refusal: error({ code: "undo_refused", message: reason, backup, diffs: [diff] }) }));

    expect(press).toEqual({ kind: "refused", refusal: { reason, backup, diffs: [diff] } });
  });

  test("shows a block its files already meet at once, with the Operation's backup folder", () => {
    const blocked = String.raw`\`mcpServers.notes\` in C:\Users\me\.claude.json appeared since Operation 20261002-095812-a3f, and the Undo would overwrite it`;

    const press = undoPress(refused, detail({ undo_refusal: error({ code: "blocked", message: blocked }) }));

    expect(press).toEqual({ kind: "refused", refusal: { reason: blocked, backup, diffs: [] } });
  });

  test("opens the dialog on the Undo's preview when its detail holds no refusal", () => {
    expect(undoPress(summary(), detail({ summary: summary() }))).toEqual({ kind: "dialog" });
  });

  test("waits for the detail while it is read, when History already says the Undo would be refused", () => {
    expect(undoPress(refused, null)).toEqual({ kind: "reading" });
    expect(undoPress(summary(), null)).toEqual({ kind: "dialog" });
  });

  test("opens the dialog when the detail couldn't be read, so that planning the Undo says why it is refused", () => {
    expect(undoPress(refused, "unread")).toEqual({ kind: "dialog" });
  });

  test("opens the dialog when checking the Undo failed otherwise than by a refusal or a block", () => {
    const unexpected = error({ code: "unexpected", message: "cannot read the manifest" });

    expect(undoPress(refused, detail({ undo_refusal: unexpected }))).toEqual({ kind: "dialog" });
  });
});

describe("the Operation the detail pane shows", () => {
  const newest = summary({ id: "20261002-101500-b10" });
  const older = summary({ id: "20261001-080000-c20" });

  test("is the one selected while History lists it", () => {
    expect(selectedOperation([newest, older], "20261001-080000-c20")).toBe(older);
  });

  test("is the newest when none is selected, or the one selected is gone", () => {
    expect(selectedOperation([newest, older], null)).toBe(newest);
    expect(selectedOperation([newest, older], "20260901-000000-0aa")).toBe(newest);
    expect(selectedOperation([], null)).toBeNull();
  });
});

describe("filtering History", () => {
  const disabled = summary({ id: "20261002-101500-b10", action: "disable", title: "Disable 1 Entry" });
  const removed = summary({ id: "20261002-095812-a3f", action: "remove" });
  const undo = summary({ id: "20261002-110000-d30", action: "undo", undo_of: "20261002-101500-b10" });
  const ids = (operations: OperationSummary[]) => operations.map((operation) => operation.id);

  test("lists every Operation with no filter", () => {
    expect(ids(filterOperations([undo, disabled, removed], NO_FILTER))).toEqual([
      "20261002-110000-d30",
      "20261002-101500-b10",
      "20261002-095812-a3f",
    ]);
  });

  test("by verb lists only the Operations of that verb", () => {
    expect(ids(filterOperations([undo, disabled, removed], { ...NO_FILTER, verb: "disable" }))).toEqual([
      "20261002-101500-b10",
    ]);
    expect(ids(filterOperations([undo, disabled, removed], { ...NO_FILTER, verb: "undo" }))).toEqual([
      "20261002-110000-d30",
    ]);
  });

  test("by status lists only the Operations with that status, an applied Undo among the applied ones", () => {
    const undone = summary({ id: "20261002-101500-b10", undone_by: "20261002-110000-d30", status_text: "Undone by 20261002-110000-d30" });
    const failed = summary({ id: "20261001-090000-e40", status: "failed", status_text: "Failed, rolled back" });
    const interrupted = summary({ id: "20261001-080000-f50", status: "in_progress", status_text: "Incomplete" });
    const attention = summary({ id: "20261001-070000-a60", status: "needs_attention", needs_attention: true, status_text: "Needs attention" });
    const dismissed = summary({
      id: "20261001-060000-b70",
      status: "needs_attention",
      needs_attention: false,
      status_text: "Rollback incomplete, dismissed",
    });
    const all = [undo, undone, removed, failed, interrupted, attention, dismissed];
    const by = (status: OperationFilter["status"]) => ids(filterOperations(all, { ...NO_FILTER, status }));

    expect(by("applied")).toEqual(["20261002-110000-d30", "20261002-095812-a3f"]);
    expect(by("undone")).toEqual(["20261002-101500-b10"]);
    expect(by("failed")).toEqual(["20261001-090000-e40", "20261001-080000-f50", "20261001-060000-b70"]);
    expect(by("attention")).toEqual(["20261001-070000-a60"]);
  });

  test("by text lists only the Operations that changed an Entry whose name holds it, in any case", () => {
    const linear = summary({ id: "20261002-101500-b10", entries: ["cd-throwaway-linear", "cd-throwaway-notes"] });
    const deploy = summary({ id: "20261002-095812-a3f", entries: ["cd-throwaway-deploy"] });
    const backup = summary({ id: "20261002-090000-c20", action: "delete_backup", entries: [] });
    const by = (text: string) => ids(filterOperations([linear, deploy, backup], { ...NO_FILTER, text }));

    expect(by("NOTES")).toEqual(["20261002-101500-b10"]);
    expect(by("throwaway")).toEqual(["20261002-101500-b10", "20261002-095812-a3f"]);
    expect(by("  deploy ")).toEqual(["20261002-095812-a3f"]);
    expect(by("missing")).toEqual([]);
  });

  test("keeps only the Operations every part of the filter keeps", () => {
    const linear = summary({ id: "20261002-101500-b10", action: "disable", entries: ["cd-throwaway-linear"] });
    const notes = summary({ id: "20261002-095812-a3f", action: "disable", entries: ["cd-throwaway-notes"] });
    const removed = summary({ id: "20261002-090000-c20", action: "remove", entries: ["cd-throwaway-linear"] });
    const undone = summary({ ...linear, id: "20261002-080000-d30", undone_by: "20261002-110000-e40" });

    expect(
      ids(filterOperations([linear, notes, removed, undone], { verb: "disable", status: "applied", text: "linear" })),
    ).toEqual(["20261002-101500-b10"]);
  });
});

describe("the count of Operations", () => {
  test("is the number History holds", () => {
    expect(operationCount(1)).toBe("1 Operation");
    expect(operationCount(40, 40)).toBe("40 Operations");
  });

  test("says how many of them the filter keeps", () => {
    expect(operationCount(3, 40)).toBe("3 of 40 Operations");
    expect(operationCount(0, 1)).toBe("0 of 1 Operation");
  });
});

describe("the filter's choices", () => {
  test("are any verb or each verb History records an Operation of, which Delete backup is not", () => {
    expect(FILTER_VERBS).toEqual([
      { value: "", label: "Any verb" },
      { value: "disable", label: "Disable" },
      { value: "enable", label: "Enable" },
      { value: "remove", label: "Remove" },
      { value: "undo", label: "Undo" },
    ]);
  });

  test("are any status or each status History shows", () => {
    expect(FILTER_STATUSES).toEqual([
      { value: "", label: "Any status" },
      { value: "applied", label: "Applied" },
      { value: "undone", label: "Undone" },
      { value: "failed", label: "Failed" },
      { value: "attention", label: "Needs attention" },
    ]);
  });
});
