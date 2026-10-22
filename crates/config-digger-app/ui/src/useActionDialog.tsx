import { useState, type ReactNode } from "react";
import { ActionDialog, type Applied } from "./ActionDialog";
import type { ActionRequest } from "./bindings";
import type { Notify } from "./desktop";
import type { UndoRefusal } from "./history";
import { Snackbar, SNACKBAR_MS } from "./Snackbar";
import { useTransient } from "./useTransient";

type Options = {
  /** Runs whenever the dialog on `request` closes, whatever it did, so that the screen reads again. */
  onClosed: (request: ActionRequest) => void;
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
  const [snackbar, setSnackbar, holdSnackbar] = useTransient<Applied>(SNACKBAR_MS);
  const open = (request: ActionRequest) => {
    setSnackbar(null);
    setDialog((current) => ({ request, key: (current?.key ?? 0) + 1 }));
  };
  const close = (request: ActionRequest) => {
    setDialog(null);
    onClosed(request);
  };

  const view = (
    <>
      <Snackbar
        applied={dialog ? null : snackbar}
        onUndo={(operation) => open({ kind: "undo", operation })}
        onDismiss={() => setSnackbar(null)}
        onHold={holdSnackbar}
      />

      {dialog && (
        <ActionDialog
          key={dialog.key}
          request={dialog.request}
          onClose={(applied) => {
            close(dialog.request);
            setSnackbar(applied);
          }}
          onUndo={(operation) => {
            onClosed(dialog.request);
            open({ kind: "undo", operation });
          }}
          onOpenHistory={() => {
            close(dialog.request);
            onOpenHistory();
          }}
          onRefused={
            onRefused &&
            ((operation, refusal) => {
              close(dialog.request);
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
