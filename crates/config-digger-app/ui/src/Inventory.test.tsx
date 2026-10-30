import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import type { Item, LastOpened, LinkKind, Project, ReadFailure, State } from "./bindings";
import { CLOSED, onKey, onOptionClick } from "./dropdown";
import { EntryCard } from "./EntryCard";
import { Banner, FailuresNotice, homeLabel, HomesPane, projectField, StateBars } from "./Inventory";
import { ReadProgress } from "./Loading";
import { LinkTag } from "./Location";
import { applyReaction } from "./Select";
import { entry, home, inventory } from "./testData";
import { chosenHomes, failuresInContext, homeChoices, lastOpenedHomes, withHomeChosen, withProjectChosen } from "./view";

// A tooltip portals into `document.body`, which a render to markup has not.
vi.mock("./Tooltip", () => ({ Tooltip: ({ children }: { children: ReactElement }) => children }));

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

describe("the name on an Agent home's tile", () => {
  const added = (folder: string) => home({ home: folder, folder, source: { kind: "added" } });
  const names = (homes: ReturnType<typeof home>[]) =>
    renderToStaticMarkup(<HomesPane inventory={inventory({ homes })} selection={{ agent: "codex", homes: { codex: "default" } }} onHome={() => {}} />).match(
      /<span class="home-name">[\s\S]*?<\/span><\/span>/g,
    ) ?? [];

  test("keeps a folder's last part apart from the rest, so that a name too long for its tile is cut before it", () => {
    expect(names([added(String.raw`E:\cd-throwaway\claude-code-many-home`), added("/home/me/.codex-work")])).toEqual([
      String.raw`<span class="home-name"><span class="home-name-head">E:\cd-throwaway</span><span class="home-name-tail">\claude-code-many-home</span></span>`,
      '<span class="home-name"><span class="home-name-head">/home/me</span><span class="home-name-tail">/.codex-work</span></span>',
    ]);
  });

  test("is one part when it has no folder to cut, such as the default home", () => {
    expect(names([home()])).toEqual(['<span class="home-name"><span class="home-name-tail">default</span></span>']);
  });
});

describe("the name of an Agent's one home under All Agents", () => {
  const row = (folder: string) =>
    renderToStaticMarkup(
      <HomesPane inventory={inventory({ homes: [home({ home: folder, folder, source: { kind: "added" } })] })} selection={{ agent: "all", homes: {} }} onHome={() => {}} />,
    ).match(/<span class="dim single-home"[\s\S]*?<\/span><\/span><\/span>/)?.[0];

  test("is cut before its folder's last part, as on the tiles, and stays a tab stop for its tooltip", () => {
    expect(row(String.raw`E:\cd-throwaway\paths-that-wrap\claude-desktop-home`)).toBe(
      String.raw`<span class="dim single-home" tabindex="0"><span class="home-name"><span class="home-name-head">E:\cd-throwaway\paths-that-wrap</span><span class="home-name-tail">\claude-desktop-home</span></span></span>`,
    );
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
    const field = projectField(read, shown, { agent: "all", homes: {} }, homeChoices(), (next) => loaded.push(next));
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
    const field = projectField(read, first, { agent: "all", homes: {} }, homeChoices(), (next) => clicked.push(next));
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
      choices={homeChoices()}
      selection={{ agent: "all", homes: {} }}
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

describe("the Project field and the notice of the Read failures", () => {
  const picked = String.raw`E:\cd-throwaway\project`;
  const other = String.raw`E:\cd-throwaway\project-extra`;
  const shapeHome = String.raw`E:\cd-throwaway\claude-code-shape-home`;
  const claudeCode = { agent: "claude-code", agent_name: "Claude Code" };
  const homes = [home({ ...claudeCode }), home({ ...claudeCode, home: shapeHome, folder: shapeHome, source: { kind: "added" } })];
  const failure: ReadFailure = {
    ...claudeCode,
    home: shapeHome,
    path: String.raw`${shapeHome}\.claude.json`,
    project: picked,
    location: null,
    kind: "shape",
    position: null,
    reaction: "unknown",
    sentence: "Claude Code can't read it.",
    notes: [],
    warnings: [],
  };

  const opened = lastOpened(shapeHome, { agent: "claude-code", name: null, sentence: "" });

  /**
   * The Project field's label of a path, the banner and the notice, with the
   * home `chosen` for Claude Code and `shown` in the field, while the
   * inventory of `readFor` is shown, `shown`'s once it is read.
   */
  function screenWith(shown: string | null, chosen: string, { last_opened = [] as LastOpened[], readFor = shown } = {}) {
    const projects = [project(picked, false, last_opened), project(other)];
    const read = inventory({ project: readFor, projects, homes, read_failures: [failure] });
    const choices = withProjectChosen(withHomeChosen(homeChoices(), "claude-code", chosen), read.projects, shown);
    const selection = { agent: "all", homes: chosenHomes(read.homes, choices, read.items, lastOpenedHomes(read)) };
    const label = (path: string) => projectField(read, shown, selection, choices, () => {}).options.find((o) => o.value === path)?.label;
    const banner = renderToStaticMarkup(
      <Banner
        inventory={read}
        project={shown}
        choices={choices}
        selection={selection}
        agentName="All Agents"
        stats={{ items: 0, inUse: 0, noEffect: 0 }}
        reading={false}
        rereading={false}
        refresh={() => {}}
        selectProject={() => {}}
      />,
    );
    const notice = renderToStaticMarkup(<FailuresNotice failures={failuresInContext(read.read_failures, read.project, selection)} onView={() => {}} />);
    return { label, banner, notice };
  }
  const badge = (banner: string) => banner.match(/<span class="badge">.*?<\/span>/)?.[0] ?? null;

  test("count nothing of a failure in a home the Project doesn't open on, before or after it is picked", () => {
    expect(screenWith(null, "default").label(picked)).toBe(picked);
    const after = screenWith(picked, "default");
    expect(after.label(picked)).toBe(picked);
    expect(badge(after.banner)).toBeNull();
    expect(after.notice).toBe("");
  });

  test("count a failure in the home chosen alike in the option, the badge and the notice", () => {
    expect(screenWith(null, shapeHome).label(picked)).toBe(`${picked} · 1 location couldn't be read`);
    const after = screenWith(picked, shapeHome);
    expect(after.label(picked)).toBe(`${picked} · 1 location couldn't be read`);
    expect(badge(after.banner)).toMatch(/>1<\/span>$/);
    expect(after.notice).toContain("1 location couldn&#x27;t be read.");
  });

  test("count a failure in the Project's Last-opened home before it is picked and in the notice after", () => {
    expect(screenWith(null, "default", { last_opened: [opened] }).label(picked)).toBe(`${picked} · 1 location couldn't be read`);
    const after = screenWith(picked, "default", { last_opened: [opened] });
    expect(badge(after.banner)).toMatch(/>1<\/span>$/);
    expect(after.notice).toContain("1 location couldn&#x27;t be read.");
  });

  test("count what the Project picked opens on while it is read, not the homes of the Project read before", () => {
    const reading = screenWith(picked, "default", { last_opened: [opened], readFor: null });
    expect(reading.label(picked)).toBe(`${picked} · 1 location couldn't be read`);
    expect(badge(reading.banner)).toMatch(/>1<\/span>$/);
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

  test("says why Remove is not offered under its buttons, not only in the disabled button's tooltip", () => {
    const why = "Remove is not offered: this Entry is managed, or a copy of the user's account, and config-digger only reads it.";
    const markup = renderToStaticMarkup(
      <EntryCard
        entry={entry({ remove: { status: "not_offered", code: "read_only", sentence: why, how_to_remove: null } })}
        rank={1}
        agentName="Claude Code"
        flash={false}
        busy={false}
        inItem={() => true}
        onShowWinner={() => {}}
        onReader={null}
        onAction={() => {}}
        notify={() => {}}
      />,
    );
    expect(markup).toMatch(/<button type="button" class="cta danger" disabled="">/);
    expect(markup).toContain(`<p class="action-note">${why.replace("'", "&#x27;")}</p>`);
  });
});
