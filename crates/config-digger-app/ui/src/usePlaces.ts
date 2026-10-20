import { useCallback, useEffect, useRef, useState } from "react";
import { commands, type Error as ApiError, type ListChange, type ListChanged, type Places } from "./bindings";
import { call, type CommandResult } from "./desktop";
import { reads, whenIdle } from "./reads";

export type PlacesRead = {
  /** Every Project and Agent home, kept while a new read runs or after one fails. */
  places: Places | null;
  error: ApiError | null;
  /** Whether a read of the Projects and Agent homes runs. */
  reading: boolean;
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
  const [reading, setReading] = useState(false);
  // A reload during a read reads again once that one is done, as the disk may
  // have changed since it started, rather than running beside it.
  const inFlight = useRef<Promise<void> | null>(null);
  // Counts the changes, so that a read started before one is dropped.
  const changes = useRef(0);

  const read = useCallback(async () => {
    const before = changes.current;
    setReading(true);
    const result = await reads.during(() => call(commands.readPlaces)).finally(() => setReading(false));
    if (changes.current !== before) return;
    if (result.status === "ok") {
      setPlaces(result.data);
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
    // On start, a read already running is enough.
    if (!inFlight.current) void reload();
    const onFocus = whenIdle(reads, () => void reload());
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [reload]);

  return { places, error, reading, reload, change };
}
