import { useEffect, useRef, useState } from "react";
import { dismissOperation, rereadAfter } from "./acting";
import { BackupDiffs, BackupPath, Changes, DialogSection, Diff, ROLLBACK_LABELS } from "./ActionDialog";
import { commands, type Error as ApiError, type OperationDetail, type OperationLocation, type OperationSummary } from "./bindings";
import { call, NOTICE_MS, type Notify } from "./desktop";
import {
  agentNames,
  operationActions,
  operationCount,
  operationRow,
  operationTime,
  selectedOperation,
  undoPress,
  type OperationRow,
  type UndoAction,
  type UndoPress,
  type UndoRefusal,
} from "./history";
import { Icon } from "./icons";
import { Loading, ReadProgress } from "./Loading";
import { Status, type Notice } from "./Notice";
import { readCue } from "./reads";
import { useActionDialog } from "./useActionDialog";
import { useBusy } from "./useBusy";
import { useHistory } from "./useHistory";
import { useTransient } from "./useTransient";
import { Tooltip } from "./Tooltip";

/** The History · Undo screen: every Operation, newest first, and the detail of the one selected. */
export function HistoryScreen() {
  const { history, error, reading, reload } = useHistory();
  const cue = readCue({ shown: history !== null, reading, failed: error !== null });
  const [selected, setSelected] = useState<string | null>(null);
  const [detail, setDetail] = useState<{ detail: OperationDetail } | { error: ApiError } | null>(null);
  // An Undo of the selected Operation that the core refused, or that its detail already held refused, shown in its detail.
  const [refused, setRefused] = useState<{ operation: string; refusal: UndoRefusal } | null>(null);
  const [notice, setNotice, holdNotice] = useTransient<Notice>(NOTICE_MS);
  const { actions, rows: busyRows, busy } = useBusy();
  // Until History is read again, the Operation the dialog acted on stays busy, so that its buttons can't start the action again.
  const dialog = useActionDialog({
    onClosed: (request) => void rereadAfter(request, { text: "Reading History again…", reload, busy, notify: setNotice }),
    // History is the screen already open.
    onOpenHistory: () => {},
    notify: setNotice,
    // A refused Undo shows in the detail of its Operation, even one started from the snackbar.
    onRefused: (operation, refusal) => {
      setSelected(operation);
      setRefused({ operation, refusal });
    },
  });

  const summary = history ? selectedOperation(history, selected) : null;
  const id = summary?.id ?? null;

  // The detail is read again whenever History is, since an Undo or a Dismiss changes it.
  useEffect(() => {
    if (id === null) {
      setDetail(null);
      return;
    }
    let current = true;
    void call(() => commands.historyDetail(id)).then((result) => {
      if (current) setDetail(result.status === "ok" ? { detail: result.data } : { error: result.error });
    });
    return () => {
      current = false;
    };
  }, [id, history]);

  const select = (operation: string) => {
    setSelected(operation);
    setRefused(null);
  };

  const now = new Date();
  const shown = detail && "detail" in detail && detail.detail.summary.id === id ? detail.detail : null;
  return (
    <div className="history">
      <header className="panel banner">
        <div>
          <div className="caption">History · Undo</div>
          <div className="banner-title">{operationCount(history?.length ?? 0)}</div>
        </div>
        <p className="history-about dim">
          Any Operation can be undone on its own, across restarts. Undo is refused when something it touched changed
          since.
        </p>
        {cue === "rereading" && <ReadProgress text="Reading History again" />}
      </header>

      <div className="panes">
        <section className="panel history-list">
          <h2 className="panel-title">Operations</h2>
          {error && (
            <div className="alert" role="alert">
              <Icon name="flag" small />
              History couldn't be read: {error.message}
            </div>
          )}
          <div className="scroll">
            {cue === "loading" ? (
              <Loading text="Reading History…" rows={6} />
            ) : history === null ? null : history.length === 0 ? (
              <p className="empty">No Operations yet. Disable, Enable or Remove an Entry and it is listed here.</p>
            ) : (
              history.map((operation) => (
                <OperationRowButton
                  key={operation.id}
                  row={operationRow(operation)}
                  time={operationTime(operation.created_at, now)}
                  selected={operation.id === id}
                  onSelect={() => select(operation.id)}
                />
              ))
            )}
          </div>
        </section>

        <section className="panel detail history-detail">
          {summary === null ? (
            <>
              <h2 className="panel-title">Details</h2>
              <p className="empty">Select an Operation to see what it changed.</p>
            </>
          ) : (
            <OperationPane
              summary={summary}
              detail={shown}
              detailError={detail && "error" in detail ? detail.error : null}
              time={operationTime(summary.created_at, now)}
              refusal={refused?.operation === summary.id ? refused.refusal : null}
              busy={busyRows.includes(summary.id)}
              onUndo={(press) => {
                // A refusal the detail already holds shows at once, without a preview.
                if (press.kind === "refused") {
                  setRefused({ operation: summary.id, refusal: press.refusal });
                  return;
                }
                setRefused(null);
                dialog.open({ kind: "undo", operation: summary.id });
              }}
              onDeleteBackup={() => dialog.open({ kind: "delete_backup", operation: summary.id })}
              onDismiss={() =>
                void dismissOperation(summary.id, {
                  dismiss: (operation) => call(() => commands.dismissOperation(operation)),
                  reload,
                  busy,
                  notify: setNotice,
                })
              }
              notify={setNotice}
            />
          )}
        </section>
      </div>

      <Status notice={notice} actions={actions} onHold={holdNotice} />

      {dialog.view}
    </div>
  );
}

type RowProps = { row: OperationRow; time: string; selected: boolean; onSelect: () => void };

/** One Operation in the list: time, title, Agents, locations, number and status. */
function OperationRowButton({ row, time, selected, onSelect }: RowProps) {
  return (
    <button type="button" className={`operation-row${selected ? " selected" : ""}`} aria-current={selected || undefined} onClick={onSelect}>
      <span className="operation-time">{time}</span>
      <span className="operation-text">
        <span className="operation-title">{row.title}</span>
        <span className="operation-meta">{row.meta}</span>
      </span>
      <StatusChip status={row.status} />
    </button>
  );
}

/** The status the core renders, with a badge for one that needs attention. */
function StatusChip({ status }: { status: OperationRow["status"] }) {
  return (
    <span className={`status-chip ${status.tone}`}>
      {status.tone === "attention" && <Icon name="flag" small />}
      {status.label}
    </span>
  );
}

type PaneProps = {
  summary: OperationSummary;
  /** `null` while it is read. */
  detail: OperationDetail | null;
  detailError: ApiError | null;
  time: string;
  refusal: UndoRefusal | null;
  /** An action on it runs, until the screen shows what it did, so its actions are disabled. */
  busy: boolean;
  onUndo: (press: UndoPress) => void;
  onDeleteBackup: () => void;
  onDismiss: () => void;
  notify: Notify;
};

/** What one Operation changed: its States, files with masked diffs and backup, then what can be done to it. */
export function OperationPane({ summary, detail, detailError, time, refusal, busy, onUndo, onDeleteBackup, onDismiss, notify }: PaneProps) {
  const row = operationRow(summary);
  const actions = operationActions(summary);
  const press = undoPress(summary, detailError ? "unread" : detail);
  return (
    <>
      <h2 className="panel-title">
        {row.title}
        <StatusChip status={row.status} />
      </h2>
      <p className="caption">
        Operation {summary.id} · {time} · {agentNames(summary)}
      </p>

      <div className="scroll history-body">
        {refusal && <RefusalBlock refusal={refusal} notify={notify} />}

        {summary.needs_attention && (
          <div className="dialog-callout danger strong" role="alert">
            <Icon name="flag" small />
            <div>
              <div className="callout-title">This Operation needs manual recovery</div>
              Its Rollback could not restore every file. Recover them from the backup by hand, then dismiss it.
            </div>
          </div>
        )}
        {summary.cause && (
          <div className="dialog-callout danger" role="status">
            <Icon name="flag" small />
            <div>{summary.cause}</div>
          </div>
        )}
        {actions.undo.offered && actions.undo.refusal && !refusal && (
          <div className="alert" role="status">
            <Icon name="info" small />
            {actions.undo.refusal}
          </div>
        )}

        {detailError && (
          <div className="alert" role="alert">
            <Icon name="flag" small />
            Its detail couldn't be read: {detailError.message}
          </div>
        )}
        {detail === null && !detailError && <p className="empty">Reading what it changed…</p>}
        {detail && <DetailSections detail={detail} notify={notify} />}
      </div>

      <footer className="dialog-foot">
        {actions.dismiss && (
          <button type="button" className="cta secondary" disabled={busy} onClick={onDismiss}>
            Dismiss
          </button>
        )}
        {actions.deleteBackup && (
          <button type="button" className="cta secondary" disabled={busy} onClick={onDeleteBackup}>
            <Icon name="trash" small />
            Delete backup
          </button>
        )}
        <span className="grow" />
        <UndoButton undo={actions.undo} disabled={busy || press.kind === "reading"} onUndo={() => onUndo(press)} />
      </footer>
    </>
  );
}

/** Undo, or in its place why the Operation cannot be undone. It is `disabled` while it waits for the detail, or while an action on the Operation runs. */
function UndoButton({ undo, disabled, onUndo }: { undo: UndoAction; disabled: boolean; onUndo: () => void }) {
  if (!undo.offered) {
    return (
      <Tooltip text={undo.why}>
        <span className="not-undoable" tabIndex={0} aria-label={`Can't undo: ${undo.why}`}>
          <Icon name="info" small />
          Can't undo
        </span>
      </Tooltip>
    );
  }
  return (
    <button type="button" className="cta" disabled={disabled} onClick={onUndo}>
      <Icon name="history" small />
      Undo
    </button>
  );
}

/** Why the Undo was refused, the backup → now diffs, and where to recover from by hand. No "restore anyway". */
function RefusalBlock({ refusal, notify }: { refusal: UndoRefusal; notify: Notify }) {
  const block = useRef<HTMLDivElement>(null);
  // Each refusal is brought into view, such as one the dialog closed on while the detail was scrolled down.
  useEffect(() => block.current?.scrollIntoView({ behavior: "smooth", block: "nearest" }), [refusal]);
  return (
    <>
      <div ref={block} className="dialog-callout danger" role="alert">
        <Icon name="flag" small />
        <div>
          <div className="callout-title">Can't undo</div>
          {refusal.reason}
        </div>
      </div>
      <BackupDiffs diffs={refusal.diffs} />
      <BackupPath backup={refusal.backup} notify={notify} />
    </>
  );
}

/** The States it changed, its files with masked diffs, its Rollback and its backup. */
function DetailSections({ detail, notify }: { detail: OperationDetail; notify: Notify }) {
  return (
    <>
      <DialogSection title="States changed">
        <Changes changes={detail.changes} notify={notify} />
      </DialogSection>

      {detail.rollback.length > 0 && (
        <DialogSection title="Rollback">
          {detail.rollback.map((location) => (
            <div key={location.path} className={`rollback-location ${location.outcome}`}>
              <span className="mono">{location.path}</span>
              <span className="rollback-outcome">{ROLLBACK_LABELS[location.outcome]}</span>
              {location.reason && <div className="dim">{location.reason}</div>}
            </div>
          ))}
        </DialogSection>
      )}

      <DialogSection title={`Files · ${detail.locations.length}`}>
        {detail.locations.map((location, index) => (
          <LocationDiff key={`${location.path}|${index}`} location={location} />
        ))}
      </DialogSection>

      <DialogSection title="Backup">
        {detail.summary.backup_deleted ? (
          <p className="file-note">
            Its backup was deleted; only its record is kept in <span className="mono">{detail.backup}</span>.
          </p>
        ) : (
          <BackupPath backup={detail.backup} notify={notify} />
        )}
      </DialogSection>
    </>
  );
}

/** One location the Operation touched, with the masked diff of what it changed there. */
function LocationDiff({ location }: { location: OperationLocation }) {
  return (
    <div className="file-block">
      <div className="file-head static">
        <Icon name={location.diff ? "doc" : "folder"} small />
        <span className="mono file-path">{location.path}</span>
      </div>
      {location.diff ? <Diff diff={location.diff} /> : <p className="file-note">A skill folder or link: it has no diff.</p>}
    </div>
  );
}
