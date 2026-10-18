import { describe, expect, test } from "vitest";
import {
  ADD_LABELS,
  addHomeOffer,
  addProjectOffer,
  homeToCheck,
  NO_HOME_CHOSEN,
  projectToCheck,
  type HomeChecked,
  type ProjectChecked,
} from "./adding";
import type { HomeCheck } from "./bindings";

const CODEX_HOME = String.raw`E:\cd-throwaway\codex-home`;

describe("what the header button adds", () => {
  test("names it, as the title of the dialog it opens does, for each tab", () => {
    expect(ADD_LABELS).toEqual({ projects: "Add Project", homes: "Add Agent home" });
  });
});

describe("the Add Agent home dialog", () => {
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

  const CHOSEN = { agent: "claude-code", folder: CODEX_HOME };
  const CHECKED: HomeChecked = { asked: CHOSEN, result: check() };

  test("checks the folder only once an Agent and a folder are chosen", () => {
    expect(homeToCheck(NO_HOME_CHOSEN)).toBeNull();
    expect(homeToCheck({ agent: "", folder: CODEX_HOME })).toBeNull();
    expect(homeToCheck({ agent: "claude-code", folder: "  " })).toBeNull();
    expect(homeToCheck({ agent: "claude-code", folder: ` ${CODEX_HOME} ` })).toEqual(CHOSEN);
  });

  test("starts with no Agent chosen, and offers Add only once an Agent and a folder are chosen and checked", () => {
    expect(addHomeOffer(NO_HOME_CHOSEN, null, false).addable).toBe(false);
    expect(addHomeOffer({ agent: "", folder: CODEX_HOME }, null, false).addable).toBe(false);
    expect(addHomeOffer(CHOSEN, null, false).addable).toBe(false);
    expect(addHomeOffer(CHOSEN, CHECKED, false).addable).toBe(true);
    expect(addHomeOffer(CHOSEN, CHECKED, true).addable).toBe(false);
  });

  test("warns before Add, still offering it, when the folder holds none of the files the Agent reads", () => {
    expect(addHomeOffer(CHOSEN, CHECKED, false)).toEqual({ addable: true, warning: EMPTY });
    expect(addHomeOffer(CHOSEN, { asked: CHOSEN, result: check({ no_configuration: null }) }, false)).toEqual({
      addable: true,
      warning: null,
    });
  });

  test("leaves a folder the core could not check to Add, which says why it refuses it", () => {
    expect(addHomeOffer(CHOSEN, { asked: CHOSEN, result: null }, false)).toEqual({ addable: true, warning: null });
  });

  test("waits for a new check once the Agent or the folder changes", () => {
    expect(addHomeOffer({ agent: "codex", folder: CODEX_HOME }, CHECKED, false)).toEqual({ addable: false, warning: null });
    expect(addHomeOffer({ agent: "claude-code", folder: String.raw`E:\other` }, CHECKED, false)).toEqual({
      addable: false,
      warning: null,
    });
  });
});

describe("the Add Project dialog", () => {
  const WARNING = `${CODEX_HOME} is already listed as an Agent home of Claude Code. As a Project, it is read as a folder Agents are started on, not as their configuration.`;
  const CHECKED: ProjectChecked = { asked: CODEX_HOME, result: { path: CODEX_HOME, home_of: ["Claude Code"], warning: WARNING } };

  test("checks the path only once one is typed or picked", () => {
    expect(projectToCheck("  ")).toBeNull();
    expect(projectToCheck(` ${CODEX_HOME} `)).toBe(CODEX_HOME);
  });

  test("warns before Add, still offering it, when the folder is an Agent home already in the list", () => {
    expect(addProjectOffer(` ${CODEX_HOME}`, CHECKED, false)).toEqual({ addable: true, warning: WARNING });
  });

  test("offers Add without a warning once checked, and waits for the check of a changed path", () => {
    const elsewhere = String.raw`E:\code\shop`;

    expect(addProjectOffer(elsewhere, { asked: elsewhere, result: { path: elsewhere, home_of: [], warning: null } }, false)).toEqual({
      addable: true,
      warning: null,
    });
    expect(addProjectOffer(elsewhere, { asked: elsewhere, result: null }, false)).toEqual({ addable: true, warning: null });
    expect(addProjectOffer(elsewhere, CHECKED, false)).toEqual({ addable: false, warning: null });
    expect(addProjectOffer("", null, false).addable).toBe(false);
    expect(addProjectOffer(CODEX_HOME, CHECKED, true).addable).toBe(false);
  });
});
