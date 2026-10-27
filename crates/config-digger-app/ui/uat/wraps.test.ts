import { describe, expect, test } from "vitest";
import { wrapFault } from "./wraps";

describe("the wrap check of a list's paths, each as its lines", () => {
  test("fails when none takes more than one line", () => {
    expect(wrapFault([[String.raw`E:\cd-throwaway\project`], [String.raw`E:\cd-throwaway\project-extra`]])).toBe(
      "None of the 2 paths takes more than one line, so none shows where it wraps.",
    );
  });

  test("fails when there is no path", () => {
    expect(wrapFault([])).toBe("None of the 0 paths takes more than one line, so none shows where it wraps.");
  });

  test("fails when one breaks inside a folder's name, though another wraps right after a separator", () => {
    expect(wrapFault([["E:\\cd-throwaway\\", "project"], ["E:\\cd-throwaway\\proj", "ect-extra"]])).toBe(
      String.raw`A path breaks inside a folder's name: E:\cd-throwaway\proj ⏎ ect-extra`,
    );
  });

  test("passes when one wraps, each line but its last ending in a separator, beside one that fits on a line", () => {
    expect(wrapFault([["E:\\cd-throwaway\\project"], ["E:\\cd-throwaway\\", "a-folder/", "project-long"]])).toBeUndefined();
  });
});
