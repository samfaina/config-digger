// What a step does in the UI, in the words of the plan: open a screen, an
// Item, wait for the Inventory. Each helper finds its element by what the
// tester reads where the app gives a role or a label to find it by.
import { expect, type Locator, type Page } from "@playwright/test";
import { wrapFault } from "./wraps";

/** The throwaway data's root, as the fixtures write every path under it. */
export const ROOT = String.raw`E:\cd-throwaway`;

/** How long a busy-state step holds the answer it waits on, in milliseconds. */
export const HELD = 2000;

/** Why a check that needs what a write leaves stays with a person: the fake backend's writes change nothing. */
export const NO_WRITE = "the fake backend's writes change nothing, so what a real write leaves is seen in the real app";

/** The rail's button of a screen, by the tooltip the tester sees on it. */
export function railButton(page: Page, screen: string): Locator {
  return page.getByRole("button", { name: screen, exact: true });
}

/** Opens a screen from the rail. */
export async function rail(page: Page, screen: string) {
  await railButton(page, screen).click();
}

/** Waits for the Inventory to show its Items. */
export async function inventoryShown(page: Page) {
  await page.locator(".item-row").first().waitFor();
}

/** Picks the Item named `name` in the Items list and waits for its Entries in Details. */
export async function selectItem(page: Page, name: string) {
  await page.locator(".item-row", { has: page.locator(".item-name", { hasText: whole(name) }) }).first().click();
  await page.locator(".entry-card").first().waitFor();
}

/**
 * A pattern of `text`, whose path separators may be slashes or backslashes:
 * the plan writes a Project Claude Code keys with slashes as Claude Code does,
 * the fixtures write it as Windows does, and the WebDriver walk takes either.
 */
export function anySeparator(text: string): string {
  return text
    .split(/[\\/]/)
    .map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
    .join("[\\\\/]");
}

/** A pattern matching `text` whole, whichever path separator it is written with. */
export function whole(text: string): RegExp {
  return new RegExp(`^${anySeparator(text)}$`);
}

/** The text of each element `locator` finds, line by line as the browser lays it out. */
export function textLines(locator: Locator): Promise<string[][]> {
  return locator.evaluateAll((elements) =>
    elements.map((element) => {
      const lines: { top: number; text: string }[] = [];
      const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
      const range = document.createRange();
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        for (let at = 0; at < node.textContent!.length; at++) {
          range.setStart(node, at);
          range.setEnd(node, at + 1);
          const top = Math.round(range.getBoundingClientRect().top);
          const line = lines.at(-1);
          if (line && Math.abs(line.top - top) <= 2) line.text += node.textContent![at];
          else lines.push({ top, text: node.textContent![at] });
        }
      }
      return lines.map((line) => line.text);
    }),
  );
}

/**
 * Expects a path `locator` finds to take more than one line, and each line but
 * the last of each path to end right after a `\` or a `/`.
 */
export async function wrapsAtSeparators(locator: Locator) {
  const fault = wrapFault(await textLines(locator));
  if (fault !== undefined) throw new Error(fault);
}

/** A computed style of the element `locator` finds. */
export function css(locator: Locator, property: string): Promise<string> {
  return locator.evaluate((element, property) => getComputedStyle(element).getPropertyValue(property), property);
}

/** Moves the mouse off to a quiet spot at the top of the window. */
export async function away(page: Page) {
  await page.mouse.move(600, 4);
}

/** Pauses the page's clock a second ahead of its own time, once `page.clock.install()` runs it. */
export async function pauseClock(page: Page) {
  await page.clock.pauseAt((await page.evaluate(() => Date.now())) + 1000);
}

/** The busy notice at the bottom of the screen. */
export function busy(page: Page): Locator {
  return page.locator(".notice.busy");
}

/** Asserts the busy notice: its spinner turns and it says `text`. */
export async function spinning(page: Page, text: string) {
  await expect(busy(page)).toHaveText(text);
  await expect(busy(page).locator(".spinner")).not.toHaveCSS("animation-name", "none");
}

/** The group of the Projects & homes list whose title, after its ▾ or ▸, starts with `title` and a count. */
export function group(page: Page, title: string): Locator {
  return page.locator("h3.place-group", { hasText: new RegExp(`^[▾▸]?\\s*${title} · \\d+$`) });
}

/** Opens the group titled `title` if it is closed. */
export async function openGroup(page: Page, title: string) {
  const toggle = group(page, title).locator(".place-group-toggle");
  if ((await toggle.getAttribute("aria-expanded")) !== "true") await toggle.click();
}

/** The row of the open Projects & homes tab named `name`: a path, whichever its separators, or «No Project». */
export function row(page: Page, name: string): Locator {
  return page.locator(".place-row", { has: page.locator(".place-name", { hasText: whole(name) }) });
}
