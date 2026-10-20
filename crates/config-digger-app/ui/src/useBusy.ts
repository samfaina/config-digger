import { useState } from "react";
import { createBusy, type BusyAction, type BusyActions } from "./busy";

/** The actions a screen runs, for its busy indicator and the rows they run on. */
export function useBusy(): { actions: BusyAction[]; busy: BusyActions } {
  const [actions, setActions] = useState<BusyAction[]>([]);
  const [busy] = useState(() => createBusy(setActions));
  return { actions, busy };
}
