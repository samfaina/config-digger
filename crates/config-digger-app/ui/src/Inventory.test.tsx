import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import type { Entry, Item, State } from "./bindings";
import { StateBars } from "./Inventory";

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
    copied_from: null,
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

const items = [item(), item({ name: "context7" }), item({ name: "github", state: "shadowed", entries: [entry({ state: "shadowed" })] })];

/** The opening tag of each bar, in the order shown. */
const bars = (state: State | null) => renderToStaticMarkup(<StateBars items={items} state={state} onState={() => {}} />).match(/<button[^>]*class="bar [^"]*"[^>]*>/g) ?? [];

describe("the Entries by State filter", () => {
  test("marks the bar of the selected State as pressed and selected, and only that bar", () => {
    const [effective, shadowed] = bars("shadowed");
    expect(shadowed).toMatch(/aria-pressed="true"/);
    expect(shadowed).toMatch(/class="bar state-shadowed selected"/);
    expect(effective).toMatch(/aria-pressed="false"/);
    expect(effective).toMatch(/class="bar state-effective"/);
  });

  test("marks no bar as pressed while no State filters", () => {
    const found = bars(null);
    expect(found).toHaveLength(2);
    for (const bar of found) {
      expect(bar).toMatch(/aria-pressed="false"/);
      expect(bar).not.toMatch(/selected/);
    }
  });
});
