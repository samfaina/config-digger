import { describe, expect, test } from "vitest";
import { checkFailedText, installFailedText, lastCheckedText } from "./updates";

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

describe("an update that was not installed", () => {
  test("says it was not installed once, then why", () => {
    expect(installFailedText("Download request failed with status: 404 Not Found")).toBe(
      "The update was not installed: Download request failed with status: 404 Not Found",
    );
  });

  test("reads the same when no update was found or the CLI runs", () => {
    expect(installFailedText("no update was found: check for updates first")).toBe(
      "The update was not installed: no update was found: check for updates first",
    );
    const cliRunning =
      "the config-digger CLI is running (C:\\Programs\\config-digger\\config-digger.exe), and the update cannot replace it while it runs: let it finish or close it, then try again";
    expect(installFailedText(cliRunning)).toBe(`The update was not installed: ${cliRunning}`);
  });
});
