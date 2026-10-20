/**
 * The reads of the disk running, so that a focus re-read starts only when
 * none is: one full read takes seconds. A list change reads too, and so
 * does the Windows picker count, since its closing gives the window its
 * focus back.
 */
export type Reads = {
  /** Runs `read`, counted as running until it settles. */
  during: <T>(read: () => Promise<T>) => Promise<T>;
  /** Whether any read is running. */
  busy: () => boolean;
};

/** A new set of reads, none of them running. */
export function createReads(): Reads {
  let running = 0;
  return {
    during: async (read) => {
      running += 1;
      try {
        return await read();
      } finally {
        running -= 1;
      }
    },
    busy: () => running > 0,
  };
}

/** One screen's reads of its data, each run once the one before it is done. */
export type ReadQueue = {
  /**
   * Runs `read` once the read running is done, or at once when none is,
   * rather than beside it, as the disk may have changed since that one
   * started; done once `read` is.
   */
  next: (read: () => Promise<void>) => Promise<void>;
  /** Whether a read runs or waits to. */
  running: () => boolean;
};

/** A new queue of reads, none of them running. */
export function createReadQueue(): ReadQueue {
  let last: Promise<void> | null = null;
  return {
    next: (read) => {
      const reading: Promise<void> = (last ? last.then(read, read) : read()).finally(() => {
        if (last === reading) last = null;
      });
      last = reading;
      return reading;
    },
    running: () => last !== null,
  };
}

/** The app's reads, shared by every screen. */
export const reads = createReads();

/** A focus handler that runs `reread` only when none of `running` is running. */
export function whenIdle(running: Reads, reread: () => void): () => void {
  return () => {
    if (!running.busy()) reread();
  };
}

/**
 * What a screen shows of a read of its data: the loading state until its
 * first read is done, and a cue that does not block it while it reads again
 * over what it shows, its data or the error of the last read.
 */
export type ReadCue = "loading" | "rereading" | null;

/** The cue of a screen that `shown` its data or `failed` to read it, while `reading` it or not. */
export function readCue({ shown, reading, failed }: { shown: boolean; reading: boolean; failed: boolean }): ReadCue {
  if (!shown && !failed) return "loading";
  return reading ? "rereading" : null;
}
