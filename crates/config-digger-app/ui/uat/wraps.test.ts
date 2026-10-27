import { describe, expect, test } from "vitest";
import { wrapFault } from "./wraps";

describe("the paths of a list, each as its lines", () => {
  test("have nothing to show when none takes more than one line", () => {
    expect(wrapFault([[String.raw`E:\cd-throwaway\project`], [String.raw`E:\cd-throwaway\project-extra`]])).toBe(
      "None of the 2 paths takes more than one line, so none shows where it wraps.",
    );
  });

  test("have nothing to show when there is no path", () => {
    expect(wrapFault([])).toBe("None of the 0 paths takes more than one line, so none shows where it wraps.");
  });

  test("show a path that breaks inside a folder's name, though another wraps right after a separator", () => {
    expect(wrapFault([["E:\\cd-throwaway\\", "project"], ["E:\\cd-throwaway\\proj", "ect-extra"]])).toBe(
      String.raw`A path breaks inside a folder's name: E:\cd-throwaway\proj ⏎ ect-extra`,
    );
  });

  test("have nothing wrong when one wraps, each line but its last ending in a separator, beside one that fits on a line", () => {
    expect(wrapFault([["E:\\cd-throwaway\\project"], ["E:\\cd-throwaway\\", "a-folder/", "project-long"]])).toBeUndefined();
  });
});
