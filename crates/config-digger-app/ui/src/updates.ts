// What About and the update notice say of updates, worked out from what the
// core reads. Nothing here calls the core or renders.

import { operationTime } from "./history";

/** When the app last checked for updates, on the user's clock. */
export function lastCheckedText(lastChecked: string | null, now: Date): string {
  return `Last checked: ${lastChecked === null ? "never" : operationTime(lastChecked, now)}.`;
}
