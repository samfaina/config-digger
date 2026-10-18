import { describe, expect, test } from "vitest";
import { createReads } from "./reads";

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
