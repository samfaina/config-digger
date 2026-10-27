// @vitest-environment happy-dom
import { cleanup, render, screen, within } from "@testing-library/react";
import { renderToStaticMarkup } from "react-dom/server";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterAll, afterEach, beforeAll, describe, expect, test, vi } from "vitest";
import { ActionDialog, BackupPath, Changes } from "./ActionDialog";
import { commands, type ActionPreview, type Change, type Error as ApiError } from "./bindings";
import { Icon, type IconName } from "./icons";
import { Loading } from "./Loading";

vi.mock("./bindings", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./bindings")>();
  const done = async () => ({ status: "ok" as const, data: null });
  return {
    ...actual,
    commands: { ...actual.commands, planAction: vi.fn(), executePlan: vi.fn(), cancelPlan: vi.fn(done), openBackupFolder: vi.fn(done), revealLocation: vi.fn(done) },
  };
});

afterEach(cleanup);

describe("Open backup folder", () => {
  test("opens Explorer inside the backup folder, rather than showing it selected in its parent", () => {
    const backup = String.raw`C:\Users\me\AppData\Local\config-digger\backups\20260101T000000Z-remove`;
    render(<BackupPath backup={backup} notify={() => {}} />);

    screen.getByRole("button", { name: "Open backup folder" }).click();

    expect(commands.openBackupFolder).toHaveBeenCalledWith(backup);
    expect(commands.revealLocation).not.toHaveBeenCalled();
  });
});

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

  test("scrolls only What changes and Files in regions of their own, each under its title, with the title, Before you apply and the buttons outside them", async () => {
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
  const desktopConfig = String.raw`E:\cd-throwaway\paths-that-wrap\in-the-smallest-window\of-config-digger\claude-desktop-home\claude_desktop_config.json`;
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

describe("the action dialog in each phase", () => {
  const claudeJson = String.raw`E:\cd-throwaway\claude-code-home\.claude.json`;
  const mcpJson = String.raw`E:\cd-throwaway\project\.mcp.json`;
  const backup = String.raw`E:\cd-throwaway\backups\1`;
  const backupNote = `Each touched file is copied to ${backup} before anything is written.`;
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
      info: [{ code: "version_outside_tested_range", path: String.raw`E:\cd-throwaway\bin\claude.exe`, sentence: "Claude Code 2.1.310 is newer than the versions this definition was tested with." }],
      files: [],
      folders: [],
      deleted: [],
      backup_folder: backup,
      backup_note: backupNote,
    },
    warnings: [],
    blocks: [],
  };
  const diffs = [{ path: claudeJson, diff: "@@ -1 +1 @@\n-{}\n+{ }", diff_unavailable: null, diff_note: null, changed_lines: 1 }];
  const failure = (code: ApiError["code"], failed: ApiError["failed"], withDiffs = false): ApiError => ({
    code,
    message: `cannot write ${claudeJson}: Access is denied. (os error 5)`,
    candidates: [],
    backup,
    diffs: withDiffs ? diffs : [],
    failed,
  });
  type Executed = Awaited<ReturnType<typeof commands.executePlan>>;
  const verifiedWithDifferences: Executed = {
    status: "ok",
    data: {
      kind: "applied",
      outcome: {
        operation: "20261023-101500-a1b",
        verify: "verified_with_differences",
        differences: [{ agent: "claude-code", home: "default", locator: `${claudeJson}#mcpServers.cd-throwaway-notes`, predicted: "removed", actual: "effective" }],
        backup,
        sentence: "Removed 1 Entry. Operation 20261023-101500-a1b: verified with differences.",
        hints: [],
      },
    },
  };
  const rolledBack: Executed = {
    status: "error",
    error: failure("verify_failed", {
      operation: "20261023-101500-a1b",
      failure: null,
      rollback: [
        { path: claudeJson, outcome: "restored", reason: null },
        { path: mcpJson, outcome: "not_written", reason: null },
      ],
    }),
  };
  const notApplied: Executed = { status: "error", error: failure("unexpected", null) };
  const rollbackIncomplete: Executed = {
    status: "error",
    error: failure(
      "rollback_incomplete",
      { operation: "20261023-101500-a1b", failure: "verify_failed", rollback: [{ path: claudeJson, outcome: "not_restored", reason: "Something else is there since config-digger touched it." }] },
      true,
    ),
  };

  const open = () =>
    render(<ActionDialog request={{ kind: "entry", verb: "remove", locator: "x", project: null }} onClose={() => {}} onUndo={() => {}} onOpenHistory={() => {}} notify={() => {}} />);

  const preview = async () => {
    vi.mocked(commands.planAction).mockResolvedValue({ status: "ok", data: planned });
    open();
    await screen.findByRole("button", { name: "Remove" });
  };

  const planFailed = async () => {
    vi.mocked(commands.planAction).mockResolvedValue({ status: "error", error: failure("undo_refused", null, true) });
    open();
    await screen.findByText("This action can't be previewed");
  };

  /** Opens the dialog, applies its preview and waits for `executed`'s result, which shows the button `settled`. */
  const applyTo = async (executed: Executed, settled: string) => {
    vi.mocked(commands.executePlan).mockResolvedValue(executed);
    await preview();
    screen.getByRole("button", { name: "Remove" }).click();
    await screen.findByRole("button", { name: settled });
  };

  describe("keeps its title, its backup note and its buttons out of what scrolls", () => {
    const stylesheet = document.createElement("style");
    beforeAll(() => {
      stylesheet.textContent = readFileSync(join(import.meta.dirname, "styles.css"), "utf8");
      document.head.append(stylesheet);
    });
    afterAll(() => stylesheet.remove());

    /** The elements around `element` that scroll what they hold, by the app's stylesheet, up to its dialog and the dialog too. */
    const scrollingAround = (element: Element) => {
      const found: Element[] = [];
      for (let around = element.parentElement; around !== null; around = around.parentElement) {
        const style = getComputedStyle(around);
        if ([style.overflow, style.overflowY].some((value) => /auto|scroll/.test(value))) found.push(around);
        if (around.getAttribute("role") === "dialog") break;
      }
      return found;
    };

    type Parts = { title: string; buttons: string[]; noted: boolean; between: () => Element[] };
    const phases: [string, () => Promise<void>, Parts][] = [
      [
        "planning",
        async () => {
          vi.mocked(commands.planAction).mockReturnValue(new Promise(() => {}));
          open();
        },
        { title: "Preparing the preview…", buttons: [], noted: false, between: () => [screen.getByText("Reading the files this action touches…")] },
      ],
      [
        "plan_failed",
        planFailed,
        {
          title: "Preparing the preview…",
          buttons: ["Open backup folder", "Copy path", "Check again", "Close"],
          noted: false,
          between: () => [screen.getByRole("alert"), screen.getByRole("region", { name: "Backup → now" })],
        },
      ],
      [
        "preview",
        preview,
        {
          title: "Remove 1 Entry",
          buttons: ["Cancel", "Remove"],
          noted: true,
          between: () => [
            screen.getByRole("region", { name: "What changes" }),
            screen.getByRole("heading", { name: "Before you apply" }),
            screen.getByText(/is newer than the versions/),
            screen.getByRole("region", { name: "Files · 0" }),
          ],
        },
      ],
      [
        "applying",
        async () => {
          vi.mocked(commands.executePlan).mockReturnValue(new Promise(() => {}));
          await preview();
          screen.getByRole("button", { name: "Remove" }).click();
          await screen.findByText("Back up");
        },
        { title: "Remove 1 Entry", buttons: [], noted: true, between: () => [screen.getByRole("list")] },
      ],
      [
        "done, verified with differences",
        () => applyTo(verifiedWithDifferences, "Done"),
        {
          title: "Remove 1 Entry",
          buttons: ["Undo", "Open History", "Done"],
          noted: false,
          between: () => [screen.getByRole("list"), screen.getByRole("status"), screen.getByRole("region", { name: "Verified with differences" })],
        },
      ],
      [
        "done, failed and rolled back",
        () => applyTo(rolledBack, "Retry"),
        {
          title: "Remove 1 Entry",
          buttons: ["Close", "Retry"],
          noted: false,
          between: () => [screen.getByRole("list"), screen.getByRole("alert"), screen.getByRole("region", { name: "Restored" }), screen.getByRole("region", { name: "Untouched" })],
        },
      ],
      [
        "done, not applied",
        () => applyTo(notApplied, "Retry"),
        { title: "Remove 1 Entry", buttons: ["Close", "Retry"], noted: false, between: () => [screen.getByRole("alert")] },
      ],
      [
        "done, with its Rollback incomplete",
        () => applyTo(rollbackIncomplete, "Open History"),
        {
          title: "Remove 1 Entry",
          buttons: ["Open backup folder", "Open History", "Close"],
          noted: false,
          between: () => [
            screen.getByRole("list"),
            screen.getByRole("alert"),
            screen.getByRole("region", { name: "Locations" }),
            screen.getByRole("region", { name: "Backup → now" }),
          ],
        },
      ],
    ];

    test.each(phases)("%s, and scrolls all that lies between them as one", async (_, reach, { title, buttons, noted, between }) => {
      await reach();

      const fixed = [screen.getByRole("heading", { level: 1, name: title }), ...buttons.map((name) => screen.getByRole("button", { name }))];
      if (noted) fixed.push(screen.getByText(backupNote));
      for (const element of fixed) expect(scrollingAround(element), element.textContent ?? "").toEqual([]);

      // The outermost element that scrolls each part between the head and the foot: one body, inside the dialog.
      const bodies = between().map((element) => scrollingAround(element).at(-1));
      expect(bodies[0]?.getAttribute("role")).toBeNull();
      for (const body of bodies) expect(body).toBe(bodies[0]);
    });
  });

  test.each([
    ["a preview that failed, with its backup → now diffs", planFailed, ["Backup → now"]],
    ["an Operation verified with differences", () => applyTo(verifiedWithDifferences, "Done"), ["Verified with differences"]],
    ["a failed Operation, rolled back", () => applyTo(rolledBack, "Retry"), ["Restored", "Untouched"]],
    ["an incomplete Rollback", () => applyTo(rollbackIncomplete, "Open History"), ["Locations", "Backup → now"]],
  ])("scrolls each list of %s in a region of its own, named by its title, that the keyboard can reach", async (_, reach: () => Promise<void>, titles: string[]) => {
    await reach();
    expect(screen.getAllByRole("region")).toEqual(titles.map((title) => screen.getByRole("region", { name: title })));
    for (const region of screen.getAllByRole("region")) expect(region.tabIndex).toBe(0);
  });
});
