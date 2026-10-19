// What the History · Undo screen shows, worked out from the Operations the
// core lists and details. Nothing here calls the core or renders.

import type { Error as ApiError, FileDiff, OperationDetail, OperationSummary } from "./bindings";

/** How a status reads at a glance. */
export type StatusTone = "applied" | "undone" | "undo" | "failed" | "attention";

/** One Operation as the list shows it. */
export type OperationRow = {
  title: string;
  /** Its Agents, how many locations it touched and its number. */
  meta: string;
  /** The status the core renders, toned by what became of the Operation. */
  status: { label: string; tone: StatusTone };
};

/** The Agents whose States an Operation changed, or "No Agent". */
export function agentNames(summary: OperationSummary): string {
  return summary.agents.length > 0 ? summary.agents.join(", ") : "No Agent";
}

/** "1 Operation", "3 Operations". */
export function operationCount(count: number): string {
  return `${count} ${count === 1 ? "Operation" : "Operations"}`;
}

/** An Operation's row in the list. */
export function operationRow(summary: OperationSummary): OperationRow {
  const count = summary.locations.length;
  const locations = `${count} ${count === 1 ? "location" : "locations"}`;
  return {
    title: summary.title,
    meta: `${agentNames(summary)} · ${locations} · Operation ${summary.id}`,
    status: { label: summary.status_text, tone: statusTone(summary) },
  };
}

function statusTone(summary: OperationSummary): StatusTone {
  if (summary.needs_attention) return "attention";
  if (summary.status !== "applied") return "failed";
  if (summary.undone_by !== null) return "undone";
  if (summary.undo_of !== null) return "undo";
  return "applied";
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** When an Operation started, on the user's clock: "Today 09:58", "Yesterday 17:20", "Sep 30 09:12". */
export function operationTime(createdAt: string, now: Date): string {
  const at = new Date(createdAt);
  const two = (value: number) => String(value).padStart(2, "0");
  const time = `${two(at.getHours())}:${two(at.getMinutes())}`;
  const day = (date: Date) => new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
  const yesterday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1);
  if (day(at) === day(now)) return `Today ${time}`;
  if (day(at) === day(yesterday)) return `Yesterday ${time}`;
  const year = at.getFullYear() === now.getFullYear() ? "" : ` ${at.getFullYear()}`;
  return `${MONTHS[at.getMonth()]} ${at.getDate()}${year} ${time}`;
}

/** Undo, offered with why it would be refused when its files changed since, or why it is not offered. */
export type UndoAction = { offered: true; refusal: string | null } | { offered: false; why: string };

/** What the detail pane offers for one Operation. */
export type OperationActions = {
  undo: UndoAction;
  deleteBackup: boolean;
  /** For one that needs attention, once its files are recovered by hand. */
  dismiss: boolean;
};

/**
 * What can be done to an Operation. Undo is offered for an applied one with
 * its backup, not undone yet, even when its files changed since: trying it
 * shows what changed. Any other says why it cannot be undone.
 */
export function operationActions(summary: OperationSummary): OperationActions {
  const offered = summary.status === "applied" && !summary.backup_deleted && summary.undone_by === null;
  const undo: UndoAction = offered
    ? { offered: true, refusal: summary.undoable ? null : summary.not_undoable }
    : { offered: false, why: summary.not_undoable ?? "It cannot be undone." };
  return { undo, deleteBackup: summary.backup_deletable, dismiss: summary.needs_attention };
}

/** The Operation the detail pane shows: the one selected while History lists it, else the newest. */
export function selectedOperation(history: OperationSummary[], selected: string | null): OperationSummary | null {
  return history.find((operation) => operation.id === selected) ?? history[0] ?? null;
}

/** What the persistent banner says while any Operation needs attention, or `null` once none does. */
export function recoveryBanner(history: OperationSummary[]): string | null {
  const count = history.filter((summary) => summary.needs_attention).length;
  if (count === 0) return null;
  return count === 1 ? "An Operation needs manual recovery" : `${count} Operations need manual recovery`;
}

/** An Undo the core refused because what the Operation touched changed since. */
export type UndoRefusal = {
  reason: string;
  /** The Operation's backup folder, to recover from by hand. */
  backup: string;
  /** A masked diff from each backed-up file to what it holds now, or the notice of why none can be shown. */
  diffs: FileDiff[];
};

/**
 * What pressing Undo does: shows at once the refusal the detail already
 * holds, waits while the detail of one History says is refused is read, or
 * opens the dialog on the Undo's preview.
 */
export type UndoPress = { kind: "refused"; refusal: UndoRefusal } | { kind: "reading" } | { kind: "dialog" };

/** What pressing Undo does on `summary`, whose detail is `null` while it is read. */
export function undoPress(summary: OperationSummary, detail: OperationDetail | null): UndoPress {
  if (detail === null) return summary.undoable ? { kind: "dialog" } : { kind: "reading" };
  const error = detail.undo_refusal;
  if (error === null) return { kind: "dialog" };
  return { kind: "refused", refusal: { reason: error.message, backup: error.backup ?? detail.backup, diffs: error.diffs } };
}

/** The refusal planning an Undo failed with, or `null` when it failed otherwise. */
export function undoRefusal(error: ApiError): UndoRefusal | null {
  if (error.code !== "undo_refused" || error.backup === null) return null;
  return { reason: error.message, backup: error.backup, diffs: error.diffs };
}
