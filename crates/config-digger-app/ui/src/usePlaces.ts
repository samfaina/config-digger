import { useCallback, useEffect, useRef, useState } from "react";
import { commands, type Error as ApiError, type ListChange, type ListChanged, type Places } from "./bindings";
import { call, type CommandResult } from "./desktop";
import { reads, whenIdle } from "./reads";

export type PlacesRead = {
  /** Every Project and Agent home, kept while a new read runs or after one fails. */
  places: Places | null;
  error: ApiError | null;
  /** Reads again, done once the screen has what it read. */
  reload: () => Promise<void>;
  /** Applies `change` to config-digger's lists, which are read again at once. */
  change: (change: ListChange) => Promise<CommandResult<ListChanged>>;
};

/**
 * Reads every Project and Agent home on start, whenever the window regains
 * focus while nothing else is read, and on `reload`.
 */
export function usePlaces(): PlacesRead {
  const [places, setPlaces] = useState<Places | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  // A reload during a read waits for that one rather than starting a second.
  const inFlight = useRef<Promise<void> | null>(null);
  // Counts the changes, so that a read started before one is dropped.
  const changes = useRef(0);

  const read = useCallback(async () => {
    const before = changes.current;
    const result = await reads.during(() => call(commands.readPlaces));
    if (changes.current !== before) return;
    if (result.status === "ok") {
      setPlaces(result.data);
      setError(null);
    } else {
      setError(result.error);
    }
  }, []);

  const reload = useCallback(() => {
    inFlight.current ??= read().finally(() => {
      inFlight.current = null;
    });
    return inFlight.current;
  }, [read]);

  const change = useCallback(async (change: ListChange) => {
    changes.current += 1;
    const result = await reads.during(() => call(() => commands.changeList(change)));
    if (result.status === "ok") {
      setPlaces(result.data.places);
      setError(null);
    }
    return result;
  }, []);

  useEffect(() => {
    void reload();
    const onFocus = whenIdle(reads, () => void reload());
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [reload]);

  return { places, error, reload, change };
}
