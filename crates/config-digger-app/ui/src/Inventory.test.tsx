import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import type { Entry, Home, Inventory, Item, LinkKind, Project, State } from "./bindings";
import { CLOSED, onKey, onOptionClick } from "./dropdown";
import { EntryCard } from "./EntryCard";
import { Banner, projectField, StateBars } from "./Inventory";
import { ReadProgress } from "./Loading";
import { LinkTag } from "./Location";
import { applyReaction } from "./Select";
import { chosenHomes, homeChoices, withHomeChosen, withProjectChosen } from "./view";

// A tooltip portals into `document.body`, which a render to markup has not.
vi.mock("./Tooltip", () => ({ Tooltip: ({ children }: { children: ReactElement }) => children }));

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

function inventory(overrides: Partial<Inventory> = {}): Inventory {
  return { read_at: "2026-10-20T09:30:00Z", project: null, projects: [], homes: [home()], items: [], read_failures: [], store_read_failures: [], agent_flags: [], ...overrides };
}

describe("a key on the closed Project field", () => {
  const first = String.raw`E:\cd-throwaway\project-a`;
  const second = String.raw`E:\cd-throwaway\project-b`;
  const gone = String.raw`E:\cd-throwaway\project-gone`;
  const read = inventory({ project: first, projects: [project(first), project(second), project(gone, true)] });

  /** The Projects the field loads on `key`, pressed on it closed while it shows `shown`, and whether its panel opens. */
  function press(key: string, shown: string | null) {
    const loaded: (string | null)[] = [];
    const field = projectField(read, shown, (next) => loaded.push(next));
    const state = applyReaction(onKey(CLOSED, field.options, field.value, { key, altKey: false }, 0), field.onChange);
    return { loaded, open: state.open };
  }

  test("Arrow down loads the next Project without opening the panel", () => {
    expect(press("ArrowDown", first)).toEqual({ loaded: [second], open: false });
  });

  test("Arrow up loads the Project before without opening the panel", () => {
    expect(press("ArrowUp", second)).toEqual({ loaded: [first], open: false });
  });

  test("loads the Project it moves to as a click on it does", () => {
    const clicked: (string | null)[] = [];
    const field = projectField(read, first, (next) => clicked.push(next));
    const at = field.options.findIndex((option) => option.value === second);
    applyReaction(onOptionClick({ ...CLOSED, open: true }, field.options, field.value, at), field.onChange);

    expect(clicked).toEqual([second]);
    expect(press("ArrowDown", first).loaded).toEqual(clicked);
  });

  test("Arrow up from the first Project loads no Project", () => {
    expect(press("ArrowUp", first).loaded).toEqual([null]);
  });

  test("Arrow down from the last Project loads the first missing Project", () => {
    expect(press("ArrowDown", second).loaded).toEqual([gone]);
  });
});

describe("the banner while a newly chosen Project is read", () => {
  const before = String.raw`E:\cd-throwaway\project-a`;
  const chosen = String.raw`E:\cd-throwaway\project-b`;
  const markup = renderToStaticMarkup(
    <Banner
      inventory={inventory({ project: before, projects: [project(before), project(chosen)] })}
      project={chosen}
      agentName="All Agents"
      stats={{ items: 0, inUse: 0, noEffect: 0 }}
      reading
      rereading
      refresh={() => {}}
      selectProject={() => {}}
    />,
  );

  test("shows the chosen Project in the field over the inventory read before", () => {
    expect(markup).toContain(`<span class="select-value">${chosen}</span>`);
  });

  test("runs the reading bar and says Reading on Refresh", () => {
    expect(markup).toContain(renderToStaticMarkup(<ReadProgress text="Reading the inventory again" />));
    expect(markup).toMatch(/<button type="button" class="cta secondary" disabled="">.*Reading<\/button>/);
  });
});

/** The opening tag of each action button of an Entry card, Disable or Enable then Remove. */
const actionButtons = (busy: boolean) => {
  const markup = renderToStaticMarkup(
    <EntryCard
      entry={entry()}
      rank={1}
      agentName="Claude Code"
      flash={false}
      busy={busy}
      inItem={() => true}
      onShowWinner={() => {}}
      onReader={null}
      onAction={() => {}}
      notify={() => {}}
    />,
  );
  const row = markup.slice(markup.indexOf('<div class="entry-actions">'));
  return {
    card: markup.match(/^<article[^>]*>/)?.[0] ?? "",
    head: markup.match(/<header class="entry-head">[\s\S]*?<\/header>/)?.[0] ?? "",
    buttons: row.match(/<button[^>]*class="cta[^>]*>/g) ?? [],
  };
};

describe("the tag of a Linked Entry", () => {
  const tag = (kind: LinkKind, broken: boolean, withTarget = false) =>
    renderToStaticMarkup(<LinkTag link={{ target: String.raw`C:\skills\x`, kind, broken }} withTarget={withTarget} />);

  test("names a junction or a symlink as the CLI does, and when its target is gone", () => {
    expect(tag("junction", false)).toBe('<span class="tag linked">junction</span>');
    expect(tag("junction", true)).toBe('<span class="tag linked">junction, broken</span>');
    expect(tag("folder_symlink", false)).toBe('<span class="tag linked">symlink</span>');
    expect(tag("file_symlink", true)).toBe('<span class="tag linked">symlink, broken</span>');
  });

  test("shows where it points beside its kind on an Entry card", () => {
    expect(tag("folder_symlink", false, true)).toBe(String.raw`<span class="tag linked">symlink → C:\skills\x</span>`);
  });
});

describe("an Entry card", () => {
  test("disables Disable and Remove while its Entry is read again, and shows it is being read beside its State", () => {
    const { card, head, buttons } = actionButtons(true);
    expect(card).toMatch(/aria-busy="true"/);
    expect(head).toContain('<span class="entry-reading"><span class="spinner" aria-hidden="true"></span>Reading…</span>');
    expect(buttons).toHaveLength(2);
    for (const button of buttons) expect(button).toMatch(/disabled=""/);
  });

  test("offers Disable and Remove while nothing runs on its Entry", () => {
    const { card, head, buttons } = actionButtons(false);
    expect(card).toMatch(/aria-busy="false"/);
    expect(head).not.toContain("Reading…");
    expect(buttons).toHaveLength(2);
    for (const button of buttons) expect(button).not.toMatch(/disabled/);
  });

  test("draws Remove in the style of a button that deletes, beside Disable too", () => {
    const { buttons } = actionButtons(false);
    expect(buttons.map((button) => button.match(/class="([^"]*)"/)?.[1])).toEqual(["cta", "cta danger"]);
  });
});
