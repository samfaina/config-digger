import type { State } from "./bindings";

/** How each State reads on screen. Its colour comes from the `state-<state>` CSS class. */
export const STATE_LABELS: Record<State, string> = {
  effective: "Effective",
  shadowed: "Shadowed",
  disabled: "Disabled",
  invalid: "Invalid",
  unread: "Unread",
};
