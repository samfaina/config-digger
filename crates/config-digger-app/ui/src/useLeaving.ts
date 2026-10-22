import { useEffect, useState } from "react";

/**
 * What to show of `value`: the value itself, or, for `ms` once it turns null,
 * the last one, leaving, so that it can go with an animation.
 */
export function useLeaving<T>(value: T | null, ms: number): { shown: T | null; leaving: boolean } {
  const [last, setLast] = useState(value);
  if (value !== null && value !== last) setLast(value);
  useEffect(() => {
    if (value !== null || last === null) return;
    const timer = window.setTimeout(() => setLast(null), ms);
    return () => window.clearTimeout(timer);
  }, [value, last, ms]);
  return value === null ? { shown: last, leaving: last !== null } : { shown: value, leaving: false };
}
