// @vitest-environment happy-dom
import { cleanup, render, screen, within } from "@testing-library/react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, test, vi } from "vitest";
import { ActionDialog, Changes } from "./ActionDialog";
import { commands, type ActionPreview, type Change } from "./bindings";
import { Icon, type IconName } from "./icons";
import { Loading } from "./Loading";

vi.mock("./bindings", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./bindings")>();
  return { ...actual, commands: { ...actual.commands, planAction: vi.fn(), executePlan: vi.fn(), cancelPlan: vi.fn(async () => ({ status: "ok", data: null })) } };
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
    warnings: [
      {
        class: "git-versioned",
        sentence: ".claude.json is in a git repo, so every clone gets this change.",
        advice: [{ todo: "Check the git diff before you commit.", why: "This location is git-versioned.", paths: [claudeJson] }],
      },
    ],
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

  test("confirms a Remove with a button in the style of a button that deletes, beside a Cancel that is not", async () => {
    await openPreview();
    expect(screen.getByRole("button", { name: "Remove" }).className).toBe("cta danger");
    expect(screen.getByRole("button", { name: "Cancel" }).className).toBe("cta secondary");
  });
});

describe("Before you apply", () => {
  const claudeJson = String.raw`E:\cd-throwaway\claude-code-home\.claude.json`;
  const mcpJson = String.raw`E:\cd-throwaway\project\.mcp.json`;
  const desktopConfig = String.raw`E:\cd-throwaway\claude-desktop-home\claude_desktop_config.json`;
  const planned: ActionPreview = {
    plan_id: "plan-1",
    preview: {
      action: "remove",
      title: "Remove 1 Entry",
      undo_of: null,
      backup_of: null,
      project: null,
      changes: [],
      effective: [],
      info: [
        {
          code: "version_outside_tested_range",
          path: String.raw`E:\cd-throwaway\bin\claude.exe`,
          sentence: "Claude Code 2.1.310 is newer than the versions this definition was tested with (2.1.286 to 2.1.302).",
        },
      ],
      files: [],
      folders: [],
      deleted: [],
      backup_folder: String.raw`E:\cd-throwaway\backups\1`,
      backup_note: String.raw`Each touched file is copied to E:\cd-throwaway\backups\1 before anything is written.`,
    },
    warnings: [
      {
        class: "running-agent",
        sentence: "Claude Code is running and rewrites … Close Claude Code first to be safe. Claude Code is running and reads …",
        advice: [
          {
            todo: "Close Claude Code first to be safe.",
            why: "Claude Code is running and rewrites this location while it runs: it may overwrite what this Operation writes.",
            paths: [claudeJson],
          },
          {
            todo: "Restart Claude Code once this is applied.",
            why: "Claude Code is running and reads this location: it may not see what this Operation writes until it restarts.",
            paths: [mcpJson],
          },
        ],
      },
    ],
    blocks: [
      {
        code: "running_agent_parking",
        sentence: "Claude Desktop is running and rewrites … Close Claude Desktop and retry.",
        advice: [
          {
            todo: "Close Claude Desktop and retry.",
            why: "Claude Desktop is running and rewrites this location from memory, which would undo moving an Entry in or out of it.",
            paths: [desktopConfig],
          },
        ],
      },
    ],
  };

  const openPreview = async () => {
    vi.mocked(commands.planAction).mockResolvedValue({ status: "ok", data: planned });
    render(<ActionDialog request={{ kind: "entry", verb: "remove", locator: `${claudeJson}#mcpServers.cd-throwaway-notes`, project: null }} onClose={() => {}} onUndo={() => {}} onOpenHistory={() => {}} notify={() => {}} />);
    await screen.findByRole("heading", { name: "Before you apply" });
  };

  /** The name of the icon an item starts with, from the shapes each icon draws. */
  const iconOf = (item: Element) => {
    const shapes = (markup: string) => markup.replace(/ (id|mask)="[^"]*"/g, "").replace(/<svg[^>]*>|<\/svg>/g, "");
    const icon = item.querySelector(":scope > svg.icon");
    const names: IconName[] = ["warning", "flag", "info"];
    return names.find((name) => icon !== null && shapes(icon.outerHTML) === shapes(renderToStaticMarkup(<Icon name={name} small />)));
  };

  /** Each line of an item's body, as [its class, its text]. */
  const lines = (item: Element) => [...item.querySelectorAll(".check-body > *")].map((line) => [line.className, line.textContent]);

  test("gives a warning a block of its own, with the warning icon, what to do on its first line and each path on a line of its own", async () => {
    await openPreview();
    const item = screen.getByRole("checkbox").closest(".check")!;

    expect(item.className).toBe("check warning");
    expect(iconOf(item)).toBe("warning");
    expect(lines(item)).toEqual([
      ["check-todo", "WarningClose Claude Code first to be safe."],
      ["check-why", "Claude Code is running and rewrites this location while it runs: it may overwrite what this Operation writes."],
      ["check-path", claudeJson],
      ["check-todo", "Restart Claude Code once this is applied."],
      ["check-why", "Claude Code is running and reads this location: it may not see what this Operation writes until it restarts."],
      ["check-path", mcpJson],
    ]);
  });

  test("gives a block that stops Apply an item of its own, what to do on its first line, its path apart, then Check again", async () => {
    await openPreview();
    const item = screen.getByRole("button", { name: "Check again" }).closest(".check")!;

    expect(item.className).toBe("check block");
    expect(iconOf(item)).toBe("flag");
    expect(lines(item)).toEqual([
      ["check-todo", "BlockedClose Claude Desktop and retry."],
      ["check-why", "Claude Desktop is running and rewrites this location from memory, which would undo moving an Entry in or out of it."],
      ["check-path", desktopConfig],
      ["check-fix", "Check again"],
    ]);
  });

  test("shows the sentence of a warning that comes without advice", async () => {
    const sentence = "Claude Code is running and reads the file.";
    vi.mocked(commands.planAction).mockResolvedValue({ status: "ok", data: { ...planned, warnings: [{ class: "running-agent", sentence, advice: [] }] } });
    render(<ActionDialog request={{ kind: "entry", verb: "remove", locator: "x", project: null }} onClose={() => {}} onUndo={() => {}} onOpenHistory={() => {}} notify={() => {}} />);

    const item = (await screen.findByRole("checkbox")).closest(".check")!;
    expect(lines(item)).toEqual([["check-why", `Warning${sentence}`]]);
  });

  test("shows a note, such as an Installation outside its Tested range, as information, with its icon", async () => {
    await openPreview();
    const item = screen.getByText(/is newer than the versions/).closest(".check")!;

    expect(item.className).toBe("check info");
    expect(iconOf(item)).toBe("info");
  });
});

describe("the result of an action", () => {
  test("gives its hints and its note in the style of a dialog's hints", async () => {
    vi.mocked(commands.planAction).mockResolvedValue({
      status: "ok",
      data: {
        plan_id: "plan-1",
        preview: {
          action: "remove",
          title: "Remove 1 Entry",
          undo_of: null,
          backup_of: null,
          project: null,
          changes: [],
          effective: [],
          info: [],
          files: [],
          folders: [],
          deleted: [],
          backup_folder: String.raw`E:\cd-throwaway\backups\1`,
          backup_note: "Each touched file is copied first.",
        },
        warnings: [],
        blocks: [],
      },
    });
    vi.mocked(commands.executePlan).mockResolvedValue({
      status: "ok",
      data: {
        kind: "applied",
        outcome: {
          operation: "20261023-101500-a1b",
          verify: "verified",
          differences: [],
          backup: String.raw`E:\cd-throwaway\backups\1`,
          sentence: "Removed 1 Entry. Operation 20261023-101500-a1b: verified.",
          hints: ["Restart Claude Code so that it reads what the Operation wrote."],
        },
      },
    });
    render(<ActionDialog request={{ kind: "entry", verb: "remove", locator: "x", project: null }} onClose={() => {}} onUndo={() => {}} onOpenHistory={() => {}} notify={() => {}} />);
    (await screen.findByRole("button", { name: "Remove" })).click();

    const hint = await screen.findByText("Restart Claude Code so that it reads what the Operation wrote.");
    expect(hint.className).toBe("callout-hint");
    expect(screen.getByText("Operation 20261023-101500-a1b · backup kept").className).toBe("callout-hint");
  });
});
