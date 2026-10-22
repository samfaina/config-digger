import { useCallback, useEffect, useState } from "react";

/**
 * A value that clears itself `ms` after it is set, but not while it is held,
 * such as while the pointer is over it; once let go, it stays `ms` again.
 */
export function useTransient<T>(ms: number): [T | null, (value: T | null) => void, (held: boolean) => void] {
  const [value, setValue] = useState<T | null>(null);
  const [held, setHeld] = useState(false);
  useEffect(() => {
    if (value === null || held) return;
    const timer = window.setTimeout(() => setValue(null), ms);
    return () => window.clearTimeout(timer);
  }, [value, held, ms]);
  // A new value is not held, as the pointer may have left the last one by its going.
  const set = useCallback((next: T | null) => {
    setHeld(false);
    setValue(next);
  }, []);
  return [value, set, setHeld];
}
