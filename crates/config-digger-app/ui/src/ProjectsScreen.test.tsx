import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import type { Entry, Home, Item, Places, Project } from "./bindings";
import { rowOf } from "./places";
import { Loading } from "./Loading";
import { HomesTab, ProjectsScreen, ProjectsTab, type Tab } from "./ProjectsScreen";

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
const PORTABLE = String.raw`D:\portable`;

const places: Places = {
  read_at: "2026-10-20T09:00:00Z",
  projects: [project(), project({ path: KEEP }), project({ path: GONE, missing: true })],
  leftovers: [],
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

/** The opening tag of each button of the row named `name`. */
const buttonsOf = (markup: string, name: string) => {
  const row = markup.split('<div class="place-row').find((each) => each.includes(`>${name}</div>`)) ?? "";
  return row.match(/<button[^>]*>/g) ?? [];
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
      <ProjectsTab places={places} search="" busy={[HIDE, GONE]} onShow={() => {}} onChange={() => {}} onRemoveLeftovers={() => {}} />,
    );

    expectDisabled(markup, [
      [HIDE, true],
      [GONE, true],
      [KEEP, false],
    ]);
  });

  test("draws Remove leftovers of a missing Project in the style of a button that deletes, and Show leftovers not", () => {
    const claudeJson = String.raw`C:\Users\me\.claude.json`;
    const entry = { path: claudeJson, locator: `${claudeJson}#projects.${GONE}.mcpServers.old` } as Entry;
    const leftover = { agent: "claude-code", home: "default", kind: "mcp", name: "old", state: "unread", entries: [entry], built: null } as Item;
    const markup = renderToStaticMarkup(
      <ProjectsTab
        places={{ ...places, leftovers: [{ project: GONE, items: [leftover] }] }}
        search=""
        busy={[]}
        onShow={() => {}}
        onChange={() => {}}
        onRemoveLeftovers={() => {}}
      />,
    );

    const classes = buttonsOf(markup, GONE).map((button) => button.match(/class="([^"]*)"/)?.[1]);
    expect(classes).toEqual(["cta small secondary", "cta small danger"]);
  });
});

describe("the search of the Projects tab", () => {
  const tab = (search: string) =>
    renderToStaticMarkup(
      <ProjectsTab places={places} search={search} busy={[]} onShow={() => {}} onChange={() => {}} onRemoveLeftovers={() => {}} />,
    );

  test("is a box at the top of the Projects tab only, outside the rows that scroll", () => {
    const box = '<input aria-label="Search Projects" placeholder="Search Projects…"';
    const list = screen("projects").match(/<section class="panel places-list">[\s\S]*?<div class="scroll">/)?.[0] ?? "";
    expect(list).toContain(box);
    expect(screen("homes")).not.toContain(box);
  });

  test("narrows the rows to the Projects whose path holds it, and leaves out No Project", () => {
    const markup = tab("KEEP");
    expect(markup).toContain(`>${KEEP}</div>`);
    expect(markup).not.toContain(`>${HIDE}</div>`);
    expect(markup).not.toContain(`>${GONE}</div>`);
    expect(markup).not.toContain(">No Project</div>");
  });

  test("says when no Project holds it", () => {
    expect(tab("nothing")).toContain('<p class="empty">No Project path holds “nothing”.</p>');
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
});
