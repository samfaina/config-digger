/**
 * Writes a screenshot of each screen and state of the UI to `screenshots/`,
 * which git ignores: `npm run screenshots`. The UI runs on the fake backend,
 * as `npm run dev:fake` serves it, in a headless Edge or Chrome, whichever is
 * installed, in a window the size of the app's.
 *
 * The screenshots stay on this machine: the repo is public, so none is
 * committed or attached to a PR (see `docs/agents/screenshots.md`).
 */
import { mkdirSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium, type Browser, type Page } from "playwright-core";
import { createServer } from "vite";

const UI = fileURLToPath(new URL("..", import.meta.url));
const OUT = join(UI, "screenshots");
/** The app's window, as `tauri.conf.json` opens it. */
const { width, height, minWidth, minHeight } = JSON.parse(readFileSync(join(UI, "..", "tauri.conf.json"), "utf8")).app.windows[0];
const WINDOW: { width: number; height: number } = { width, height };
/** The smallest window the app allows. */
const SMALLEST: { width: number; height: number } = { width: minWidth, height: minHeight };

type Shot = {
  /** The PNG's name, without its extension. */
  name: string;
  /** The page's query, such as `?fail=open_location` for an error. */
  query?: string;
  /** What brings the screen to the state shot, once the Inventory is shown. */
  steps?: (page: Page) => Promise<void>;
};

const rail = (page: Page, screen: string) => page.getByRole("button", { name: screen, exact: true }).click();

const selectItem = async (page: Page, name: string) => {
  await page.locator(".item-row", { hasText: name }).first().click();
  await page.locator(".entry-card").first().waitFor();
};

const SHOTS: Shot[] = [
  { name: "inventory" },
  { name: "inventory-item", steps: (page) => selectItem(page, "cd-throwaway-user") },
  {
    name: "inventory-project-dropdown",
    steps: async (page) => {
      await page.getByRole("combobox", { name: "Project" }).click();
      await page.getByRole("listbox").waitFor();
    },
  },
  {
    name: "inventory-project-dropdown-search",
    steps: async (page) => {
      await page.getByRole("combobox", { name: "Project" }).click();
      await page.getByRole("textbox", { name: "Search Projects" }).fill("project");
    },
  },
  {
    name: "tooltip",
    steps: async (page) => {
      await page.getByRole("button", { name: "History · Undo", exact: true }).hover();
      await page.getByRole("tooltip").waitFor();
    },
  },
  {
    name: "action-dialog",
    steps: async (page) => {
      await selectItem(page, "cd-throwaway-user");
      await page.locator(".entry-card").first().getByRole("button", { name: "Remove" }).click();
      await page.getByRole("dialog").getByText("What changes").waitFor();
    },
  },
  {
    name: "action-dialog-many-changes",
    steps: async (page) => {
      await page.setViewportSize(SMALLEST);
      await rail(page, "Projects & homes");
      await page.locator(".place-row", { hasText: "many/project-gone" }).getByRole("button", { name: "Remove leftovers" }).click();
      await page.getByRole("region", { name: "What changes" }).waitFor();
    },
  },
  {
    name: "projects",
    steps: async (page) => {
      await rail(page, "Projects & homes");
      await page.getByRole("tab", { name: "Projects" }).waitFor();
    },
  },
  {
    name: "projects-search",
    steps: async (page) => {
      await rail(page, "Projects & homes");
      await page.getByRole("textbox", { name: "Search Projects" }).fill("hide");
    },
  },
  {
    name: "projects-missing",
    steps: async (page) => {
      await rail(page, "Projects & homes");
      await page.locator(".places-list .scroll").evaluate((list) => list.scrollTo(0, list.scrollHeight));
    },
  },
  {
    name: "projects-homes",
    steps: async (page) => {
      await rail(page, "Projects & homes");
      await page.getByRole("tab", { name: "Agent homes" }).click();
    },
  },
  {
    name: "history-operation",
    steps: async (page) => {
      await rail(page, "History · Undo");
      await page.locator(".operation-row").first().click();
      await page.getByText("Backup folder").first().waitFor();
    },
  },
  {
    name: "history-filter",
    steps: async (page) => {
      await page.setViewportSize(SMALLEST);
      await rail(page, "History · Undo");
      await page.getByRole("textbox", { name: "Search Entry names" }).fill("codex");
      await page.getByRole("combobox", { name: "Status" }).click();
      await page.getByRole("listbox").waitFor();
    },
  },
  {
    name: "history-filter-none",
    steps: async (page) => {
      await rail(page, "History · Undo");
      await page.getByRole("textbox", { name: "Search Entry names" }).fill("missing");
      await page.getByText("No Operation matches the filter.").waitFor();
    },
  },
  { name: "settings", steps: (page) => rail(page, "Settings") },
  {
    name: "about",
    steps: async (page) => {
      await rail(page, "About");
      await page.getByRole("heading", { name: "Links" }).waitFor();
    },
  },
  {
    name: "notice-success",
    steps: async (page) => {
      await rail(page, "Projects & homes");
      await page.getByRole("button", { name: "Hide", exact: true }).first().click();
      await page.locator(".notice.success").waitFor();
    },
  },
  {
    name: "notice-information",
    steps: async (page) => {
      await rail(page, "Settings");
      await page.locator(".place-row", { hasText: "Logs" }).getByRole("button", { name: "Open folder" }).click();
      await page.locator(".notice.information").waitFor();
    },
  },
  {
    name: "notice-error",
    query: "?fail=open_location",
    steps: async (page) => {
      await selectItem(page, "cd-throwaway-user");
      await page.locator(".entry-card .chip-button").first().click();
      await page.locator(".notice.error").waitFor();
    },
  },
];

/** Once every animation that ends has ended, such as a notice sliding in; a spinner never does. */
async function settled(page: Page) {
  await page.evaluate(() =>
    Promise.all(
      document
        .getAnimations()
        .filter((animation) => animation.effect?.getComputedTiming().iterations !== Infinity)
        .map((animation) => animation.finished),
    ),
  );
}

/** A headless Edge, or else Chrome. */
async function launch(): Promise<Browser> {
  for (const channel of ["msedge", "chrome"]) {
    try {
      return await chromium.launch({ channel, headless: true });
    } catch {
      // Not installed: try the next one.
    }
  }
  throw new Error("Neither Edge nor Chrome is installed.");
}

async function main() {
  const server = await createServer({ root: UI, mode: "fake", logLevel: "warn" });
  await server.listen();
  const url = server.resolvedUrls?.local[0];
  if (url === undefined) throw new Error("The fake backend's dev server has no URL.");
  const browser = await launch();
  rmSync(OUT, { recursive: true, force: true });
  mkdirSync(OUT, { recursive: true });
  const failed: string[] = [];
  try {
    for (const shot of SHOTS) {
      const page = await browser.newPage({ viewport: WINDOW, colorScheme: "dark" });
      try {
        await page.goto(`${url}${shot.query ?? ""}`);
        await page.locator(".item-row").first().waitFor();
        await shot.steps?.(page);
        await settled(page);
        await page.screenshot({ path: join(OUT, `${shot.name}.png`) });
        console.log(`wrote screenshots/${shot.name}.png`);
      } catch (error) {
        failed.push(shot.name);
        console.error(`${shot.name}: ${error instanceof Error ? error.message : String(error)}`);
      } finally {
        await page.close();
      }
    }
  } finally {
    await browser.close();
    await server.close();
  }
  if (failed.length > 0) {
    console.error(`No screenshot of ${failed.join(", ")}.`);
    process.exitCode = 1;
  }
}

await main();
