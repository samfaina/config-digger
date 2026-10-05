// What the action dialog shows, worked out from the plan the core previewed
// and from what executing it did. Nothing here calls the core or renders.

import type {
  Action,
  ActionPreview,
  Error as ApiError,
  Difference,
  ErrorCode,
  FileChange,
  FileDiff,
  FolderChange,
  Outcome,
  Preview,
  RollbackLocation,
  StoreClass,
  Warning,
  WarningClass,
} from "./bindings";

const ACTION_LABELS: Record<Action, string> = {
  disable: "Disable",
  enable: "Enable",
  remove: "Remove",
  undo: "Undo",
  delete_backup: "Delete backup",
};

export type ApplyButton = {
  /** Named by the verb. */
  label: string;
  /** Remove deletes the Entry for every Agent that reads it. */
  danger: boolean;
  enabled: boolean;
  /** Why it is disabled. */
  why: string | null;
};

/** The Apply button, given the warnings ticked so far. */
export function applyButton(planned: ActionPreview, acknowledged: ReadonlySet<WarningClass>): ApplyButton {
  const action = planned.preview.action;
  const left = planned.warnings.filter((warning) => !acknowledged.has(warning.class)).length;
  const why =
    planned.blocks.length > 0
      ? "Blocked: see above"
      : left > 0
        ? `Tick ${left} ${left === 1 ? "warning" : "warnings"} to apply`
        : null;
  return { label: ACTION_LABELS[action], danger: action === "remove", enabled: why === null, why };
}

/** One location the Operation touches, as the Files section lists it. */
export type Location =
  | { kind: "file"; path: string; tags: string[]; open: boolean; file: FileChange }
  | { kind: "folder"; path: string; tags: string[]; open: boolean; folder: FolderChange };

/** Locations are open when there are 1 or 2, and collapsed above that. */
const OPEN_UP_TO = 2;

const STORE_TAGS: Record<StoreClass, string | null> = {
  sqlite: "SQLite",
  "rewritten-while-running": "rewritten while running",
  plain: null,
};

/** Each file, then each skill folder or link, the Operation touches, with its tags. */
export function locations(preview: Preview): Location[] {
  const open = preview.files.length + preview.folders.length <= OPEN_UP_TO;
  const gitTag = (versioned: boolean) => (versioned ? ["git-versioned"] : []);
  return [
    ...preview.files.map((file): Location => {
      const store = STORE_TAGS[file.store_class];
      const tags = [...gitTag(file.git_versioned), ...(store ? [store] : [])];
      return { kind: "file", path: file.path, tags, open, file };
    }),
    ...preview.folders.map(
      (folder): Location => ({ kind: "folder", path: folder.path, tags: gitTag(folder.git_versioned), open, folder }),
    ),
  ];
}

/** One line of a masked diff. `secret` is the note under a line whose only change is a Secret. */
export type DiffLine = { kind: "hunk" | "context" | "added" | "removed" | "secret"; text: string };

/** A file's masked diff line by line, or the core's notice of why it cannot be shown. */
export type DiffView = { kind: "lines"; lines: DiffLine[] } | { kind: "notice"; sentence: string };

/** The note the core puts under a line whose only change is a Secret. */
const SECRET_CHANGED = String.raw`\ secret value changed`;

/** A file's masked diff, without its file headers, or the notice that replaces it. */
export function diffView(diff: Pick<FileChange, "diff" | "diff_note">): DiffView {
  if (diff.diff === null) return { kind: "notice", sentence: diff.diff_note ?? "No diff can be shown." };
  const lines: DiffLine[] = [];
  for (const text of diff.diff.split("\n")) {
    if (text === "" || text.startsWith("--- ") || text.startsWith("+++ ")) continue;
    if (text === SECRET_CHANGED) lines.push({ kind: "secret", text: "secret value changed" });
    else if (text.startsWith("@@")) lines.push({ kind: "hunk", text });
    else if (text.startsWith("+")) lines.push({ kind: "added", text });
    else if (text.startsWith("-")) lines.push({ kind: "removed", text });
    else lines.push({ kind: "context", text });
  }
  return { kind: "lines", lines };
}

/** The chain Apply runs, and the Rollback that follows a failure once anything was written. */
export type StepName = "Back up" | "Write" | "Verify" | "Rollback";
/** `warn` is a Verify that found States other than the predicted ones. */
export type StepStatus = "done" | "warn" | "failed";
/** One step of the chain, as far as it got. */
export type Step = { name: StepName; status: StepStatus };

/** What Apply did, as the dialog shows it once it is done. */
export type Result =
  | {
      kind: "applied";
      steps: Step[];
      sentence: string;
      /** Such as a restart hint for a running Agent that reads a written file. */
      hints: string[];
      /** The Operation to undo. */
      undo: string;
      /** Where the States after differ from the preview's prediction. */
      differences: Difference[];
    }
  | {
      kind: "failed";
      steps: Step[];
      cause: string;
      /** The locations the Rollback restored. */
      restored: string[];
      /** The locations it had no need to touch: never written, or changed since by someone else. */
      untouched: string[];
    }
  | {
      kind: "rollback_incomplete";
      steps: Step[];
      cause: string;
      backup: string | null;
      rollback: RollbackLocation[];
      /** A masked diff from each backed-up file to what it holds now. */
      diffs: FileDiff[];
    }
  | { kind: "not_applied"; steps: Step[]; cause: string };

const done = (name: StepName): Step => ({ name, status: "done" });
const failed = (name: StepName): Step => ({ name, status: "failed" });

/** What an applied Operation did, with Verify amber when States differ from the prediction. */
export function appliedResult(outcome: Outcome): Result {
  return {
    kind: "applied",
    steps: [
      done("Back up"),
      done("Write"),
      { name: "Verify", status: outcome.verify === "verified" ? "done" : "warn" },
    ],
    sentence: outcome.sentence,
    hints: outcome.hints,
    undo: outcome.operation,
    differences: outcome.differences,
  };
}

/** The steps up to the one a failure of `code` stopped at, `null` for a failure before any of them. */
function failedSteps(code: ErrorCode | null): Step[] | null {
  switch (code) {
    case "backup_failed":
      return [failed("Back up")];
    case "write_failed":
      return [done("Back up"), failed("Write")];
    case "verify_failed":
    case "overwritten_by_agent":
    case "read_failure_introduced":
      return [done("Back up"), done("Write"), failed("Verify")];
    default:
      return null;
  }
}

/** What a failed Apply did: how far the chain got, and where the Rollback left each location. */
export function failedResult(error: ApiError): Result {
  const cause = error.message;
  const rollback = error.failed?.rollback ?? [];
  if (error.code === "rollback_incomplete") {
    const steps = failedSteps(error.failed?.failure ?? null) ?? [done("Back up"), done("Write")];
    return {
      kind: "rollback_incomplete",
      steps: [...steps, failed("Rollback")],
      cause,
      backup: error.backup,
      rollback,
      diffs: error.diffs,
    };
  }
  const steps = failedSteps(error.code);
  if (steps === null) return { kind: "not_applied", steps: [], cause };
  const wrote = error.code !== "backup_failed";
  return {
    kind: "failed",
    steps: wrote ? [...steps, done("Rollback")] : steps,
    cause,
    restored: rollback.filter((location) => location.outcome === "restored").map((location) => location.path),
    untouched: rollback.filter((location) => location.outcome !== "restored").map((location) => location.path),
  };
}

/**
 * The ticks that survive planning again, as on Check again: one stays only
 * while a warning of its class says the same as before, since a warning
 * that changed asks for a new acknowledgement.
 */
export function keptAcknowledgements(
  before: Warning[],
  acknowledged: ReadonlySet<WarningClass>,
  after: Warning[],
): Set<WarningClass> {
  const same = (warning: Warning) =>
    before.some((earlier) => earlier.class === warning.class && earlier.sentence === warning.sentence);
  return new Set(after.filter((warning) => acknowledged.has(warning.class) && same(warning)).map((warning) => warning.class));
}

/** The Agents, Items and context the Operation changes, or the Operation an Undo undoes. */
export function subtitle(preview: Preview): string {
  if (preview.undo_of !== null) return `Undoes Operation ${preview.undo_of}`;
  const unique = (values: string[]) => [...new Set(values)].join(", ");
  return [
    unique(preview.changes.map((change) => change.agent_name)),
    unique(preview.changes.map((change) => change.item)),
    preview.project ?? "No Project",
  ]
    .filter((part) => part !== "")
    .join(" · ");
}
