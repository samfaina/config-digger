import type { State } from "./bindings";
import { STATE_LABELS } from "./states";

/** A State as a pill in its colour, from the `state-<state>` CSS class. */
export function StatePill({ state, small = false }: { state: State; small?: boolean }) {
  return (
    <span className={`state-pill state-${state}${small ? " small" : ""}`}>
      {STATE_LABELS[state]}
      <span className="dot" />
    </span>
  );
}
