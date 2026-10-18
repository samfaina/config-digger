import { afterEach, describe, expect, test, vi } from "vitest";
import { copy } from "./desktop";
import type { Notice } from "./Notice";

afterEach(() => {
  vi.unstubAllGlobals();
});

/** The notices `copy` gives with a clipboard whose `writeText` is `writeText`. */
const copying = async (writeText: (text: string) => Promise<void>) => {
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  const notices: Notice[] = [];
  await copy("C:\\Users\\me\\.claude.json", "path", (notice) => notices.push(notice));
  return notices;
};

describe("copying", () => {
  test("says it copied, as a success", async () => {
    expect(await copying(async () => {})).toEqual([{ kind: "success", text: "Copied the path." }]);
  });

  test("says why it couldn't, as an error", async () => {
    const notices = await copying(async () => {
      throw new Error("denied");
    });
    expect(notices).toEqual([{ kind: "error", text: "The path couldn't be copied: Error: denied" }]);
  });
});
