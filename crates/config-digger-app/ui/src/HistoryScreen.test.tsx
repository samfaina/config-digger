import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import type { OperationSummary } from "./bindings";
import { HistoryScreen, OperationPane } from "./HistoryScreen";
import { Loading } from "./Loading";

describe("History before its first read is done", () => {
  test("fills its list of Operations with the loading state", () => {
    const list = renderToStaticMarkup(<HistoryScreen />).match(/<section class="panel history-list">[\s\S]*?<\/section>/)?.[0] ?? "";
    expect(list).toContain(renderToStaticMarkup(<Loading text="Reading History…" rows={6} />));
    expect(list).not.toContain('<p class="empty">');
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

/** The opening tag of each button in the footer of the Operation's pane. */
const footerButtons = (busy: boolean) => {
  const markup = renderToStaticMarkup(
    <OperationPane
      summary={summary}
      detail={null}
      detailError={null}
      time="09:58"
      refusal={null}
      busy={busy}
      onUndo={() => {}}
      onDeleteBackup={() => {}}
      onDismiss={() => {}}
      notify={() => {}}
    />,
  );
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
});
