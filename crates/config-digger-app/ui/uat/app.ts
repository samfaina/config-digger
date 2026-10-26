// What a step does in the UI, in the words of the plan: open a screen, an
// Item, wait for the Inventory. Each helper finds its element by what the
// tester reads where the app gives a role or a label to find it by.
import type { Page } from "@playwright/test";

/** Opens a screen from the rail, by the tooltip the tester sees on its button. */
export async function rail(page: Page, screen: string) {
  await page.getByRole("button", { name: screen, exact: true }).click();
}

/** Waits for the Inventory to show its Items. */
export async function inventoryShown(page: Page) {
  await page.locator(".item-row").first().waitFor();
}

/** Picks the Item named `name` in the Items list and waits for its Entries in Details. */
export async function selectItem(page: Page, name: string) {
  await page.locator(".item-row", { has: page.locator(".item-name", { hasText: new RegExp(`^${name}$`) }) }).first().click();
  await page.locator(".entry-card").first().waitFor();
}
