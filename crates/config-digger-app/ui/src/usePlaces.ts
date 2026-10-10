import { useCallback, useEffect, useRef, useState } from "react";
import { commands, type Error as ApiError, type ListChange, type ListChanged, type Places } from "./bindings";
import { call, type CommandResult } from "./desktop";

export type PlacesRead = {
  /** Every Project and Agent home, kept while a new read runs or after one fails. */
  places: Places | null;
  error: ApiError | null;
  reload: () => void;
  /** Applies `change` to config-digger's lists, which are read again at once. */
  change: (change: ListChange) => Promise<CommandResult<ListChanged>>;
};

/** Reads every Project and Agent home on start, whenever the window regains focus, and on `reload`. */
export function usePlaces(): PlacesRead {
  const [places, setPlaces] = useState<Places | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  // A focus event during a read does not start a second one.
  const inFlight = useRef(false);

  const reload = useCallback(async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    try {
      const result = await call(commands.readPlaces);
      if (result.status === "ok") {
        setPlaces(result.data);
        setError(null);
      } else {
        setError(result.error);
      }
    } finally {
      inFlight.current = false;
    }
  }, []);

  const change = useCallback(async (change: ListChange) => {
    const result = await call(() => commands.changeList(change));
    if (result.status === "ok") {
      setPlaces(result.data.places);
      setError(null);
    }
    return result;
  }, []);

  useEffect(() => {
    void reload();
    const onFocus = () => void reload();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [reload]);

  return { places, error, reload: () => void reload(), change };
}
