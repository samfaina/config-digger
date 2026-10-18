import { describe, expect, test } from "vitest";
import { addHomeOffer, ADDS, homeToCheck, NO_HOME_CHOSEN, projectWarning, type HomeChecked } from "./adding";
import type { Agent, Home, HomeCheck, Places } from "./bindings";

const CODEX_HOME = String.raw`E:\cd-throwaway\codex-home`;

describe("what the header button adds", () => {
  test("names it, as the title of the dialog it opens does, for each tab", () => {
    expect(ADDS).toEqual({ projects: "Add Project", homes: "Add Agent home" });
  });
});

describe("the Add Agent home dialog", () => {
  test("starts with no Agent chosen, and offers Add only once an Agent and a folder are", () => {
    expect(addHomeOffer(NO_HOME_CHOSEN, null, false).addable).toBe(false);
    expect(addHomeOffer({ agent: "", folder: CODEX_HOME }, null, false).addable).toBe(false);
    expect(addHomeOffer({ agent: "codex", folder: "  " }, null, false).addable).toBe(false);
    expect(addHomeOffer({ agent: "codex", folder: CODEX_HOME }, null, false).addable).toBe(true);
    expect(addHomeOffer({ agent: "codex", folder: CODEX_HOME }, null, true).addable).toBe(false);
  });

  const EMPTY = String.raw`No configuration found for Claude Code in E:\cd-throwaway\codex-home: none of .claude.json, .claude\plugins, .claude\settings.json and .claude\skills is there. It can still be added, as a home with no Entries.`;

  function check(overrides: Partial<HomeCheck> = {}): HomeCheck {
    return {
      agent: "claude-code",
      agent_name: "Claude Code",
      folder: CODEX_HOME,
      looked_for: [".claude.json", String.raw`.claude\plugins`, String.raw`.claude\settings.json`, String.raw`.claude\skills`],
      no_configuration: EMPTY,
      ...overrides,
    };
  }

  test("checks the folder once an Agent and a folder are chosen", () => {
    expect(homeToCheck(NO_HOME_CHOSEN)).toBeNull();
    expect(homeToCheck({ agent: "", folder: CODEX_HOME })).toBeNull();
    expect(homeToCheck({ agent: "claude-code", folder: ` ${CODEX_HOME} ` })).toEqual({ agent: "claude-code", folder: CODEX_HOME });
  });

  test("warns before Add, still offering it, when the folder holds none of the files the Agent reads", () => {
    const choice = { agent: "claude-code", folder: CODEX_HOME };
    const checked: HomeChecked = { checked: homeToCheck(choice)!, check: check() };

    expect(addHomeOffer(choice, checked, false)).toEqual({ addable: true, warning: EMPTY });
    expect(addHomeOffer(choice, { ...checked, check: check({ no_configuration: null }) }, false).warning).toBeNull();
  });

  test("drops the warning of a check made for another Agent or folder", () => {
    const checked: HomeChecked = { checked: { agent: "claude-code", folder: CODEX_HOME }, check: check() };

    expect(addHomeOffer({ agent: "codex", folder: CODEX_HOME }, checked, false).warning).toBeNull();
    expect(addHomeOffer({ agent: "claude-code", folder: String.raw`E:\other` }, checked, false).warning).toBeNull();
  });
});

describe("the Add Project dialog", () => {
  function home(agent: string, agent_name: string, folder: string, hidden = false): Home {
    return { agent, agent_name, home: folder, folder, source: { kind: "added" }, hidden } as Home;
  }

  function places(agents: Pick<Agent, "id" | "name" | "homes">[]): Places {
    return { read_at: "2026-10-10T09:00:00Z", projects: [], leftovers: [], agents: agents as Agent[] };
  }

  const listed = places([
    { id: "claude-code", name: "Claude Code", homes: [home("claude-code", "Claude Code", CODEX_HOME)] },
    { id: "codex", name: "Codex", homes: [home("codex", "Codex", CODEX_HOME, true)] },
  ]);

  test("warns when the folder is an Agent home already in the list, hidden or not, in any spelling", () => {
    const sentence = "is already listed as an Agent home of Claude Code and Codex. As a Project, it is read as a folder Agents are started on, not as their configuration.";

    expect(projectWarning(listed, CODEX_HOME)).toBe(`${CODEX_HOME} ${sentence}`);
    expect(projectWarning(listed, " e:/CD-throwaway/codex-home/ ")).toBe(`e:/CD-throwaway/codex-home/ ${sentence}`);
  });

  test("says nothing for a folder no listed home is at", () => {
    expect(projectWarning(listed, String.raw`E:\cd-throwaway\codex-home\src`)).toBeNull();
    expect(projectWarning(listed, "")).toBeNull();
  });
});
