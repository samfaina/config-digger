import { useCallback, useEffect, useRef, useState } from "react";
import { commands, type Error as ApiError, type OperationSummary } from "./bindings";
import { call } from "./desktop";

export type HistoryRead = {
  /** Every Operation, newest first, kept while a new read runs or after one fails. */
  history: OperationSummary[] | null;
  error: ApiError | null;
  /** Whether a read of History runs. */
  reading: boolean;
  reload: () => void;
};

/** Reads History on start, whenever the window regains focus, and on `reload`. */
export function useHistory(): HistoryRead {
  const [history, setHistory] = useState<OperationSummary[] | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [reading, setReading] = useState(false);
  // A focus event during a read does not start a second one.
  const inFlight = useRef(false);

  const reload = useCallback(async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    setReading(true);
    try {
      const result = await call(commands.readHistory);
      if (result.status === "ok") {
        setHistory(result.data);
        setError(null);
      } else {
        setError(result.error);
      }
    } finally {
      inFlight.current = false;
      setReading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
    const onFocus = () => void reload();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [reload]);

  return { history, error, reading, reload: () => void reload() };
}
