import { act, cleanup } from "@testing-library/react";
import { afterEach, beforeEach, vi } from "vitest";

/** Runs each test of the calling `describe` on a fake clock, and cleans up its renders after it. */
export function onAFakeClock() {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });
}

/** Lets `ms` go by in steps, so that a timer an effect sets on the way runs too. */
export function wait(ms: number) {
  for (let left = ms; left > 0; left -= 100) act(() => vi.advanceTimersByTime(Math.min(left, 100)));
}
