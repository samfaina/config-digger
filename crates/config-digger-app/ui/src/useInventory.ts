import { useCallback, useEffect, useRef, useState } from "react";
import { commands, type Error as ApiError, type Inventory } from "./bindings";
import { call, type CommandResult } from "./desktop";
import { reads, whenIdle } from "./reads";

export type InventoryRead = {
  /** The last inventory read, kept while a new read runs or after one fails. */
  inventory: Inventory | null;
  /** The context's Project, `null` for no Project. */
  project: string | null;
  error: ApiError | null;
  /** Whether a read runs, of the disk or of another Project. */
  reading: boolean;
  refresh: () => void;
  /** Resolves the inventory for another Project, reading the disk only if it was never read. */
  selectProject: (project: string | null) => void;
};

/**
 * Reads the inventory on start, for `initialProject` or for no Project,
 * whenever the window regains focus while nothing else is read, and on
 * `refresh`, for the Project last selected.
 */
export function useInventory(initialProject: string | null = null): InventoryRead {
  const [inventory, setInventory] = useState<Inventory | null>(null);
  const [project, setProject] = useState<string | null>(initialProject);
  const [error, setError] = useState<ApiError | null>(null);
  // The reads running: a refresh and the Project switches since, which may each wait on a scan.
  const [running, setRunning] = useState(0);
  // A focus event during a read does not start a second one.
  const inFlight = useRef(false);
  const projectRef = useRef<string | null>(initialProject);

  /** Runs `read` for `project`, dropping what it returns when another Project was selected meanwhile. */
  const load = useCallback(async (project: string | null, read: (project: string | null) => Promise<CommandResult<Inventory>>) => {
    const result = await call(() => read(project));
    if (project !== projectRef.current) return;
    if (result.status === "ok") {
      setInventory(result.data);
      setError(null);
    } else {
      setError(result.error);
    }
  }, []);

  /** Runs `read`, counted as running until it settles. */
  const counted = useCallback(async (read: () => Promise<void>) => {
    setRunning((count) => count + 1);
    try {
      await reads.during(read);
    } finally {
      setRunning((count) => count - 1);
    }
  }, []);

  const refresh = useCallback(async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    try {
      await counted(() => load(projectRef.current, commands.readInventory));
    } finally {
      inFlight.current = false;
    }
  }, [load, counted]);

  const selectProject = useCallback(
    (next: string | null) => {
      projectRef.current = next;
      setProject(next);
      void counted(() => load(next, commands.switchProject));
    },
    [load, counted],
  );

  useEffect(() => {
    void refresh();
    const onFocus = whenIdle(reads, () => void refresh());
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);

  return { inventory, project, error, reading: running > 0, refresh: () => void refresh(), selectProject };
}
