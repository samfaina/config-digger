/** An action a screen runs: what its busy indicator says, and the row it runs on, if any. */
export type Running = { text: string; row: string | null };

/** The actions a screen runs, each shown from the moment it starts until its command answers. */
export type Runnings = {
  /** Runs `action`, shown as `running` until it settles. */
  during: <T>(running: Running, action: () => Promise<T>) => Promise<T>;
};

/** A new set of running actions, none of them running, that `show` gets as they start and end. */
export function createRunning(show: (running: Running[]) => void): Runnings {
  let now: Running[] = [];
  const set = (next: Running[]) => {
    now = next;
    show(now);
  };
  return {
    during: async (running, action) => {
      // Its own copy, so that the same action started twice ends twice.
      const started = { ...running };
      set([...now, started]);
      try {
        return await action();
      } finally {
        set(now.filter((each) => each !== started));
      }
    },
  };
}
