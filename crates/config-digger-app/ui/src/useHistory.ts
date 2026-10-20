import { useCallback, useEffect, useRef, useState } from "react";
import { commands, type Error as ApiError, type OperationSummary } from "./bindings";
import { call } from "./desktop";

export type HistoryRead = {
  /** Every Operation, newest first, kept while a new read runs or after one fails. */
  history: OperationSummary[] | null;
  error: ApiError | null;
  /** Whether a read of History runs. */
  reading: boolean;
  /** Reads again, done once the screen has what it read. */
  reload: () => Promise<void>;
};

/** Reads History on start, whenever the window regains focus, and on `reload`. */
export function useHistory(): HistoryRead {
  const [history, setHistory] = useState<OperationSummary[] | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [reading, setReading] = useState(false);
  // A reload during a read reads again once that one is done, as an action
  // may have changed History since it started, rather than running beside it.
  const inFlight = useRef<Promise<void> | null>(null);

  const read = useCallback(async () => {
    setReading(true);
    const result = await call(commands.readHistory).finally(() => setReading(false));
    if (result.status === "ok") {
      setHistory(result.data);
      setError(null);
    } else {
      setError(result.error);
    }
  }, []);

  const reload = useCallback(() => {
    const reading: Promise<void> = (inFlight.current ?? Promise.resolve()).then(read, read).finally(() => {
      if (inFlight.current === reading) inFlight.current = null;
    });
    inFlight.current = reading;
    return reading;
  }, [read]);

  useEffect(() => {
    // On start, a read already running is enough.
    if (!inFlight.current) void reload();
    // A focus event during a read does not start a second one.
    const onFocus = () => {
      if (!inFlight.current) void reload();
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [reload]);

  return { history, error, reading, reload };
}
