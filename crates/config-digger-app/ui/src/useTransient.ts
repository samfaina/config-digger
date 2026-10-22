import { useEffect, useState } from "react";

/** Holds a transient value while the pointer is over what shows it, so that it stays. */
export type Hold = (held: boolean) => void;

/**
 * A value that clears itself `ms` after it is set, but not while it is held,
 * such as while the pointer is over it; once let go, it stays `ms` again.
 */
export function useTransient<T>(ms: number): [T | null, (value: T | null) => void, Hold] {
  const [value, setValue] = useState<T | null>(null);
  const [held, setHeld] = useState(false);
  useEffect(() => {
    if (value === null || held) return;
    const timer = window.setTimeout(() => setValue(null), ms);
    return () => window.clearTimeout(timer);
  }, [value, held, ms]);
  return [value, setValue, setHeld];
}

/**
 * The pointer handlers that hold a transient value while the pointer is over
 * what shows it. That lets go once it is no longer shown, as the pointer then
 * leaves it without a pointerleave.
 */
export function useHold(onHold: Hold | undefined) {
  useEffect(() => () => onHold?.(false), [onHold]);
  return { onPointerEnter: () => onHold?.(true), onPointerLeave: () => onHold?.(false) };
}
