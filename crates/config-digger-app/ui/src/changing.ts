import type { ListChange, ListChanged, Places } from "./bindings";
import type { BusyActions } from "./busy";
import type { CommandResult } from "./desktop";
import type { Notice } from "./Notice";
import { changeBusy, changedSentence } from "./places";

/** What a list change from the Projects & homes screen goes through. */
export type Changing = {
  /** Applies a change to config-digger's lists, done once the screen has the lists it answers with. */
  change: (change: ListChange) => Promise<CommandResult<ListChanged>>;
  busy: BusyActions;
  notify: (notice: Notice | null) => void;
  /** The lists shown, which name the Agent of a home. */
  places: Places | null;
};

/**
 * Applies `request`, busy from the moment it starts until the screen has the
 * lists it answers with, and says what it did; returns why it was refused, if it was.
 */
export async function applyChange(request: ListChange, { change, busy, notify, places }: Changing): Promise<string | null> {
  // An error notice left over would hide the busy indicator.
  notify(null);
  const result = await busy.during(changeBusy(request, places), () => change(request));
  if (result.status === "error") return result.error.message;
  notify({ kind: "success", text: changedSentence(request, result.data.listed) });
  return null;
}
