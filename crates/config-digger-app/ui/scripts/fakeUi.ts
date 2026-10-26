/**
 * What `npm run screenshots` and `npm run uat:fake` share: the UI served on
 * the fake backend, as `npm run dev:fake` serves it, a headless Edge or
 * Chrome, whichever is installed, and the app's window sizes.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium, type Browser } from "playwright-core";
import { createServer } from "vite";

export const UI = fileURLToPath(new URL("..", import.meta.url));
/** The app's window, as `tauri.conf.json` opens it. */
const { width, height, minWidth, minHeight } = JSON.parse(readFileSync(join(UI, "..", "tauri.conf.json"), "utf8")).app.windows[0];
export const WINDOW: { width: number; height: number } = { width, height };
/** The smallest window the app allows. */
export const SMALLEST: { width: number; height: number } = { width: minWidth, height: minHeight };

/** Serves the UI on the fake backend, at the returned URL, until `close`. */
export async function serveFakeUi(): Promise<{ url: string; close: () => Promise<void> }> {
  const server = await createServer({ root: UI, mode: "fake", logLevel: "warn" });
  await server.listen();
  const url = server.resolvedUrls?.local[0];
  if (url === undefined) {
    await server.close();
    throw new Error("The fake backend's dev server has no URL.");
  }
  return { url, close: () => server.close() };
}

/** A headless Edge, or else Chrome. */
export async function launch(): Promise<Browser> {
  for (const channel of ["msedge", "chrome"]) {
    try {
      return await chromium.launch({ channel, headless: true });
    } catch {
      // Not installed: try the next one.
    }
  }
  throw new Error("Neither Edge nor Chrome is installed.");
}
