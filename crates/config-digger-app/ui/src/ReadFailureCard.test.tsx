// @vitest-environment happy-dom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";
import { commands, type Backup, type ReadFailure } from "./bindings";
import { ReadFailureCard } from "./ReadFailureCard";

const CONFIG = String.raw`E:\cd-throwaway\codex-home\config.toml`;
const backup: Backup = {
  operation: "20260101T000000Z-disable",
  created_at: "2026-01-01T00:00:00Z",
  title: "Disable 1 Entry",
  copy: String.raw`C:\Users\me\AppData\Local\config-digger\backups\20260101T000000Z-disable\1-config.toml`,
};

vi.mock("./bindings", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./bindings")>();
  const done = async () => ({ status: "ok" as const, data: null });
  return {
    ...actual,
    commands: {
      ...actual.commands,
      backupsOf: vi.fn(async () => ({ status: "ok" as const, data: [backup] })),
      openBackupFolder: vi.fn(done),
      revealLocation: vi.fn(done),
    },
  };
});

afterEach(cleanup);

const failure: ReadFailure = {
  agent: "codex",
  agent_name: "Codex",
  home: "default",
  path: CONFIG,
  project: null,
  location: null,
  kind: "parse",
  position: { line: 3, column: 1 },
  reaction: "ignores_file",
  sentence: "config.toml does not parse.",
  notes: [],
  warnings: [],
};

describe("a Read failure's backups in History", () => {
  test("show the backup copy selected in its folder, rather than opening the folder", async () => {
    render(<ReadFailureCard failure={failure} notify={() => {}} />);

    (await screen.findByRole("button", { name: /Disable 1 Entry/ })).click();

    expect(commands.revealLocation).toHaveBeenCalledWith(backup.copy);
    expect(commands.openBackupFolder).not.toHaveBeenCalled();
  });
});
