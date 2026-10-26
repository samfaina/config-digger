// Section 6: a row of Entries by State, chosen as a filter. Its look is read
// from computed styles: the row's background and gradient, its halo and its
// track.
import { expect, type Locator, type Page } from "@playwright/test";
import { inventoryShown } from "./app";
import { uatStep } from "./step";

/** What a row or a tile shows of its look. */
function look(element: Locator) {
  return element.evaluate((own) => {
    const style = getComputedStyle(own);
    const track = own.querySelector(".bar-track");
    return {
      pressed: own.getAttribute("aria-pressed"),
      background: style.backgroundColor,
      image: style.backgroundImage,
      shadow: style.boxShadow,
      track: track && getComputedStyle(track).backgroundColor,
    };
  });
}

/** The channels of each `rgb(…)` or `rgba(…)` colour in `text`, with alpha 1 when it has none. */
function colours(text: string | null): number[][] {
  return [...(text ?? "").matchAll(/rgba?\(([^)]+)\)/g)].map(([, inner]) => {
    const [red, green, blue, alpha = 1] = inner.split(",").map(Number);
    return [red, green, blue, alpha];
  });
}

const itemNames = (page: Page) => page.locator(".item-row .item-name").allTextContents();

uatStep("6.1", async ({ page, open, check, leave }) => {
  await open();
  await inventoryShown(page);
  // The fixtures' default Claude Code home is the test home of step 1.2, E:\cd-throwaway\claude-code-home.
  const allAgents = page.locator(".agent-tile", { hasText: /^All Agents/ });
  await expect(allAgents).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByRole("combobox", { name: "Claude Code home" })).toHaveText("default");
  const all = await itemNames(page);
  const row = page.locator(".panel.states .bar").first();
  const rest = await look(row);

  await row.hover();
  await check("Bajo el ratón, la fila solo toma un fondo gris un poco más claro.", async () => {
    await expect.poll(async () => (await look(row)).background).not.toBe(rest.background);
    const hovered = await look(row);
    expect(hovered.image).toBe("none");
    const [[red, green, blue, alpha]] = colours(hovered.background);
    expect(Math.max(red, green, blue) - Math.min(red, green, blue)).toBeLessThanOrEqual(16);
    expect(alpha).toBe(1);
    const panel = colours(await page.locator(".panel.states").evaluate((own) => getComputedStyle(own).backgroundColor))[0];
    expect(red + green + blue).toBeGreaterThan(panel[0] + panel[1] + panel[2]);
    expect(hovered.shadow).toBe(rest.shadow);
  });

  await row.click();
  const tile = await look(allAgents);
  await check("Al pulsarla, toma el mismo degradado lila con halo que la tarjeta «All Agents».", async () => {
    const chosen = await look(row);
    expect(chosen.pressed).toBe("true");
    expect(chosen.image).toContain("linear-gradient");
    expect(chosen.image).toBe(tile.image);
    expect(chosen.shadow).not.toBe("none");
    expect(chosen.shadow).toBe(tile.shadow);
  });

  await page.mouse.move(600, 4);
  await check("La fila conserva el degradado al apartar el ratón.", async () => {
    expect((await look(row)).image).toBe(tile.image);
  });
  await check("Su carril se oscurece.", async () => {
    const [track] = colours((await look(row)).track);
    const [before] = colours(rest.track);
    // A black veil on the gradient, where the row at rest had a white one on the panel.
    expect(track.slice(0, 3)).toEqual([0, 0, 0]);
    expect(track[3]).toBeGreaterThan(0);
    expect(before.slice(0, 3)).toEqual([255, 255, 255]);
  });
  // White on the green bar measures 3.2:1, under WCAG AA's 4.5:1 for text, yet the walk took it as legible.
  leave("La barra de color y la cuenta se leen bien sobre el degradado.", "whether it reads well is judged by eye");
  await check("La lista de Items solo muestra los de ese State.", async () => {
    const state = (await row.locator(".bar-label").textContent())!.trim();
    const pills = await page.locator(".item-row .state-pill").evaluateAll((all) => all.map((pill) => pill.firstChild?.textContent?.trim()));
    expect(pills.length).toBeGreaterThan(0);
    expect(pills.length).toBeLessThan(all.length);
    expect(new Set(pills)).toEqual(new Set([state]));
  });

  await page.locator(".panel.states").getByRole("button", { name: "clear" }).click();
  await check("Con clear, la fila vuelve a no tener fondo.", async () => {
    const cleared = await look(row);
    expect(cleared.pressed).toBe("false");
    expect(cleared.image).toBe("none");
    expect(colours(cleared.background)[0][3]).toBe(0);
  });
  await check("Con clear, la lista muestra otra vez todos los Items.", async () => {
    expect(await itemNames(page)).toEqual(all);
  });
});
