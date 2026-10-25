import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import type { Entry, Home, Inventory, Item, LastOpened, LinkKind, Project, State } from "./bindings";
import { CLOSED, onKey, onOptionClick } from "./dropdown";
import { EntryCard } from "./EntryCard";
import { Banner, homeLabel, HomesPane, projectField, StateBars } from "./Inventory";
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

function project(path: string, missing = false, last_opened: LastOpened[] = []): Project {
  return { path, missing, workspace_file: false, members: [], registered_by: [], added: true, hidden: false, entries: 0, last_opened };
}

/** VS Code's Last-opened home `home`, the profile Work. */
function lastOpened(home: string, overrides: Partial<LastOpened> = {}): LastOpened {
  return {
    agent: "vscode",
    home,
    name: "Work",
    shares_home: false,
    hidden: false,
    sentence: "VS Code last opened this Project in Work.",
    ...overrides,
  };
}

describe("choosing a Project VS Code last opened in a profile", () => {
  const work = String.raw`C:\Users\me\AppData\Roaming\Code\User\profiles\-5a1b2c3d`;
  const vscode = { agent: "vscode", agent_name: "VS Code" };
  const homes = [home(), home({ ...vscode }), home({ ...vscode, home: work, folder: work, source: { kind: "known_location" } })];
  const opened = String.raw`E:\cd-throwaway\project`;
  const other = String.raw`E:\cd-throwaway\project-extra`;
  const inWork = [lastOpened(work)];
  const projects = [project(opened, false, inWork), project(other)];
  const earlier = withHomeChosen(withHomeChosen(homeChoices(), "vscode", "default"), "codex", "default");

  test("opens that Agent on its Last-opened home, over a home chosen earlier, and keeps the other Agents on theirs", () => {
    const chosen = withProjectChosen(earlier, projects, opened);

    expect(chosenHomes(homes, chosen, [], inWork)).toEqual({ codex: "default", vscode: work });
  });

  test("keeps a home the user then chooses until another Project is chosen", () => {
    const switched = withHomeChosen(withProjectChosen(earlier, projects, opened), "vscode", "default");
    const again = withProjectChosen(switched, projects, opened);

    expect(chosenHomes(homes, switched, [], inWork)).toEqual({ codex: "default", vscode: "default" });
    expect(chosenHomes(homes, again, [], inWork)).toEqual({ codex: "default", vscode: work });
  });

  test("changes nothing for a Project with no Last-opened home, nor for no Project", () => {
    const fromWork = withProjectChosen(earlier, projects, opened);

    expect(chosenHomes(homes, withProjectChosen(fromWork, projects, other), [], [])).toEqual({ codex: "default", vscode: "default" });
    expect(chosenHomes(homes, withProjectChosen(fromWork, projects, null), [], [])).toEqual({ codex: "default", vscode: "default" });
  });

  test("never opens a hidden Last-opened home, and follows the rule before it instead", () => {
    const shown = homes.filter((listed) => listed.home !== work);
    const hidden = [lastOpened(work, { hidden: true })];
    const chosen = withProjectChosen(homeChoices(), projects, opened);

    expect(chosenHomes(shown, chosen, [], hidden)).toEqual({ codex: "default", vscode: "default" });
    expect(chosenHomes(homes, chosen, [], hidden)).toEqual({ codex: "default", vscode: "default" });
  });

  test("opens a missing Project on the home holding its leftovers first", () => {
    const gone = String.raw`E:\cd-throwaway\project-gone`;
    const leftover = item({
      agent: "vscode",
      home: "default",
      state: "unread",
      entries: [entry({ state: "unread", reason: { code: "project_missing", effective: null, switch: null, sentence: "Its Project is missing." } })],
    });
    const chosen = withProjectChosen(homeChoices(), [project(gone, true, inWork)], gone);

    expect(chosenHomes(homes, chosen, [leftover], inWork)).toEqual({ codex: "default", vscode: "default" });
  });
});

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

describe("the Agent home pane of a Project VS Code last opened in a profile", () => {
  const work = String.raw`C:\Users\me\AppData\Roaming\Code\User\profiles\-5a1b2c3d`;
  const vscode = { agent: "vscode", agent_name: "VS Code" };
  const homes = [home(), home({ ...vscode }), home({ ...vscode, home: work, folder: work, source: { kind: "known_location" } })];
  const path = String.raw`E:\cd-throwaway\project`;
  const pane = (agent: string, shown: string, opened: LastOpened, listed = homes) =>
    renderToStaticMarkup(
      <HomesPane
        inventory={inventory({ project: path, projects: [project(path, false, [opened])], homes: listed })}
        selection={{ agent, homes: { codex: "default", vscode: shown } }}
        onHome={() => {}}
      />,
    );
  const note = (markup: string) => markup.match(/<p class="home-note">(.*?)<\/p>/g) ?? [];
  const inWork = lastOpened(work);

  test("marks the Last-opened home in the select of All Agents", () => {
    expect(homeLabel(homes[2], 0, true)).toBe(`${work} · last opened here`);
    expect(homeLabel(homes[1], 0, false)).toBe("default");
    expect(pane("all", work, inWork)).toContain(`<span class="select-value">${work} · last opened here</span>`);
  });

  test("marks the Last-opened home's tile among one Agent's", () => {
    const tiles = pane("vscode", "default", inWork).match(/<button[^>]*class="tile home-tile[\s\S]*?<\/button>/g) ?? [];

    expect(tiles).toHaveLength(2);
    expect(tiles[0]).not.toContain("last opened here");
    expect(tiles[1]).toContain('<span class="tag">last opened here</span>');
  });

  test("says where VS Code last opened the Project while another home is shown", () => {
    expect(note(pane("vscode", "default", inWork))).toEqual(['<p class="home-note">VS Code last opened this Project in Work.</p>']);
    expect(note(pane("all", "default", inWork))).toEqual(['<p class="home-note">VS Code last opened this Project in Work.</p>']);
  });

  test("says when the profile reads Default's MCP servers, or the home is hidden", () => {
    const shared = lastOpened("default", {
      name: "Shared",
      shares_home: true,
      sentence: "VS Code last opened this Project in Shared, which reads Default's MCP servers.",
    });
    const hidden = lastOpened(work, { hidden: true, sentence: "VS Code last opened this Project in Work, which is hidden." });

    expect(note(pane("vscode", work, shared))).toEqual([
      `<p class="home-note">VS Code last opened this Project in Shared, which reads Default&#x27;s MCP servers.</p>`,
    ]);
    expect(note(pane("vscode", "default", hidden, homes.slice(0, 2)))).toEqual([
      '<p class="home-note">VS Code last opened this Project in Work, which is hidden.</p>',
    ]);
  });

  test("says nothing while the Last-opened home itself is shown, nor of the Agents not selected", () => {
    expect(note(pane("vscode", work, inWork))).toEqual([]);
    expect(note(pane("all", work, inWork))).toEqual([]);
    expect(note(pane("codex", "default", inWork))).toEqual([]);
  });
});

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
