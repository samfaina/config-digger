import { commands, type Error as ApiError } from "./bindings";
import type { Notice } from "./Notice";

/** What a desktop action did, as the screen tells the user. */
export type Notify = (notice: Notice) => void;

/** How long a notice stays. */
export const NOTICE_MS = 2500;

/** What a command returns. */
export type CommandResult<T> = { status: "ok"; data: T } | { status: "error"; error: ApiError };

/** Runs a command, turning a thrown failure into an error result. */
export async function call<T>(command: () => Promise<CommandResult<T>>): Promise<CommandResult<T>> {
  try {
    return await command();
  } catch (thrown) {
    return {
      status: "error",
      error: { code: "unexpected", message: String(thrown), candidates: [], backup: null, diffs: [], failed: null },
    };
  }
}

/** Copies `text` and says so. */
export async function copy(text: string, what: string, notify: Notify): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    notify({ kind: "success", text: `Copied the ${what}.` });
  } catch (error) {
    notify({ kind: "error", text: `The ${what} couldn't be copied: ${String(error)}` });
  }
}

/** Opens a file the inventory shows, or selects a folder in Explorer. */
export async function openLocation(path: string, notify: Notify): Promise<void> {
  const result = await commands.openLocation(path);
  if (result.status === "error") notify({ kind: "error", text: `It couldn't be opened: ${result.error.message}` });
}

/** Shows a file selected in Explorer, such as a backup copy. */
export async function revealLocation(path: string, notify: Notify): Promise<void> {
  const result = await commands.revealLocation(path);
  if (result.status === "error") notify({ kind: "error", text: `Explorer couldn't show it: ${result.error.message}` });
}
