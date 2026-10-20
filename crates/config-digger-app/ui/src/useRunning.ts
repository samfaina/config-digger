import { useState } from "react";
import { createRunning, type Running, type Runnings } from "./running";

/** The actions a screen runs, for its busy indicator and the rows they run on. */
export function useRunning(): { running: Running[]; during: Runnings["during"] } {
  const [running, setRunning] = useState<Running[]>([]);
  const [runnings] = useState(() => createRunning(setRunning));
  return { running, during: runnings.during };
}
