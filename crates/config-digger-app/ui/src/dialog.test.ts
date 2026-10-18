import { describe, expect, test } from "vitest";
import type { ActionPreview, Change, Error as ApiError, FileChange, FolderChange, Outcome, Preview, Warning } from "./bindings";
import {
  appliedResult,
  applyButton,
  changeGroups,
  diffView,
  failedResult,
  keptAcknowledgements,
  locations,
  subtitle,
  type Result,
} from "./dialog";

function preview(overrides: Partial<Preview> = {}): Preview {
  return {
    action: "disable",
    title: "Disable notes",
    undo_of: null,
    backup_of: null,
    project: null,
    changes: [],
    effective: [],
    info: [],
    files: [],
    folders: [],
    deleted: [],
    backup_folder: String.raw`C:\Users\me\AppData\Local\config-digger\backups`,
    backup_note: "Each touched file is copied first.",
    ...overrides,
  };
}

function planned(overrides: Partial<ActionPreview> = {}): ActionPreview {
  return { plan_id: "plan-1", preview: preview(), warnings: [], blocks: [], ...overrides };
}

const gitWarning: Warning = { class: "git-versioned", sentence: "The edit shows in the git diff." };
const runningWarning: Warning = { class: "running-agent", sentence: "Claude Code is running." };

describe("the Apply button", () => {
  test("is named by the verb, in the danger style for Remove", () => {
    expect(applyButton(planned(), new Set())).toMatchObject({ label: "Disable", danger: false });
    expect(applyButton(planned({ preview: preview({ action: "enable" }) }), new Set())).toMatchObject({
      label: "Enable",
      danger: false,
    });
    expect(applyButton(planned({ preview: preview({ action: "remove" }) }), new Set())).toMatchObject({
      label: "Remove",
      danger: true,
    });
    expect(applyButton(planned({ preview: preview({ action: "undo" }) }), new Set())).toMatchObject({
      label: "Undo",
      danger: false,
    });
  });

  test("deletes a backup in the danger style, since it is gone for good", () => {
    expect(applyButton(planned({ preview: preview({ action: "delete_backup" }) }), new Set())).toMatchObject({
      label: "Delete backup",
      danger: true,
    });
  });

  test("is enabled with nothing to acknowledge", () => {
    expect(applyButton(planned(), new Set())).toMatchObject({ enabled: true, why: null });
  });

  test("says how many warnings are left to tick", () => {
    const plan = planned({ warnings: [gitWarning, runningWarning] });

    expect(applyButton(plan, new Set())).toMatchObject({ enabled: false, why: "Tick 2 warnings to apply" });
    expect(applyButton(plan, new Set(["running-agent"]))).toMatchObject({
      enabled: false,
      why: "Tick 1 warning to apply",
    });
    expect(applyButton(plan, new Set(["running-agent", "git-versioned"]))).toMatchObject({ enabled: true, why: null });
  });

  test("stays disabled while a block stands, whatever is ticked", () => {
    const plan = planned({
      warnings: [runningWarning],
      blocks: [{ code: "running_agent_sqlite", sentence: "VS Code holds state.vscdb open. Close VS Code and retry." }],
    });

    expect(applyButton(plan, new Set(["running-agent"]))).toMatchObject({
      enabled: false,
      why: "Blocked: see above",
    });
  });

  test("names how many leftovers Remove leftovers removes, each once however many Agents read it", () => {
    const leftover = (name: string) => String.raw`C:\Users\me\.claude.json#projects.E:/gone.mcpServers.${name}`;
    const plan = planned({
      preview: preview({
        action: "remove",
        changes: [
          change({ locator: leftover("old"), after: "removed" }),
          change({ locator: leftover("old"), agent: "vscode", after: "removed" }),
          change({ locator: leftover("search"), after: "removed" }),
        ],
      }),
    });

    expect(applyButton(plan, new Set(), { leftovers: true })).toMatchObject({ label: "Remove 2 leftovers", danger: true });
    expect(applyButton(planned({ preview: preview({ action: "remove", changes: [change()] }) }), new Set(), { leftovers: true })).toMatchObject({
      label: "Remove 1 leftover",
    });
  });
});

function file(overrides: Partial<FileChange> = {}): FileChange {
  return {
    path: String.raw`C:\Users\me\.claude.json`,
    git_versioned: false,
    store_class: "plain",
    edits: [{ op: "remove", key_path: ["mcpServers", "notes"] }],
    diff: "--- a/x\n+++ b/x\n@@ -1,3 +1,2 @@\n {\n-  \"notes\": {}\n }\n",
    diff_unavailable: null,
    diff_note: null,
    changed_lines: 1,
    ...overrides,
  };
}

function folder(overrides: Partial<FolderChange> = {}): FolderChange {
  return {
    path: String.raw`C:\Users\me\.claude\skills\pdf`,
    change: "moved_to_backup",
    target: null,
    files: 3,
    size: 1200,
    git_versioned: false,
    sentence: "The skill folder is moved into the backup.",
    ...overrides,
  };
}

describe("the Files section", () => {
  test("opens each location when there are 1 or 2, and collapses them above that", () => {
    expect(locations(preview({ files: [file()] })).map((location) => location.open)).toEqual([true]);
    expect(locations(preview({ files: [file()], folders: [folder()] })).map((location) => location.open)).toEqual([
      true,
      true,
    ]);
    expect(locations(preview({ files: [file(), file()], folders: [folder()] })).map((location) => location.open)).toEqual(
      [false, false, false],
    );
  });

  test("tags each location git-versioned, SQLite or rewritten while running", () => {
    const tags = locations(
      preview({
        files: [
          file({ git_versioned: true }),
          file({ store_class: "sqlite" }),
          file({ store_class: "rewritten-while-running" }),
        ],
        folders: [folder({ git_versioned: true })],
      }),
    ).map((location) => location.tags);

    expect(tags).toEqual([["git-versioned"], ["SQLite"], ["rewritten while running"], ["git-versioned"]]);
  });

  test("lists each backed-up copy a Delete backup deletes", () => {
    const copy = String.raw`C:\Users\me\AppData\Local\config-digger\backups\20261001-143012-a3f\1-.claude.json`;

    expect(locations(preview({ action: "delete_backup", deleted: [copy] }))).toEqual([
      { kind: "copy", path: copy, tags: ["deleted for good"], open: true },
    ]);
  });
});

describe("a diff", () => {
  test("is shown line by line, without its file headers", () => {
    expect(diffView(file())).toEqual({
      kind: "lines",
      lines: [
        { kind: "hunk", text: "@@ -1,3 +1,2 @@" },
        { kind: "context", text: " {" },
        { kind: "removed", text: '-  "notes": {}' },
        { kind: "context", text: " }" },
      ],
    });
  });

  test("says so under a line whose only change is a Secret", () => {
    const diff = [
      "--- a/x",
      "+++ b/x",
      "@@ -1,1 +1,1 @@",
      '-  "TOKEN": "ghp_••••"',
      '+  "TOKEN": "ghp_••••"',
      String.raw`\ secret value changed`,
      "",
    ].join("\n");

    expect(diffView(file({ diff }))).toMatchObject({
      lines: [
        { kind: "hunk" },
        { kind: "removed", text: '-  "TOKEN": "ghp_••••"' },
        { kind: "added", text: '+  "TOKEN": "ghp_••••"' },
        { kind: "secret", text: "secret value changed" },
      ],
    });
  });

  test("that cannot be masked is replaced by the core's notice, never by the content", () => {
    const note = "Can't show a masked diff: the file before the Operation doesn't parse at 3:7; 2 lines differ.";

    expect(diffView(file({ diff: null, diff_unavailable: "parse", diff_note: note, changed_lines: 2 }))).toEqual({
      kind: "notice",
      sentence: note,
    });
  });
});

function outcome(overrides: Partial<Outcome> = {}): Outcome {
  return {
    operation: "20261001-143012-a3f",
    verify: "verified",
    differences: [],
    backup: String.raw`C:\Users\me\AppData\Local\config-digger\backups\20261001-143012-a3f`,
    sentence: "Disabled notes for Claude Code.",
    hints: [],
    ...overrides,
  };
}

function error(overrides: Partial<ApiError> = {}): ApiError {
  return { code: "unexpected", message: "", candidates: [], backup: null, diffs: [], failed: null, ...overrides };
}

const steps = (result: Result) => result.steps.map(({ name, status }) => `${name} ${status}`);

describe("the result", () => {
  test("of a verified Operation completes Back up, Write and Verify, and offers Undo", () => {
    const result = appliedResult(
      outcome({ hints: ["Restart Claude Code so that it reads what the Operation wrote."] }),
      "disable",
    );

    expect(steps(result)).toEqual(["Back up done", "Write done", "Verify done"]);
    expect(result).toMatchObject({
      kind: "applied",
      sentence: "Disabled notes for Claude Code.",
      hints: ["Restart Claude Code so that it reads what the Operation wrote."],
      undo: "20261001-143012-a3f",
      differences: [],
    });
  });

  test("of a Delete backup offers no Undo, since its Operation can no longer be undone", () => {
    const result = appliedResult(outcome({ sentence: "Deleted the backup of 20261001-143012-a3f." }), "delete_backup");

    expect(result).toMatchObject({ kind: "applied", undo: null });
  });

  test("verified with differences marks Verify amber and lists Predicted against Actual", () => {
    const difference = {
      agent: "vscode",
      home: "default",
      locator: String.raw`E:\app\.mcp.json#mcpServers.notes`,
      predicted: "effective" as const,
      actual: "shadowed" as const,
    };

    const result = appliedResult(outcome({ verify: "verified_with_differences", differences: [difference] }), "disable");

    expect(steps(result)).toEqual(["Back up done", "Write done", "Verify warn"]);
    expect(result).toMatchObject({ kind: "applied", differences: [difference] });
  });

  test("of a failed Verify shows the Rollback, the cause, and what was restored or left untouched", () => {
    const cause = "Claude Code rewrote ~/.claude.json after the write; close it and retry";
    const result = failedResult(
      error({
        code: "overwritten_by_agent",
        message: cause,
        failed: {
          operation: "20261001-143012-a3f",
          failure: null,
          rollback: [
            { path: String.raw`C:\Users\me\.claude.json`, outcome: "restored", reason: null },
            { path: String.raw`E:\app\.mcp.json`, outcome: "not_written", reason: null },
          ],
        },
      }),
    );

    expect(steps(result)).toEqual(["Back up done", "Write done", "Verify failed", "Rollback done"]);
    expect(result).toMatchObject({
      kind: "failed",
      cause,
      restored: [String.raw`C:\Users\me\.claude.json`],
      untouched: [String.raw`E:\app\.mcp.json`],
    });
  });

  test("of a failed write rolls back without verifying", () => {
    expect(steps(failedResult(error({ code: "write_failed", message: "cannot write" })))).toEqual([
      "Back up done",
      "Write failed",
      "Rollback done",
    ]);
  });

  test("of a backup that does not hold the bytes read writes nothing", () => {
    expect(steps(failedResult(error({ code: "backup_failed", message: "nothing was written" })))).toEqual([
      "Back up failed",
    ]);
  });

  test("of an incomplete Rollback lists every location, the backup folder and the masked diffs", () => {
    const diff = { path: String.raw`C:\Users\me\.claude.json`, diff: "@@ -1 +1 @@\n-a\n+b\n", diff_unavailable: null, diff_note: null, changed_lines: 1 };
    const rollback = [
      { path: String.raw`C:\Users\me\.claude.json`, outcome: "not_restored" as const, reason: "It changed again." },
      { path: String.raw`E:\app\.mcp.json`, outcome: "restored" as const, reason: null },
    ];

    const result = failedResult(
      error({
        code: "rollback_incomplete",
        message: "the Rollback could not restore every file",
        backup: String.raw`C:\backups\20261001-143012-a3f`,
        diffs: [diff],
        failed: { operation: "20261001-143012-a3f", failure: "verify_failed", rollback },
      }),
    );

    expect(steps(result)).toEqual(["Back up done", "Write done", "Verify failed", "Rollback failed"]);
    expect(result).toMatchObject({
      kind: "rollback_incomplete",
      backup: String.raw`C:\backups\20261001-143012-a3f`,
      rollback,
      diffs: [diff],
    });
  });

  test("of an Operation that never started has no chain", () => {
    const result = failedResult(error({ code: "locked", message: "another config-digger write is in progress" }));

    expect(result).toMatchObject({ kind: "not_applied", steps: [], cause: "another config-digger write is in progress" });
  });
});

describe("checking again", () => {
  test("keeps a tick only for a warning that says the same as before", () => {
    const vsCode: Warning = { class: "running-agent", sentence: "Claude Code and VS Code are running." };

    expect([...keptAcknowledgements([gitWarning, runningWarning], new Set(["git-versioned", "running-agent"]), [gitWarning, vsCode])]).toEqual([
      "git-versioned",
    ]);
    expect([...keptAcknowledgements([runningWarning], new Set(["running-agent"]), [])]).toEqual([]);
  });
});

function change(overrides: Partial<Change> = {}): Change {
  return {
    agent: "claude-code",
    agent_name: "Claude Code",
    home: "default",
    item: "notes",
    path: String.raw`C:\Users\me\.claude.json`,
    locator: String.raw`C:\Users\me\.claude.json#mcpServers.notes`,
    project: null,
    before: "effective",
    after: "disabled",
    ...overrides,
  };
}

describe("the subtitle", () => {
  test("names the Agents, the Item and the context", () => {
    expect(subtitle(preview({ changes: [change()] }))).toBe("Claude Code · notes · No Project");
    expect(
      subtitle(
        preview({
          project: String.raw`E:\app`,
          changes: [change({ project: String.raw`E:\app` }), change({ agent: "vscode", agent_name: "VS Code", project: String.raw`E:\app` })],
        }),
      ),
    ).toBe(String.raw`Claude Code, VS Code · notes · E:\app`);
  });

  test("of an Undo names the Operation it undoes", () => {
    expect(subtitle(preview({ action: "undo", undo_of: "20261001-143012-a3f", changes: [change()] }))).toBe(
      "Undoes Operation 20261001-143012-a3f",
    );
  });
});

describe("the What changes block", () => {
  const claudeJson = String.raw`C:\Users\me\.claude.json`;
  const gone = "E:/cd-throwaway/project-gone";
  const other = "E:/cd-throwaway/project-other";
  const leftover = (project: string, name: string) => `${claudeJson}#projects.${project}.mcpServers.${name}`;
  // Three leftovers' changes: one read by two Agents in one Project, one in another Project.
  const changes = [
    change({ item: "cd-throwaway-leftover", project: gone, locator: leftover(gone, "cd-throwaway-leftover"), after: "removed" }),
    change({ item: "cd-throwaway-other", project: other, locator: leftover(other, "cd-throwaway-other"), after: "removed" }),
    change({ agent: "vscode", agent_name: "VS Code", item: "cd-throwaway-leftover", project: gone, locator: leftover(gone, "cd-throwaway-leftover"), after: "removed" }),
  ];

  test("groups the changes under their Project, then under their file, in the order each first comes", () => {
    const groups = changeGroups(changes);

    expect(groups.map((group) => group.project)).toEqual([gone, other]);
    expect(groups[0].files.map((file) => [file.name, file.path])).toEqual([[".claude.json", claudeJson]]);
    expect(groups[0].files[0].changes.map((line) => line.change.agent_name)).toEqual(["Claude Code", "VS Code"]);
    expect(groups[1].files[0].changes.map((line) => line.change.item)).toEqual(["cd-throwaway-other"]);
  });

  test("shortens a long locator to its key under its file's name, in pieces that each end at a path or key separator", () => {
    const [line] = changeGroups(changes)[0].files[0].changes;

    expect(line.keyPieces).toEqual(["projects.", "E:/", "cd-throwaway/", "project-gone.", "mcpServers.", "cd-throwaway-leftover"]);
    expect(line.change.locator).toBe(String.raw`C:\Users\me\.claude.json#projects.E:/cd-throwaway/project-gone.mcpServers.cd-throwaway-leftover`);
  });

  test("gives a skill folder, whose locator is its path, no key, its folder's name standing for it", () => {
    const folder = String.raw`C:\Users\me\.claude\skills\pdf`;
    const [group] = changeGroups([change({ item: "pdf", path: folder, locator: folder })]);

    expect(group.files[0].name).toBe("pdf");
    expect(group.files[0].changes[0].keyPieces).toEqual([]);
  });
});
