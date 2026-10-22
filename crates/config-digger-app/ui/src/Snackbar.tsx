import type { Applied } from "./ActionDialog";
import { LEAVE_MS, type Hold } from "./Notice";
import { Tooltip } from "./Tooltip";
import { useLeaving } from "./useLeaving";

/** How long the Undo snackbar stays after an Operation. */
export const SNACKBAR_MS = 10_000;

type Props = {
  /** The Operation last applied, or null once the snackbar goes. */
  applied: Applied | null;
  onUndo: (operation: string) => void;
  onDismiss: () => void;
  onHold: Hold;
};

/** The Undo snackbar an Operation leaves, which leaves with an animation. */
export function Snackbar({ applied, onUndo, onDismiss, onHold }: Props) {
  const { shown, leaving } = useLeaving(applied, LEAVE_MS);
  if (shown === null) return null;
  return (
    <div
      className={`snackbar${leaving ? " leaving" : ""}`}
      role="status"
      onPointerEnter={() => onHold(true)}
      onPointerLeave={() => onHold(false)}
    >
      {shown.sentence}
      <button type="button" className="snackbar-action" onClick={() => onUndo(shown.operation)}>
        Undo
      </button>
      <Tooltip text="Dismiss">
        <button type="button" className="icon-button" aria-label="Dismiss" onClick={onDismiss}>
          ✕
        </button>
      </Tooltip>
    </div>
  );
}
