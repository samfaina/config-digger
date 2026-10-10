import { useCallback, useEffect, useState } from "react";
import { commands, type UpdateStatus } from "./bindings";
import { call } from "./desktop";

export type UpdatesState = {
  /** What the app knows of updates, `null` until it was read. */
  status: UpdateStatus | null;
  /** A check that runs now, from Check now or at startup. */
  checking: boolean;
  /** Whether the user checked since the app started, so About can say what the check found. */
  checkedNow: boolean;
  installing: boolean;
  /** Why the last check or install failed, cleared by the next one. */
  error: string | null;
  /** The user put the notice away with Later; About still shows the update. */
  later: boolean;
  checkNow: () => Promise<void>;
  install: () => Promise<void>;
  putOff: () => void;
  openNotes: () => Promise<void>;
};

/**
 * What the app knows of updates. On start it asks the core for the startup
 * check, which reaches the network only when "Check for updates on startup"
 * is on; otherwise nothing is sent until Check now.
 */
export function useUpdates(): UpdatesState {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const [checkedNow, setCheckedNow] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [later, setLater] = useState(false);

  useEffect(() => {
    // The core runs the startup check once per start, so a second mount, as in development, only
    // reads the status, which returns before the check does.
    setChecking(true);
    void call(commands.startupUpdateCheck).then((result) => {
      setChecking(false);
      if (result.status === "ok") setStatus(result.data);
      // A failed startup check stays quiet: the user did not ask for it. About still reads the last check.
      else void call(commands.updateStatus).then((read) => read.status === "ok" && setStatus(read.data));
    });
  }, []);

  const checkNow = useCallback(async () => {
    setChecking(true);
    setError(null);
    try {
      const result = await call(commands.checkForUpdates);
      if (result.status === "ok") {
        setStatus(result.data);
        setCheckedNow(true);
        setLater(false);
      } else {
        setError(`The update check failed: ${result.error.message}`);
      }
    } finally {
      setChecking(false);
    }
  }, []);

  const install = useCallback(async () => {
    setInstalling(true);
    setError(null);
    try {
      // On success the app exits and the installer restarts it.
      const result = await call(commands.installUpdate);
      if (result.status === "error") setError(`The update was not installed: ${result.error.message}`);
    } finally {
      setInstalling(false);
    }
  }, []);

  const putOff = useCallback(() => setLater(true), []);

  const openNotes = useCallback(async () => {
    const result = await call(commands.openUpdateNotes);
    if (result.status === "error") setError(`The browser couldn't open the release notes: ${result.error.message}`);
  }, []);

  return { status, checking, checkedNow, installing, error, later, checkNow, install, putOff, openNotes };
}
