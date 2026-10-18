import { useEffect, useId, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import {
  CLOSED,
  onFieldClick,
  onKey,
  onOptionClick,
  onOptionHover,
  placePanel,
  type DropdownOption,
  type DropdownState,
  type Placement,
  type Reaction,
} from "./dropdown";
import { Icon } from "./icons";

type SelectProps = {
  value: string;
  options: DropdownOption[];
  onChange: (value: string) => void;
  /** The accessible name, when no visible label names the field. */
  label?: string;
  title?: string;
  className?: string;
};

/** Runs of options next to each other with the same group, with each option's index. */
function groupRuns(options: DropdownOption[]): { group?: string; items: { option: DropdownOption; at: number }[] }[] {
  const all: { group?: string; items: { option: DropdownOption; at: number }[] }[] = [];
  options.forEach((option, at) => {
    const last = all.at(-1);
    if (last && last.group === option.group) last.items.push({ option, at });
    else all.push({ group: option.group, items: [{ option, at }] });
  });
  return all;
}

/**
 * The app's dropdown: a field and a panel of options drawn with the app's
 * look, where a native `<select>` would show the system's list. The field
 * keeps the focus while the panel is open, so Escape leaves it there.
 */
export function Select({ value, options, onChange, label, title, className }: SelectProps) {
  const id = useId();
  const [state, setState] = useState<DropdownState>(CLOSED);
  const [placement, setPlacement] = useState<Placement | null>(null);
  const field = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const optionId = (at: number) => `${id}-option-${at}`;
  const chosen = options.find((option) => option.value === value);

  const apply = (reaction: Reaction) => {
    setState(reaction.state);
    if (reaction.choose !== null) onChange(reaction.choose);
  };
  const close = () => setState(CLOSED);
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
    setPlacement(placePanel(box, content, { width: window.innerWidth, height: window.innerHeight }));
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
      <div
        ref={field}
        className={`select-field${className ? ` ${className}` : ""}${state.open ? " open" : ""}`}
        role="combobox"
        tabIndex={0}
        aria-label={label}
        aria-haspopup="listbox"
        aria-expanded={state.open}
        aria-controls={state.open ? `${id}-list` : undefined}
        aria-activedescendant={state.open && state.active >= 0 ? optionId(state.active) : undefined}
        title={title}
        onClick={() => setState(onFieldClick(state, options, value))}
        onKeyDown={(event) => {
          if (event.ctrlKey || event.metaKey) return;
          const reaction = onKey(state, options, value, { key: event.key, altKey: event.altKey }, event.timeStamp);
          if (reaction.handled) {
            // An Escape that closes the panel is not one for a dialog around it.
            event.preventDefault();
            event.stopPropagation();
          }
          apply(reaction);
        }}
        onBlur={close}
      >
        <span className="select-value">{chosen?.label ?? ""}</span>
        <Icon name="chevron" small />
      </div>
      {state.open &&
        createPortal(
          <div
            ref={panel}
            id={`${id}-list`}
            className="select-panel"
            role="listbox"
            aria-label={label}
            style={style}
            // The focus stays on the field, so a click in the panel does not blur it.
            onMouseDown={(event) => event.preventDefault()}
          >
            {groupRuns(options).map((run, index) => {
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
          </div>,
          document.body,
        )}
    </>
  );
}
