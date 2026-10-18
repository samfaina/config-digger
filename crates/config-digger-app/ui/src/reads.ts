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

/** The app's reads, shared by every screen. */
export const reads = createReads();
