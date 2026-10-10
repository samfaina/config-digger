import { useCallback, useEffect, useRef, useState } from "react";
import { commands, type Error as ApiError, type Settings, type SettingsRead } from "./bindings";
import { call, type CommandResult } from "./desktop";

export type SettingsState = {
  /** The settings, data folders and parked Entries, kept after a read fails. */
  read: SettingsRead | null;
  error: ApiError | null;
  /** Saves `settings`, which show as saved once the core returns them. */
  save: (settings: Settings) => Promise<CommandResult<Settings>>;
};

/** Reads the settings, data folders and parked Entries on start and whenever the window regains focus. */
export function useSettings(): SettingsState {
  const [read, setRead] = useState<SettingsRead | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  // A focus event during a read does not start a second one.
  const inFlight = useRef(false);
  // Counts the saves, so that a read started before one is dropped.
  const saves = useRef(0);

  const reload = useCallback(async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    const before = saves.current;
    try {
      const result = await call(commands.readSettings);
      if (saves.current !== before) return;
      if (result.status === "ok") {
        setRead(result.data);
        setError(null);
      } else {
        setError(result.error);
      }
    } finally {
      inFlight.current = false;
    }
  }, []);

  const save = useCallback(async (settings: Settings) => {
    saves.current += 1;
    const result = await call(() => commands.saveSettings(settings));
    if (result.status === "ok") {
      const saved = result.data;
      setRead((current) => current && { ...current, settings: saved });
    }
    return result;
  }, []);

  useEffect(() => {
    void reload();
    const onFocus = () => void reload();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [reload]);

  return { read, error, save };
}
