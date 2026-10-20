/** An action a screen runs: what its busy indicator says, and the row it runs on, if any. */
export type BusyAction = { text: string; row: string | null };

/** The actions a screen runs, each shown from the moment it starts until its command answers. */
export type BusyActions = {
  /** Runs `run`, shown as `action` until it settles. */
  during: <T>(action: BusyAction, run: () => Promise<T>) => Promise<T>;
};

/** A new set of busy actions, none of them running, that `show` gets as they start and end. */
export function createBusy(show: (actions: BusyAction[]) => void): BusyActions {
  let now: BusyAction[] = [];
  const set = (next: BusyAction[]) => {
    now = next;
    show(now);
  };
  return {
    during: async (action, run) => {
      // Its own copy, so that the same action started twice ends twice.
      const started = { ...action };
      set([...now, started]);
      try {
        return await run();
      } finally {
        set(now.filter((each) => each !== started));
      }
    },
  };
}
