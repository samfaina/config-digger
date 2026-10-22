import { useEffect, useId, useLayoutEffect, useRef, useState, type CSSProperties, type KeyboardEvent } from "react";
import { createPortal } from "react-dom";
import {
  CLOSED,
  matches,
  onFieldClick,
  onFilterKey,
  onKey,
  onOptionClick,
  onOptionHover,
  onQuery,
  placePanel,
  type DropdownOption,
  type DropdownState,
  type Placement,
  type Reaction,
} from "./dropdown";
import { Icon } from "./icons";
import { Tooltip } from "./Tooltip";

type SelectProps = {
  value: string;
  options: DropdownOption[];
  onChange: (value: string) => void;
  /** The accessible name, when no visible label names the field. */
  label?: string;
  /** Shown on the field in a Tooltip. */
  tooltip?: string;
  /** The name of a text box at the top of the panel that narrows its options, for a long list. */
  searchLabel?: string;
  className?: string;
};

/** An option the panel shows, with its index among all the options. */
type Shown = { option: DropdownOption; at: number };

/** Runs of shown options next to each other with the same group. */
function groupRuns(shown: Shown[]): { group?: string; items: Shown[] }[] {
  const all: { group?: string; items: Shown[] }[] = [];
  for (const each of shown) {
    const last = all.at(-1);
    if (last && last.group === each.option.group) last.items.push(each);
    else all.push({ group: each.option.group, items: [each] });
  }
  return all;
}

/** Passes the value `reaction` chooses, if any, to `onChange`, whether a key or a click chose it, and returns the dropdown's next state. */
export function applyReaction(reaction: Reaction, onChange: (value: string) => void): DropdownState {
  if (reaction.choose !== null) onChange(reaction.choose);
  return reaction.state;
}

/**
 * The app's dropdown: a field and a panel of options drawn with the app's
 * look, where a native `<select>` would show the system's list. The field
 * keeps the focus while the panel is open, so Escape leaves it there, unless
 * the panel has a text box: the box takes the focus and gives it back on close.
 */
export function Select({ value, options, onChange, label, tooltip, searchLabel, className }: SelectProps) {
  const id = useId();
  const [state, setState] = useState<DropdownState>(CLOSED);
  // With the height it opened at, kept while the text box narrows the options, so that the panel stays by its field.
  const [placement, setPlacement] = useState<(Placement & { height: number }) | null>(null);
  const field = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const searchBox = useRef<HTMLInputElement>(null);
  const optionId = (at: number) => `${id}-option-${at}`;
  const chosen = options.find((option) => option.value === value);
  const shown = options.map((option, at) => ({ option, at })).filter(({ option }) => matches(option, state.query));
  const activeId = state.open && state.active >= 0 ? optionId(state.active) : undefined;

  const apply = (reaction: Reaction) => {
    // Closed from its text box, the panel gives the focus back to the field, as Escape on the field keeps it there.
    if (!reaction.state.open && panel.current?.contains(document.activeElement)) field.current?.focus();
    setState(applyReaction(reaction, onChange));
  };
  /** Handles a key on the field, or in the text box, with `react`. */
  const keyDown = (react: typeof onKey) => (event: KeyboardEvent) => {
    if (event.ctrlKey || event.metaKey) return;
    const reaction = react(state, options, value, { key: event.key, altKey: event.altKey }, event.timeStamp);
    if (reaction.handled) {
      // An Escape that closes the panel is not one for a dialog around it.
      event.preventDefault();
      event.stopPropagation();
    }
    apply(reaction);
  };
  const close = () => setState(CLOSED);
  /** Closes the panel when the focus leaves for somewhere other than the field or its panel. */
  const closeUnlessWithin = (to: EventTarget | null) => {
    if (!(to instanceof Node && (field.current?.contains(to) || panel.current?.contains(to)))) close();
  };
  // Options that change while the panel is open, as on an Inventory read, would leave it with the old list's size and active option.
  const optionsKey = JSON.stringify(options.map((option) => [option.value, option.label]));
  useEffect(close, [optionsKey]);

  // Measured once open, before paint: the panel first renders hidden at its content's size.
  useLayoutEffect(() => {
    if (!state.open) {
      setPlacement(null);
      return;
    }
    if (!field.current || !panel.current) return;
    const box = field.current.getBoundingClientRect();
    // Rounded up, borders included: a size rounded down would bring up a scrollbar the content does not need.
    const size = panel.current.getBoundingClientRect();
    const content = { width: Math.ceil(size.width), height: Math.ceil(size.height) };
    const placed = placePanel(box, content, { width: window.innerWidth, height: window.innerHeight });
    setPlacement({ ...placed, height: Math.min(content.height, placed.maxHeight) });
    searchBox.current?.focus();
  }, [state.open]);

  // A resize or a scroll under the panel would leave it away from its field, so it closes, as a native select does.
  useEffect(() => {
    if (!state.open) return;
    const onScroll = (event: Event) => {
      if (!(event.target instanceof Node && panel.current?.contains(event.target))) close();
    };
    window.addEventListener("resize", close);
    window.addEventListener("blur", close);
    document.addEventListener("scroll", onScroll, true);
    return () => {
      window.removeEventListener("resize", close);
      window.removeEventListener("blur", close);
      document.removeEventListener("scroll", onScroll, true);
    };
  }, [state.open]);

  useEffect(() => {
    if (state.open && placement) {
      panel.current?.querySelector(`[data-index="${state.active}"]`)?.scrollIntoView({ block: "nearest" });
    }
  }, [state.open, state.active, placement]);

  const style: CSSProperties = placement ?? { top: 0, left: 0, visibility: "hidden" };

  return (
    <>
      {/* None while the panel is open, which it would cover. */}
      <Tooltip text={state.open ? undefined : tooltip}>
        <div
          ref={field}
          className={`select-field${className ? ` ${className}` : ""}${state.open ? " open" : ""}`}
          role="combobox"
          tabIndex={0}
          aria-label={label}
          aria-haspopup="listbox"
          aria-expanded={state.open}
          aria-controls={state.open ? `${id}-list` : undefined}
          aria-activedescendant={activeId}
          onClick={() => setState(onFieldClick(state, options, value))}
          onKeyDown={keyDown(onKey)}
          onBlur={(event) => closeUnlessWithin(event.relatedTarget)}
        >
          <span className="select-value">{chosen?.label ?? ""}</span>
          <Icon name="chevron" small />
        </div>
      </Tooltip>
      {state.open &&
        createPortal(
          <div
            ref={panel}
            className="select-panel"
            style={style}
            // The focus stays on the field, or in the text box, so a click elsewhere in the panel does not blur it.
            onMouseDown={(event) => {
              if (event.target !== searchBox.current) event.preventDefault();
            }}
          >
            {searchLabel !== undefined && (
              <label className="search select-search">
                <Icon name="search" small />
                <input
                  ref={searchBox}
                  type="text"
                  aria-label={searchLabel}
                  placeholder={`${searchLabel}…`}
                  aria-controls={`${id}-list`}
                  aria-autocomplete="list"
                  aria-activedescendant={activeId}
                  autoComplete="off"
                  spellCheck={false}
                  value={state.query}
                  onChange={(event) => setState(onQuery(state, options, event.target.value))}
                  onKeyDown={keyDown(onFilterKey)}
                  onBlur={(event) => closeUnlessWithin(event.relatedTarget)}
                />
              </label>
            )}
            <div id={`${id}-list`} className="select-list" role="listbox" aria-label={label}>
              {shown.length === 0 && <div className="select-empty">No match</div>}
              {groupRuns(shown).map((run, index) => {
                const items = run.items.map(({ option, at }) => (
                  <div
                    key={at}
                    id={optionId(at)}
                    data-index={at}
                    className={`select-option${at === state.active ? " active" : ""}${option.value === value && !option.disabled ? " chosen" : ""}`}
                    role="option"
                    aria-selected={at === state.active}
                    aria-disabled={option.disabled || undefined}
                    onMouseMove={() => {
                      if (at !== state.active) setState(onOptionHover(state, options, at));
                    }}
                    onClick={() => apply(onOptionClick(state, options, value, at))}
                  >
                    {option.label}
                  </div>
                ));
                if (run.group === undefined) return items;
                return (
                  <div key={`group-${index}`} role="group" aria-labelledby={`${id}-group-${index}`}>
                    <div id={`${id}-group-${index}`} className="select-group" role="presentation">
                      {run.group}
                    </div>
                    {items}
                  </div>
                );
              })}
            </div>
          </div>,
          document.body,
        )}
    </>
  );
}
