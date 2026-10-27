/**
 * Writes a screenshot of each screen and state of the UI to `screenshots/`,
 * which git ignores: `npm run screenshots`. The UI runs on the fake backend,
 * as `npm run dev:fake` serves it, in a headless Edge or Chrome, whichever is
 * installed, in a window the size of the app's.
 *
 * `npm run screenshots:compare` then compares each shot with its reference in
 * `screenshots/reference/`, which only `npm run screenshots:accept` writes.
 *
 * The screenshots stay on this machine: the repo is public, so none is
 * committed or attached to a PR (see `docs/agents/screenshots.md`).
 */
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "playwright-core";
import { planStep } from "../uat/plan.ts";
import { compare, REFERENCE, SCREENSHOTS, THRESHOLD, type Size } from "./compare.ts";
import { launch, serveFakeUi, SMALLEST, WINDOW } from "./fakeUi.ts";

type Shot = {
  /** The PNG's name, without its extension. */
  name: string;
  /** The ids of the steps of `uat/plan.json` whose look the shot shows. */
  uat?: string[];
  /** The page's query, such as `?fail=open_location` for an error. */
  query?: string;
  /** Whether the query holds the Inventory's first read, so the shot starts before any Item shows. */
  firstRead?: boolean;
  /** What brings the screen to the state shot, once the Inventory is shown. */
  steps?: (page: Page) => Promise<void>;
};

/** How long a busy state's answer is held, in milliseconds: longer than any shot takes. */
const HOLD = 60_000;

const rail = (page: Page, screen: string) => page.getByRole("button", { name: screen, exact: true }).click();

const selectItem = async (page: Page, name: string) => {
  await page.locator(".item-row", { hasText: name }).first().click();
  await page.locator(".entry-card").first().waitFor();
};

/** The Projects & homes row of the Project at `path`, whichever separator it is written with. */
const placeRow = (page: Page, path: string) =>
  page.locator(".place-row", { has: page.locator(".place-name", { hasText: new RegExp(`^${path.replace(/[\\/]/g, "[\\\\/]")}$`) }) });

/**
 * Holds the answer of each of `commands` from now on, in a busy state: the
 * fake backend reads the page's `delay` parameter at each command, so the
 * screen's first read answers at once and the next is held.
 */
const hold = (page: Page, ...commands: string[]) =>
  page.evaluate((delay) => history.replaceState(null, "", `?delay=${delay}`), commands.map((command) => `${command}:${HOLD}`).join(","));

/** Moves the mouse off to a quiet spot at the top of the window, so a tooltip opens on the next hover. */
const away = (page: Page) => page.mouse.move(600, 4);

/** The Inventory of Claude Code's claude-code-many-home, whose Items list overflows. */
const manyHome = async (page: Page) => {
  await page.locator(".agent-tile", { hasText: /^Claude Code/ }).click();
  await page.locator(".home-tile", { has: page.locator(".home-name", { hasText: /claude-code-many-home$/ }) }).click();
  await page.locator(".item-name", { hasText: /^cd-throwaway-many-01$/ }).waitFor();
};

/** Opens `screen` in the smallest window while its first read, `command`, is held. */
const smallestFirstRead = async (page: Page, screen: string, command: string) => {
  await page.setViewportSize(SMALLEST);
  await hold(page, command);
  await rail(page, screen);
  await page.locator("main .loading").waitFor();
};

/** Disables cd-throwaway-project, whose card offers Disable, up to Done, which shows the Undo snackbar «Disabled 1 Entry…». */
const undoSnackbar = async (page: Page) => {
  await page.getByRole("combobox", { name: "Project" }).click();
  await page.getByRole("option", { name: String.raw`E:\cd-throwaway\project`, exact: true }).click();
  await selectItem(page, "cd-throwaway-project");
  await page.locator(".entry-card").first().getByRole("button", { name: "Disable", exact: true }).click();
  const foot = page.getByRole("dialog").locator(".dialog-foot");
  await foot.getByRole("button", { name: /^Disable/ }).click();
  await foot.getByRole("button", { name: "Done" }).click();
  await page.locator(".snackbar").waitFor();
};

/** Opens the action dialog of Remove leftovers on the Projects tab's row of `project`. */
const removeLeftovers = async (page: Page, project: string) => {
  await rail(page, "Projects & homes");
  await placeRow(page, project).getByRole("button", { name: "Remove leftovers" }).click();
  await page.getByRole("dialog").getByRole("region", { name: "What changes" }).waitFor();
};

const SHOTS: Shot[] = [
  { name: "inventory" },
  { name: "inventory-item", uat: ["7.5"], steps: (page) => selectItem(page, "cd-throwaway-user") },
  {
    name: "inventory-project-dropdown",
    uat: ["2.1"],
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
    uat: ["3.1"],
    steps: async (page) => {
      await page.getByRole("button", { name: "History · Undo", exact: true }).hover();
      await page.getByRole("tooltip").waitFor();
    },
  },
  {
    name: "tooltip-projects-homes",
    uat: ["10.1"],
    steps: async (page) => {
      await page.getByRole("button", { name: "Projects & homes", exact: true }).hover();
      await page.getByRole("tooltip").waitFor();
    },
  },
  {
    name: "action-dialog",
    uat: ["7.5"],
    steps: async (page) => {
      await selectItem(page, "cd-throwaway-user");
      await page.locator(".entry-card").first().getByRole("button", { name: "Remove" }).click();
      await page.getByRole("dialog").getByText("What changes").waitFor();
    },
  },
  {
    name: "action-dialog-many-changes",
    uat: ["7.4"],
    steps: async (page) => {
      await page.setViewportSize(SMALLEST);
      await rail(page, "Projects & homes");
      await page.locator(".place-row", { hasText: "many/project-gone" }).getByRole("button", { name: "Remove leftovers" }).click();
      await page.getByRole("region", { name: "What changes" }).waitFor();
    },
  },
  {
    name: "projects",
    uat: ["10.1", "10.21"],
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
    uat: ["7.5"],
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
    uat: ["10.8"],
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
    uat: ["7.5"],
    steps: async (page) => {
      await rail(page, "Projects & homes");
      await page.getByRole("tab", { name: "Agent homes" }).click();
    },
  },
  {
    name: "history-operation",
    uat: ["7.5"],
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
    uat: ["10.8"],
    steps: async (page) => {
      await rail(page, "Projects & homes");
      await page.getByRole("button", { name: "Hide", exact: true }).first().click();
      await page.locator(".notice.success").waitFor();
    },
  },
  {
    name: "notice-information",
    uat: ["5.2"],
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
  {
    name: "notice-copied",
    uat: ["5.1"],
    steps: async (page) => {
      await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
      await selectItem(page, "cd-throwaway-user");
      await page.locator(".panel.detail .entry-location").first().getByRole("button", { name: "Copy path" }).click();
      await page.locator(".notice.success", { hasText: "Copied the path." }).waitFor();
    },
  },
  {
    name: "notice-error-setting",
    uat: ["5.3"],
    query: "?fail=save_settings",
    steps: async (page) => {
      await rail(page, "Settings");
      await page.getByRole("switch", { name: "Check for updates on startup" }).click();
      await page.locator(".notice.error").waitFor();
    },
  },
  {
    name: "notice-undo",
    uat: ["11.10"],
    steps: async (page) => {
      await undoSnackbar(page);
      // The mouse on the snackbar's text holds it while it is shot.
      await page.locator(".snackbar").hover({ position: { x: 20, y: 10 } });
    },
  },
  {
    name: "notice-undo-hover",
    uat: ["11.10"],
    steps: async (page) => {
      await undoSnackbar(page);
      await page.locator(".snackbar").getByRole("button", { name: "Undo" }).hover();
    },
  },
  {
    name: "loading-inventory",
    uat: ["15.1"],
    query: `?delay=read_inventory:${HOLD}`,
    firstRead: true,
    steps: async (page) => {
      await page.locator(".inventory .loading").waitFor();
    },
  },
  {
    name: "inventory-reading",
    uat: ["15.2"],
    steps: async (page) => {
      await hold(page, "read_inventory");
      await page.locator("header.banner .cta", { hasText: /^Refresh$/ }).click();
      await page.locator("header.banner .read-progress").waitFor();
    },
  },
  {
    name: "loading-show-leftovers",
    uat: ["10.14"],
    steps: async (page) => {
      await rail(page, "Projects & homes");
      await hold(page, "read_inventory", "switch_project");
      await placeRow(page, "E:/cd-throwaway/project-gone").getByRole("button", { name: "Show leftovers" }).click();
      await page.locator(".inventory .loading").waitFor();
    },
  },
  { name: "smallest-loading-projects", uat: ["15.3"], steps: (page) => smallestFirstRead(page, "Projects & homes", "read_places") },
  { name: "smallest-loading-history", uat: ["15.3"], steps: (page) => smallestFirstRead(page, "History · Undo", "read_history") },
  { name: "smallest-loading-settings", uat: ["15.3"], steps: (page) => smallestFirstRead(page, "Settings", "read_settings") },
  {
    name: "action-dialog-leftovers",
    uat: ["7.1", "7.5", "10.7"],
    steps: async (page) => {
      await removeLeftovers(page, "E:/cd-throwaway/project-gone");
      await page.getByRole("dialog").locator(".dialog-section-title", { hasText: /^Files/ }).scrollIntoViewIfNeeded();
    },
  },
  {
    name: "action-dialog-locator-tooltip",
    uat: ["7.1"],
    steps: async (page) => {
      await removeLeftovers(page, "E:/cd-throwaway/project-gone");
      await page.getByRole("dialog").locator(".change-key").hover();
      await page.getByRole("tooltip").waitFor();
    },
  },
  {
    name: "smallest-action-dialog-leftovers-narrow",
    uat: ["7.1"],
    steps: async (page) => {
      await page.setViewportSize(SMALLEST);
      await removeLeftovers(page, "E:/cd-throwaway/project-gone");
      // The smallest window holds the key on one line: the dialog is narrowed as a narrower window would lay it out.
      await page.getByRole("dialog").evaluate((dialog) => (dialog.style.maxWidth = "420px"));
    },
  },
  {
    name: "history-delete-backup",
    uat: ["7.5"],
    steps: async (page) => {
      await rail(page, "History · Undo");
      await page.locator(".operation-row").first().click();
      await page.getByRole("button", { name: "Delete backup" }).click();
      await page.getByRole("dialog").locator(".dialog-foot").getByRole("button", { name: /^Delete backup/ }).waitFor();
    },
  },
  {
    name: "inventory-state-chosen",
    uat: ["6.1"],
    steps: async (page) => {
      await page.locator(".panel.states .bar").first().click();
      await away(page);
    },
  },
  {
    name: "smallest-home-dropdown",
    uat: ["2.3"],
    steps: async (page) => {
      await page.setViewportSize(SMALLEST);
      await page.locator(".agent-tile", { hasText: "All Agents" }).click();
      const home = page.locator(".home-row", { hasText: "Claude Code" }).getByRole("combobox");
      await home.scrollIntoViewIfNeeded();
      await home.click();
      await page.locator(".select-panel").waitFor();
    },
  },
  {
    name: "smallest-add-home-agent-dropdown",
    uat: ["2.3"],
    steps: async (page) => {
      await page.setViewportSize(SMALLEST);
      await rail(page, "Projects & homes");
      await page.getByRole("tab", { name: /^Agent homes/ }).click();
      await page.getByRole("button", { name: "Add Agent home" }).click();
      await page.getByRole("dialog").getByRole("combobox", { name: "Agent", exact: true }).click();
      await page.locator(".select-panel").waitFor();
    },
  },
  {
    name: "smallest-tooltip-about",
    uat: ["3.3"],
    steps: async (page) => {
      await page.setViewportSize(SMALLEST);
      await page.getByRole("button", { name: "About", exact: true }).hover();
      await page.getByRole("tooltip").waitFor();
    },
  },
  {
    name: "smallest-tooltip-skill",
    uat: ["3.3"],
    steps: async (page) => {
      await page.setViewportSize(SMALLEST);
      await page.locator(".agent-tile", { hasText: "Claude Code" }).click();
      const location = page.locator(".item-row", { has: page.locator(".item-name", { hasText: /^cd-throwaway-user-skill$/ }) }).locator(".item-location");
      await location.scrollIntoViewIfNeeded();
      await location.hover();
      await page.getByRole("tooltip").waitFor();
    },
  },
  {
    name: "smallest-projects-wraps",
    uat: ["10.10"],
    steps: async (page) => {
      await page.setViewportSize(SMALLEST);
      await rail(page, "Projects & homes");
      await placeRow(page, String.raw`E:\cd-throwaway\paths-that-wrap\in-the-smallest-window\of-config-digger\project-long`).scrollIntoViewIfNeeded();
    },
  },
  {
    name: "smallest-homes-wraps",
    uat: ["10.10"],
    steps: async (page) => {
      await page.setViewportSize(SMALLEST);
      await rail(page, "Projects & homes");
      await page.getByRole("tab", { name: /^Agent homes/ }).click();
      await page.locator(".place-row", { hasText: "claude-desktop-home" }).first().scrollIntoViewIfNeeded();
    },
  },
  {
    name: "smallest-many-items",
    uat: ["4.1"],
    steps: async (page) => {
      await page.setViewportSize(SMALLEST);
      await manyHome(page);
      await page.locator(".panel.items .scroll").evaluate((list) => list.scrollTo(0, list.scrollHeight));
    },
  },
  {
    name: "smallest-licenses",
    uat: ["4.4"],
    steps: async (page) => {
      await page.setViewportSize(SMALLEST);
      await rail(page, "About");
      await page.getByRole("button", { name: "Third-party licenses" }).first().click();
      const list = page.getByRole("dialog").locator(".licenses-list");
      await list.waitFor();
      await list.locator("details.license summary").first().click();
      await list.locator("pre.license-text").first().waitFor();
    },
  },
  {
    name: "smallest-project-dropdown",
    uat: ["4.5"],
    steps: async (page) => {
      await page.setViewportSize(SMALLEST);
      await manyHome(page);
      await page.getByRole("combobox", { name: "Project" }).click();
      await page.locator(".select-panel .select-list").waitFor();
    },
  },
  {
    name: "smallest-inventory-many",
    uat: ["4.6"],
    steps: async (page) => {
      await page.setViewportSize(SMALLEST);
      await manyHome(page);
      await page.getByRole("combobox", { name: "Project" }).click();
      await page.getByRole("option", { name: String.raw`E:\cd-throwaway\many\project-01`, exact: true }).click();
      const row = page.locator(".item-row", { has: page.locator(".item-name", { hasText: /^cd-throwaway-many-01$/ }) });
      await row.click();
      await page.locator(".entry-card").first().waitFor();
      await away(page);
      await row.locator(".item-location").hover();
      await page.getByRole("tooltip").waitFor();
    },
  },
];

/** Each main screen again in the smallest window, where the text has the least room, as `smallest-<name>`. */
const SMALLEST_SHOTS: Shot[] = [
  { name: "inventory-item" },
  { name: "projects", uat: ["4.2", "10.10"] },
  { name: "projects-homes", uat: ["10.10"] },
  { name: "history-operation" },
  { name: "settings" },
  { name: "about" },
].map(({ name, uat }) => {
  const shot = SHOTS.find((each) => each.name === name);
  if (shot === undefined) throw new Error(`No shot named ${name}.`);
  return {
    name: `smallest-${name}`,
    uat,
    steps: async (page) => {
      await page.setViewportSize(SMALLEST);
      await shot.steps?.(page);
    },
  };
});

const ALL = [...SHOTS, ...SMALLEST_SHOTS];

// Each id a shot names is a step of the plan, so a step's look and its shots can be matched.
for (const shot of ALL) for (const id of shot.uat ?? []) planStep(id);

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

/** Shoots each shot to `screenshots/`, and returns the names of those it could not shoot. */
async function shoot(): Promise<string[]> {
  const server = await serveFakeUi();
  const { url } = server;
  const browser = await launch();
  mkdirSync(SCREENSHOTS, { recursive: true });
  // The last run's shots and diffs go, and the references stay.
  for (const file of readdirSync(SCREENSHOTS)) if (file.endsWith(".png")) rmSync(join(SCREENSHOTS, file));
  const failed: string[] = [];
  try {
    for (const shot of ALL) {
      const page = await browser.newPage({ viewport: WINDOW, colorScheme: "dark" });
      try {
        await page.goto(`${url}${shot.query ?? ""}`);
        if (!shot.firstRead) await page.locator(".item-row").first().waitFor();
        await shot.steps?.(page);
        await settled(page);
        // A spinner or a pulse stops at its first frame, so a busy state shoots the same each run.
        await page.screenshot({ path: join(SCREENSHOTS, `${shot.name}.png`), animations: "disabled" });
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
  if (failed.length > 0) console.error(`No screenshot of ${failed.join(", ")}.`);
  return failed;
}

/**
 * Compares each of the shots `names` with its reference, writes a diff of
 * each that differs as `<name>.diff.png`, and lists the references that match
 * no shot. Returns whether every shot matched. It never writes a reference.
 */
function compareAll(names: string[]): boolean {
  let passed = true;
  for (const name of names) {
    const reference = join(REFERENCE, `${name}.png`);
    if (!existsSync(reference)) {
      console.error(`no reference   ${name}`);
      passed = false;
      continue;
    }
    const result = compare(readFileSync(join(SCREENSHOTS, `${name}.png`)), readFileSync(reference), THRESHOLD);
    if (result.kind === "match") {
      console.log(`match          ${name}`);
    } else if (result.kind === "differs") {
      writeFileSync(join(SCREENSHOTS, `${name}.diff.png`), result.diff);
      console.error(`differs        ${name}: ${result.pixels} pixels, diff in screenshots/${name}.diff.png`);
      passed = false;
    } else {
      const size = ({ width, height }: Size) => `${width} × ${height}`;
      console.error(`size mismatch  ${name}: the shot is ${size(result.shot)}, its reference ${size(result.reference)}`);
      passed = false;
    }
  }
  const shots = new Set(ALL.map((each) => `${each.name}.png`));
  const unmatched = existsSync(REFERENCE) ? readdirSync(REFERENCE).filter((file) => file.endsWith(".png") && !shots.has(file)) : [];
  if (unmatched.length > 0) {
    console.log(`No shot has these references, which can be deleted: ${unmatched.map((file) => `screenshots/reference/${file}`).join(", ")}.`);
  }
  return passed;
}

const failed = await shoot();
let passed = failed.length === 0;
if (process.argv.includes("--compare")) {
  if (!compareAll(ALL.map((each) => each.name).filter((name) => !failed.includes(name)))) passed = false;
  console.log(
    passed
      ? "Every shot matches its reference."
      : `Not every shot matches its reference within ${THRESHOLD} pixels. Look at each new or changed shot before accepting it (docs/agents/screenshots.md).`,
  );
}
if (!passed) process.exitCode = 1;
