// @vitest-environment happy-dom
import { cleanup, render, screen as rendered } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, test } from "vitest";
import type { Entry, Home, Item, Places, Project, StoreReadFailure, Stranded } from "./bindings";
import { STRANDED_ROW } from "./acting";
import { rowOf } from "./places";
import { Loading } from "./Loading";
import { HomesTab, ProjectsScreen, ProjectsTab, type Tab } from "./ProjectsScreen";

afterEach(cleanup);

const DESCRIPTIONS: Record<Tab, string> = {
  projects: "Projects come from the Agents' own registries plus the folders you add. config-digger never scans the disk.",
  homes: "Each Agent reads its user configuration from one or more homes. Entries in different homes never compete.",
};

/** `text` as the markup writes it. */
const escaped = (text: string) => text.replace(/'/g, "&#x27;");

const screen = (tab: Tab) => renderToStaticMarkup(<ProjectsScreen onShow={() => {}} onOpenHistory={() => {}} start={{ tab, adding: false }} />);

describe("the Projects & homes screen", () => {
  for (const tab of ["projects", "homes"] as const) {
    test(`keeps the description of the ${tab} tab out of the banner, at the top of the list`, () => {
      const markup = screen(tab);
      const banner = markup.match(/<header class="panel banner">[\s\S]*?<\/header>/)?.[0] ?? "";
      const list = markup.match(/<section class="panel places-list">[\s\S]*?<\/section>/)?.[0] ?? "";
      expect(banner).toContain("Projects &amp; homes");
      expect(banner).not.toContain(escaped(DESCRIPTIONS[tab]));
      expect(list.startsWith(`<section class="panel places-list"><p class="places-about dim">${escaped(DESCRIPTIONS[tab])}</p>`)).toBe(true);
    });

    test(`fills the list of the ${tab} tab with the loading state until the first read is done`, () => {
      const list = screen(tab).match(/<section class="panel places-list">[\s\S]*?<\/section>/)?.[0] ?? "";
      expect(list).toContain(renderToStaticMarkup(<Loading text="Reading the Projects and Agent homes…" rows={6} />));
      expect(list).not.toContain('<p class="empty">');
    });
  }
});

function project(overrides: Partial<Project> = {}): Project {
  return {
    path: String.raw`E:\cd-throwaway\project-hide`,
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

const HIDE = String.raw`E:\cd-throwaway\project-hide`;
const KEEP = String.raw`E:\cd-throwaway\project-keep`;
const GONE = String.raw`E:\cd-throwaway\project-gone`;
const DELETED = String.raw`E:\cd-throwaway\project-deleted`;
const PORTABLE = String.raw`D:\portable`;
const CLAUDE_JSON = String.raw`C:\Users\me\.claude.json`;

/** The leftover `name` of {@link GONE}, as an Item holding that one Entry. */
function leftover(name: string): Item {
  const entry = { path: CLAUDE_JSON, locator: `${CLAUDE_JSON}#projects.${GONE}.mcpServers.${name}` } as Entry;
  return { agent: "claude-code", home: "default", kind: "mcp", name, state: "unread", entries: [entry], built: null } as Item;
}

const places: Places = {
  read_at: "2026-10-20T09:00:00Z",
  projects: [project(), project({ path: KEEP }), project({ path: GONE, missing: true }), project({ path: DELETED, missing: true })],
  leftovers: [{ project: GONE, items: [leftover("old")] }],
  stranded: [],
  store_read_failures: [],
  agents: [
    {
      id: "claude-code",
      name: "Claude Code",
      built_in: true,
      tested: null,
      home_variables: [],
      homes: [home(), home({ home: PORTABLE, folder: PORTABLE, source: { kind: "added" }, hidden: true })],
    },
  ],
};

/** `markup` without the pieces a path is drawn in, so that the path reads whole. */
const unpieced = (markup: string) => markup.replace(/<span class="path-piece">([^<]*)<\/span><wbr\/>/g, "$1");

/** The markup of the row named `name`, up to the next row or group title. */
const rowMarkup = (markup: string, name: string) =>
  (markup.split('<div class="place-row').find((each) => unpieced(each).includes(`>${name}</div>`)) ?? "").split("<h3")[0];

/** The name of the row of `path`, whose text is split in the pieces it wraps between. */
const named = (path: string) => (_: string, element: Element | null) => element?.classList.contains("place-name") === true && element.textContent === path;

/** The opening tag of each button of the row named `name`. */
const buttonsOf = (markup: string, name: string) => rowMarkup(markup, name).match(/<button[^>]*>/g) ?? [];

/** Each button of the row named `name`, as [its label, its class]. */
const classesOf = (markup: string, name: string) => {
  const row = rowMarkup(markup, name);
  return [...row.matchAll(/<button[^>]*class="([^"]*)"[^>]*>([\s\S]*?)<\/button>/g)].map(([, classes, label]) => [label.replace(/<[^>]*>/g, ""), classes]);
};

/** Expects each named row's buttons to be disabled, or each to be enabled. */
const expectDisabled = (markup: string, rows: [name: string, disabled: boolean][]) => {
  for (const [name, disabled] of rows) {
    const buttons = buttonsOf(markup, name);
    expect(buttons.length, name).toBeGreaterThan(0);
    for (const button of buttons) expect(button.includes("disabled"), `${name}: ${button}`).toBe(disabled);
  }
};

describe("a row of the Projects tab", () => {
  test("disables its buttons while an action runs on it, and only its own", () => {
    const markup = renderToStaticMarkup(
      <ProjectsTab places={places} search="" busy={[HIDE, GONE]} onShow={() => {}} onChange={() => {}} onRemoveLeftovers={() => {}} onRemoveStranded={() => {}} />,
    );

    expectDisabled(markup, [
      [HIDE, true],
      [GONE, true],
      [KEEP, false],
    ]);
  });

  test("draws Remove leftovers of a missing Project in the style of a button that deletes, and Show leftovers not", () => {
    const markup = renderToStaticMarkup(
      <ProjectsTab places={places} search="" busy={[]} onShow={() => {}} onChange={() => {}} onRemoveLeftovers={() => {}} onRemoveStranded={() => {}} />,
    );

    expect(classesOf(markup, GONE)).toEqual([
      ["Show leftovers", "cta small secondary"],
      ["Remove leftovers", "cta small danger"],
    ]);
  });

  test("draws Remove from list of a Project the user added in the style of a button that deletes, and Open in Inventory not", () => {
    const markup = renderToStaticMarkup(
      <ProjectsTab
        places={{ ...places, projects: [project({ path: KEEP, registered_by: [], added: true })] }}
        search=""
        busy={[]}
        onShow={() => {}}
        onChange={() => {}}
        onRemoveLeftovers={() => {}}
        onRemoveStranded={() => {}}
      />,
    );

    expect(classesOf(markup, KEEP)).toEqual([
      ["Open in Inventory", "cta small secondary"],
      ["Remove from list", "cta small danger"],
    ]);
  });
});

describe("where a Project comes from", () => {
  test("shows as tags on a present, a missing and a hidden row, not as a line under the path", async () => {
    render(
      <ProjectsTab
        places={{
          ...places,
          projects: [project({ path: KEEP, added: true }), project({ path: GONE, missing: true }), project({ path: DELETED, missing: true, hidden: true })],
        }}
        search=""
        busy={[]}
        onShow={() => {}}
        onChange={() => {}}
        onRemoveLeftovers={() => {}}
        onRemoveStranded={() => {}}
      />,
    );

    await userEvent.click(rendered.getByRole("button", { name: "▸ Hidden · 1" }));
    const rowOfName = (name: string) => rendered.getByText(named(name)).closest(".place-row") as HTMLElement;
    const shown = (name: string) => ({
      tags: [...rowOfName(name).querySelectorAll(".tags .tag")].map((tag) => tag.textContent),
      lines: [...rowOfName(name).querySelectorAll(".place-sub")].map((line) => line.textContent),
    });
    expect(shown(KEEP)).toEqual({ tags: ["added by you", "known to Claude Code"], lines: ["Folder"] });
    expect(shown(GONE)).toEqual({ tags: ["known to Claude Code"], lines: ["The folder no longer exists"] });
    expect(shown(DELETED)).toEqual({ tags: ["known to Claude Code", "missing", "hidden"], lines: [] });
  });
});

describe("a hidden Project's row", () => {
  test("is tagged hidden once the Hidden group is open, as a hidden Agent home is, and No Project is not", async () => {
    render(
      <ProjectsTab
        places={{ ...places, projects: [project({ path: KEEP }), project({ hidden: true })] }}
        search=""
        busy={[]}
        onShow={() => {}}
        onChange={() => {}}
        onRemoveLeftovers={() => {}}
        onRemoveStranded={() => {}}
      />,
    );

    await userEvent.click(rendered.getByRole("button", { name: "▸ Hidden · 1" }));
    const rowOfName = (name: string) => rendered.getByText(named(name)).closest(".place-row") as HTMLElement;
    const tags = (row: HTMLElement) => [...row.querySelectorAll(".tags .tag")].map((tag) => tag.textContent);
    expect(tags(rowOfName(HIDE))).toEqual(["known to Claude Code", "hidden"]);
    expect(rowOfName("No Project").querySelector(".tag")).toBeNull();
  });
});

describe("the search of the Projects tab", () => {
  const tab = (search: string) =>
    renderToStaticMarkup(
      <ProjectsTab places={places} search={search} busy={[]} onShow={() => {}} onChange={() => {}} onRemoveLeftovers={() => {}} onRemoveStranded={() => {}} />,
    );

  test("is a box at the top of the Projects tab only, outside the rows that scroll", () => {
    const box = '<input aria-label="Search Projects" placeholder="Search Projects…"';
    const list = screen("projects").match(/<section class="panel places-list">[\s\S]*?<div class="scroll">/)?.[0] ?? "";
    expect(list).toContain(box);
    expect(screen("homes")).not.toContain(box);
  });

  test("narrows the rows to the Projects whose path holds it, and leaves out No Project", () => {
    const markup = unpieced(tab("KEEP"));
    expect(markup).toContain(`>${KEEP}</div>`);
    expect(markup).not.toContain(`>${HIDE}</div>`);
    expect(markup).not.toContain(`>${GONE}</div>`);
    expect(markup).not.toContain(">No Project</div>");
  });

  test("says when no Project holds it", () => {
    expect(tab("nothing")).toContain('<p class="empty">No Project path holds “nothing”.</p>');
  });
});

describe("the group of missing Projects without leftovers", () => {
  /** The Projects tab, searching for `search`. */
  const tab = (search = "") => (
    <ProjectsTab places={places} search={search} busy={[]} onShow={() => {}} onChange={() => {}} onRemoveLeftovers={() => {}} onRemoveStranded={() => {}} />
  );
  const titles = () => rendered.getAllByRole("heading").map((heading) => heading.textContent);
  const listed = (path: string) => rendered.queryByText(named(path)) !== null;

  test("follows Missing, closed, with its count, and leaves its rows out of Missing", () => {
    render(tab());

    expect(titles()).toEqual(["Projects · 2", "▾ Missing · 1", "▸ Missing, no leftovers · 1"]);
    expect(listed(GONE)).toBe(true);
    expect(listed(DELETED)).toBe(false);
  });

  test("offers Hide on each of its rows once opened", async () => {
    const changes: unknown[] = [];
    render(<ProjectsTab places={places} search="" busy={[]} onShow={() => {}} onChange={(change) => changes.push(change)} onRemoveLeftovers={() => {}} onRemoveStranded={() => {}} />);

    await userEvent.click(rendered.getByRole("button", { name: "▸ Missing, no leftovers · 1" }));
    const row = rendered.getByText(named(DELETED)).closest(".place-row") as HTMLElement;
    expect([...row.querySelectorAll("button")].map((button) => button.textContent)).toEqual(["Hide"]);
    expect(row.textContent).toContain("No leftovers");
    await userEvent.click(row.querySelector("button") as HTMLElement);
    expect(changes).toEqual([{ kind: "hide_project", path: DELETED }]);
  });

  test("opens while the search matches one of its rows, and closes again once the search is cleared", () => {
    const { rerender } = render(tab());

    rerender(tab("DELETED"));
    expect(titles()).toEqual(["Projects · 0", "Missing, no leftovers · 1"]);
    expect(listed(DELETED)).toBe(true);

    rerender(tab());
    expect(titles()).toContain("▸ Missing, no leftovers · 1");
    expect(listed(DELETED)).toBe(false);
  });

  test("stays open once the search is cleared when the user had opened it", async () => {
    const { rerender } = render(tab());

    await userEvent.click(rendered.getByRole("button", { name: "▸ Missing, no leftovers · 1" }));
    rerender(tab("DELETED"));
    rerender(tab());
    expect(listed(DELETED)).toBe(true);
  });
});

describe("a row of the Agent homes tab", () => {
  test("disables its buttons while an action runs on it, and only its own", () => {
    const markup = renderToStaticMarkup(
      <HomesTab places={places} busy={[rowOf({ agent: "claude-code", home: PORTABLE })]} onShow={() => {}} onChange={() => {}} />,
    );

    expectDisabled(markup, [
      [PORTABLE, true],
      ["default", false],
    ]);
  });

  test("draws Remove from list of a home the user added in the style of a button that deletes, and Unhide not", () => {
    const markup = renderToStaticMarkup(<HomesTab places={places} busy={[]} onShow={() => {}} onChange={() => {}} />);
    for (const [label, classes] of classesOf(markup, PORTABLE)) {
      expect(classes, label).toBe(label === "Remove from list" ? "cta small danger" : "cta small secondary");
    }
    expect(classesOf(markup, PORTABLE).map(([label]) => label)).toContain("Remove from list");
  });
});

describe("a path on a row of Projects & homes", () => {
  test("wraps a Project's path only right after a \\ or a /, each folder's name kept whole", () => {
    const markup = renderToStaticMarkup(
      <ProjectsTab places={places} search="" busy={[]} onShow={() => {}} onChange={() => {}} onRemoveLeftovers={() => {}} onRemoveStranded={() => {}} />,
    );
    for (const name of ["project-keep", "project-gone", "project-hide"]) {
      expect(markup, name).toContain(
        String.raw`<div class="place-name mono"><span class="path-piece">E:\</span><wbr/><span class="path-piece">cd-throwaway\</span><wbr/><span class="path-piece">` +
          `${name}</span><wbr/></div>`,
      );
    }
  });

  test("wraps an Agent home's folder only right after a \\ or a /, and the name of a home the user added, which is its folder", () => {
    const markup = renderToStaticMarkup(<HomesTab places={places} busy={[]} onShow={() => {}} onChange={() => {}} />);
    expect(markup).toContain(String.raw`<div class="place-name"><span class="path-piece">D:\</span><wbr/><span class="path-piece">portable</span><wbr/></div>`);
    // The default home's name is no path.
    expect(markup).toContain('<div class="place-name">default</div>');
    expect(markup).toContain(String.raw`<div class="place-sub mono"><span class="path-piece">D:\</span><wbr/><span class="path-piece">portable</span><wbr/></div>`);
    expect(markup).toContain(
      String.raw`<div class="place-sub mono"><span class="path-piece">C:\</span><wbr/><span class="path-piece">Users\</span><wbr/><span class="path-piece">me</span><wbr/></div>`,
    );
  });
});

const STORE = String.raw`C:\Users\me\AppData\Local\config-digger\parked\manifest.json`;

/** `name`, parked from a Claude Desktop home that is gone. */
function stranded(name: string): Stranded {
  const config = String.raw`E:\cd-throwaway\claude-desktop-stranded-home\claude_desktop_config.json`;
  return {
    name,
    kind: "mcp",
    agent: "claude-desktop",
    agent_name: "Claude Desktop",
    origin: {
      home: String.raw`E:\cd-throwaway\claude-desktop-stranded-home`,
      location: "user-mcp",
      project: null,
      path: config,
      key_path: ["mcpServers", name],
      locator: `${config}#mcpServers.${name}`,
    },
    reason: "home_not_found",
    sentence: `Stranded: the Claude Desktop home is not found, so config-digger cannot put \`${name}\` back.`,
    parked_at: "2026-10-01T12:00:00Z",
    path: STORE,
    key_path: ["entries", name],
    locator: `${STORE}#entries.${name}`,
    config: {},
    masked: [],
  };
}

describe("the Stranded group of the Projects tab", () => {
  const withStranded: Places = { ...places, stranded: [stranded("cd-throwaway-stranded"), stranded("cd-throwaway-other")] };
  const tab = (busy: string[] = [], onRemoveStranded: (locator: string | null) => void = () => {}, shown: Places = withStranded) => (
    <ProjectsTab places={shown} search="" busy={busy} onShow={() => {}} onChange={() => {}} onRemoveLeftovers={() => {}} onRemoveStranded={onRemoveStranded} />
  );

  test("lists each Stranded Entry with why it is Stranded and a Remove that deletes", () => {
    const markup = renderToStaticMarkup(tab());

    expect(markup).toContain("Stranded · 2");
    const row = rowMarkup(markup, "cd-throwaway-stranded");
    expect(row).toContain("MCP server · Claude Desktop");
    expect(row).toContain("cannot put `cd-throwaway-stranded` back");
    expect(row).toContain(`${STORE}#entries.cd-throwaway-stranded`);
    expect(classesOf(markup, "cd-throwaway-stranded")).toEqual([["Remove", "cta small danger"]]);
  });

  test("Removes one by its locator in the store, and all of them in one Operation", async () => {
    const removed: (string | null)[] = [];
    render(tab([], (locator) => removed.push(locator)));

    const row = rendered.getByText("cd-throwaway-other").closest(".place-row") as HTMLElement;
    await userEvent.click(row.querySelector("button") as HTMLElement);
    await userEvent.click(rendered.getByRole("button", { name: "Remove all" }));

    expect(removed).toEqual([`${STORE}#entries.cd-throwaway-other`, null]);
  });

  test("disables a row while its Remove runs, and every row while Remove all runs", () => {
    expectDisabled(renderToStaticMarkup(tab([`${STORE}#entries.cd-throwaway-stranded`])), [
      ["cd-throwaway-stranded", true],
      ["cd-throwaway-other", false],
    ]);
    expectDisabled(renderToStaticMarkup(tab([STRANDED_ROW])), [
      ["cd-throwaway-stranded", true],
      ["cd-throwaway-other", true],
    ]);
  });

  test("shows the Read failures of the parked-Entry store, and no group without either", () => {
    const failure: StoreReadFailure = {
      path: STORE,
      entry: "broken",
      kind: "shape",
      position: null,
      sentence: "The Entry `broken` of config-digger's parked-Entry store does not read as a parked Entry.",
    };

    const markup = renderToStaticMarkup(tab([], () => {}, { ...places, store_read_failures: [failure] }));

    expect(markup).toContain("Stranded · 0");
    expect(markup).toContain("The Entry `broken` of config-digger&#x27;s parked-Entry store does not read as a parked Entry.");
    expect(markup).not.toContain("Remove all");
    expect(renderToStaticMarkup(tab([], () => {}, places))).not.toContain("Stranded ·");
  });
});
