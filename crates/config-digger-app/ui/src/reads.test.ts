import { describe, expect, test } from "vitest";
import { createReads, readCue, whenIdle } from "./reads";

/** A read that settles when told to. */
function pending() {
  let settle: (ok: boolean) => void = () => {};
  const promise = new Promise<string>((resolve, reject) => {
    settle = (ok) => (ok ? resolve("read") : reject(new Error("failed")));
  });
  return { promise, settle };
}

describe("a focus re-read", () => {
  test("may start when nothing is read", () => {
    expect(createReads().busy()).toBe(false);
  });

  test("waits while a read runs, and may start once it is done", async () => {
    const reads = createReads();
    const read = pending();

    const running = reads.during(() => read.promise);
    expect(reads.busy()).toBe(true);

    read.settle(true);
    expect(await running).toBe("read");
    expect(reads.busy()).toBe(false);
  });

  test("may start once a failed read is done", async () => {
    const reads = createReads();
    const read = pending();

    const running = reads.during(() => read.promise);
    read.settle(false);

    await expect(running).rejects.toThrow("failed");
    expect(reads.busy()).toBe(false);
  });

  test("waits until every read running is done", async () => {
    const reads = createReads();
    const first = pending();
    const second = pending();

    const runningFirst = reads.during(() => first.promise);
    const runningSecond = reads.during(() => second.promise);
    first.settle(true);
    await runningFirst;
    expect(reads.busy()).toBe(true);

    second.settle(true);
    await runningSecond;
    expect(reads.busy()).toBe(false);
  });
});

describe("a focus handler", () => {
  test("reads again only when nothing else is read", async () => {
    const reads = createReads();
    let reread = 0;
    const onFocus = whenIdle(reads, () => (reread += 1));
    const read = pending();

    const running = reads.during(() => read.promise);
    onFocus();
    expect(reread).toBe(0);

    read.settle(true);
    await running;
    onFocus();
    expect(reread).toBe(1);
  });
});

describe("what a screen shows while it reads its data", () => {
  test("is the loading state before its first read is done, even before that read starts", () => {
    expect(readCue({ shown: false, reading: false, failed: false })).toBe("loading");
    expect(readCue({ shown: false, reading: true, failed: false })).toBe("loading");
  });

  test("is a cue over the data already shown while it reads again", () => {
    expect(readCue({ shown: true, reading: true, failed: false })).toBe("rereading");
    expect(readCue({ shown: true, reading: true, failed: true })).toBe("rereading");
  });

  test("is nothing once its data is shown and nothing is read", () => {
    expect(readCue({ shown: true, reading: false, failed: false })).toBeNull();
  });

  test("is nothing when its first read failed, so that the error shows instead", () => {
    expect(readCue({ shown: false, reading: false, failed: true })).toBeNull();
  });

  test("is a cue over the error while it reads again after its first read failed", () => {
    expect(readCue({ shown: false, reading: true, failed: true })).toBe("rereading");
  });
});
