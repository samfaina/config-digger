import { describe, expect, test } from "vitest";
import { checkFailedText, lastCheckedText } from "./updates";

/** A time on the user's clock, as the core records it. */
function at(year: number, month: number, day: number, hours: number, minutes: number): string {
  return new Date(year, month, day, hours, minutes).toISOString();
}

describe("the last update check", () => {
  const now = new Date(2026, 9, 10, 18, 0);

  test("was never, until the first check", () => {
    expect(lastCheckedText(null, now)).toBe("Last checked: never.");
  });

  test("is when the latest check ran, on the user's clock", () => {
    expect(lastCheckedText(at(2026, 9, 10, 17, 47), now)).toBe("Last checked: Today 17:47.");
    expect(lastCheckedText(at(2026, 8, 30, 9, 12), now)).toBe("Last checked: Sep 30 09:12.");
  });
});

describe("a failed update check", () => {
  test("says it failed once, then why", () => {
    expect(checkFailedText("Could not fetch a valid release JSON from the remote")).toBe(
      "The update check failed: Could not fetch a valid release JSON from the remote",
    );
  });
});
