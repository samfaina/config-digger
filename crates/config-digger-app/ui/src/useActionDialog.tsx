import { useState, type ReactNode } from "react";
import { ActionDialog, type Applied } from "./ActionDialog";
import type { ActionRequest } from "./bindings";
import type { Notify } from "./desktop";
import type { UndoRefusal } from "./history";
import { useTransient } from "./useTransient";

/** How long the Undo snackbar stays after an Operation. */
const SNACKBAR_MS = 10_000;

type Options = {
  /** Runs whenever the dialog closes, whatever it did, so that the screen reads again. */
  onClosed: () => void;
  onOpenHistory: () => void;
  notify: Notify;
  /** Shows a refused Undo on the screen instead of in the dialog, which then closes. */
  onRefused?: (operation: string, refusal: UndoRefusal) => void;
};

/** The action dialog a screen opens, and the Undo snackbar it leaves after an Operation. */
export function useActionDialog({ onClosed, onOpenHistory, notify, onRefused }: Options): {
  open: (request: ActionRequest) => void;
  view: ReactNode;
} {
  // The action open in the dialog, numbered so that each one opens afresh.
  const [dialog, setDialog] = useState<{ request: ActionRequest; key: number } | null>(null);
  const [snackbar, setSnackbar] = useTransient<Applied>(SNACKBAR_MS);
  const open = (request: ActionRequest) => {
    setSnackbar(null);
    setDialog((current) => ({ request, key: (current?.key ?? 0) + 1 }));
  };
  const close = () => {
    setDialog(null);
    onClosed();
  };

  const view = (
    <>
      {snackbar && !dialog && (
        <div className="snackbar" role="status">
          {snackbar.sentence}
          <button type="button" className="snackbar-action" onClick={() => open({ kind: "undo", operation: snackbar.operation })}>
            Undo
          </button>
          <button type="button" className="icon-button" aria-label="Dismiss" title="Dismiss" onClick={() => setSnackbar(null)}>
            ✕
          </button>
        </div>
      )}

      {dialog && (
        <ActionDialog
          key={dialog.key}
          request={dialog.request}
          onClose={(applied) => {
            close();
            setSnackbar(applied);
          }}
          onUndo={(operation) => {
            onClosed();
            open({ kind: "undo", operation });
          }}
          onOpenHistory={() => {
            close();
            onOpenHistory();
          }}
          onRefused={
            onRefused &&
            ((operation, refusal) => {
              close();
              onRefused(operation, refusal);
            })
          }
          notify={notify}
        />
      )}
    </>
  );
  return { open, view };
}
