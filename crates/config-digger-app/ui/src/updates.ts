// What About and the update notice say of updates, worked out from what the
// core reads. Nothing here calls the core or renders.

import { operationTime } from "./history";

/** When the app last checked for updates, on the user's clock. */
export function lastCheckedText(lastChecked: string | null, now: Date): string {
  return `Last checked: ${lastChecked === null ? "never" : operationTime(lastChecked, now)}.`;
}

/** What About says when a check fails: the core's message is only the cause. */
export function checkFailedText(cause: string): string {
  return `The update check failed: ${cause}`;
}

/** What About says when an install fails: the core's message is only the cause. */
export function installFailedText(cause: string): string {
  return `The update was not installed: ${cause}`;
}
