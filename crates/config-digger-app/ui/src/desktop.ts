import { commands } from "./bindings";

/** What a desktop action did, as the screen tells the user. */
export type Notify = (message: string) => void;

/** Copies `text` and says so. */
export async function copy(text: string, what: string, notify: Notify): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    notify(`Copied the ${what}.`);
  } catch (error) {
    notify(`The ${what} couldn't be copied: ${String(error)}`);
  }
}

/** Opens a file the inventory shows, or selects a folder in Explorer. */
export async function openLocation(path: string, notify: Notify): Promise<void> {
  const result = await commands.openLocation(path);
  if (result.status === "error") notify(`It couldn't be opened: ${result.error.message}`);
}

/** Shows a file selected in Explorer, such as a backup copy. */
export async function revealLocation(path: string, notify: Notify): Promise<void> {
  const result = await commands.revealLocation(path);
  if (result.status === "error") notify(`Explorer couldn't show it: ${result.error.message}`);
}
