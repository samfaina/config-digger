// Section 3: the app's tooltips, by mouse and by keyboard. Each control's
// tooltip is a `.tooltip` in the page's body, `hidden` until it shows.
import { expect, type Locator, type Page } from "@playwright/test";
import { SMALLEST } from "../scripts/fakeUi.ts";
import { away, css, inventoryShown, pauseClock, railButton, ROOT, selectItem, whole } from "./app";
import { dark } from "./look";
import { uatStep } from "./step";

/** The tooltip on screen. */
const tooltip = (page: Page) => page.locator(".tooltip[role=tooltip]:not([hidden])");
/** The grey line under the name of the Items list's row `name`: its location. */
const location = (page: Page, name: string) => page.locator(".item-row", { has: page.locator(".item-name", { hasText: whole(name) }) }).locator(".item-location");

/** Moves the mouse away and then onto `target`, as the tester does: a tooltip opens on the mouse entering. */
async function hoverFresh(page: Page, target: Locator) {
  await away(page);
  await target.hover();
}

/** Whether `box` lies inside the window `size`. */
function inside(box: { x: number; y: number; width: number; height: number } | null, size: { width: number; height: number }) {
  expect(box).not.toBeNull();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.y).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(size.width);
  expect(box!.y + box!.height).toBeLessThanOrEqual(size.height);
}

uatStep("3.1", async ({ page, open, check }) => {
  // The page's clock, paused once the Inventory is shown, times the hover's delay.
  await page.clock.install();
  await open();
  await inventoryShown(page);
  await page.locator(".agent-tile", { hasText: "Claude Code" }).click();
  await expect(page.locator(".home-tile.selected")).toContainText("default");
  await pauseClock(page);

  await check("Tras medio segundo sale un recuadro oscuro con el texto «Inventory».", async () => {
    await hoverFresh(page, railButton(page, "Inventory"));
    // Lets the hover's render set its timer, on the page's own time.
    await page.waitForTimeout(100);
    await page.clock.runFor(450);
    await expect(tooltip(page)).toHaveCount(0);
    await page.clock.runFor(100);
    await expect(tooltip(page)).toHaveText("Inventory");
    await expect(tooltip(page)).toBeVisible();
    const background = await css(tooltip(page), "background-color");
    expect(dark(background), background).toBe(true);
  });
  await check("El recuadro tiene esquinas redondeadas, borde fino y sombra, como los paneles de la app.", async () => {
    expect(parseFloat(await css(tooltip(page), "border-top-left-radius"))).toBeGreaterThan(0);
    const border = parseFloat(await css(tooltip(page), "border-top-width"));
    expect(border).toBeGreaterThan(0);
    expect(border).toBeLessThanOrEqual(2);
    await expect(tooltip(page)).not.toHaveCSS("box-shadow", "none");
  });
  await check("El recuadro desaparece al apartar el ratón.", async () => {
    await page.mouse.move(600, 300);
    await expect(tooltip(page)).toHaveCount(0);
  });
  await page.clock.resume();
  await selectItem(page, "cd-throwaway-user");
  await pauseClock(page);
  await check("El botón de copiar muestra «Copy the configuration, with every Secret masked».", async () => {
    await hoverFresh(page, page.locator(".panel.detail button[aria-label='Copy the configuration']"));
    await page.waitForTimeout(100);
    await page.clock.runFor(600);
    await expect(tooltip(page)).toHaveText("Copy the configuration, with every Secret masked");
  });
  await check("La fila muestra su ruta entera.", async () => {
    await hoverFresh(page, location(page, "cd-throwaway-user"));
    await page.waitForTimeout(100);
    await page.clock.runFor(600);
    await expect(tooltip(page)).toHaveText(String.raw`${ROOT}\claude-code-home\.claude.json#mcpServers.cd-throwaway-user`);
  });
  await check("No sale en ningún momento la etiqueta amarilla o blanca del sistema.", async () => {
    // The system's label is a `title`'s: no element of the app has one.
    const titled = await page.evaluate(() =>
      [...document.querySelectorAll("[title]")].filter((el) => !(el instanceof SVGElement)).map((el) => el.outerHTML.slice(0, 80)),
    );
    expect(titled).toEqual([]);
  });
});

uatStep("3.2", async ({ page, open, check }) => {
  await open();
  await inventoryShown(page);
  await page.mouse.move(600, 300);
  const inventory = railButton(page, "Inventory");
  for (let presses = 0; presses < 40 && !(await inventory.evaluate((el) => el === document.activeElement)); presses++) await page.keyboard.press("Tab");

  await check("El tooltip «Inventory» sale en cuanto el botón recibe el foco, sin pausa.", async () => {
    await expect(inventory).toBeFocused();
    // No clock runs: the tooltip is there with no time gone by.
    expect(await tooltip(page).count()).toBe(1);
    await expect(tooltip(page)).toHaveText("Inventory");
  });
  await check("Escape lo oculta y el foco sigue en el botón.", async () => {
    await page.keyboard.press("Escape");
    await expect(tooltip(page)).toHaveCount(0);
    await expect(inventory).toBeFocused();
  });
  await check("Con Tab sale el tooltip «History · Undo» del botón siguiente.", async () => {
    await page.keyboard.press("Tab");
    await expect(railButton(page, "History · Undo")).toBeFocused();
    await expect(tooltip(page)).toHaveText("History · Undo");
  });
});

uatStep("3.3", async ({ page, open, check }) => {
  await page.setViewportSize(SMALLEST);
  await open();
  await inventoryShown(page);
  const about = railButton(page, "About");
  await hoverFresh(page, about);
  await expect(tooltip(page)).toHaveText("About");
  const aboutTip = await tooltip(page).boundingBox();
  const aboutButton = await about.boundingBox();
  await page.locator(".agent-tile", { hasText: "Claude Code" }).click();
  await expect(page.locator(".home-tile.selected")).toContainText("default");
  const skill = location(page, "cd-throwaway-user-skill");
  await skill.scrollIntoViewIfNeeded();
  await hoverFresh(page, skill);
  const path = String.raw`${ROOT}\claude-code-home\.claude\skills\cd-throwaway-user-skill`;
  await expect(tooltip(page)).toHaveText(path);

  await check("Los dos tooltips quedan enteros dentro de la ventana.", async () => {
    inside(aboutTip, SMALLEST);
    inside(await tooltip(page).boundingBox(), SMALLEST);
  });
  await check("El de About, en la esquina de abajo a la izquierda, sale encima del botón, sin pasar del borde izquierdo.", async () => {
    expect(aboutButton!.y).toBeGreaterThan(SMALLEST.height / 2);
    expect(aboutTip!.y + aboutTip!.height).toBeLessThanOrEqual(aboutButton!.y + 2);
    expect(aboutTip!.x).toBeGreaterThanOrEqual(0);
  });
  await check("El de la fila muestra la ruta entera de la skill, partida en varias líneas.", async () => {
    await expect(tooltip(page)).toHaveText(path);
    const lines = await tooltip(page).evaluate((tip) => {
      const range = document.createRange();
      range.selectNodeContents(tip);
      return new Set([...range.getClientRects()].filter((rect) => rect.width > 0).map((rect) => Math.round(rect.top))).size;
    });
    expect(lines).toBeGreaterThan(1);
  });
});
