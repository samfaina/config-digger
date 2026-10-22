// @vitest-environment happy-dom
import { cleanup, render, screen, within } from "@testing-library/react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, test } from "vitest";
import { ActionDialog, Changes } from "./ActionDialog";
import type { Change } from "./bindings";
import { Loading } from "./Loading";

afterEach(cleanup);

describe("the action dialog while it prepares the preview", () => {
  test("shows the busy indicator naming what it reads, over placeholder rows", () => {
    const markup = renderToStaticMarkup(
      <ActionDialog
        request={{ kind: "entry", verb: "disable", locator: String.raw`C:\Users\me\.claude.json#mcpServers.notes`, project: null }}
        onClose={() => {}}
        onUndo={() => {}}
        onOpenHistory={() => {}}
        notify={() => {}}
      />,
    );

    expect(markup).toContain("Preparing the preview…");
    expect(markup).toContain(renderToStaticMarkup(<Loading text="Reading the files this action touches…" rows={3} />));
  });
});

describe("the What changes block", () => {
  const claudeJson = String.raw`E:\cd-throwaway\claude-code-home\.claude.json`;
  const gone = "E:/cd-throwaway/project-gone";
  const leftover: Change = {
    agent: "claude-code",
    agent_name: "Claude Code",
    home: "default",
    item: "cd-throwaway-leftover",
    path: claudeJson,
    locator: `${claudeJson}#projects.${gone}.mcpServers.cd-throwaway-leftover`,
    project: gone,
    before: "unread",
    after: "removed",
  };

  /** Each labelled part of `term`'s list, as [label, what it says]. */
  const partsOf = (term: HTMLElement) =>
    within(term.closest("dl")!)
      .getAllByRole("term")
      .map((term) => [term.textContent, term.nextElementSibling?.textContent]);

  /** Each labelled part of the change to the Entry `item`, as [label, what it says]. */
  const parts = (item: string) =>
    partsOf(screen.getAllByRole("term").find((term) => term.textContent === "Entry" && term.nextElementSibling?.textContent === item)!);

  test("labels each part of a change: its Entry, its Agent, its key and its change of State", () => {
    render(<Changes changes={[leftover]} notify={() => {}} />);

    expect(parts("cd-throwaway-leftover")).toEqual([
      ["Entry", "cd-throwaway-leftover"],
      ["Agent", "Claude Code"],
      ["Key", "projects.E:/cd-throwaway/project-gone.mcpServers.cd-throwaway-leftover"],
      ["State", "Unread → Removed"],
    ]);
  });

  test("labels the Project and the file a change is under, and a skill folder as a folder", () => {
    const folder = String.raw`E:\cd-throwaway\claude-code-home\skills\pdf`;
    const skill: Change = { ...leftover, item: "pdf", path: folder, locator: folder, project: null, before: "effective", after: "disabled" };
    render(<Changes changes={[leftover, skill]} notify={() => {}} />);

    expect(screen.getAllByRole("heading", { level: 3 }).map((heading) => heading.textContent)).toEqual([`Project ${gone}`, "No Project"]);
    expect(screen.getAllByRole("heading", { level: 4 }).map((heading) => heading.textContent)).toEqual(["File .claude.json", "Folder pdf"]);
    expect(parts("pdf")).toEqual([
      ["Entry", "pdf"],
      ["Agent", "Claude Code"],
      ["State", "Effective → Disabled"],
    ]);
  });
});
