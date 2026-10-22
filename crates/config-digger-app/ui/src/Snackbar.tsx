import type { Applied } from "./ActionDialog";
import { LEAVE_MS } from "./Notice";
import { Tooltip } from "./Tooltip";
import { useLeaving } from "./useLeaving";
import { useHold, type Hold } from "./useTransient";

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
export function Snackbar({ applied, ...card }: Props) {
  const { shown, leaving, key } = useLeaving(applied, LEAVE_MS);
  return shown && <Card key={key} applied={shown} leaving={leaving} {...card} />;
}

/** The snackbar of one Operation, holding it while the pointer is over it. */
function Card({ applied, leaving, onUndo, onDismiss, onHold }: Omit<Props, "applied"> & { applied: Applied; leaving: boolean }) {
  const hold = useHold(onHold);
  return (
    <div className={`snackbar${leaving ? " leaving" : ""}`} role="status" {...hold}>
      {applied.sentence}
      <button type="button" className="snackbar-action" onClick={() => onUndo(applied.operation)}>
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
