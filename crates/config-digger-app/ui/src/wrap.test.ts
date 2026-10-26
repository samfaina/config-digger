import { describe, expect, test } from "vitest";
import { keyPieces, pathPieces } from "./wrap";

describe("a path in pieces that each end where its line may wrap", () => {
  test("ends a piece after each backslash of a Windows path", () => {
    expect(pathPieces(String.raw`E:\cd-throwaway\project-extra`)).toEqual(["E:\\", "cd-throwaway\\", "project-extra"]);
  });

  test("ends a piece after each slash of a POSIX path", () => {
    expect(pathPieces("/home/me/project.v2")).toEqual(["/", "home/", "me/", "project.v2"]);
  });

  test("keeps the dots and hashes of a folder name inside its piece", () => {
    expect(pathPieces(String.raw`C:\Users\me\.claude#x`)).toEqual(["C:\\", "Users\\", "me\\", ".claude#x"]);
  });

  test("keeps a path with no separator whole", () => {
    expect(pathPieces("project-extra")).toEqual(["project-extra"]);
  });

  test("ends its last piece at a trailing separator", () => {
    expect(pathPieces("E:\\cd-throwaway\\")).toEqual(["E:\\", "cd-throwaway\\"]);
    expect(pathPieces("/srv/")).toEqual(["/", "srv/"]);
  });

  test("has no piece for an empty path", () => {
    expect(pathPieces("")).toEqual([]);
  });
});

describe("a key in pieces that each end where its line may wrap", () => {
  test("ends a piece after each path or key separator", () => {
    expect(keyPieces("projects.E:/cd-throwaway/project-gone.mcpServers.x#y")).toEqual([
      "projects.",
      "E:/",
      "cd-throwaway/",
      "project-gone.",
      "mcpServers.",
      "x#",
      "y",
    ]);
  });
});
