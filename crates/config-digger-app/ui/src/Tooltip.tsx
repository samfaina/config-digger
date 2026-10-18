import {
  cloneElement,
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type FocusEvent,
  type KeyboardEvent,
  type MouseEvent,
  type ReactElement,
  type Ref,
} from "react";
import { createPortal } from "react-dom";
import { HIDDEN, HOVER_DELAY_MS, describes, onTooltip, placeTooltip, type TooltipEvent } from "./tooltips";

/** The props of a control the Tooltip adds to, and keeps calling. */
type ControlProps = {
  ref?: Ref<HTMLElement>;
  "aria-label"?: string;
  "aria-describedby"?: string;
  onMouseEnter?: (event: MouseEvent<HTMLElement>) => void;
  onMouseLeave?: (event: MouseEvent<HTMLElement>) => void;
  onMouseDown?: (event: MouseEvent<HTMLElement>) => void;
  onFocus?: (event: FocusEvent<HTMLElement>) => void;
  onBlur?: (event: FocusEvent<HTMLElement>) => void;
  onKeyDown?: (event: KeyboardEvent<HTMLElement>) => void;
};

function setRef<T>(ref: Ref<T> | undefined, value: T | null) {
  if (typeof ref === "function") ref(value);
  else if (ref) ref.current = value;
}

/**
 * The app's tooltip, where a `title` would show the system's: `text` shown
 * next to the one element in `children` on hover after a short delay and on
 * keyboard focus, hidden on leave, on blur, on a click or a key, and on
 * Escape. Screen readers get the text through `aria-describedby`, unless the
 * control's `aria-label` already says it. No `text`, no tooltip.
 */
export function Tooltip({ text, children }: { text?: string; children: ReactElement<ControlProps> }) {
  const id = useId();
  const [state, setState] = useState(HIDDEN);
  const [place, setPlace] = useState<{ top: number; left: number } | null>(null);
  const control = useRef<HTMLElement | null>(null);
  const tip = useRef<HTMLDivElement>(null);
  const send = (event: TooltipEvent) => setState((state) => onTooltip(state, event).state);
  const props = children.props;

  const childRef = props.ref;
  const ref = useCallback(
    (node: HTMLElement | null) => {
      control.current = node;
      setRef(childRef, node);
    },
    [childRef],
  );

  useEffect(() => {
    if (!text) setState(HIDDEN);
  }, [text]);

  useEffect(() => {
    if (!state.waiting) return;
    const timer = window.setTimeout(() => send("delay"), HOVER_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [state.waiting]);

  // Escape hides the tooltip wherever the focus is. Caught before a dialog's own Escape, which it then keeps from closing.
  useEffect(() => {
    if (!state.shown && !state.waiting) return;
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape") return;
      const reaction = onTooltip(state, "escape");
      if (reaction.handled) event.stopPropagation();
      setState(reaction.state);
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [state]);

  // Measured once shown, before paint: the tooltip first renders hidden at its content's size.
  useLayoutEffect(() => {
    if (!state.shown || !control.current || !tip.current) {
      setPlace(null);
      return;
    }
    const size = tip.current.getBoundingClientRect();
    const content = { width: Math.ceil(size.width), height: Math.ceil(size.height) };
    setPlace(placeTooltip(control.current.getBoundingClientRect(), content, { width: window.innerWidth, height: window.innerHeight }));
  }, [state.shown, text]);

  if (!text) return children;

  const describedBy = describes(text, props["aria-label"]) ? [props["aria-describedby"], id].filter(Boolean).join(" ") : props["aria-describedby"];
  const style: CSSProperties = place ?? { top: 0, left: 0, visibility: "hidden" };
  return (
    <>
      {cloneElement(children, {
        ref,
        "aria-describedby": describedBy,
        onMouseEnter: (event) => {
          props.onMouseEnter?.(event);
          send("enter");
        },
        onMouseLeave: (event) => {
          props.onMouseLeave?.(event);
          send("leave");
        },
        onMouseDown: (event) => {
          props.onMouseDown?.(event);
          send("press");
        },
        onFocus: (event) => {
          props.onFocus?.(event);
          send(event.currentTarget.matches(":focus-visible") ? "keyboard focus" : "pointer focus");
        },
        onBlur: (event) => {
          props.onBlur?.(event);
          send("blur");
        },
        onKeyDown: (event) => {
          props.onKeyDown?.(event);
          if (event.key !== "Escape") send("press");
        },
      })}
      {/* Always there, hidden, so `aria-describedby` has its text before it shows. */}
      {createPortal(
        <div ref={tip} id={id} className="tooltip" role="tooltip" hidden={!state.shown} style={style}>
          {text}
        </div>,
        document.body,
      )}
    </>
  );
}
