import { useCallback, useEffect, useState } from "react";
import { commands, type Error as ApiError, type OperationSummary } from "./bindings";
import { call } from "./desktop";
import { createReadQueue } from "./reads";

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
  // A reload during a read reads again once that one is done, as after an action.
  const [queue] = useState(createReadQueue);

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

  const reload = useCallback(() => queue.next(read), [queue, read]);

  useEffect(() => {
    // On start, a read already running is enough.
    if (!queue.running()) void reload();
    // A focus event during a read does not start a second one. A read of
    // History is not one of the app's reads, which other screens wait on.
    const onFocus = () => {
      if (!queue.running()) void reload();
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [reload]);

  return { history, error, reading, reload };
}
