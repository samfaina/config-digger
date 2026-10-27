/**
 * Writes a screenshot of each screen and state of the UI to `screenshots/`,
 * which git ignores: `npm run screenshots`. The UI runs on the fake backend,
 * as `npm run dev:fake` serves it, in a headless Edge or Chrome, whichever is
 * installed, in a window the size of the app's.
 *
 * The screenshots stay on this machine: the repo is public, so none is
 * committed or attached to a PR (see `docs/agents/screenshots.md`).
 */
import { mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "playwright-core";
import { launch, serveFakeUi, SMALLEST, UI, WINDOW } from "./fakeUi.ts";

const OUT = join(UI, "screenshots");

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
      await page.getByRole("listbox").waitFor();
      // Typed where the focus is, as a user does: fill() focuses the box itself, so the shot would hide a panel that opens without it.
      await page.keyboard.type("project");
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
    name: "projects-stranded",
    steps: async (page) => {
      await rail(page, "Projects & homes");
      await page.getByRole("heading", { name: /^Stranded · / }).scrollIntoViewIfNeeded();
    },
  },
  {
    name: "projects-hidden",
    steps: async (page) => {
      await rail(page, "Projects & homes");
      await page.getByRole("button", { name: /Hidden · / }).click();
      await page.locator(".place-row.dimmed").first().scrollIntoViewIfNeeded();
    },
  },
  {
    name: "action-dialog-stranded",
    steps: async (page) => {
      await rail(page, "Projects & homes");
      await page.getByRole("button", { name: "Remove all" }).click();
      await page.getByRole("dialog").getByText("Removes the Stranded Entry", { exact: false }).waitFor();
    },
  },
  {
    name: "action-dialog-untouched",
    query: "?untouched=40",
    steps: async (page) => {
      await page.setViewportSize(SMALLEST);
      await page.getByRole("combobox", { name: "Project" }).click();
      await page.getByRole("option", { name: String.raw`E:\cd-throwaway\project`, exact: true }).click();
      await selectItem(page, "cd-throwaway-project");
      await page.locator(".entry-card").first().getByRole("button", { name: "Disable", exact: true }).click();
      const dialog = page.getByRole("dialog");
      await dialog.locator(".dialog-foot").getByRole("button", { name: /^Disable/ }).click();
      await dialog.getByRole("region", { name: "Untouched" }).waitFor();
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

/** Each main screen again in the smallest window, where the text has the least room, as `smallest-<name>`. */
const SMALLEST_SHOTS: Shot[] = ["inventory-item", "projects", "projects-homes", "history-operation", "settings", "about"].map((name) => {
  const shot = SHOTS.find((each) => each.name === name);
  if (shot === undefined) throw new Error(`No shot named ${name}.`);
  return {
    name: `smallest-${name}`,
    steps: async (page) => {
      await page.setViewportSize(SMALLEST);
      await shot.steps?.(page);
    },
  };
});

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

async function main() {
  const server = await serveFakeUi();
  const { url } = server;
  const browser = await launch();
  rmSync(OUT, { recursive: true, force: true });
  mkdirSync(OUT, { recursive: true });
  const failed: string[] = [];
  try {
    for (const shot of [...SHOTS, ...SMALLEST_SHOTS]) {
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
