import type { Entry, Home, Inventory } from "./bindings";

/** An Entry of Claude Code's user configuration, the MCP server `notes`, in effect. */
export function entry(overrides: Partial<Entry> = {}): Entry {
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
    copied_from: null,
    ...overrides,
  };
}

/** Codex's default home. */
export function home(overrides: Partial<Home> = {}): Home {
  return {
    agent: "codex",
    agent_name: "Codex",
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
    entries: 0,
    notes: [],
    ...overrides,
  };
}

/** An inventory of Codex's default home and no Project. */
export function inventory(overrides: Partial<Inventory> = {}): Inventory {
  return { read_at: "2026-10-20T09:30:00Z", project: null, projects: [], homes: [home()], items: [], read_failures: [], store_read_failures: [], agent_flags: [], ...overrides };
}
