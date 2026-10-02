import { useCallback, useEffect, useRef, useState } from "react";
import { commands, type Error as ApiError, type Inventory } from "./bindings";

export type InventoryRead = {
  /** The last inventory read, kept while a new read runs or after one fails. */
  inventory: Inventory | null;
  error: ApiError | null;
  reading: boolean;
  refresh: () => void;
};

/** Reads the inventory on start, whenever the window regains focus, and on `refresh`. */
export function useInventory(): InventoryRead {
  const [inventory, setInventory] = useState<Inventory | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [reading, setReading] = useState(false);
  // A focus event during a read does not start a second one.
  const inFlight = useRef(false);

  const refresh = useCallback(async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    setReading(true);
    try {
      const result = await commands.readInventory();
      if (result.status === "ok") {
        setInventory(result.data);
        setError(null);
      } else {
        setError(result.error);
      }
    } catch (thrown) {
      setError({ code: "unexpected", message: String(thrown), candidates: [], backup: null, diff: null });
    } finally {
      inFlight.current = false;
      setReading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);

  return { inventory, error, reading, refresh: () => void refresh() };
}
