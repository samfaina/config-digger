import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import type { Entry, Home, Item, Project, State } from "./bindings";
import { StateBars } from "./Inventory";
import { chosenHomes, homeChoices, withHomeChosen, withProjectChosen } from "./view";

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

function home(overrides: Partial<Home> = {}): Home {
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

function project(path: string, missing = false): Project {
  return { path, missing, workspace_file: false, members: [], registered_by: [], added: true, hidden: false, entries: 0 };
}

describe("choosing a Project", () => {
  const codexHome = String.raw`E:\cd-throwaway\codex-home`;
  const homes = [home(), home({ home: codexHome, folder: codexHome, source: { kind: "added" } })];
  const present = String.raw`E:\cd-throwaway\project`;
  const projects = [project(present)];

  test("keeps the home chosen for each Agent", () => {
    const chosen = withProjectChosen(withHomeChosen(homeChoices(), "codex", codexHome), projects, present);

    expect(chosenHomes(homes, chosen, [])).toEqual({ codex: codexHome });
  });

  describe("that is missing", () => {
    const gone = String.raw`E:\cd-throwaway\project-gone`;
    const claudeHome = String.raw`E:\cd-throwaway\claude-code-home`;
    const claudeFile = String.raw`E:\cd-throwaway\claude-code-home\.claude.json`;
    const both = [
      ...homes,
      home({ agent: "claude-code", agent_name: "Claude Code" }),
      home({ agent: "claude-code", agent_name: "Claude Code", home: claudeHome, folder: claudeHome, source: { kind: "added" } }),
    ];
    const leftover = item({
      home: claudeHome,
      name: "cd-throwaway-leftover",
      state: "unread",
      entries: [
        entry({
          path: claudeFile,
          key_path: ["projects", "E:/cd-throwaway/project-gone", "mcpServers", "cd-throwaway-leftover"],
          locator: `${claudeFile}#projects.E:/cd-throwaway/project-gone.mcpServers.cd-throwaway-leftover`,
          scope: "local",
          state: "unread",
          reason: { code: "project_missing", effective: null, switch: null, sentence: "Its Project is missing." },
        }),
      ],
    });
    const withGone = [...projects, project(gone, true)];
    // The user chose the default home of Claude Code and codex-home before choosing the missing Project.
    const earlier = withHomeChosen(withHomeChosen(homeChoices(), "claude-code", "default"), "codex", codexHome);

    test("opens on the homes holding its leftovers, and keeps the home chosen for the other Agents", () => {
      const chosen = withProjectChosen(earlier, withGone, gone);

      expect(chosenHomes(both, chosen, [leftover])).toEqual({ codex: codexHome, "claude-code": claudeHome });
    });

    test("shows the home the user then chooses, and keeps the other Agents on theirs", () => {
      const opened = withProjectChosen(earlier, withGone, gone);
      const codexDefault = withHomeChosen(opened, "codex", "default");
      const claudeDefault = withHomeChosen(codexDefault, "claude-code", "default");

      expect(chosenHomes(both, codexDefault, [leftover])).toEqual({ codex: "default", "claude-code": claudeHome });
      expect(chosenHomes(both, claudeDefault, [leftover])).toEqual({ codex: "default", "claude-code": "default" });
    });

    test("leaves no home chosen that the user didn't choose, once another Project is chosen", () => {
      const opened = withProjectChosen(homeChoices(), withGone, gone);
      const after = withProjectChosen(withHomeChosen(opened, "codex", codexHome), withGone, present);

      expect(chosenHomes(both, after, [])).toEqual({ codex: codexHome, "claude-code": "default" });
    });
  });
});
