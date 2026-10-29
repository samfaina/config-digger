// Section 2: the app's dropdowns, by mouse and by keyboard. Their panel is
// `.select-panel`, whose options say by a class which one is lit (`active`)
// and which is the field's value (`chosen`).
import { expect, type Page } from "@playwright/test";
import { SMALLEST } from "../scripts/fakeUi.ts";
import { css, inventoryShown, rail, ROOT } from "./app";
import { dark, grey, purple } from "./look";
import { uatStep } from "./step";

/** The field labelled `label`, the last one when a dialog sits over the screen. */
const field = (page: Page, label: string) => page.getByRole("combobox", { name: label, exact: true }).last();
/** The open dropdown's panel. */
const panel = (page: Page) => page.locator(".select-panel");
/** The options of the open panel. */
const options = (page: Page) => panel(page).getByRole("option");
/** The option the open panel lights. */
const lit = (page: Page) => panel(page).locator(".select-option.active");

/** The Inventory on the Claude Code home that step 1.2 adds, the one the fixtures record as its default. */
async function claudeCodeHome(page: Page) {
  await page.locator(".agent-tile", { hasText: "Claude Code" }).click();
  await expect(page.locator(".home-tile.selected")).toContainText("default");
  await expect(page.locator(".item-row .item-location").first()).toContainText(String.raw`${ROOT}\claude-code-home`);
}

uatStep("2.1", async ({ page, open, check }) => {
  await open();
  await inventoryShown(page);
  await claudeCodeHome(page);
  await field(page, "Project").click();
  await panel(page).waitFor();
  const other = options(page).filter({ hasText: String.raw`${ROOT}\project-extra` });

  await check("El panel es oscuro, como los paneles de la app.", async () => {
    const background = await css(panel(page), "background-color");
    expect(dark(background), background).toBe(true);
  });
  await check("El panel tiene esquinas redondeadas y sombra.", async () => {
    expect(parseFloat(await css(panel(page), "border-top-left-radius"))).toBeGreaterThan(0);
    await expect(panel(page)).not.toHaveCSS("box-shadow", "none");
  });
  await check("El valor actual sale en morado.", async () => {
    const chosen = panel(page).locator(".select-option.chosen");
    await expect(chosen).toHaveText(await field(page, "Project").innerText());
    const colour = await css(chosen, "color");
    expect(purple(colour), colour).toBe(true);
  });
  await check("La opción bajo el ratón se ilumina.", async () => {
    await other.hover();
    await expect(other).toHaveClass(/\bactive\b/);
    await expect(other).not.toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
  });
  await check("Los Projects que faltan salen bajo el título «MISSING PROJECTS».", async () => {
    const group = panel(page).getByRole("group").filter({ has: page.locator(".select-group") });
    expect(await group.locator(".select-group").evaluate((title) => (title as HTMLElement).innerText)).toBe("MISSING PROJECTS");
    await expect(group.getByRole("option")).toContainText([String.raw`${ROOT}\project-gone`]);
  });
  await check("Al hacer clic, el panel se cierra y el campo muestra el Project elegido.", async () => {
    await other.click();
    await expect(panel(page)).toBeHidden();
    await expect(field(page, "Project")).toHaveText(String.raw`${ROOT}\project-extra`);
  });
});

uatStep("2.2", async ({ page, open, check, leave }) => {
  await open();
  await inventoryShown(page);
  const project = field(page, "Project");
  // Tab from the page's start until the field has the focus.
  for (let presses = 0; presses < 40 && !(await project.evaluate((el) => el === document.activeElement)); presses++) await page.keyboard.press("Tab");
  await expect(project).toBeFocused();
  const start = await project.innerText();

  await check("Alt+Flecha abajo abre el panel sobre el valor actual.", async () => {
    await page.keyboard.press("Alt+ArrowDown");
    await expect(panel(page)).toBeVisible();
    await expect(lit(page)).toHaveText(start);
  });
  await check("Las flechas mueven la opción iluminada.", async () => {
    await page.keyboard.press("ArrowDown");
    await expect(lit(page)).not.toHaveText(start);
    await page.keyboard.press("ArrowUp");
    await expect(lit(page)).toHaveText(start);
  });
  await check("Lo que escribes sale en la caja «Search Projects…» de arriba del panel.", async () => {
    await page.keyboard.type("hide");
    const search = panel(page).getByRole("textbox", { name: "Search Projects" });
    await expect(search).toHaveAttribute("placeholder", "Search Projects…");
    await expect(search).toHaveValue("hide");
  });
  leave(
    "La caja deja solo «E:\\cd-throwaway\\project-hide», iluminado.",
    "the fixtures record project-hide hidden, as step 10.8 leaves it, so the Project field does not offer it",
  );
  await check("Escape cierra el panel sin cambiar el Project, y el foco sigue en el campo.", async () => {
    await page.keyboard.press("Escape");
    await expect(panel(page)).toBeHidden();
    await expect(project).toHaveText(start);
    await expect(project).toBeFocused();
  });
  await check("Enter elige la opción iluminada y cierra el panel.", async () => {
    await page.keyboard.press("Space");
    await expect(panel(page)).toBeVisible();
    const target = String.raw`${ROOT}\project`;
    for (let presses = 0; presses < 40 && (await lit(page).innerText()) !== target; presses++) await page.keyboard.press("ArrowDown");
    await expect(lit(page)).toHaveText(target);
    await page.keyboard.press("Enter");
    await expect(panel(page)).toBeHidden();
    await expect(project).toHaveText(target);
  });
  await check("Con el panel cerrado, Flecha abajo pasa al Project siguiente sin abrir el panel.", async () => {
    await page.keyboard.press("ArrowDown");
    await expect(project).toHaveText(String.raw`${ROOT}\project-extra`);
    await expect(panel(page)).toBeHidden();
  });
  await check("Flecha arriba vuelve a «E:\\cd-throwaway\\project» sin abrir el panel.", async () => {
    await page.keyboard.press("ArrowUp");
    await expect(project).toHaveText(String.raw`${ROOT}\project`);
    await expect(panel(page)).toBeHidden();
  });
  await check("Cada flecha carga el Project al que pasa, como un clic: tras Flecha arriba, la lista de Items muestra «cd-throwaway-project».", async () => {
    await expect(page.locator(".item-row .item-name", { hasText: /^cd-throwaway-project$/ })).toBeVisible();
  });
});

uatStep("2.5", async ({ page, open, check }) => {
  await open();
  await inventoryShown(page);
  const project = field(page, "Project");
  const search = panel(page).getByRole("textbox", { name: "Search Projects" });
  const extra = String.raw`${ROOT}\project-extra`;
  await project.click();
  await panel(page).waitFor();
  const all = await options(page).count();

  await check("El panel se abre con la caja «Search Projects…» arriba, vacía y con el cursor dentro.", async () => {
    await expect(search).toHaveAttribute("placeholder", "Search Projects…");
    await expect(search).toHaveValue("");
    await expect(search).toBeFocused();
    const [box, list] = [await search.boundingBox(), await panel(page).getByRole("listbox").boundingBox()];
    expect(box!.y + box!.height).toBeLessThanOrEqual(list!.y + 1);
  });
  await check("Bajo la caja salen todas las opciones.", async () => {
    await expect(options(page).first()).toHaveText("No Project");
    expect(all).toBeGreaterThan(30);
  });
  await check("Con EXTRA queda solo «E:\\cd-throwaway\\project-extra», iluminado.", async () => {
    await page.keyboard.type("EXTRA");
    await expect(options(page)).toHaveText([extra]);
    await expect(lit(page)).toHaveText(extra);
  });
  await check("Al borrar vuelven todas las opciones.", async () => {
    for (let at = 0; at < "EXTRA".length; at++) await page.keyboard.press("Backspace");
    await expect(search).toHaveValue("");
    await expect(options(page)).toHaveCount(all);
  });
  await check("Con E, «E:\\cd-throwaway\\project-extra» sigue iluminado y no salta a otra opción que empiece por E.", async () => {
    await page.keyboard.type("E");
    await expect(search).toHaveValue("E");
    await expect(lit(page)).toHaveText(extra);
  });
  await page.keyboard.press("Backspace");
  await page.keyboard.type("throwaway");
  await check("Con throwaway quedan los Projects de «E:\\cd-throwaway», con los que faltan bajo «MISSING PROJECTS».", async () => {
    for (const text of await options(page).allInnerTexts()) expect(text.replaceAll("/", "\\")).toMatch(/^E:\\cd-throwaway\\/);
    const group = panel(page).getByRole("group").filter({ has: page.locator(".select-group") });
    expect(await group.locator(".select-group").evaluate((title) => (title as HTMLElement).innerText)).toBe("MISSING PROJECTS");
    await expect(group.getByRole("option")).toContainText([String.raw`${ROOT}\project-gone`]);
  });
  await check("Con throwaway, «No Project» no sale.", async () => {
    await expect(options(page).filter({ hasText: /^No Project$/ })).toHaveCount(0);
  });
  await check("Con throwaway, «E:\\cd-throwaway\\project-extra» sigue iluminado.", async () => {
    await expect(lit(page)).toHaveText(extra);
  });
  const texts = await options(page).allInnerTexts();
  const below = texts[texts.indexOf(extra) + 1];
  await check("Flecha abajo ilumina el Project de debajo.", async () => {
    await page.keyboard.press("ArrowDown");
    await expect(lit(page)).toHaveText(below);
  });
  await check("Enter lo elige y el panel se cierra.", async () => {
    await page.keyboard.press("Enter");
    await expect(panel(page)).toBeHidden();
  });
  await check("El campo muestra el Project elegido.", async () => {
    await expect(project).toHaveText(below);
  });
  await check("El foco vuelve al campo, con borde morado.", async () => {
    await expect(project).toBeFocused();
    const border = await css(project, "border-top-color");
    expect(purple(border), border).toBe(true);
  });
  await check("Al abrir otra vez, la caja está vacía.", async () => {
    await project.click();
    await expect(search).toHaveValue("");
  });
  await check("Con zz, el panel dice «No match».", async () => {
    await page.keyboard.type("zz");
    await expect(panel(page).locator(".select-empty")).toHaveText("No match");
  });
  await check("Escape cierra el panel sin cambiar el Project.", async () => {
    await page.keyboard.press("Escape");
    await expect(panel(page)).toBeHidden();
    await expect(project).toHaveText(below);
  });
});

/** The rows under Claude Code on the Agent homes tab. */
async function claudeCodeHomes(page: Page) {
  await rail(page, "Projects & homes");
  await page.getByRole("tab", { name: /^Agent homes/ }).click();
  return page.locator(".places .scroll", { has: page.locator("h3.agent-group") }).evaluate((list) => {
    let agent = "";
    const names: string[] = [];
    for (const el of list.children) {
      if (el.matches("h3.agent-group")) agent = el.firstChild?.textContent?.trim() ?? "";
      else if (el.matches(".place-row") && agent === "Claude Code") names.push(el.querySelector(".place-name")!.textContent!.trim());
    }
    return names;
  });
}

uatStep("2.3", async ({ page, open, check }) => {
  await page.setViewportSize(SMALLEST);
  await open();
  await inventoryShown(page);
  // The fixtures hold more than one Claude Code home, so the step adds none.
  const before = await claudeCodeHomes(page);
  expect(before.length, "Claude Code homes in the fixtures").toBeGreaterThan(1);
  await rail(page, "Inventory");
  await page.locator(".agent-tile", { hasText: "All Agents" }).click();
  const home = page.locator(".home-row", { hasText: "Claude Code" }).getByRole("combobox");
  await home.scrollIntoViewIfNeeded();
  await home.click();
  await panel(page).waitFor();

  await check("El panel de la fila de Claude Code, cerca del borde de abajo de la ventana, se abre debajo del campo si cabe entero, y encima si no.", async () => {
    const [box, opened] = [(await home.boundingBox())!, (await panel(page).boundingBox())!];
    expect(box.y + box.height).toBeGreaterThan(SMALLEST.height / 2);
    const below = opened.y >= box.y + box.height - 2;
    const above = opened.y + opened.height <= box.y + 2 && SMALLEST.height - (box.y + box.height) < opened.height;
    expect(below || above).toBe(true);
  });
  await check("Ese panel sale entero, sin pasar del borde de la ventana.", async () => {
    const box = (await panel(page).boundingBox())!;
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.y).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(SMALLEST.width);
    expect(box.y + box.height).toBeLessThanOrEqual(SMALLEST.height);
  });
  await page.keyboard.press("Escape");
  await rail(page, "Projects & homes");
  await page.getByRole("tab", { name: /^Agent homes/ }).click();
  await page.getByRole("button", { name: "Add Agent home" }).click();
  const agent = field(page, "Agent");
  await agent.click();
  await panel(page).waitFor();

  await check("El panel de Agent se abre entero debajo de su campo.", async () => {
    const [box, below] = [await agent.boundingBox(), (await panel(page).boundingBox())!];
    expect(below.y).toBeGreaterThanOrEqual(box!.y + box!.height - 2);
    expect(below.y + below.height).toBeLessThanOrEqual(SMALLEST.height);
    expect(below.x).toBeGreaterThanOrEqual(0);
    expect(below.x + below.width).toBeLessThanOrEqual(SMALLEST.width);
  });
  await page.keyboard.press("Escape");
  await page.getByRole("dialog").getByRole("button", { name: "Cancel" }).click();

  await check("Al final, el grupo Claude Code de la pestaña Agent homes tiene las mismas filas que al empezar.", async () => {
    expect(await claudeCodeHomes(page)).toEqual(before);
  });
});

uatStep("2.4", async ({ page, open, check }) => {
  await open();
  await inventoryShown(page);
  await rail(page, "Projects & homes");
  await page.getByRole("tab", { name: /^Agent homes/ }).click();
  await page.getByRole("button", { name: "Add Agent home" }).click();
  const agent = field(page, "Agent");
  const none = options(page).filter({ hasText: /^Choose an Agent…$/ });

  await check("El campo empieza en «Choose an Agent…», en gris dentro del panel.", async () => {
    await expect(agent).toHaveText("Choose an Agent…");
    await agent.click();
    await expect(none).toHaveAttribute("aria-disabled", "true");
    const colour = await css(none, "color");
    expect(grey(colour), colour).toBe(true);
  });
  await check("El clic en esa opción no hace nada y el panel sigue abierto.", async () => {
    // Forced: Playwright waits for an aria-disabled element to be enabled, but the tester's click reaches it.
    await none.click({ force: true });
    await expect(panel(page)).toBeVisible();
    await expect(agent).toHaveText("Choose an Agent…");
  });
  await check("El clic en «Codex» cierra el panel y el campo muestra «Codex».", async () => {
    await options(page).filter({ hasText: /^Codex$/ }).click();
    await expect(panel(page)).toBeHidden();
    await expect(agent).toHaveText("Codex");
  });
});
