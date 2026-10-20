import { describe, expect, test } from "vitest";
import type { CommandResult } from "./desktop";
import { createRunning, type Running } from "./running";

/** A command that settles when the test tells it to. */
function pending() {
  let settle: (result: CommandResult<string>) => void = () => {};
  let fail: () => void = () => {};
  const promise = new Promise<CommandResult<string>>((resolve, reject) => {
    settle = resolve;
    fail = () => reject(new Error("crashed"));
  });
  return { promise, settle, fail };
}

const HIDING: Running = { text: String.raw`Hiding E:\cd-throwaway\project-hide…`, row: String.raw`E:\cd-throwaway\project-hide` };
const UNHIDING: Running = { text: String.raw`Unhiding E:\cd-throwaway\project-old…`, row: String.raw`E:\cd-throwaway\project-old` };

/** A set of running actions, and every list of them it showed. */
function watched() {
  const shown: Running[][] = [];
  return { running: createRunning((now) => shown.push(now)), shown, now: () => shown.at(-1) ?? [] };
}

describe("an action on the Projects & homes screen", () => {
  test("shows as running as soon as it starts, before its command answers", () => {
    const { running, now } = watched();
    const command = pending();

    void running.during(HIDING, () => command.promise);

    expect(now()).toEqual([HIDING]);
  });

  test("stops showing once its command answers, which it hands back", async () => {
    const { running, now } = watched();
    const command = pending();

    const done = running.during(HIDING, () => command.promise);
    command.settle({ status: "ok", data: "hidden" });

    expect(await done).toEqual({ status: "ok", data: "hidden" });
    expect(now()).toEqual([]);
  });

  test("stops showing once its command is refused, so that the refusal shows instead", async () => {
    const { running, now } = watched();
    const command = pending();
    const refused: CommandResult<string> = {
      status: "error",
      error: { code: "unexpected", message: "refused", candidates: [], backup: null, diffs: [], failed: null },
    };

    const done = running.during(HIDING, () => command.promise);
    command.settle(refused);

    expect(await done).toEqual(refused);
    expect(now()).toEqual([]);
  });

  test("stops showing when its command throws", async () => {
    const { running, now } = watched();
    const command = pending();

    const done = running.during(HIDING, () => command.promise);
    command.fail();

    await expect(done).rejects.toThrow("crashed");
    expect(now()).toEqual([]);
  });

  test("on one row leaves another row's action showing until that one answers too", async () => {
    const { running, now } = watched();
    const hide = pending();
    const unhide = pending();

    const hidden = running.during(HIDING, () => hide.promise);
    const unhidden = running.during(UNHIDING, () => unhide.promise);
    expect(now()).toEqual([HIDING, UNHIDING]);

    hide.settle({ status: "ok", data: "hidden" });
    await hidden;
    expect(now()).toEqual([UNHIDING]);

    unhide.settle({ status: "ok", data: "listed" });
    await unhidden;
    expect(now()).toEqual([]);
  });
});
