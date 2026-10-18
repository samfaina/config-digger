import type { Box } from "./dropdown";

/** The pointer rests on a control this long before its tooltip shows. */
export const HOVER_DELAY_MS = 500;

/**
 * Whether the pointer is on the control and the keyboard's focus in it,
 * whether the tooltip shows, and whether it waits for the hover delay to end.
 */
export type TooltipState = { hovered: boolean; focused: boolean; shown: boolean; waiting: boolean };

export const HIDDEN: TooltipState = { hovered: false, focused: false, shown: false, waiting: false };

/**
 * What happens to the control: the pointer comes in or leaves, the hover
 * delay ends, the control gets the focus from the keyboard or a click, or
 * loses it, the control is clicked or gets another key, the window scrolls
 * or resizes, which moves the control, or Escape is pressed.
 */
export type TooltipEvent = "enter" | "leave" | "delay" | "keyboard focus" | "pointer focus" | "blur" | "press" | "move" | "escape";

/**
 * The next state, and whether the event was the tooltip's, so it goes no
 * further: an Escape that hides a tooltip does not also close a dialog.
 */
export type TooltipReaction = { state: TooltipState; handled: boolean };

export function onTooltip(state: TooltipState, event: TooltipEvent): TooltipReaction {
  if (event === "escape") return { state: { ...state, shown: false, waiting: false }, handled: state.shown };
  return { state: next(state, event), handled: false };
}

function next(state: TooltipState, event: Exclude<TooltipEvent, "escape">): TooltipState {
  switch (event) {
    case "enter":
      return { ...state, hovered: true, waiting: !state.shown };
    case "delay":
      return state.waiting ? { ...state, shown: true, waiting: false } : state;
    case "leave":
      return { ...state, hovered: false, waiting: false, shown: state.shown && state.focused };
    case "keyboard focus":
      return { ...state, focused: true, shown: true, waiting: false };
    case "pointer focus":
      return state;
    case "blur":
      return { ...state, focused: false, shown: state.shown && state.hovered };
    case "press":
    case "move":
      return { ...state, shown: false, waiting: false };
  }
}

/** The tooltip keeps this far from the window's edges, and this far from its control. */
export const TOOLTIP_EDGE = 8;
const TOOLTIP_GAP = 6;

type Size = { width: number; height: number };

/**
 * Places the tooltip of `control`, `size` big, inside a window `view` big:
 * below the control and centred on it, above it when it does not fit below,
 * and pulled in from the window's edges.
 */
export function placeTooltip(control: Box, size: Size, view: Size): { top: number; left: number } {
  const within = (at: number, length: number, room: number) => Math.max(TOOLTIP_EDGE, Math.min(at, room - TOOLTIP_EDGE - length));
  const left = within(control.left + control.width / 2 - size.width / 2, size.width, view.width);
  const below = control.top + control.height + TOOLTIP_GAP;
  const above = control.top - TOOLTIP_GAP - size.height;
  if (below + size.height <= view.height - TOOLTIP_EDGE) return { top: below, left };
  if (above >= TOOLTIP_EDGE) return { top: above, left };
  return { top: within(below, size.height, view.height), left };
}

/**
 * Whether the tooltip's text describes its control, through
 * `aria-describedby`: not when the control's accessible name `label` already
 * says it, so a screen reader reads it once.
 */
export function describes(text: string, label: string | undefined): boolean {
  return !label?.includes(text);
}
