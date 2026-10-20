import { useState } from "react";
import { createBusy, type BusyAction, type BusyActions } from "./busy";

/** The actions a screen runs, for its busy indicator, and the rows they run on, whose buttons are disabled meanwhile. */
export function useBusy(): { actions: BusyAction[]; rows: string[]; busy: BusyActions } {
  const [actions, setActions] = useState<BusyAction[]>([]);
  const [busy] = useState(() => createBusy(setActions));
  const rows = actions.flatMap((each) => (each.row === null ? [] : [each.row]));
  return { actions, rows, busy };
}
