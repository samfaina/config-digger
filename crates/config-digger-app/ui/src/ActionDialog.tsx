import { useCallback, useEffect, useRef, useState } from "react";
import {
  commands,
  type ActionPreview,
  type ActionRequest,
  type Error as ApiError,
  type Change,
  type FileDiff,
  type StateAfter,
  type WarningClass,
} from "./bindings";
import { call, copy, revealLocation, type Notify } from "./desktop";
import {
  appliedResult,
  applyButton,
  diffView,
  failedResult,
  keptAcknowledgements,
  locations,
  subtitle,
  type Location,
  type Result,
  type Step,
} from "./dialog";
import { undoRefusal, type UndoRefusal } from "./history";
import { Icon } from "./icons";
import { StatePill } from "./StatePill";
import { Tooltip } from "./Tooltip";

/** Where the dialog is: planning, previewing, applying, or done applying. */
type Phase =
  | { kind: "planning" }
  | { kind: "plan_failed"; error: ApiError }
  | {
      kind: "preview";
      planned: ActionPreview;
      acknowledged: ReadonlySet<WarningClass>;
      /** Why this is a fresh preview, when the last one went stale. */
      fresh: string | null;
    }
  | { kind: "applying"; planned: ActionPreview }
  | { kind: "done"; planned: ActionPreview; result: Result };

/** An Operation the dialog applied, which the Undo snackbar offers to undo. */
export type Applied = { operation: string; sentence: string };

type Props = {
  request: ActionRequest;
  /** Closes the dialog, with the Operation applied, if any, for the Undo snackbar. */
  onClose: (applied: Applied | null) => void;
  /** Opens the dialog on the Undo of `operation`. */
  onUndo: (operation: string) => void;
  onOpenHistory: () => void;
  /** Takes over a refused Undo of `operation`, to show it elsewhere; the dialog shows it otherwise. */
  onRefused?: (operation: string, refusal: UndoRefusal) => void;
  notify: Notify;
};

/**
 * One modal dialog per action: its preview with everything that matters for
 * safety on one page, then the write as it happens and what it did.
 */
export function ActionDialog({ request, onClose, onUndo, onOpenHistory, onRefused, notify }: Props) {
  const [phase, setPhase] = useState<Phase>({ kind: "planning" });
  // Read when planning fails, so that a new callback does not plan again.
  const refusedRef = useRef(onRefused);
  refusedRef.current = onRefused;
  // The plan held in Rust, dropped when the dialog goes without applying it.
  const held = useRef<string | null>(null);
  const phaseRef = useRef(phase);
  phaseRef.current = phase;
  const mounted = useRef(false);
  const checking = useRef(false);

  const hold = (planned: ActionPreview | null) => {
    const previous = held.current;
    held.current = planned?.plan_id ?? null;
    if (previous !== null && previous !== held.current) void commands.cancelPlan(previous);
  };

  /**
   * Plans the request. With `replacing`, the id of the plan previewed, the
   * new plan replaces it, keeping the ticks of the warnings that say the
   * same, unless that preview is gone by then.
   */
  const plan = useCallback(
    async (replacing: string | null) => {
      const result = await call(() => commands.planAction(request));
      const current = phaseRef.current;
      const stillPreviewed = current.kind === "preview" && current.planned.plan_id === replacing;
      if (!mounted.current || (replacing !== null && !stillPreviewed)) {
        if (result.status === "ok") void commands.cancelPlan(result.data.plan_id);
        return;
      }
      if (result.status === "error") {
        hold(null);
        const refusal = undoRefusal(result.error);
        if (request.kind === "undo" && refusal && refusedRef.current) {
          refusedRef.current(request.operation, refusal);
          return;
        }
        setPhase({ kind: "plan_failed", error: result.error });
        return;
      }
      hold(result.data);
      const acknowledged =
        current.kind === "preview" && stillPreviewed
          ? keptAcknowledgements(current.planned.warnings, current.acknowledged, result.data.warnings)
          : new Set<WarningClass>();
      setPhase({ kind: "preview", planned: result.data, acknowledged, fresh: null });
    },
    [request],
  );

  // Plans on opening, and drops the plan held on closing.
  useEffect(() => {
    mounted.current = true;
    void plan(null);
    return () => {
      mounted.current = false;
      hold(null);
    };
  }, [plan]);

  /** Plans again, as on Check again and when the window regains focus, so that detection runs again. */
  const checkAgain = useCallback(async () => {
    const current = phaseRef.current;
    if (current.kind !== "preview" || checking.current) return;
    checking.current = true;
    try {
      await plan(current.planned.plan_id);
    } finally {
      checking.current = false;
    }
  }, [plan]);

  useEffect(() => {
    const onFocus = () => void checkAgain();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [checkAgain]);

  const apply = async (planned: ActionPreview, acknowledged: ReadonlySet<WarningClass>) => {
    setPhase({ kind: "applying", planned });
    // Executing drops the plan in Rust, whatever it does.
    held.current = null;
    const result = await call(() => commands.executePlan(planned.plan_id, [...acknowledged]));
    if (!mounted.current) {
      if (result.status === "ok" && result.data.kind === "replanned") void commands.cancelPlan(result.data.preview.plan_id);
      return;
    }
    if (result.status === "error") {
      setPhase({ kind: "done", planned, result: failedResult(result.error) });
    } else if (result.data.kind === "applied") {
      setPhase({ kind: "done", planned, result: appliedResult(result.data.outcome, planned.preview.action) });
    } else {
      // Nothing was written: every tick is cleared for the fresh preview.
      hold(result.data.preview);
      setPhase({ kind: "preview", planned: result.data.preview, acknowledged: new Set(), fresh: result.data.cause });
    }
  };

  const retry = () => {
    setPhase({ kind: "planning" });
    void plan(null);
  };

  const applied: Applied | null =
    phase.kind === "done" && phase.result.kind === "applied" && phase.result.undo !== null
      ? { operation: phase.result.undo, sentence: phase.result.sentence }
      : null;
  const close = () => {
    if (phase.kind !== "applying") onClose(applied);
  };

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const planned = "planned" in phase ? phase.planned : null;
  return (
    <div className="dialog-backdrop" onClick={(event) => event.target === event.currentTarget && close()}>
      <section className="dialog" role="dialog" aria-modal="true" aria-labelledby="dialog-title">
        <header className="dialog-head">
          <h1 id="dialog-title" className="dialog-title">
            {planned ? planned.preview.title : "Preparing the preview…"}
          </h1>
          {planned && <p className="dialog-subtitle">{subtitle(planned.preview)}</p>}
        </header>

        {phase.kind === "planning" && <p className="empty">Reading the files this action touches…</p>}

        {phase.kind === "plan_failed" && (
          <>
            <div className="dialog-callout danger" role="alert">
              <Icon name="flag" small />
              <div>
                <div className="callout-title">This action can't be previewed</div>
                {phase.error.message}
              </div>
            </div>
            <BackupDiffs diffs={phase.error.diffs} />
            <footer className="dialog-foot">
              <span className="grow" />
              <BackupFolderButton backup={phase.error.backup} notify={notify} />
              {phase.error.backup !== null && (
                <CopyPathButton path={phase.error.backup} what="backup folder path" notify={notify} />
              )}
              <button type="button" className="cta secondary" onClick={retry}>
                <Icon name="refresh" small />
                Check again
              </button>
              <button type="button" className="cta" onClick={close}>
                Close
              </button>
            </footer>
          </>
        )}

        {phase.kind === "preview" && (
          <PreviewBody
            planned={phase.planned}
            acknowledged={phase.acknowledged}
            fresh={phase.fresh}
            leftovers={request.kind === "leftovers"}
            onToggle={(warning) => {
              const next = new Set(phase.acknowledged);
              if (!next.delete(warning)) next.add(warning);
              setPhase({ ...phase, acknowledged: next });
            }}
            onCheckAgain={() => void checkAgain()}
            onCancel={close}
            onApply={() => void apply(phase.planned, phase.acknowledged)}
          />
        )}

        {phase.kind === "applying" && (
          <>
            <Steps steps={[]} running />
            <footer className="dialog-foot">
              <span className="dialog-backup">{phase.planned.preview.backup_note}</span>
            </footer>
          </>
        )}

        {phase.kind === "done" && (
          <ResultBody
            result={phase.result}
            onUndo={onUndo}
            onOpenHistory={onOpenHistory}
            onRetry={retry}
            onClose={close}
            notify={notify}
          />
        )}
      </section>
    </div>
  );
}

type PreviewBodyProps = {
  planned: ActionPreview;
  acknowledged: ReadonlySet<WarningClass>;
  fresh: string | null;
  /** It is a Remove leftovers, whose Apply names how many it removes. */
  leftovers: boolean;
  onToggle: (warning: WarningClass) => void;
  onCheckAgain: () => void;
  onCancel: () => void;
  onApply: () => void;
};

/** The preview: What changes, Before you apply, Files, and the footer with Cancel and Apply. */
function PreviewBody({ planned, acknowledged, fresh, leftovers, onToggle, onCheckAgain, onCancel, onApply }: PreviewBodyProps) {
  const { preview, warnings, blocks } = planned;
  const button = applyButton(planned, acknowledged, { leftovers });
  const before = blocks.length + warnings.length + preview.info.length;
  return (
    <>
      {fresh !== null && (
        <div className="dialog-callout fresh" role="status">
          <Icon name="refresh" small />
          <div>
            <div className="callout-title">Nothing was written, this is a fresh preview</div>
            {fresh}. Check it and confirm again.
          </div>
        </div>
      )}

      <DialogSection title="What changes">
        {preview.changes.length === 0 ? (
          <p className="dim">No State changes in any Agent.</p>
        ) : (
          <div className="changes">
            {preview.changes.map((change) => (
              <ChangeRow key={`${change.agent}|${change.home}|${change.locator}|${change.project ?? ""}`} change={change} />
            ))}
          </div>
        )}
        {preview.effective.map((effective) => (
          <p key={`${effective.agent}|${effective.home}|${effective.item}`} className="effective-change">
            {effective.sentence}
          </p>
        ))}
      </DialogSection>

      {before > 0 && (
        <DialogSection title="Before you apply">
          {blocks.map((block, index) => (
            <div key={`block-${index}`} className="check block">
              <Icon name="flag" small />
              <div>
                <span className="check-kind">Blocked</span>
                {block.sentence}
                <div className="check-fix">
                  <button type="button" className="cta secondary" onClick={onCheckAgain}>
                    <Icon name="refresh" small />
                    Check again
                  </button>
                </div>
              </div>
            </div>
          ))}
          {warnings.map((warning) => {
            const ticked = acknowledged.has(warning.class);
            const git = warning.class === "git-versioned";
            return (
              <label key={warning.class} className={`check warning${git ? " git" : ""}`}>
                <input type="checkbox" checked={ticked} onChange={() => onToggle(warning.class)} />
                <div>
                  <span className="check-kind">{git ? "Git-versioned" : "Warning"}</span>
                  {warning.sentence}
                </div>
              </label>
            );
          })}
          {preview.info.map((info, index) => (
            <Tooltip key={`info-${index}`} text={info.path}>
              <div className="check info">
                <Icon name="info" small />
                <div>{info.sentence}</div>
              </div>
            </Tooltip>
          ))}
        </DialogSection>
      )}

      <DialogSection title={`Files · ${preview.files.length + preview.folders.length}`}>
        {locations(preview).map((location, index) => (
          <LocationBlock key={`${location.path}|${index}`} location={location} />
        ))}
      </DialogSection>

      <footer className="dialog-foot">
        <span className="dialog-backup">{preview.backup_note}</span>
        <span className="grow" />
        <button type="button" className="cta secondary" onClick={onCancel}>
          Cancel
        </button>
        {button.why && <span className="apply-why">{button.why}</span>}
        <button type="button" className={`cta${button.danger ? " danger" : ""}`} disabled={!button.enabled} onClick={onApply}>
          {button.label}
        </button>
      </footer>
    </>
  );
}

/** A titled section of the dialog, its title ruled to the edge. */
export function DialogSection({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="dialog-section">
      <h2 className="dialog-section-title">
        {title}
        <span className="rule" />
      </h2>
      {children}
    </section>
  );
}

/** One (Agent, Entry, Project) whose State the Operation changes, before → after. */
export function ChangeRow({ change }: { change: Change }) {
  return (
    <div className="change-row">
      <div className="change-what">
        <div>
          <b>{change.agent_name}</b>
          {change.home !== "default" && <span className="dim"> ({change.home})</span>} · {change.item}
          <span className="dim"> · {change.project ?? "No Project"}</span>
        </div>
        <div className="mono dim">{change.locator}</div>
      </div>
      <div className="change-states">
        <StateAfterPill state={change.before} />
        <span className="dim">→</span>
        <StateAfterPill state={change.after} />
      </div>
    </div>
  );
}

/** A State after an Operation, or that the Entry is removed. */
function StateAfterPill({ state }: { state: StateAfter }) {
  if (state === "removed") return <span className="state-pill small removed">Removed</span>;
  return <StatePill state={state} small />;
}

/** One location the Operation touches, open or collapsed, with its tags and masked diff. */
function LocationBlock({ location }: { location: Location }) {
  const [open, setOpen] = useState(location.open);
  if (location.kind === "copy") {
    return (
      <div className="file-block">
        <div className="file-head static">
          <Icon name="doc" small />
          <span className="mono file-path">{location.path}</span>
          {location.tags.map((tag) => (
            <span key={tag} className="tag">
              {tag}
            </span>
          ))}
        </div>
      </div>
    );
  }
  return (
    <div className="file-block">
      <button type="button" className="file-head" aria-expanded={open} onClick={() => setOpen(!open)}>
        <Icon name={location.kind === "file" ? "doc" : "folder"} small />
        <span className="mono file-path">{location.path}</span>
        {location.tags.map((tag) => (
          <span key={tag} className={`tag${tag === "git-versioned" ? " git" : ""}`}>
            {tag}
          </span>
        ))}
      </button>
      {open &&
        (location.kind === "file" ? (
          <Diff diff={location.file} />
        ) : (
          <p className="file-note">{location.folder.sentence}</p>
        ))}
    </div>
  );
}

/** A masked diff, or the core's notice of why none can be shown. */
export function Diff({ diff }: { diff: Pick<FileDiff, "diff" | "diff_note"> }) {
  const view = diffView(diff);
  if (view.kind === "notice") return <p className="file-note">{view.sentence}</p>;
  return (
    <pre className="diff">
      {view.lines.map((line, index) => (
        <div key={index} className={`diff-${line.kind}`}>
          {line.text}
        </div>
      ))}
    </pre>
  );
}

/** A masked diff from each backed-up file to what it holds now, as for a refused Undo or an incomplete Rollback. */
export function BackupDiffs({ diffs }: { diffs: FileDiff[] }) {
  if (diffs.length === 0) return null;
  return (
    <DialogSection title="Backup → now">
      {diffs.map((diff) => (
        <div key={diff.path} className="file-block">
          <div className="file-head static">
            <Icon name="doc" small />
            <span className="mono file-path">{diff.path}</span>
          </div>
          <Diff diff={diff} />
        </div>
      ))}
    </DialogSection>
  );
}

/** Copies a path, such as a backup folder's, to recover from by hand. */
export function CopyPathButton({ path, what, notify }: { path: string; what: string; notify: Notify }) {
  return (
    <button type="button" className="cta secondary" onClick={() => void copy(path, what, notify)}>
      Copy path
    </button>
  );
}

/** A backup folder's path, with Open backup folder and Copy path to recover from it by hand. */
export function BackupPath({ backup, notify }: { backup: string; notify: Notify }) {
  return (
    <div className="backup-path">
      <span className="mono">{backup}</span>
      <span className="grow" />
      <BackupFolderButton backup={backup} notify={notify} />
      <CopyPathButton path={backup} what="backup folder path" notify={notify} />
    </div>
  );
}

/** Shows the backup folder an error points at in Explorer. */
export function BackupFolderButton({ backup, notify }: { backup: string | null; notify: Notify }) {
  if (backup === null) return null;
  return (
    <button type="button" className="cta secondary" onClick={() => void revealLocation(backup, notify)}>
      <Icon name="folder" small />
      Open backup folder
    </button>
  );
}

const STEP_NAMES = ["Back up", "Write", "Verify"] as const;

/** What each step's dot shows once it ran; a step yet to run shows its number. */
const STEP_GLYPHS: Partial<Record<Step["status"] | "running" | "pending", string>> = {
  done: "✓",
  warn: "!",
  failed: "✗",
};

/** The Back up → Write → Verify chain, then Rollback when it ran. While `running`, the write is under way. */
function Steps({ steps, running = false }: { steps: Step[]; running?: boolean }) {
  const shown: { name: string; status: Step["status"] | "running" | "pending" }[] = running
    ? STEP_NAMES.map((name, index) => ({ name, status: index === 0 ? "running" : "pending" }))
    : steps;
  return (
    <ol className="steps" aria-live="polite">
      {shown.map((step, index) => (
        <li key={step.name} className={`step ${step.status}`}>
          <span className="step-dot">{STEP_GLYPHS[step.status] ?? index + 1}</span>
          {step.status === "failed" ? `${step.name} failed` : step.name}
          {step.status === "warn" && <span className="dim"> · with differences</span>}
        </li>
      ))}
    </ol>
  );
}

type ResultBodyProps = {
  result: Result;
  onUndo: (operation: string) => void;
  onOpenHistory: () => void;
  onRetry: () => void;
  onClose: () => void;
  notify: Notify;
};

/** The chain as far as it got, then what Apply did and what can be done next. */
function ResultBody({ result, onUndo, onOpenHistory, onRetry, onClose, notify }: ResultBodyProps) {
  const undo = result.kind === "applied" ? result.undo : null;
  return (
    <>
      <Steps steps={result.steps} />

      {result.kind === "applied" && (
        <>
          <div className="dialog-callout success" role="status">
            <Icon name="play" small />
            <div>
              <div className="callout-title">{result.sentence}</div>
              {result.hints.map((hint) => (
                <div key={hint} className="dim">
                  {hint}
                </div>
              ))}
              <div className="faint">Operation {result.undo} · backup kept</div>
            </div>
          </div>
          {result.differences.length > 0 && (
            <DialogSection title="Verified with differences">
              <table className="differences">
                <thead>
                  <tr>
                    <th>Entry</th>
                    <th>Predicted</th>
                    <th>Actual</th>
                  </tr>
                </thead>
                <tbody>
                  {result.differences.map((difference) => (
                    <tr key={`${difference.agent}|${difference.home}|${difference.locator}`}>
                      <td>
                        <div>{difference.agent}</div>
                        <div className="mono dim">{difference.locator}</div>
                      </td>
                      <td>
                        <StateAfterPill state={difference.predicted} />
                      </td>
                      <td>
                        <StateAfterPill state={difference.actual} />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </DialogSection>
          )}
          <footer className="dialog-foot">
            <span className="grow" />
            {undo !== null && (
              <button type="button" className="cta secondary" onClick={() => onUndo(undo)}>
                <Icon name="history" small />
                Undo
              </button>
            )}
            <button type="button" className="cta secondary" onClick={onOpenHistory}>
              Open History
            </button>
            <button type="button" className="cta" onClick={onClose}>
              Done
            </button>
          </footer>
        </>
      )}

      {(result.kind === "failed" || result.kind === "not_applied") && (
        <>
          <div className="dialog-callout danger" role="alert">
            <Icon name="flag" small />
            <div>
              <div className="callout-title">
                {result.kind === "failed" ? "Nothing of this Operation was kept" : "Nothing was written"}
              </div>
              {result.cause}
            </div>
          </div>
          {result.kind === "failed" && result.restored.length > 0 && (
            <DialogSection title="Restored">
              {result.restored.map((path) => (
                <div key={path} className="mono">
                  {path}
                </div>
              ))}
            </DialogSection>
          )}
          {result.kind === "failed" && result.untouched.length > 0 && (
            <DialogSection title="Untouched">
              {result.untouched.map((path) => (
                <div key={path} className="mono">
                  {path}
                </div>
              ))}
            </DialogSection>
          )}
          <footer className="dialog-foot">
            <span className="grow" />
            <button type="button" className="cta secondary" onClick={onClose}>
              Close
            </button>
            <button type="button" className="cta" onClick={onRetry}>
              <Icon name="refresh" small />
              Retry
            </button>
          </footer>
        </>
      )}

      {result.kind === "rollback_incomplete" && (
        <>
          <div className="dialog-callout danger strong" role="alert">
            <Icon name="flag" small />
            <div>
              <div className="callout-title">Rollback incomplete: these locations need your attention</div>
              {result.cause}
            </div>
          </div>
          <DialogSection title="Locations">
            {result.rollback.map((location) => (
              <div key={location.path} className={`rollback-location ${location.outcome}`}>
                <span className="mono">{location.path}</span>
                <span className="rollback-outcome">{ROLLBACK_LABELS[location.outcome]}</span>
                {location.reason && <div className="dim">{location.reason}</div>}
              </div>
            ))}
          </DialogSection>
          <BackupDiffs diffs={result.diffs} />
          <footer className="dialog-foot">
            <span className="grow" />
            <BackupFolderButton backup={result.backup} notify={notify} />
            <button type="button" className="cta secondary" onClick={onOpenHistory}>
              Open History
            </button>
            <button type="button" className="cta" onClick={onClose}>
              Close
            </button>
          </footer>
        </>
      )}
    </>
  );
}

/** Where a Rollback left each location, as a label. */
export const ROLLBACK_LABELS = {
  restored: "restored",
  not_written: "never written",
  left_alone: "left alone",
  not_restored: "not restored",
} as const;
