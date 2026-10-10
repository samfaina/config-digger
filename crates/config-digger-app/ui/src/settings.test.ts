import { describe, expect, test } from "vitest";
import type { Agent, ParkedEntry, SettingsRead } from "./bindings";
import { dataFolderRows, emptyInventory, parkedRows } from "./settings";

const LOCAL = String.raw`C:\Users\me\AppData\Local\config-digger`;

function parked(overrides: Partial<ParkedEntry> = {}): ParkedEntry {
  return {
    agent: "claude-desktop",
    agent_name: "Claude Desktop",
    home: "msix",
    kind: "mcp",
    name: "notes",
    project: null,
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

  test("counts one Operation", () => {
    expect(dataFolderRows(read({ operations: 1 }))[0].description).toMatch(/^1 Operation kept\. /);
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
    expect(rows[1].start.project).toBe(project);
  });
});

describe("the empty inventory", () => {
  function agent(overrides: Partial<Agent> = {}): Agent {
    return { id: "claude-code", name: "Claude Code", built_in: true, tested: null, variables: [], homes: [], ...overrides };
  }

  test("says what was looked for when no Agent home was found", () => {
    const empty = emptyInventory([
      agent(),
      agent({ id: "codex", name: "Codex", variables: ["CODEX_HOME"] }),
      agent({ id: "gemini-cli", name: "Gemini CLI", variables: ["GEMINI_CLI_HOME"] }),
    ]);

    expect(empty).toEqual({
      kind: "none_found",
      lookedFor:
        "config-digger looked for Claude Code, Codex and Gemini CLI in their default and known locations " +
        "and through CODEX_HOME and GEMINI_CLI_HOME. It does not scan the disk. If an Agent keeps its " +
        "configuration somewhere else, add that folder as an Agent home.",
    });
  });

  test("names one variable and one Agent without a list", () => {
    const empty = emptyInventory([agent({ id: "codex", name: "Codex", variables: ["CODEX_HOME"] })]);

    expect(empty.kind === "none_found" && empty.lookedFor).toMatch(
      /^config-digger looked for Codex in its default and known locations and through CODEX_HOME\. /,
    );
  });

  test("says every home is hidden when the user hid all of those found", () => {
    const hidden = { agent: "claude-code", home: "default", hidden: true } as Agent["homes"][number];

    expect(emptyInventory([agent({ homes: [hidden, { ...hidden, home: "other" }] })])).toEqual({ kind: "all_hidden", hidden: 2 });
  });
});
