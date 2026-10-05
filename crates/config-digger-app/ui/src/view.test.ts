import { describe, expect, test } from "vitest";
import type { Entry, Home, Inventory, Item, ReadFailure } from "./bindings";
import {
  agentStats,
  chosenHomes,
  entryActions,
  failuresInContext,
  filterItems,
  groupItems,
  flagsShown,
  homeFailures,
  itemKey,
  itemsShown,
  projectFailures,
  readerItem,
  stateCounts,
} from "./view";

function entry(overrides: Partial<Entry> = {}): Entry {
  return {
    path: String.raw`C:\Users\me\.claude.json`,
    key_path: ["mcpServers", "notes"],
    locator: String.raw`C:\Users\me\.claude.json#mcpServers.notes`,
    scope: "user",
    provenance: "file",
    skill_manager: null,
    plugin: null,
    git_versioned: false,
    link: null,
    parked: null,
    state: "effective",
    reason: null,
    secondary: [],
    what_if: null,
    notes: [],
    config: { command: "notes-mcp" },
    masked: [],
    disable: { status: "offered", path: String.raw`C:\Users\me\.claude.json`, sentence: "Disable adds `notes` to `disabledMcpServers`." },
    enable: null,
    remove: { status: "offered" },
    caveats: [],
    also_read_by: [],
    source: null,
    ...overrides,
  };
}

function item(overrides: Partial<Item> = {}): Item {
  return {
    agent: "claude-code",
    home: "default",
    kind: "mcp",
    name: "notes",
    state: "effective",
    entries: [entry()],
    built: null,
    caveats: [],
    ...overrides,
  };
}

function home(overrides: Partial<Home> = {}): Home {
  return {
    agent: "claude-code",
    agent_name: "Claude Code",
    home: "default",
    folder: String.raw`C:\Users\me`,
    source: { kind: "default" },
    hidden: false,
    not_read: null,
    mirror: null,
    no_configuration: null,
    scopes: ["user", "project"],
    installations: [],
    out_of_range: false,
    last_ran: null,
    notes: [],
    ...overrides,
  };
}

function failure(overrides: Partial<ReadFailure> = {}): ReadFailure {
  return {
    agent: "claude-code",
    agent_name: "Claude Code",
    home: "default",
    path: String.raw`C:\Users\me\.claude.json`,
    project: null,
    kind: "parse",
    position: { line: 3, column: 7 },
    reaction: "unknown",
    sentence: "Claude Code can't parse it.",
    notes: [],
    warnings: [],
    ...overrides,
  };
}

function inventory(overrides: Partial<Inventory> = {}): Inventory {
  return {
    read_at: "2026-10-04T10:42:00Z",
    project: null,
    projects: [],
    homes: [home()],
    items: [item()],
    read_failures: [],
    agent_flags: [],
    ...overrides,
  };
}

const desktopStore = home({ agent: "claude-desktop", agent_name: "Claude Desktop", home: "store" });
const desktopClassic = home({ agent: "claude-desktop", agent_name: "Claude Desktop", home: "classic" });

describe("chosen homes", () => {
  test("default to each Agent's first home the Agent reads", () => {
    const skipped = home({ home: "skipped", not_read: { read_instead: "default", sentence: "Skipped." } });
    const homes = [skipped, home(), desktopStore, desktopClassic];

    expect(chosenHomes(homes, {})).toEqual({ "claude-code": "default", "claude-desktop": "store" });
  });

  test("keep a choice the user made while that home is listed", () => {
    const homes = [desktopStore, desktopClassic];

    expect(chosenHomes(homes, { "claude-desktop": "classic" })).toEqual({ "claude-desktop": "classic" });
    expect(chosenHomes(homes, { "claude-desktop": "gone" })).toEqual({ "claude-desktop": "store" });
  });

  test("fall back to the first home when the Agent reads none of them", () => {
    const skipped = home({ not_read: { read_instead: null, sentence: "Skipped." } });

    expect(chosenHomes([skipped], {})).toEqual({ "claude-code": "default" });
  });
});

describe("items shown", () => {
  const items = [
    item({ name: "a" }),
    item({ name: "b", agent: "claude-desktop", home: "store" }),
    item({ name: "c", agent: "claude-desktop", home: "classic" }),
  ];
  const homes = { "claude-code": "default", "claude-desktop": "store" };

  test("are every Agent's Items in its chosen home under All Agents", () => {
    expect(itemsShown(items, { agent: "all", homes }).map((i) => i.name)).toEqual(["a", "b"]);
  });

  test("are one Agent's Items in its chosen home", () => {
    expect(itemsShown(items, { agent: "claude-desktop", homes }).map((i) => i.name)).toEqual(["b"]);
  });
});

describe("agent flags shown", () => {
  test("are those of the selected Agents in their chosen homes", () => {
    const flag = (agent: string, home: string) => ({
      agent,
      agent_name: agent,
      home,
      code: "cannot_start" as const,
      path: "config.toml",
      sentence: "It cannot start.",
    });
    const flags = [flag("codex", "default"), flag("codex", "orca"), flag("gemini", "default")];
    const homes = { codex: "default", gemini: "default" };

    expect(flagsShown(flags, { agent: "codex", homes })).toEqual([flags[0]]);
    expect(flagsShown(flags, { agent: "all", homes })).toEqual([flags[0], flags[2]]);
  });
});

describe("agent stats", () => {
  test("count Items in use and Entries with no effect", () => {
    const items = [
      item({ entries: [entry(), entry({ state: "shadowed" })] }),
      item({ name: "off", state: "disabled", entries: [entry({ state: "disabled" })] }),
    ];

    expect(agentStats(items)).toEqual({ items: 2, inUse: 1, noEffect: 2 });
  });
});

describe("state counts", () => {
  test("count Entries per State, in display order, leaving out States none has", () => {
    const items = [
      item({ entries: [entry(), entry({ state: "shadowed" }), entry({ state: "shadowed" })] }),
      item({ state: "invalid", entries: [entry({ state: "invalid" })] }),
    ];

    expect(stateCounts(items)).toEqual({
      total: 4,
      counts: [
        { state: "effective", count: 1 },
        { state: "shadowed", count: 2 },
        { state: "invalid", count: 1 },
      ],
    });
  });
});

describe("filtering items", () => {
  const items = [
    item({ name: "GitHub" }),
    item({ name: "deploy", kind: "skill" }),
    item({ name: "linear", entries: [entry(), entry({ state: "shadowed" })] }),
  ];
  const names = (filtered: Item[]) => filtered.map((i) => i.name);

  test("by kind", () => {
    expect(names(filterItems(items, { kind: "skill", text: "", state: null }))).toEqual(["deploy"]);
  });

  test("by text, ignoring case", () => {
    expect(names(filterItems(items, { kind: "all", text: "git", state: null }))).toEqual(["GitHub"]);
  });

  test("by a State any of their Entries has", () => {
    expect(names(filterItems(items, { kind: "all", text: "", state: "shadowed" }))).toEqual(["linear"]);
  });
});

describe("grouping items", () => {
  const homes = [home(), desktopStore];
  const items = [
    item({ name: "a" }),
    item({ name: "s", kind: "skill" }),
    item({ name: "b", agent: "claude-desktop", home: "store" }),
  ];

  test("is by Agent under All Agents", () => {
    const groups = groupItems(items, "all", homes);

    expect(groups.map((g) => [g.title, g.items.map((i) => i.name)])).toEqual([
      ["Claude Code", ["a", "s"]],
      ["Claude Desktop", ["b"]],
    ]);
  });

  test("is by kind for one Agent", () => {
    const groups = groupItems(items.slice(0, 2), "claude-code", homes);

    expect(groups.map((g) => [g.title, g.items.map((i) => i.name)])).toEqual([
      ["MCP servers", ["a"]],
      ["Skills", ["s"]],
    ]);
  });
});

describe("read failures in context", () => {
  const homes = { "claude-code": "default" };

  test("are those outside any Project and those of the context's Project, in the chosen homes", () => {
    const failures = [
      failure({ path: "user" }),
      failure({ path: "here", project: String.raw`E:\app` }),
      failure({ path: "elsewhere", project: String.raw`E:\other` }),
      failure({ path: "other home", home: "work" }),
    ];

    const inContext = failuresInContext(failures, String.raw`e:\APP`, { agent: "all", homes });

    expect(inContext.map((f) => f.path)).toEqual(["user", "here"]);
  });

  test("with no Project are only those outside any Project", () => {
    const failures = [failure({ path: "user" }), failure({ path: "here", project: String.raw`E:\app` })];

    expect(failuresInContext(failures, null, { agent: "all", homes }).map((f) => f.path)).toEqual(["user"]);
  });

  test("badge their home when outside any Project, or else their Project", () => {
    const failures = [
      failure({ path: "user" }),
      failure({ path: "work", home: "work" }),
      failure({ path: "here", project: String.raw`E:\app` }),
    ];

    expect(homeFailures(failures, home())).toBe(1);
    expect(projectFailures(failures, String.raw`e:\app`)).toBe(1);
    expect(projectFailures(failures, String.raw`E:\other`)).toBe(0);
  });
});

describe("entry actions", () => {
  test("offer Disable first and Remove second, with what each reaches", () => {
    const actions = entryActions(
      entry({
        also_read_by: [
          { agent: "vscode", agent_name: "VS Code", home: "default", state: "shadowed", sentence: "" },
          { agent: "vscode", agent_name: "VS Code", home: "insiders", state: "shadowed", sentence: "" },
        ],
      }),
      "Claude Code",
    );

    expect(actions.primary).toEqual({
      verb: "disable",
      offered: true,
      note: "Disable adds `notes` to `disabledMcpServers`.",
    });
    expect(actions.remove).toEqual({
      offered: true,
      danger: false,
      note: "Remove reaches Claude Code, VS Code.",
    });
    expect(actions.howToRemove).toBeNull();
  });

  test("note when Remove reaches clones or removes a link only", () => {
    const actions = entryActions(
      entry({ git_versioned: true, link: { target: String.raw`C:\skills\x`, broken: false } }),
      "Claude Code",
    );

    expect(actions.remove.note).toBe(
      "Remove reaches Claude Code. It is git-versioned, so it needs its own confirmation. It removes the junction only, never its target.",
    );
  });

  test("offer Enable for a Disabled Entry, or say why not", () => {
    const offered = entryActions(
      entry({ state: "disabled", enable: { status: "offered", sentence: "Enable removes `notes`." } }),
      "Claude Code",
    );
    const refused = entryActions(
      entry({ state: "disabled", enable: { status: "not_offered", sentence: "Enable is not offered: a rule." } }),
      "Claude Code",
    );

    expect(offered.primary).toEqual({ verb: "enable", offered: true, note: "Enable removes `notes`." });
    expect(refused.primary).toEqual({ verb: "enable", offered: false, note: "Enable is not offered: a rule." });
  });

  test("show Disable not offered with the reason", () => {
    const actions = entryActions(
      entry({ disable: { status: "not_offered", code: "git_versioned", sentence: "Disable is not offered: git." } }),
      "Claude Code",
    );

    expect(actions.primary).toEqual({ verb: "disable", offered: false, note: "Disable is not offered: git." });
    expect(actions.remove.danger).toBe(false);
  });

  test("make Remove the danger action when it is the only one, as for an Unread Entry", () => {
    const actions = entryActions(
      entry({ state: "unread", disable: { status: "not_offered", code: "unread", sentence: "Disable is not offered." } }),
      "Claude Code",
    );

    expect(actions.primary).toBeNull();
    expect(actions.remove.danger).toBe(true);
  });

  test("say how to remove what config-digger does not Remove", () => {
    const how = { command: "claude plugin uninstall supabase", sentence: "Uninstall the plugin in Claude Code." };
    const actions = entryActions(
      entry({
        provenance: "plugin",
        remove: { status: "not_offered", code: "plugin", sentence: "Not in v0.1: uninstall the plugin.", how_to_remove: how },
      }),
      "Claude Code",
    );

    expect(actions.remove).toEqual({ offered: false, danger: false, note: "Not in v0.1: uninstall the plugin." });
    expect(actions.howToRemove).toEqual(how);
  });
});

describe("item keys", () => {
  test("tell apart same-named Items of an Agent whose Entries never compete", () => {
    const user = item({ agent: "cursor", kind: "skill", entries: [entry({ locator: "C:/a/unslop" })] });
    const synced = item({ agent: "cursor", kind: "skill", entries: [entry({ locator: "C:/b/unslop" })] });

    expect(itemKey(user)).not.toBe(itemKey(synced));
  });
});

describe("the Item of another Agent that reads an Entry", () => {
  const shared = entry({ locator: "E:/app/.mcp.json#mcpServers.x" });
  const vscodeItem = item({ agent: "vscode", home: "default", entries: [entry(), shared] });

  test("is found by its Agent, home and the Entry's locator", () => {
    const reader = { agent: "vscode", agent_name: "VS Code", home: "default", state: "shadowed" as const, sentence: "" };

    expect(readerItem(inventory({ items: [item(), vscodeItem] }), reader, shared.locator)).toBe(vscodeItem);
  });

  test("is none when that home is not listed", () => {
    const reader = { agent: "vscode", agent_name: "VS Code", home: "insiders", state: "shadowed" as const, sentence: "" };

    expect(readerItem(inventory({ items: [vscodeItem] }), reader, shared.locator)).toBeNull();
  });
});
