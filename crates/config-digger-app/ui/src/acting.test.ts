import { describe, expect, test } from "vitest";
import type { ActionRequest, OperationSummary } from "./bindings";
import { createBusy, type BusyAction } from "./busy";
import type { CommandResult } from "./desktop";
import type { Notice } from "./Notice";
import { dismissOperation, rereadAfter } from "./acting";

const LOCATOR = String.raw`C:\Users\me\.claude.json#mcpServers.cd-throwaway-notes`;
const DISABLE: ActionRequest = { kind: "entry", verb: "disable", locator: LOCATOR, project: null };

/** A screen whose reads answer when the test tells them to, with what it showed busy. */
function screen() {
  let answer: () => void = () => {};
  let reads = 0;
  const shown: BusyAction[][] = [];
  const reading = {
    text: "Reading the inventory again…",
    reload: () => {
      reads += 1;
      return new Promise<void>((resolve) => (answer = resolve));
    },
    busy: createBusy((now) => shown.push(now)),
  };
  return { reading, reads: () => reads, busy: () => shown.at(-1) ?? [], answer: () => answer() };
}

describe("the read after an Inventory action's dialog closes", () => {
  test("shows as busy on the Entry it acted on as soon as it starts", () => {
    const { reading, reads, busy } = screen();

    void rereadAfter(DISABLE, reading);

    expect(reads()).toBe(1);
    expect(busy()).toEqual([{ text: "Reading the inventory again…", row: LOCATOR }]);
  });

  test("stays busy until the screen has what it read", async () => {
    const { reading, busy, answer } = screen();

    const done = rereadAfter(DISABLE, reading);
    await Promise.resolve();
    expect(busy()).toHaveLength(1);

    answer();
    await done;
    expect(busy()).toEqual([]);
  });
});

describe("the read after a History action's dialog closes", () => {
  test("shows as busy on the Operation it acted on until History is read again", async () => {
    const { reading, busy, answer } = screen();

    const done = rereadAfter({ kind: "undo", operation: OPERATION }, { ...reading, text: "Reading History again…" });
    expect(busy()).toEqual([{ text: "Reading History again…", row: OPERATION }]);

    answer();
    await done;
    expect(busy()).toEqual([]);
  });
});

const OPERATION = "20261002-095812-a3f";

const dismissed: OperationSummary = {
  id: OPERATION,
  created_at: "2026-10-02T09:58:12Z",
  action: "remove",
  title: "Remove 1 Entry",
  status: "failed",
  status_text: "Failed · dismissed",
  cause: null,
  needs_attention: false,
  undo_of: null,
  undone_by: null,
  backup_deleted: false,
  backup_deletable: true,
  agents: ["Claude Code"],
  locations: [String.raw`C:\Users\me\.claude.json`],
  undoable: false,
  not_undoable: "It failed, so nothing of it was kept.",
};

/** History, whose Dismiss and reads answer when the test tells them to, with what it showed. */
function history() {
  let answerDismiss: (result: CommandResult<OperationSummary>) => void = () => {};
  let answerRead: () => void = () => {};
  const asked: string[] = [];
  let reads = 0;
  const shown: BusyAction[][] = [];
  const notices: (Notice | null)[] = [];
  const dismissing = {
    dismiss: (operation: string) => {
      asked.push(operation);
      return new Promise<CommandResult<OperationSummary>>((resolve) => (answerDismiss = resolve));
    },
    reload: () => {
      reads += 1;
      return new Promise<void>((resolve) => (answerRead = resolve));
    },
    busy: createBusy((now) => shown.push(now)),
    notify: (notice: Notice | null) => notices.push(notice),
  };
  return {
    dismissing,
    asked,
    notices,
    reads: () => reads,
    busy: () => shown.at(-1) ?? [],
    answerDismiss: (result: CommandResult<OperationSummary>) => answerDismiss(result),
    answerRead: () => answerRead(),
  };
}

/** Lets every promise the test already settled run on. */
const settled = () => new Promise((resolve) => setTimeout(resolve));

describe("a Dismiss in History", () => {
  test("shows as busy on its Operation, with no notice left over, as soon as it starts", () => {
    const { dismissing, asked, notices, busy } = history();

    void dismissOperation(OPERATION, dismissing);

    expect(asked).toEqual([OPERATION]);
    expect(busy()).toEqual([{ text: `Dismissing Operation ${OPERATION}…`, row: OPERATION }]);
    expect(notices).toEqual([null]);
  });

  test("stays busy until History is read again, then says what it did", async () => {
    const { dismissing, notices, reads, busy, answerDismiss, answerRead } = history();

    const done = dismissOperation(OPERATION, dismissing);
    answerDismiss({ status: "ok", data: dismissed });
    await settled();
    expect(reads()).toBe(1);
    expect(busy()).toHaveLength(1);
    expect(notices).toEqual([null]);

    answerRead();
    await done;
    expect(busy()).toEqual([]);
    expect(notices.at(-1)).toEqual({ kind: "success", text: `Dismissed Operation ${OPERATION}.` });
  });

  test("when refused, reads History again and then says why", async () => {
    const { dismissing, notices, busy, answerDismiss, answerRead } = history();

    const done = dismissOperation(OPERATION, dismissing);
    answerDismiss({
      status: "error",
      error: { code: "unexpected", message: "History couldn't be written.", candidates: [], backup: null, diffs: [], failed: null },
    });
    await settled();
    answerRead();
    await done;

    expect(busy()).toEqual([]);
    expect(notices.at(-1)).toEqual({ kind: "error", text: "It couldn't be dismissed: History couldn't be written." });
  });
});
