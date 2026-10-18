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
  reading: boolean;
  refresh: () => void;
  /** Resolves the inventory for another Project, without reading the disk. */
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
  const [reading, setReading] = useState(false);
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

  const refresh = useCallback(async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    setReading(true);
    try {
      await reads.during(() => load(projectRef.current, commands.readInventory));
    } finally {
      inFlight.current = false;
      setReading(false);
    }
  }, [load]);

  const selectProject = useCallback(
    (next: string | null) => {
      projectRef.current = next;
      setProject(next);
      void load(next, commands.switchProject);
    },
    [load],
  );

  useEffect(() => {
    void refresh();
    const onFocus = whenIdle(reads, () => void refresh());
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);

  return { inventory, project, error, reading, refresh: () => void refresh(), selectProject };
}
