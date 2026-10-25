import type { ActionRequest, OperationSummary } from "./bindings";
import type { BusyActions } from "./busy";
import type { CommandResult } from "./desktop";
import type { Notice } from "./Notice";

/** What a screen reads again once an action's dialog closes. */
export type Rereading = {
  /** What its busy indicator says meanwhile. */
  text: string;
  /** Reads again, done once the screen has what it read. */
  reload: () => Promise<void>;
  busy: BusyActions;
  notify: (notice: Notice | null) => void;
};

/** What the Stranded group is known by, while its Remove all runs. */
export const STRANDED_ROW = "\u0000stranded";

/** The row an action runs on: the Entry's locator, the missing Project, the Stranded group, or the Operation. */
function rowOfRequest(request: ActionRequest): string {
  switch (request.kind) {
    case "entry":
      return request.locator;
    case "leftovers":
      return request.project;
    case "stranded":
      return STRANDED_ROW;
    case "undo":
    case "delete_backup":
      return request.operation;
  }
}

/**
 * Reads the screen again after `request`'s dialog closed, busy on its row
 * until the screen has what it read, so that the row's buttons stay disabled
 * until it shows what the action did.
 */
export async function rereadAfter(request: ActionRequest, { text, reload, busy, notify }: Rereading): Promise<void> {
  // An error notice left over would hide the busy indicator.
  notify(null);
  await busy.during({ text, row: rowOfRequest(request) }, reload);
}

/** What a Dismiss from History goes through. */
export type Dismissing = {
  dismiss: (operation: string) => Promise<CommandResult<OperationSummary>>;
  /** Reads History again, done once the screen has what it read. */
  reload: () => Promise<void>;
  busy: BusyActions;
  notify: (notice: Notice | null) => void;
};

/**
 * Dismisses `operation`, busy on it from the moment it starts until History
 * is read again, then says what it did or why it was refused.
 */
export async function dismissOperation(operation: string, { dismiss, reload, busy, notify }: Dismissing): Promise<void> {
  // An error notice left over would hide the busy indicator.
  notify(null);
  const result = await busy.during({ text: `Dismissing Operation ${operation}…`, row: operation }, async () => {
    const result = await dismiss(operation);
    await reload();
    return result;
  });
  notify(
    result.status === "ok"
      ? { kind: "success", text: `Dismissed Operation ${operation}.` }
      : { kind: "error", text: `It couldn't be dismissed: ${result.error.message}` },
  );
}
