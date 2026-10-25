import { describe, expect, test } from "vitest";
import type { Agent, Entry, Home, Item, Leftovers, Places, Project, Stranded } from "./bindings";
import { changedSentence, changeBusy, homeGroups, inventoryStart, projectSections, rowOf, strandedRows } from "./places";

const CLAUDE_JSON = String.raw`C:\Users\me\.claude.json`;

function project(overrides: Partial<Project> = {}): Project {
  return {
    path: String.raw`E:\code\shop`,
    missing: false,
    workspace_file: false,
    members: [],
    registered_by: ["claude-code"],
    added: false,
    hidden: false,
    entries: 0,
    last_opened: [],
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
    scopes: ["user"],
    installations: [],
    out_of_range: false,
    last_ran: null,
    entries: 0,
    notes: [],
    ...overrides,
  };
}

function agent(overrides: Partial<Agent> = {}): Agent {
  return { id: "claude-code", name: "Claude Code", built_in: true, tested: null, home_variables: [], homes: [home()], ...overrides };
}

function places(overrides: Partial<Places> = {}): Places {
  return {
    read_at: "2026-10-10T09:00:00Z",
    projects: [],
    leftovers: [],
    stranded: [],
    store_read_failures: [],
    agents: [agent(), agent({ id: "vscode", name: "VS Code", homes: [] })],
    ...overrides,
  };
}

describe("the Projects tab", () => {
  test("lists each Project with its kind, where it came from and its Project Entries", () => {
    const { present } = projectSections(
      places({
        projects: [
          project({ registered_by: ["claude-code", "vscode"], entries: 3 }),
          project({
            path: String.raw`E:\code\shop.code-workspace`,
            workspace_file: true,
            members: [String.raw`E:\code\api`, String.raw`E:\code\web`],
            registered_by: [],
            added: true,
            entries: 1,
          }),
          project({ path: String.raw`E:\code\tools`, added: true, entries: 0 }),
        ],
      }),
    );

    expect(present.map(({ path, kind, source, entries }) => ({ path, kind, source, entries }))).toEqual([
      { path: String.raw`E:\code\shop`, kind: "Folder", source: "Known to Claude Code, VS Code", entries: "3 Project Entries" },
      {
        path: String.raw`E:\code\shop.code-workspace`,
        kind: String.raw`.code-workspace · members E:\code\api, E:\code\web`,
        source: "Added by you",
        entries: "1 Project Entry",
      },
      { path: String.raw`E:\code\tools`, kind: "Folder", source: "Added by you · also known to Claude Code", entries: "No Project Entries" },
    ]);
  });

  test("offers Remove from list only on what the user added and no registry records, and Hide otherwise", () => {
    const { present } = projectSections(
      places({
        projects: [
          project(),
          project({ path: String.raw`E:\code\added`, registered_by: [], added: true }),
          project({ path: String.raw`E:\code\both`, added: true }),
        ],
      }),
    );

    expect(present.map((row) => row.offer)).toEqual(["hide", "remove_from_list", "hide"]);
  });

  const GONE = String.raw`E:\code\gone`;

  /** The leftover `name` of {@link GONE}, as an Item of `agent` holding that one Entry. */
  function leftover(name: string, agentId = "claude-code"): Item {
    const keyPath = ["projects", "E:/code/gone", "mcpServers", name];
    const entry = { path: CLAUDE_JSON, key_path: keyPath, locator: `${CLAUDE_JSON}#${keyPath.join(".")}` } as Entry;
    return { agent: agentId, home: "default", kind: "mcp", name, state: "unread", entries: [entry], built: null } as Item;
  }

  function leftovers(items: Item[]): Leftovers {
    return { project: GONE, items };
  }

  test("lists each missing Project with its leftovers, each once however many Agents read it", () => {
    const { missing } = projectSections(
      places({
        projects: [project({ path: GONE, missing: true })],
        leftovers: [leftovers([leftover("old"), leftover("search"), leftover("old", "vscode")])],
      }),
    );

    expect(missing).toEqual([
      {
        project: expect.objectContaining({ path: GONE }),
        path: GONE,
        source: "The folder no longer exists · known to Claude Code",
        leftovers: [
          `${CLAUDE_JSON} → projects.E:/code/gone.mcpServers.old`,
          `${CLAUDE_JSON} → projects.E:/code/gone.mcpServers.search`,
        ],
        count: "2 leftovers",
        hideOffered: false,
      },
    ]);
  });

  const DELETED = String.raw`E:\code\deleted`;

  test("lists the missing Projects without leftovers apart from those with, each offering Hide", () => {
    const { missing, noLeftovers } = projectSections(
      places({
        projects: [project({ path: DELETED, missing: true }), project({ path: GONE, missing: true })],
        leftovers: [leftovers([leftover("old")])],
      }),
    );

    expect(missing.map((row) => row.path)).toEqual([GONE]);
    expect(noLeftovers).toEqual([
      {
        project: expect.objectContaining({ path: DELETED }),
        path: DELETED,
        source: "The folder no longer exists · known to Claude Code",
        leftovers: [],
        count: "No leftovers",
        hideOffered: true,
      },
    ]);
  });

  test("counts no leftover of a missing Project in a hidden Agent home", () => {
    const { missing, noLeftovers } = projectSections(
      places({
        projects: [project({ path: GONE, missing: true })],
        leftovers: [leftovers([leftover("old"), leftover("search", "vscode")])],
        agents: [agent({ homes: [home({ hidden: true })] }), agent({ id: "vscode", name: "VS Code", homes: [home({ agent: "vscode" })] })],
      }),
    );

    expect(missing.map(({ leftovers, count }) => ({ leftovers, count }))).toEqual([
      { leftovers: [`${CLAUDE_JSON} → projects.E:/code/gone.mcpServers.search`], count: "1 leftover" },
    ]);
    expect(noLeftovers).toEqual([]);
  });

  test("counts the leftovers of a missing Project in a shown home of an Agent but not those in its hidden home", () => {
    const ALT = String.raw`D:\alt-claude`;
    const { missing } = projectSections(
      places({
        projects: [project({ path: GONE, missing: true })],
        leftovers: [leftovers([{ ...leftover("alt"), home: ALT }, leftover("old")])],
        agents: [agent({ homes: [home(), home({ home: ALT, folder: ALT, hidden: true })] })],
      }),
    );

    expect(missing.map(({ leftovers, count }) => ({ leftovers, count }))).toEqual([
      { leftovers: [`${CLAUDE_JSON} → projects.E:/code/gone.mcpServers.old`], count: "1 leftover" },
    ]);
  });

  test("moves a missing Project whose leftovers are all in hidden Agent homes to those without leftovers", () => {
    const { missing, noLeftovers } = projectSections(
      places({
        projects: [project({ path: GONE, missing: true })],
        leftovers: [leftovers([leftover("old")])],
        agents: [agent({ homes: [home({ hidden: true })] })],
      }),
    );

    expect(missing).toEqual([]);
    expect(noLeftovers).toMatchObject([{ path: GONE, count: "No leftovers", hideOffered: true }]);
  });

  test("lists hidden Projects, missing ones included, on their own, with Remove from list for the user's own", () => {
    const sections = projectSections(
      places({
        projects: [
          project({ path: String.raw`E:\code\old`, hidden: true }),
          project({ path: GONE, missing: true, hidden: true, registered_by: [], added: true }),
        ],
        leftovers: [leftovers([leftover("old")])],
      }),
    );

    expect(sections.present).toEqual([]);
    expect(sections.missing).toEqual([]);
    expect(sections.noLeftovers).toEqual([]);
    expect(sections.hidden.map(({ path, source, removable }) => ({ path, source, removable }))).toEqual([
      { path: String.raw`E:\code\old`, source: "Known to Claude Code", removable: false },
      { path: GONE, source: "Missing · Added by you", removable: true },
    ]);
  });

  test("narrows every section, both Missing ones and Hidden included, to the Projects whose path holds the search, in any case", () => {
    const sections = projectSections(
      places({
        projects: [
          project({ path: String.raw`E:\code\shop` }),
          project({ path: String.raw`E:\code\billing` }),
          project({ path: String.raw`E:\old\Shop-legacy`, missing: true }),
          project({ path: String.raw`E:\old\gone`, missing: true }),
          project({ path: String.raw`E:\old\shop-deleted`, missing: true }),
          project({ path: String.raw`E:\old\deleted`, missing: true }),
          project({ path: String.raw`E:\code\shop-admin`, hidden: true }),
          project({ path: String.raw`E:\code\notes`, hidden: true }),
        ],
        leftovers: [String.raw`E:\old\Shop-legacy`, String.raw`E:\old\gone`].map((path) => ({ ...leftovers([leftover("old")]), project: path })),
      }),
      "SHOP",
    );

    expect(sections.present.map((row) => row.path)).toEqual([String.raw`E:\code\shop`]);
    expect(sections.missing.map((row) => row.path)).toEqual([String.raw`E:\old\Shop-legacy`]);
    expect(sections.noLeftovers.map((row) => row.path)).toEqual([String.raw`E:\old\shop-deleted`]);
    expect(sections.hidden.map((row) => row.path)).toEqual([String.raw`E:\code\shop-admin`]);
  });

  test("lists every Project with an empty search", () => {
    const all = places({ projects: [project(), project({ path: GONE, missing: true }), project({ path: String.raw`E:\x`, hidden: true })] });
    expect(projectSections(all, "")).toEqual(projectSections(all));
  });
});

const STORE = String.raw`C:\Users\me\AppData\Local\config-digger\parked\manifest.json`;

function stranded(overrides: Partial<Stranded> = {}): Stranded {
  const config = String.raw`C:\Users\me\AppData\Roaming\Claude\claude_desktop_config.json`;
  return {
    name: "notes",
    kind: "mcp",
    agent: "claude-desktop",
    agent_name: "Claude Desktop",
    origin: {
      home: "msix",
      location: "user-mcp",
      project: null,
      path: config,
      key_path: ["mcpServers", "notes"],
      locator: `${config}#mcpServers.notes`,
    },
    reason: "home_not_found",
    sentence: "Stranded: the Claude Desktop home `msix` is not found, so config-digger cannot put `notes` back.",
    parked_at: "2026-10-01T12:00:00Z",
    path: STORE,
    key_path: ["entries", "notes"],
    locator: `${STORE}#entries.notes`,
    config: { command: "notes-mcp" },
    masked: [],
    ...overrides,
  };
}

describe("the Stranded group", () => {
  test("lists each Stranded Entry with its Kind, Agent, where it was parked from and why it is Stranded", () => {
    const rows = strandedRows(
      places({
        stranded: [
          stranded(),
          stranded({
            name: "review",
            kind: "skill",
            agent_name: "gone-agent",
            origin: { ...stranded().origin, home: "default", location: "user-skills", project: String.raw`E:\code\shop` },
          }),
        ],
      }),
    );

    expect(rows.map(({ locator, name, sub, origin }) => ({ locator, name, sub, origin }))).toEqual([
      {
        locator: `${STORE}#entries.notes`,
        name: "notes",
        sub: "MCP server · Claude Desktop",
        origin: String.raw`From home msix, location user-mcp: C:\Users\me\AppData\Roaming\Claude\claude_desktop_config.json → mcpServers.notes`,
      },
      {
        locator: `${STORE}#entries.notes`,
        name: "review",
        sub: "Skill · gone-agent",
        origin: String.raw`From home default, location user-skills, Project E:\code\shop: C:\Users\me\AppData\Roaming\Claude\claude_desktop_config.json → mcpServers.notes`,
      },
    ]);
    expect(rows[0].sentence).toContain("home `msix` is not found");
    expect(rows[0].parkedAt).toBe("2026-10-01T12:00:00Z");
  });

  test("lists none without Stranded Entries", () => {
    expect(strandedRows(places())).toEqual([]);
  });
});

describe("the Agent homes tab", () => {
  const CODEX = "C:\Users\me\.codex";
  const ORCA = "C:\Users\me\.orca\codex";

  test("heads each Agent with each Installation's version, where it was read from, notes and its definition", () => {
    const native = { id: "native", path: "C:\bin\claude.exe", version: "2.1.40", version_from: "claude.exe version resource", out_of_range: false };
    const desktop = { id: "desktop", path: "C:\apps\claude.exe", version: null, version_from: null, out_of_range: false };
    const newer = { id: "native", path: "C:\bin\claude.exe", version: "2.9.0", version_from: "claude.exe version resource", out_of_range: true };
    const note = { code: "version_outside_tested_range" as const, installation: newer.path, sentence: "Claude Code 2.9.0 is newer than tested." };
    const cloud = { code: "cloud_connectors_not_on_disk" as const, installation: null, sentence: "Connectors live in your account." };
    const [group] = homeGroups(
      places({
        agents: [
          agent({
            homes: [
              home({ installations: [native, desktop], notes: [cloud] }),
              home({ home: "alt", installations: [native] }),
            ],
          }),
        ],
      }),
    );

    expect(group).toMatchObject({
      name: "Claude Code",
      installed: ["native 2.1.40 · read from claude.exe version resource", "desktop version unknown"],
      notes: [],
      definition: "built-in definition",
    });

    const [outOfRange] = homeGroups(places({ agents: [agent({ homes: [home({ installations: [newer], notes: [note] })] })] }));
    expect(outOfRange).toMatchObject({ installed: ["native 2.9.0 · read from claude.exe version resource"], notes: [note.sentence] });

    const [none] = homeGroups(places({ agents: [agent()] }));
    expect(none.installed).toEqual(["no Installation found"]);
  });

  test("shows each home's label, folder, source tag and Entry count, with Show in Inventory and Hide", () => {
    const [group] = homeGroups(
      places({
        agents: [
          agent({
            id: "codex",
            name: "Codex",
            homes: [
              home({ agent: "codex", home: "default", folder: CODEX, entries: 4 }),
              home({ agent: "codex", home: "env", folder: "D:\codex", source: { kind: "environment", variable: "CODEX_HOME" }, entries: 1 }),
              home({ agent: "codex", home: "store", folder: "D:\store", source: { kind: "known_location" } }),
            ],
          }),
        ],
      }),
    );

    expect(group.homes.map(({ label, path, tags, entries, actions }) => ({ label, path, tags, entries, actions }))).toEqual([
      { label: "default", path: CODEX, tags: [{ text: "default location", tone: "plain" }], entries: "4 Entries", actions: ["show", "hide"] },
      {
        label: "env",
        path: "D:\codex",
        tags: [{ text: "environment variable %CODEX_HOME%", tone: "plain" }],
        entries: "1 Entry",
        actions: ["show", "hide"],
      },
      { label: "store", path: "D:\store", tags: [{ text: "known location", tone: "plain" }], entries: "No Entries", actions: ["show", "hide"] },
    ]);
  });

  test("tags a Mirrored home read-only with its source, a skipped one not read with the reason, and dims a hidden one", () => {
    const mirror = { owner: "Orca", source: "default", source_folder: CODEX, label: "Mirrored by Orca", sentence: `Orca copies this Agent home from ${CODEX}, and config-digger never writes to it.` };
    const notRead = { read_instead: "default", sentence: "Codex never reads D:\old: its lookup order picks C:\Users\me\.codex first." };
    const [group] = homeGroups(
      places({
        agents: [
          agent({
            id: "codex",
            name: "Codex",
            homes: [
              home({ agent: "codex", home: "orca", folder: ORCA, source: { kind: "known_location" }, mirror }),
              home({ agent: "codex", home: "old", folder: "D:\old", not_read: notRead }),
              home({ agent: "codex", home: "hidden", folder: "D:\hidden", hidden: true }),
            ],
          }),
        ],
      }),
    );

    const [orca, old, hidden] = group.homes;
    expect(orca).toMatchObject({
      tags: [{ text: "known location", tone: "plain" }, { text: "Mirrored by Orca, read-only · from default", tone: "read_only" }],
      notes: [mirror.sentence],
    });
    expect(old).toMatchObject({ tags: [{ text: "default location", tone: "plain" }, { text: "not read", tone: "plain" }], notes: [notRead.sentence] });
    expect(hidden).toMatchObject({ hidden: true, actions: ["unhide"] });
  });

  test("offers Remove from list only on a home the user added, and says when it has no configuration", () => {
    const [group] = homeGroups(
      places({
        agents: [
          agent({
            homes: [
              home({ home: "D:\portable", folder: "D:\portable", source: { kind: "added" }, no_configuration: "No configuration found for Claude Code" }),
              home({ home: "D:\hid", folder: "D:\hid", source: { kind: "added" }, hidden: true }),
            ],
          }),
        ],
      }),
    );

    expect(group.homes[0]).toMatchObject({
      label: "D:\portable",
      tags: [{ text: "added by you", tone: "plain" }],
      notes: ["No configuration found for Claude Code."],
      actions: ["show", "remove_from_list"],
    });
    expect(group.homes[1].actions).toEqual(["unhide", "remove_from_list"]);
  });

  test("names the Agent on the row of a home the user added, whose label is its folder", () => {
    const added = String.raw`E:\cd-throwaway\codex-home`;
    const [group] = homeGroups(
      places({ agents: [agent({ homes: [home(), home({ home: added, folder: added, source: { kind: "added" } })] })] }),
    );

    expect(group.homes.map((row) => row.agent)).toEqual([null, "Claude Code"]);
  });

  test("notes on its own row a home whose last-ran version cannot be read", () => {
    const unreadable = {
      code: "last_ran_unreadable" as const,
      installation: null,
      sentence: String.raw`The version of Claude Code that last ran against this Agent home could not be read from C:\Users\me\.claude.json.`,
    };
    const [group] = homeGroups(places({ agents: [agent({ homes: [home({ notes: [unreadable] }), home({ home: "alt" })] })] }));

    expect(group.notes).toEqual([]);
    expect(group.homes.map((row) => row.notes)).toEqual([[unreadable.sentence], []]);
  });
});

describe("what the Inventory opens on", () => {
  test("Open in Inventory picks the Project, Show leftovers also filters to Unread, Show in Inventory picks the home", () => {
    const path = String.raw`E:\code\shop`;

    expect(inventoryStart({ kind: "project", path })).toEqual({ project: path, agent: "all", homes: {}, state: null, item: null });
    expect(inventoryStart({ kind: "leftovers", path })).toEqual({ project: path, agent: "all", homes: {}, state: "unread", item: null });
    expect(inventoryStart({ kind: "home", home: home({ agent: "codex", home: "orca" }) })).toEqual({
      project: null,
      agent: "codex",
      homes: { codex: "orca" },
      state: null,
      item: null,
    });
  });
});

describe("the notice after a list change", () => {
  test("says what changed, that nothing on disk did when forgetting, and when an added home has no configuration", () => {
    const path = String.raw`E:\code\shop`;
    const shop = { kind: "project" as const, project: project({ path }) };
    const portable = home({ home: "D:\portable", folder: "D:\portable", no_configuration: "No configuration found for Claude Code" });

    expect(changedSentence({ kind: "add_project", path }, shop)).toBe(`Added ${path}: every Agent is evaluated on it.`);
    expect(changedSentence({ kind: "hide_project", path }, shop)).toBe(`Hid ${path}. Picking it in the Inventory still evaluates it.`);
    expect(changedSentence({ kind: "unhide_project", path }, shop)).toBe(`${path} is listed again.`);
    expect(changedSentence({ kind: "forget_project", path }, shop)).toBe(`Removed ${path} from config-digger's list. Nothing on disk changed.`);
    expect(changedSentence({ kind: "add_home", folder: "D:\portable", agent: "claude-code" }, { kind: "home", home: portable })).toBe(
      "Added D:\portable as an Agent home of Claude Code. No configuration found for Claude Code.",
    );
    expect(changedSentence({ kind: "hide_home", agent: "claude-code", home: "default" }, { kind: "home", home: home() })).toBe(
      "Hid default of Claude Code: its Items leave the Inventory.",
    );
  });
});

const PORTABLE = String.raw`D:\portable`;

describe("the busy indicator of a list change", () => {
  test("names what runs, and the row it runs on, from the moment it starts", () => {
    const path = String.raw`E:\cd-throwaway\project-hide`;
    const listed = places({ agents: [agent({ homes: [home(), home({ home: PORTABLE, folder: PORTABLE, source: { kind: "added" } })] })] });
    const homeRow = (home: string) => rowOf({ agent: "claude-code", home });

    expect(changeBusy({ kind: "hide_project", path }, listed)).toEqual({ text: `Hiding ${path}…`, row: path });
    expect(changeBusy({ kind: "unhide_project", path }, listed)).toEqual({ text: `Unhiding ${path}…`, row: path });
    expect(changeBusy({ kind: "forget_project", path }, listed)).toEqual({ text: `Removing ${path} from config-digger's list…`, row: path });
    expect(changeBusy({ kind: "hide_home", agent: "claude-code", home: "default" }, listed)).toEqual({
      text: "Hiding default of Claude Code…",
      row: homeRow("default"),
    });
    expect(changeBusy({ kind: "unhide_home", agent: "claude-code", home: "default" }, listed)).toEqual({
      text: "Unhiding default of Claude Code…",
      row: homeRow("default"),
    });
    expect(changeBusy({ kind: "forget_home", agent: "claude-code", home: PORTABLE }, listed)).toEqual({
      text: `Removing ${PORTABLE} of Claude Code from config-digger's list…`,
      row: homeRow(PORTABLE),
    });
  });

  test("of an Add names what is added, on no row", () => {
    const path = String.raw`E:\cd-throwaway\project-new`;

    expect(changeBusy({ kind: "add_project", path }, places())).toEqual({ text: `Adding ${path}…`, row: null });
    expect(changeBusy({ kind: "add_home", folder: PORTABLE, agent: "claude-code" }, places())).toEqual({
      text: `Adding ${PORTABLE} as an Agent home of Claude Code…`,
      row: null,
    });
  });

  test("names an Agent the screen has not read by its id", () => {
    expect(changeBusy({ kind: "hide_home", agent: "codex", home: "default" }, null).text).toBe("Hiding default of codex…");
  });

  test("tells a home of one Agent from the same home of another", () => {
    expect(rowOf({ agent: "claude-code", home: "default" })).not.toBe(rowOf({ agent: "codex", home: "default" }));
  });
});
