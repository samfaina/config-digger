// @vitest-environment happy-dom
import { cleanup, render } from "@testing-library/react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, test, vi } from "vitest";
import type { OperationSummary } from "./bindings";
import type { UndoRefusal } from "./history";
import { HistoryScreen, OperationPane } from "./HistoryScreen";
import { Loading } from "./Loading";

afterEach(cleanup);

describe("History before its first read is done", () => {
  test("fills its list of Operations with the loading state", () => {
    const list = renderToStaticMarkup(<HistoryScreen />).match(/<section class="panel history-list">[\s\S]*?<\/section>/)?.[0] ?? "";
    expect(list).toContain(renderToStaticMarkup(<Loading text="Reading History…" rows={6} />));
    expect(list).not.toContain('<p class="empty">');
  });

  test("states no count of Operations in its banner's title, as it has none yet", () => {
    const title = renderToStaticMarkup(<HistoryScreen />).match(/<div class="banner-title">([\s\S]*?)<\/div>/)?.[1];
    expect(title).toBe("Operations");
    expect(title).not.toMatch(/\d/);
  });
});

/** An applied Operation whose Rollback is incomplete, so Undo, Delete backup and Dismiss are all offered. */
const summary: OperationSummary = {
  id: "20261002-095812-a3f",
  created_at: "2026-10-02T09:58:12Z",
  action: "disable",
  title: "Disable 1 Entry",
  status: "applied",
  status_text: "Applied",
  cause: null,
  needs_attention: true,
  undo_of: null,
  undone_by: null,
  backup_deleted: false,
  backup_deletable: true,
  agents: ["Claude Code"],
  entries: ["notes"],
  locations: [String.raw`C:\Users\me\.claude.json`],
  undoable: true,
  not_undoable: null,
};

/** The pane of `summary`, showing `refusal` and whether an action on it runs. */
const pane = ({ refusal = null, busy = false }: { refusal?: UndoRefusal | null; busy?: boolean }) => (
  <OperationPane
    summary={summary}
    detail={null}
    detailError={null}
    time="09:58"
    refusal={refusal}
    busy={busy}
    onUndo={() => {}}
    onDeleteBackup={() => {}}
    onDismiss={() => {}}
    notify={() => {}}
  />
);

/** The opening tag of each button in the footer of the Operation's pane. */
const footerButtons = (busy: boolean) => {
  const markup = renderToStaticMarkup(pane({ busy }));
  return markup.slice(markup.indexOf('<footer class="dialog-foot">')).match(/<button[^>]*>/g) ?? [];
};

describe("the pane of an Operation", () => {
  test("disables Dismiss, Delete backup and Undo while an action on the Operation runs", () => {
    const buttons = footerButtons(true);
    expect(buttons).toHaveLength(3);
    for (const button of buttons) expect(button).toMatch(/disabled=""/);
  });

  test("offers Dismiss, Delete backup and Undo while nothing runs on it", () => {
    const buttons = footerButtons(false);
    expect(buttons).toHaveLength(3);
    for (const button of buttons) expect(button).not.toMatch(/disabled/);
  });

  test("draws Delete backup in the style of a button that deletes, and Dismiss and Undo not", () => {
    const classes = footerButtons(false).map((button) => button.match(/class="([^"]*)"/)?.[1]);
    expect(classes).toEqual(["cta secondary", "cta danger", "cta"]);
  });
});

describe("the pane of an Operation whose Undo was refused", () => {
  /** A refused Undo, whose Can't undo block scrolls into view. */
  const refusal: UndoRefusal = { reason: "The file changed since the Operation.", backup: String.raw`C:\Users\me\.config-digger\backups\20261002-095812-a3f`, diffs: [] };

  afterEach(() => vi.restoreAllMocks());

  test("unmounts without an error when scrollIntoView returns a Promise", () => {
    const scroll = vi.spyOn(Element.prototype, "scrollIntoView").mockImplementation(() => Promise.resolve() as unknown as void);
    const { unmount } = render(pane({ refusal }));
    expect(scroll).toHaveBeenCalled();
    expect(unmount).not.toThrow();
  });
});
