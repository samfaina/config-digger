// Section 11: History · Undo, on the fake backend. Its writes change nothing,
// so a step sees the busy states of a Disable and an Undo, held with the
// page's delay parameter, and the Undo snackbar, timed with page.clock; what a
// real write leaves, a card Disabled or an Operation undone, stays with a
// person. Step 11.6 needs an Operation that needs attention, which the
// fixtures hold none of, so it has no test.
//
// The fixtures record cd-throwaway-codex Disabled, with Enable not offered, so
// the steps disable cd-throwaway-project, an Effective Claude Code server of
// the same Project, whose card offers Disable: the dialog, the busy states and
// the snackbar «Disabled 1 Entry…» are those of any Disable.
import { expect, type Page } from "@playwright/test";
import { inventoryShown, rail, selectItem } from "./app";
import { uatStep } from "./step";

const HELD = 2000;
const NO_WRITE = "the fake backend's writes change nothing, so what a real Operation leaves is seen in the real app";
const PROJECT = String.raw`E:\cd-throwaway\project`;

/** The busy notice at the bottom of the screen. */
const busy = (page: Page) => page.locator(".notice.busy");

/** The Entry card of the Item open in Details. */
const card = (page: Page) => page.locator(".panel.detail .entry-card").first();

/** A button of the card's actions, by its text. */
const cardButton = (page: Page, text: string) => card(page).locator(".entry-actions").getByRole("button", { name: text, exact: true });

/** The red, green, blue and alpha of a CSS colour. */
const rgb = (colour: string) => (colour.match(/\d+(\.\d+)?/g) ?? []).map(Number);
const lightness = (colour: string) => rgb(colour).slice(0, 3).reduce((a, b) => a + b, 0);
/** A light lilac: bluer than green, with plenty of red. */
const lilac = (colour: string) => {
  const [r, g, b] = rgb(colour);
  return b > g && r > 150;
};

/** Asserts the busy notice: its spinner turns and it says `text`. */
async function spinning(page: Page, text: string) {
  await expect(busy(page)).toHaveText(text);
  await expect(busy(page).locator(".spinner")).not.toHaveCSS("animation-name", "none");
}

/** Asserts the card reads again: a small spinner with «Reading…» in its head, by its State. */
async function cardReading(page: Page) {
  const reading = card(page).locator(".entry-head .entry-reading");
  await expect(reading).toHaveText("Reading…");
  await expect(reading.locator(".spinner")).not.toHaveCSS("animation-name", "none");
}

/** The Inventory on cd-throwaway-project in the step's Project, whose card offers Disable. */
async function disableable(page: Page) {
  await inventoryShown(page);
  await page.getByRole("combobox", { name: "Project" }).click();
  await page.getByRole("option", { name: PROJECT, exact: true }).click();
  await expect(page.getByRole("combobox", { name: "Project" })).toHaveText(PROJECT);
  await selectItem(page, "cd-throwaway-project");
}

/** Ticks every warning box of Before you apply and presses the dialog's `verb`, up to its result. */
async function confirm(page: Page, verb: string) {
  const dialog = page.getByRole("dialog");
  await expect(dialog.locator(".loading")).toHaveCount(0);
  for (const box of await dialog.locator("input[type=checkbox]").all()) await box.check();
  await dialog.locator(".dialog-foot").getByRole("button", { name: new RegExp(`^${verb}`) }).click();
  await dialog.locator(".dialog-foot").getByRole("button", { name: "Done" }).waitFor();
}

/** Presses Done on the dialog's result. */
async function done(page: Page) {
  await page.getByRole("dialog").locator(".dialog-foot").getByRole("button", { name: "Done" }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
}

/** Disables the card's Entry through its dialog, up to Done. */
async function disable(page: Page) {
  await cardButton(page, "Disable").click();
  await confirm(page, "Disable");
  await done(page);
}

uatStep("11.8", async ({ page, open, check, leave }) => {
  await open({ delay: { plan_action: HELD, read_inventory: HELD, read_history: HELD } });
  await disableable(page);
  await cardButton(page, "Disable").click();
  const dialog = page.getByRole("dialog");

  await check("Al pulsar Disable, el diálogo se titula «Preparing the preview…», aunque dure menos de un segundo.", async () => {
    await expect(dialog.locator(".dialog-title")).toHaveText("Preparing the preview…");
  });
  await check("El diálogo muestra un aviso con una rueda que gira y el texto «Reading the files this action touches…», sobre filas grises que laten, hasta que sale la vista previa.", async () => {
    const loading = dialog.locator(".loading");
    await expect(loading.getByRole("status")).toHaveText("Reading the files this action touches…");
    await expect(loading.locator(".spinner")).not.toHaveCSS("animation-name", "none");
    await expect(loading.locator(".placeholder-row").first()).not.toHaveCSS("animation-name", "none");
    await expect(loading).toHaveCount(0);
    await expect(dialog.locator(".dialog-title")).not.toHaveText("Preparing the preview…");
  });

  await confirm(page, "Disable");
  await done(page);
  await check("Tras Done sale abajo un aviso con una rueda que gira y el texto «Reading the inventory again…», aunque dure menos de un segundo.", async () => {
    await spinning(page, "Reading the inventory again…");
  });
  await check("En la cabecera de la tarjeta, junto a su State, sale una rueda pequeña con «Reading…».", async () => {
    await cardReading(page);
  });
  await check("Mientras, los botones «Disable» y «Remove» de la tarjeta se ven apagados.", async () => {
    await expect(cardButton(page, "Disable")).toBeDisabled();
    await expect(cardButton(page, "Remove")).toBeDisabled();
  });
  leave("Luego la tarjeta dice «Disabled» y ofrece «Enable».", NO_WRITE);
  await expect(busy(page)).toHaveCount(0);

  await rail(page, "History · Undo");
  // The top row, «Disable 1 Entry», as the tester picks it.
  await page.locator(".operation-row").first().click();
  const detail = page.locator(".history-detail");
  const footButton = (text: string) => detail.locator(".dialog-foot").getByRole("button", { name: text, exact: true });
  await footButton("Undo").click();
  await confirm(page, "Undo");
  await done(page);
  await check("En History, tras Done sale abajo el aviso de la rueda con «Reading History again…», aunque dure menos de un segundo.", async () => {
    await spinning(page, "Reading History again…");
  });
  await check("Mientras, los botones «Delete backup» y «Undo» del detalle se ven apagados.", async () => {
    await expect(footButton("Delete backup")).toBeDisabled();
    await expect(footButton("Undo")).toBeDisabled();
  });
  leave("Luego la Operation dice «Undone by <id nuevo>» y muestra «Can't undo».", NO_WRITE);
});

uatStep("11.9", async ({ page, open, check, leave }) => {
  await open({ delay: { read_inventory: HELD } });
  await disableable(page);
  await disable(page);
  const snackbar = page.locator(".snackbar");
  await expect(snackbar).toHaveText(/^Disabled 1 Entry/);
  await expect(busy(page)).toHaveCount(0);
  await snackbar.getByRole("button", { name: "Undo" }).click();
  await confirm(page, "Undo");
  await done(page);

  await check("Tras el último Done sale abajo un aviso con una rueda que gira y el texto «Reading the inventory again…», aunque dure menos de un segundo.", async () => {
    await spinning(page, "Reading the inventory again…");
  });
  await check("La tarjeta tiene junto a su State una rueda pequeña con «Reading…».", async () => {
    await cardReading(page);
  });
  leave("Mientras, sus botones «Enable» y «Remove» se ven apagados.", `${NO_WRITE}: the card was never Disabled, so it offers Disable, not Enable`);
  await check("Luego la tarjeta dice «Effective» y ofrece «Disable».", async () => {
    await expect(busy(page)).toHaveCount(0);
    await expect(card(page).locator(".entry-reading")).toHaveCount(0);
    await expect(card(page).locator(".entry-head .state-pill")).toHaveText(/^Effective/);
    await expect(cardButton(page, "Disable")).toBeEnabled();
  });
});

uatStep("11.10", async ({ page, open, check, leave }) => {
  await page.clock.install();
  await open();
  await disableable(page);
  await disable(page);
  const snackbar = page.locator(".snackbar");
  await expect(snackbar).toHaveText(/^Disabled 1 Entry/);
  // The pointer rests on the snackbar's text, which holds it while its look is read.
  await snackbar.hover({ position: { x: 20, y: 10 } });
  const undo = snackbar.getByRole("button", { name: "Undo" });
  const dismiss = snackbar.getByRole("button", { name: "Dismiss" });
  const style = await snackbar.evaluate((element) => {
    const css = getComputedStyle(element);
    const of = (selector: string) => getComputedStyle(document.querySelector(selector)!);
    return {
      radius: parseFloat(css.borderTopLeftRadius),
      height: element.getBoundingClientRect().height,
      cardRadius: parseFloat(of(".entry-card").borderTopLeftRadius),
      background: css.backgroundColor,
      panel: of(".panel").backgroundColor,
      cardBackground: of(".entry-card").backgroundColor,
      edge: { width: css.borderLeftWidth, style: css.borderLeftStyle, colour: css.borderLeftColor },
      colour: css.color,
    };
  });

  await check("El aviso tiene las esquinas redondeadas de los avisos del paso 5.1, no de píldora.", async () => {
    expect(style.radius).toBe(style.cardRadius);
    expect(style.radius).toBeLessThan(style.height / 2 - 2);
  });
  await check("Su fondo morado destaca, más claro que el panel y las tarjetas.", async () => {
    const [r, g, b] = rgb(style.background);
    expect(b > g + 30 && r > g, style.background).toBe(true);
    expect(lightness(style.background)).toBeGreaterThan(lightness(style.panel));
    expect(lightness(style.background)).toBeGreaterThan(lightness(style.cardBackground));
  });
  await check("Tiene un borde lila claro a la izquierda.", async () => {
    expect(style.edge.style).toBe("solid");
    expect(parseFloat(style.edge.width)).toBeGreaterThan(0);
    expect(lilac(style.edge.colour), style.edge.colour).toBe(true);
  });
  await check("Su texto es blanco.", async () => {
    expect(rgb(style.colour).slice(0, 3).every((value) => value > 230), style.colour).toBe(true);
  });
  await check("«Undo» es un botón con borde y letras lila claro.", async () => {
    const look = await undo.evaluate((element) => {
      const css = getComputedStyle(element);
      return { border: css.borderTopWidth, borderColour: css.borderTopColor, colour: css.color };
    });
    expect(parseFloat(look.border)).toBeGreaterThan(0);
    expect(lilac(look.borderColour), look.borderColour).toBe(true);
    expect(lilac(look.colour), look.colour).toBe(true);
  });
  await check("«✕» es lila claro, sin fondo.", async () => {
    await expect(dismiss).toHaveText("✕");
    await expect(dismiss).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
    expect(lilac(await dismiss.evaluate((element) => getComputedStyle(element).color))).toBe(true);
  });
  await check("Con el ratón encima, «Undo» y «✕» se rellenan de lila con letras moradas.", async () => {
    for (const button of [undo, dismiss]) {
      await button.hover();
      const look = await button.evaluate((element) => ({ background: getComputedStyle(element).backgroundColor, colour: getComputedStyle(element).color }));
      expect(lilac(look.background), JSON.stringify(look)).toBe(true);
      // Purple letters: the snackbar's own background.
      expect(look.colour, JSON.stringify(look)).toBe(style.background);
    }
    await snackbar.hover({ position: { x: 20, y: 10 } });
  });
  await check("Con el teclado, el foco en «Undo» y en «✕» se ve con un anillo blanco.", async () => {
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    const ring = () =>
      page.evaluate(() => {
        const css = getComputedStyle(document.activeElement!);
        return { label: document.activeElement!.textContent?.trim(), style: css.outlineStyle, colour: css.outlineColor };
      });
    for (let presses = 0; presses < 80 && !(await undo.evaluate((element) => element === document.activeElement)); presses++) await page.keyboard.press("Tab");
    await expect(undo).toBeFocused();
    expect(await ring()).toMatchObject({ style: "solid", colour: "rgb(255, 255, 255)" });
    await page.keyboard.press("Tab");
    await expect(dismiss).toBeFocused();
    expect(await ring()).toMatchObject({ style: "solid", colour: "rgb(255, 255, 255)" });
  });
  leave("Entra subiendo un poco mientras aparece, en un instante.", "a motion, seen by a person");
  await check("Con el ratón encima no se va.", async () => {
    await page.clock.runFor(15_000);
    await expect(snackbar).toBeVisible();
    await expect(snackbar).not.toHaveClass(/leaving/);
  });
  await check("Unos 10 segundos después de apartar el ratón, se va bajando un poco mientras se desvanece.", async () => {
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    // Records the last keyframe of each animation the snackbar leaves with, as it starts to leave.
    await snackbar.evaluate((element) => {
      const seen = new MutationObserver(() => {
        if (!element.classList.contains("leaving")) return;
        const last = element.getAnimations().map((animation) => (animation.effect as KeyframeEffect).getKeyframes().at(-1));
        Object.assign(window, { leftWith: last.map((frame) => ({ opacity: frame?.opacity, translate: frame?.translate })) });
        seen.disconnect();
      });
      seen.observe(element, { attributes: true, attributeFilter: ["class"] });
    });
    await page.mouse.move(600, 4);
    await page.clock.runFor(9_500);
    await expect(snackbar).toBeVisible();
    await expect(snackbar).not.toHaveClass(/leaving/);
    await page.clock.runFor(1_000);
    await expect(snackbar).toHaveCount(0);
    const leftWith = await page.evaluate(() => (window as unknown as { leftWith?: { opacity?: unknown; translate?: unknown }[] }).leftWith);
    // It fades out, moving down a little as it goes.
    expect(leftWith?.some((frame) => String(frame.opacity) === "0" && typeof frame.translate === "string" && frame.translate !== "none"), JSON.stringify(leftWith)).toBe(true);
  });
  leave("Al pulsar ✕, el aviso de Enable se va igual, sin desaparecer de golpe.", `${NO_WRITE}: the fixtures' card offers no Enable, and a motion is seen by a person`);
});
