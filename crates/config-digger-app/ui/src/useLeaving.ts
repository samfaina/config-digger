import { useEffect, useState } from "react";

/**
 * What to show of `value`: the value itself, or, for `ms` once it turns null,
 * the last one, leaving, so that it can go with an animation. The key changes
 * with each new value, so that what shows it comes in afresh.
 */
export function useLeaving<T>(value: T | null, ms: number): { shown: T | null; leaving: boolean; key: number } {
  const [last, setLast] = useState({ value, key: 0 });
  if (value !== null && value !== last.value) setLast({ value, key: last.key + 1 });
  useEffect(() => {
    if (value !== null || last.value === null) return;
    const timer = window.setTimeout(() => setLast((current) => ({ value: null, key: current.key })), ms);
    return () => window.clearTimeout(timer);
  }, [value, last, ms]);
  return { shown: value ?? last.value, leaving: value === null && last.value !== null, key: last.key };
}
