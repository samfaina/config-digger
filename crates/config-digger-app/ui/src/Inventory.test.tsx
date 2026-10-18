import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import type { Entry, Item, State } from "./bindings";
import { StateBars } from "./Inventory";

function entry(name: string, state: State): Entry {
  return {
    path: String.raw`C:\Users\me\.claude.json`,
    key_path: ["mcpServers", name],
    locator: String.raw`C:\Users\me\.claude.json#mcpServers.` + name,
    scope: "user",
    provenance: "file",
    skill_manager: null,
    plugin: null,
    git_versioned: false,
    link: null,
    parked: null,
    state,
    reason: null,
    secondary: [],
    what_if: null,
    notes: [],
    config: { command: `${name}-mcp` },
    masked: [],
    disable: { status: "offered", path: String.raw`C:\Users\me\.claude.json`, sentence: `Disable adds \`${name}\` to \`disabledMcpServers\`.` },
    enable: null,
    remove: { status: "offered" },
    caveats: [],
    also_read_by: [],
    copied_from: null,
  };
}

function item(name: string, state: State): Item {
  return { agent: "claude-code", home: "default", kind: "mcp", name, state, entries: [entry(name, state)], built: null, caveats: [] };
}

const items = [item("notes", "effective"), item("context7", "effective"), item("github", "shadowed")];

/** The opening tag of each filter row, in the order shown. */
const rows = (state: State | null) => renderToStaticMarkup(<StateBars items={items} state={state} onState={() => {}} />).match(/<button[^>]*class="bar [^"]*"[^>]*>/g) ?? [];

describe("the Entries by State filter", () => {
  test("marks the selected row as pressed and selected, and only that row", () => {
    const [effective, shadowed] = rows("shadowed");
    expect(shadowed).toMatch(/aria-pressed="true"/);
    expect(shadowed).toMatch(/class="bar state-shadowed selected"/);
    expect(effective).toMatch(/aria-pressed="false"/);
    expect(effective).toMatch(/class="bar state-effective"/);
  });

  test("marks no row as pressed while nothing filters", () => {
    const found = rows(null);
    expect(found).toHaveLength(2);
    for (const row of found) {
      expect(row).toMatch(/aria-pressed="false"/);
      expect(row).not.toMatch(/selected/);
    }
  });
});
