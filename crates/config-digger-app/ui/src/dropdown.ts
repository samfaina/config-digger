/**
 * One choice of a dropdown. A disabled one, such as a "Choose…" prompt, shows
 * but cannot be picked. Options next to each other with the same `group` show
 * under its name.
 */
export type DropdownOption = { value: string; label: string; disabled?: boolean; group?: string };

/**
 * Whether the panel is open, the option the keyboard or the pointer is on, the
 * letters typed so far with the time of the last one, and the text in the
 * panel's text box, which narrows its options.
 */
export type DropdownState = { open: boolean; active: number; typed: string; typedAt: number; query: string };

export const CLOSED: DropdownState = { open: false, active: -1, typed: "", typedAt: 0, query: "" };

/** A pause this long between letters starts a new search. */
export const TYPING_PAUSE_MS = 500;

export type DropdownKey = { key: string; altKey: boolean };

/**
 * What a key or a click does: the next state, the value chosen (null when the
 * value stays) and whether the event was the dropdown's, so the browser's own
 * handling stops.
 */
export type Reaction = { state: DropdownState; choose: string | null; handled: boolean };

const ignored = (state: DropdownState): Reaction => ({ state, choose: null, handled: false });

/** The index of the option holding `value`, or -1. */
const indexOf = (options: DropdownOption[], value: string) => options.findIndex((option) => option.value === value);

/** The panel opened on the chosen option, or on the first one that can be picked. */
function opened(options: DropdownOption[], value: string): DropdownState {
  const chosen = indexOf(options, value);
  return { ...CLOSED, open: true, active: chosen >= 0 ? chosen : next(options, -1, 1) };
}

/** The panel closed on option `at`, chosen when it can be picked and is not the value already. */
function chooseAt(options: DropdownOption[], value: string, at: number, handled: boolean): Reaction {
  const option = options[at];
  const chosen = option && !option.disabled && option.value !== value ? option.value : null;
  return { state: CLOSED, choose: chosen, handled };
}

/**
 * The next option that can be picked from `from`, going down (1) or up (-1),
 * or `from` itself at the end of the list.
 */
function next(options: DropdownOption[], from: number, by: 1 | -1): number {
  for (let at = from + by; at >= 0 && at < options.length; at += by) {
    if (!options[at].disabled) return at;
  }
  return from;
}

/** Where an arrow, Home or End goes from `from`, or null for any other key. */
function moveFrom(options: DropdownOption[], from: number, key: string): number | null {
  switch (key) {
    case "ArrowDown":
      return next(options, from, 1);
    case "ArrowUp":
      return next(options, from, -1);
    case "Home":
      return next(options, -1, 1);
    case "End":
      return next(options, options.length, -1);
  }
  return null;
}

/**
 * The option a search for `typed` lands on from `from`. The same letter
 * typed again goes on to the next option with it; longer text may stay on
 * `from` when it still matches. Null when nothing matches.
 */
function search(options: DropdownOption[], from: number, typed: string): number | null {
  const text = typed.toLowerCase();
  const repeated = [...text].every((char) => char === text[0]);
  const prefix = repeated ? text[0] : text;
  const first = repeated ? from + 1 : Math.max(from, 0);
  for (let offset = 0; offset < options.length; offset++) {
    const at = (first + offset) % options.length;
    const option = options[at];
    if (!option.disabled && option.label.toLowerCase().startsWith(prefix)) return at;
  }
  return null;
}

/** A key pressed on the field, which keeps the focus while the panel is open, as in a native select. */
export function onKey(state: DropdownState, options: DropdownOption[], value: string, key: DropdownKey, now: number): Reaction {
  const searching = state.typed !== "" && now - state.typedAt < TYPING_PAUSE_MS;
  if (key.key.length === 1 && !key.altKey && (key.key !== " " || searching)) {
    const typed = (searching ? state.typed : "") + key.key;
    const from = state.open ? state.active : indexOf(options, value);
    const found = search(options, from, typed);
    const next = { ...state, typed, typedAt: now };
    if (found === null) return { state: next, choose: null, handled: true };
    if (state.open) return { state: { ...next, active: found }, choose: null, handled: true };
    const chosen = options[found].value;
    return { state: next, choose: chosen !== value ? chosen : null, handled: true };
  }
  const alt = key.altKey && (key.key === "ArrowUp" || key.key === "ArrowDown");
  if (!state.open) {
    if (key.key === "Enter" || key.key === " " || alt) return { state: opened(options, value), choose: null, handled: true };
    // Closed, a move chooses at once, as a native select does on Windows.
    const to = moveFrom(options, indexOf(options, value), key.key);
    return to === null ? ignored(state) : chooseAt(options, value, to, true);
  }
  const choose = (handled: boolean) => chooseAt(options, value, state.active, handled);
  if (alt) return choose(true);
  switch (key.key) {
    case "Enter":
    case " ":
      return choose(true);
    case "Tab":
      return choose(false);
    case "Escape":
      return { state: CLOSED, choose: null, handled: true };
  }
  const to = moveFrom(options, state.active, key.key);
  if (to !== null) return { state: { ...state, active: to }, choose: null, handled: true };
  return ignored(state);
}

/** Whether `option` shows for `query`: it holds the text anywhere, in any case. A disabled one shows only with no text. */
export function matches(option: DropdownOption, query: string): boolean {
  if (query === "") return true;
  return !option.disabled && option.label.toLowerCase().includes(query.toLowerCase());
}

/** `options` with those that `query` hides made disabled, at the same indexes, so that nothing can move to or choose them. */
function narrowed(options: DropdownOption[], query: string): DropdownOption[] {
  return options.map((option) => (matches(option, query) ? option : { ...option, disabled: true }));
}

/**
 * Text typed in the panel's text box narrows the options to those it matches.
 * The active option stays while it matches, or moves to the first one that does.
 */
export function onQuery(state: DropdownState, options: DropdownOption[], query: string): DropdownState {
  const shown = narrowed(options, query);
  const active = shown[state.active] && !shown[state.active].disabled ? state.active : next(shown, -1, 1);
  return { ...state, active, query };
}

/**
 * A key pressed in the panel's text box, which has the focus while the panel is
 * open. The keys work as on the field, among the options the text matches,
 * but letters, and a space after some text, are left to the text box.
 */
export function onFilterKey(state: DropdownState, options: DropdownOption[], value: string, key: DropdownKey, now: number): Reaction {
  const typing = key.key.length === 1 && !key.altKey && (key.key !== " " || state.query !== "");
  if (typing) return ignored(state);
  return onKey(state, narrowed(options, state.query), value, key, now);
}

/** A click on the field opens the panel, or closes it without choosing. */
export function onFieldClick(state: DropdownState, options: DropdownOption[], value: string): DropdownState {
  return state.open ? CLOSED : opened(options, value);
}

/** The pointer over option `at` makes it the active one, unless it is disabled. */
export function onOptionHover(state: DropdownState, options: DropdownOption[], at: number): DropdownState {
  return options[at]?.disabled ? state : { ...state, active: at };
}

/** A click on option `at` chooses it and closes the panel; a click on a disabled one does nothing. */
export function onOptionClick(state: DropdownState, options: DropdownOption[], value: string, at: number): Reaction {
  if (options[at]?.disabled) return { state, choose: null, handled: true };
  return chooseAt(options, value, at, true);
}

/** The panel keeps this far from the window's edges, and this far from its field. */
export const PANEL_EDGE = 8;
const PANEL_GAP = 4;

export type Box = { top: number; left: number; width: number; height: number };

/** Where the panel goes, in window coordinates, with the height past which it scrolls. */
export type Placement = { top: number; left: number; width: number; maxHeight: number };

/**
 * Places the panel of `field`, whose content is `panel` big, inside a window
 * `view` big: below the field, or above it when the content does not fit below
 * and there is more room above; at least as wide as the field, and pulled in
 * from the window's edges.
 */
export function placePanel(field: Box, panel: { width: number; height: number }, view: { width: number; height: number }): Placement {
  const width = Math.min(Math.max(panel.width, field.width), view.width - 2 * PANEL_EDGE);
  const left = Math.max(PANEL_EDGE, Math.min(field.left, view.width - PANEL_EDGE - width));
  const below = view.height - PANEL_EDGE - (field.top + field.height + PANEL_GAP);
  const above = field.top - PANEL_GAP - PANEL_EDGE;
  if (panel.height <= below || below >= above) {
    return { top: field.top + field.height + PANEL_GAP, left, width, maxHeight: Math.max(below, 0) };
  }
  return { top: field.top - PANEL_GAP - Math.min(panel.height, above), left, width, maxHeight: above };
}
