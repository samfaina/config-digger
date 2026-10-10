import { describe, expect, test } from "vitest";
import type { ParkedEntry, SettingsRead } from "./bindings";
import { dataFolderRows, parkedRows } from "./settings";

const LOCAL = String.raw`C:\Users\me\AppData\Local\config-digger`;

function parked(overrides: Partial<ParkedEntry> = {}): ParkedEntry {
  return {
    agent: "claude-desktop",
    agent_name: "Claude Desktop",
    home: "msix",
    kind: "mcp",
    name: "notes",
    project: null,
    hidden: false,
    parked: {
      path: String.raw`C:\Users\me\AppData\Roaming\Claude\claude_desktop_config.json`,
      key_path: ["mcpServers", "notes"],
      locator: String.raw`C:\Users\me\AppData\Roaming\Claude\claude_desktop_config.json#mcpServers.notes`,
      parked_at: "2026-10-01T14:30:12Z",
    },
    ...overrides,
  };
}

function read(overrides: Partial<SettingsRead> = {}): SettingsRead {
  return {
    settings: { check_updates_on_startup: false },
    folders: {
      backups: String.raw`${LOCAL}\backups`,
      parked: String.raw`${LOCAL}\parked`,
      logs: String.raw`${LOCAL}\logs`,
      lists: String.raw`C:\Users\me\AppData\Roaming\config-digger`,
    },
    operations: 3,
    retention: { operations: 50, days: 30 },
    parked: [],
    ...overrides,
  };
}

describe("Data on this computer", () => {
  test("lists each folder with its path and what it holds, the fixed retention included", () => {
    const rows = dataFolderRows(read());

    expect(rows.map((row) => [row.folder, row.title, row.path])).toEqual([
      ["backups", "Backups and Operation history", String.raw`${LOCAL}\backups`],
      ["parked", "Parked Entries", String.raw`${LOCAL}\parked`],
      ["logs", "Logs", String.raw`${LOCAL}\logs`],
      ["lists", "Your lists and settings", String.raw`C:\Users\me\AppData\Roaming\config-digger`],
    ]);
    expect(rows[0].description).toBe(
      "3 Operations kept. History keeps the last 50 Operations and everything from the last 30 days, " +
        "and prunes older ones with their backups at startup. Operations that need attention are never pruned.",
    );
    expect(rows[1].description).toBe(
      "Entries config-digger Disabled for Agents that have no Disable of their own. Never pruned: Enable puts them back.",
    );
    expect(rows[2].description).toBe("Paths, key paths, reason codes and errors only, never configuration values.");
    expect(rows[3].description).toBe("The Agent homes and Projects you added or hid, and these settings.");
  });

  test("counts one Operation, and says how long History keeps them as the core does", () => {
    const description = dataFolderRows(read({ operations: 1, retention: { operations: 20, days: 7 } }))[0].description;

    expect(description).toMatch(/^1 Operation kept\. History keeps the last 20 Operations and everything from the last 7 days, /);
  });
});

describe("the parked Entries", () => {
  test("say what each one is and where it was, and open the Inventory on its Item", () => {
    const project = String.raw`E:\code\shop`;
    const rows = parkedRows([
      parked(),
      parked({ agent: "cursor", agent_name: "Cursor", home: "default", kind: "skill", name: "pdf", project }),
    ]);

    expect(rows.map((row) => [row.title, row.meta])).toEqual([
      [
        "notes",
        String.raw`MCP server · Claude Desktop · from C:\Users\me\AppData\Roaming\Claude\claude_desktop_config.json#mcpServers.notes`,
      ],
      ["pdf", String.raw`Skill · Cursor · E:\code\shop · from C:\Users\me\AppData\Roaming\Claude\claude_desktop_config.json#mcpServers.notes`],
    ]);
    expect(rows[0].start).toEqual({
      project: null,
      agent: "claude-desktop",
      homes: { "claude-desktop": "msix" },
      state: null,
      item: { agent: "claude-desktop", home: "msix", kind: "mcp", name: "notes" },
    });
    expect(rows[1].start?.project).toBe(project);
  });

  test("say when an Entry's Agent home is hidden, which the Inventory does not list", () => {
    const [row] = parkedRows([parked({ hidden: true })]);

    expect(row.meta).toMatch(/ · in a hidden Agent home$/);
    expect(row.start).toBeNull();
  });
});
