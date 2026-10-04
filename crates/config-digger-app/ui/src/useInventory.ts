import { useCallback, useEffect, useRef, useState } from "react";
import { commands, type Error as ApiError, type Inventory } from "./bindings";

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

type Result = { status: "ok"; data: Inventory } | { status: "error"; error: ApiError };

/**
 * Reads the inventory on start, whenever the window regains focus, and on
 * `refresh`, for the Project last selected.
 */
export function useInventory(): InventoryRead {
  const [inventory, setInventory] = useState<Inventory | null>(null);
  const [project, setProject] = useState<string | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [reading, setReading] = useState(false);
  // A focus event during a read does not start a second one.
  const inFlight = useRef(false);
  const projectRef = useRef<string | null>(null);

  /** Runs `call` for `project`, dropping what it returns when another Project was selected meanwhile. */
  const load = useCallback(async (project: string | null, call: (project: string | null) => Promise<Result>) => {
    let result: Result;
    try {
      result = await call(project);
    } catch (thrown) {
      result = {
        status: "error",
        error: { code: "unexpected", message: String(thrown), candidates: [], backup: null, diffs: [], failed: null },
      };
    }
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
      await load(projectRef.current, commands.readInventory);
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
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);

  return { inventory, project, error, reading, refresh: () => void refresh(), selectProject };
}
