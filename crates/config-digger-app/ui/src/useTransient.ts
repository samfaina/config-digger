import { useEffect, useState } from "react";

/** A value that clears itself `ms` after it is set. */
export function useTransient<T>(ms: number): [T | null, (value: T | null) => void] {
  const [value, setValue] = useState<T | null>(null);
  useEffect(() => {
    if (value === null) return;
    const timer = window.setTimeout(() => setValue(null), ms);
    return () => window.clearTimeout(timer);
  }, [value, ms]);
  return [value, setValue];
}
