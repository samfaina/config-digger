// @vitest-environment happy-dom
import { cleanup, render, screen, within } from "@testing-library/react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, test, vi } from "vitest";
import { ActionDialog, Changes } from "./ActionDialog";
import { commands, type ActionPreview, type Change } from "./bindings";
import { Loading } from "./Loading";

vi.mock("./bindings", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./bindings")>();
  return { ...actual, commands: { ...actual.commands, planAction: vi.fn(), cancelPlan: vi.fn(async () => ({ status: "ok", data: null })) } };
});

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

  /** Each labelled part of the change to the Entry `item`, as [label, what it says]. */
  const parts = (item: string) => {
    const entry = screen.getAllByRole("term").find((term) => term.textContent === "Entry" && term.nextElementSibling?.textContent === item);
    if (!entry) throw new Error(`No change to the Entry ${item}`);
    return within(entry.closest("dl")!)
      .getAllByRole("term")
      .map((label) => [label.textContent, label.nextElementSibling?.textContent]);
  };

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

describe("the preview of an action with many changes", () => {
  const claudeJson = String.raw`E:\cd-throwaway\claude-code-home\.claude.json`;
  const changes: Change[] = Array.from({ length: 40 }, (_, index) => ({
    agent: "claude-code",
    agent_name: "Claude Code",
    home: "default",
    item: `cd-throwaway-server-${index}`,
    path: claudeJson,
    locator: `${claudeJson}#mcpServers.cd-throwaway-server-${index}`,
    project: null,
    before: "effective",
    after: "removed",
  }));
  const planned: ActionPreview = {
    plan_id: "plan-1",
    preview: {
      action: "remove",
      title: "Remove 40 Entries",
      undo_of: null,
      backup_of: null,
      project: null,
      changes,
      effective: [],
      info: [],
      files: [
        {
          path: claudeJson,
          git_versioned: true,
          store_class: "plain",
          edits: [],
          diff: "@@ -1,2 +1,1 @@\n {\n-  \"mcpServers\": {}",
          diff_unavailable: null,
          diff_note: null,
          changed_lines: 1,
        },
      ],
      folders: [],
      deleted: [],
      backup_folder: String.raw`E:\cd-throwaway\backups\1`,
      backup_note: String.raw`Each file is copied to E:\cd-throwaway\backups\1 first.`,
    },
    warnings: [{ class: "git-versioned", sentence: ".claude.json is in a git repo, so every clone gets this change." }],
    blocks: [],
  };

  /** Opens the dialog on the preview `planned`, once its What changes shows. */
  const openPreview = async () => {
    vi.mocked(commands.planAction).mockResolvedValue({ status: "ok", data: planned });
    render(<ActionDialog request={{ kind: "entry", verb: "remove", locator: changes[0].locator, project: null }} onClose={() => {}} onUndo={() => {}} onOpenHistory={() => {}} notify={() => {}} />);
    return screen.findByRole("region", { name: "What changes" });
  };

  test("scrolls only What changes and Files, each under its title, keeping the title, Before you apply and the buttons in view", async () => {
    const whatChanges = await openPreview();
    const files = screen.getByRole("region", { name: "Files · 1" });
    expect(screen.getAllByRole("region")).toEqual([whatChanges, files]);
    // Its Entry and the last piece of its key name each change.
    for (const change of changes) expect(within(whatChanges).getAllByText(change.item)).toHaveLength(2);
    expect(within(files).getByText(claudeJson)).toBeTruthy();
    // Each scrolls under its title, which stays in view.
    expect(within(whatChanges).queryByRole("heading", { name: "What changes" })).toBeNull();
    expect(within(files).queryByRole("heading", { name: "Files · 1" })).toBeNull();

    const inView = [
      screen.getByRole("heading", { level: 1, name: "Remove 40 Entries" }),
      screen.getByRole("heading", { name: "Before you apply" }),
      screen.getByRole("checkbox"),
      screen.getByRole("button", { name: "Cancel" }),
      screen.getByText("Tick 1 warning to apply"),
      screen.getByRole("button", { name: "Remove" }),
    ];
    for (const element of inView) expect(element.closest("[role=region]"), element.textContent ?? "").toBeNull();
  });

  test("lets the keyboard reach each scrolling block, to scroll it", async () => {
    await openPreview();
    for (const region of screen.getAllByRole("region")) expect(region.tabIndex).toBe(0);
  });
});
