import { describe, expect, test } from "vitest";
import { keyPieces, lastPart, pathPieces } from "./wrap";

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

describe("a path's last part apart from the rest", () => {
  test("starts the last part at the separator before it", () => {
    expect(lastPart(String.raw`E:\cd-throwaway\claude-code-many-home`)).toEqual({ rest: String.raw`E:\cd-throwaway`, last: String.raw`\claude-code-many-home` });
    expect(lastPart("/home/me/.codex-work")).toEqual({ rest: "/home/me", last: "/.codex-work" });
  });

  test("keeps a trailing separator with the last part", () => {
    expect(lastPart("E:\\cd-throwaway\\")).toEqual({ rest: "E:", last: "\\cd-throwaway\\" });
  });

  test("is all last part when there is no folder before it", () => {
    expect(lastPart("default")).toEqual({ rest: "", last: "default" });
    expect(lastPart("/srv")).toEqual({ rest: "", last: "/srv" });
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
