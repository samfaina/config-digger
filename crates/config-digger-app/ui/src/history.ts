// What the History · Undo screen shows, worked out from the Operations the
// core lists and details. Nothing here calls the core or renders.

import type { Action, Error as ApiError, FileDiff, OperationDetail, OperationSummary } from "./bindings";
import { ACTION_LABELS } from "./dialog";
import type { DropdownOption } from "./dropdown";

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

/** "1 Operation", "3 Operations", or with the `total` History holds when a filter keeps fewer: "3 of 40 Operations". */
export function operationCount(count: number, total = count): string {
  const noun = total === 1 ? "Operation" : "Operations";
  return count === total ? `${count} ${noun}` : `${count} of ${total} ${noun}`;
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

/** What became of an Operation, as History filters by it: an applied Undo is applied, and an Operation that did not complete failed. */
export type FilterStatus = Exclude<StatusTone, "undo">;

/**
 * What History's list is narrowed to: an Operation's verb and status, each
 * `""` for any, and text that one of the names of the Entries it changed
 * holds, in any case.
 */
export type OperationFilter = { verb: Action | ""; status: FilterStatus | ""; text: string };

export const NO_FILTER: OperationFilter = { verb: "", status: "", text: "" };

// Delete backup records no Operation of its own: it marks the backup of the one it deletes.
const VERBS: Action[] = ["disable", "enable", "remove", "undo"];

/** The choices of the filter's verb. */
export const FILTER_VERBS: DropdownOption[] = [
  { value: "", label: "Any verb" },
  ...VERBS.map((verb) => ({ value: verb, label: ACTION_LABELS[verb] })),
];

/** The choices of the filter's status. */
export const FILTER_STATUSES: DropdownOption[] = [
  { value: "", label: "Any status" },
  { value: "applied", label: "Applied" },
  { value: "undone", label: "Undone" },
  { value: "failed", label: "Failed" },
  { value: "attention", label: "Needs attention" },
];

/** The Operations of `history` that `filter` keeps, in their order. */
export function filterOperations(history: OperationSummary[], filter: OperationFilter): OperationSummary[] {
  const text = filter.text.trim().toLowerCase();
  return history.filter(
    (summary) =>
      (filter.verb === "" || summary.action === filter.verb) &&
      (filter.status === "" || filterStatus(summary) === filter.status) &&
      (text === "" || summary.entries.some((entry) => entry.toLowerCase().includes(text))),
  );
}

function filterStatus(summary: OperationSummary): FilterStatus {
  const tone = statusTone(summary);
  return tone === "undo" ? "applied" : tone;
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

/** An Undo the core refuses, or blocks, because what the Operation touched changed since. */
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

/**
 * What pressing Undo does on `summary`, whose detail is `null` while it is
 * read, or `"unread"` when it couldn't be: then the dialog plans the Undo
 * and says why it is refused, if it is.
 */
export function undoPress(summary: OperationSummary, detail: OperationDetail | "unread" | null): UndoPress {
  if (detail === null) return summary.undoable ? { kind: "dialog" } : { kind: "reading" };
  if (detail === "unread") return { kind: "dialog" };
  const error = detail.undo_refusal;
  if (error === null || (error.code !== "undo_refused" && error.code !== "blocked")) return { kind: "dialog" };
  return { kind: "refused", refusal: refusalOf(error, error.backup ?? detail.backup) };
}

/** The refusal planning an Undo failed with, or `null` when it failed otherwise. */
export function undoRefusal(error: ApiError): UndoRefusal | null {
  if (error.code !== "undo_refused" || error.backup === null) return null;
  return refusalOf(error, error.backup);
}

function refusalOf(error: ApiError, backup: string): UndoRefusal {
  return { reason: error.message, backup, diffs: error.diffs };
}
