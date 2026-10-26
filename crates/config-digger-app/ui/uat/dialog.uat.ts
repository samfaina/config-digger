// Section 7: the action dialog's What changes, and the dialog of an action
// with many changes in the smallest window. The fixtures hold the leftovers
// of E:/cd-throwaway/project-gone and those of many/project-gone, which the
// step's home claude-code-many-home brings, so no step adds a home here.
// The fixtures write these paths with backslashes, where the plan writes
// slashes, as the WebDriver walk also finds: a path matches either way.
import { expect, type Locator, type Page } from "@playwright/test";
import { SMALLEST } from "../scripts/fakeUi.ts";
import { inventoryShown, rail } from "./app";
import { uatStep } from "./step";

const rgb = (colour: string) => (colour.match(/\d+(\.\d+)?/g) ?? []).map(Number);
const grey = (colour: string) => {
  const [r, g, b, alpha = 1] = rgb(colour);
  return alpha === 0 || (Math.abs(r - g) < 20 && Math.abs(g - b) < 25);
};
const violet = (colour: string) => {
  const [r, g, b] = rgb(colour);
  return b > g + 40 && r > g;
};

/** A pattern of `text` whose path separators may be slashes or backslashes. */
function anySlash(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&").replace(/\\\\|\//g, "[\\\\/]");
}

/** Presses Remove leftovers on the Projects tab's row of `project` and waits for the preview's What changes. */
async function removeLeftovers(page: Page, project: string) {
  await rail(page, "Projects & homes");
  const row = page.locator(".place-row", { has: page.locator(".place-name", { hasText: new RegExp(`^${anySlash(project)}$`) }) });
  await row.getByRole("button", { name: "Remove leftovers" }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByRole("region", { name: "What changes" }).or(dialog.locator(".dialog-section", { hasText: "What changes" })).first().waitFor();
  return dialog;
}

/** One line of the change: its label and what it says, as the tester reads them. */
function part(dialog: Locator, label: string) {
  return dialog.locator(".change-part", { has: dialog.page().locator(".change-label", { hasText: new RegExp(`^${label}$`, "i") }) }).locator("dd");
}

uatStep("7.1", async ({ page, open, check }) => {
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
  await open();
  await inventoryShown(page);
  const dialog = await removeLeftovers(page, "E:/cd-throwaway/project-gone");
  const row = dialog.locator(".change-row");
  const key = dialog.locator(".change-key");

  await check("Arriba sale «PROJECT E:/cd-throwaway/project-gone» en su línea.", async () => {
    await expect(dialog.locator(".change-project-name")).toHaveText(new RegExp(`^PROJECT\\s+${anySlash("E:/cd-throwaway/project-gone")}$`, "i"));
    await expect(dialog.locator(".change-project-name .change-label")).toHaveCSS("text-transform", "uppercase");
  });
  await check("Debajo sale «FILE .claude.json».", async () => {
    await expect(dialog.locator(".change-file-name")).toHaveText(/^FILE\s+\.claude\.json$/i);
    const [project, file] = await Promise.all([dialog.locator(".change-project-name").boundingBox(), dialog.locator(".change-file-name").boundingBox()]);
    expect(file!.y).toBeGreaterThanOrEqual(project!.y + project!.height - 1);
  });
  await check("Debajo, el cambio sale en su recuadro, con una etiqueta gris en mayúsculas a la izquierda de cada línea, en una columna.", async () => {
    const look = await row.evaluate((r) => {
      const c = getComputedStyle(r);
      const labels = [...r.querySelectorAll(".change-label")];
      return {
        box: c.borderTopStyle !== "none" || c.backgroundColor !== "rgba(0, 0, 0, 0)",
        lefts: labels.map((label) => Math.round(label.getBoundingClientRect().left)),
        labelsLeftOfText: [...r.querySelectorAll(".change-part")].every((p) => p.querySelector("dt")!.getBoundingClientRect().right <= p.querySelector("dd")!.getBoundingClientRect().left),
        colours: labels.map((label) => getComputedStyle(label).color),
        cases: labels.map((label) => getComputedStyle(label).textTransform),
      };
    });
    expect(look.box, "the change's box").toBe(true);
    expect(new Set(look.lefts).size, `label lefts ${look.lefts}`).toBe(1);
    expect(look.labelsLeftOfText, "each label left of its line").toBe(true);
    expect(look.colours.filter((colour) => !grey(colour)), "labels not grey").toEqual([]);
    expect(look.cases.filter((textCase) => textCase !== "uppercase"), "labels not in capitals").toEqual([]);
  });
  await check("La primera línea del cambio es «ENTRY cd-throwaway-leftover», en negrita.", async () => {
    await expect(row.locator(".change-label").first()).toHaveText(/^entry$/i);
    await expect(part(dialog, "entry")).toHaveText("cd-throwaway-leftover");
    expect(Number(await part(dialog, "entry").locator("b").evaluate((b) => getComputedStyle(b).fontWeight))).toBeGreaterThanOrEqual(600);
  });
  await check("Luego sale «AGENT Claude Code».", async () => {
    await expect(row.locator(".change-label").nth(1)).toHaveText(/^agent$/i);
    await expect(part(dialog, "agent")).toHaveText("Claude Code");
  });
  await check("Luego sale «KEY projects.E:/cd-throwaway/project-gone.mcpServers.cd-throwaway-leftover», con la clave en gris y monoespaciada.", async () => {
    await expect(row.locator(".change-label").nth(2)).toHaveText(/^key$/i);
    await expect(key).toHaveText(new RegExp(`^${anySlash("projects.E:/cd-throwaway/project-gone.mcpServers.cd-throwaway-leftover")}$`));
    const look = await key.evaluate((k) => ({ colour: getComputedStyle(k).color, font: getComputedStyle(k).fontFamily }));
    expect(grey(look.colour), look.colour).toBe(true);
    expect(look.font).toMatch(/mono|consol/i);
  });
  await check("Al final sale «STATE Unread → Removed».", async () => {
    await expect(row.locator(".change-label").last()).toHaveText(/^state$/i);
    await expect(part(dialog, "state")).toHaveText(/^Unread\s*→\s*Removed$/);
  });
  await check("Hay espacio entre las líneas.", async () => {
    const gaps = await row.evaluate((r) => {
      const lines = [...r.querySelectorAll("dd")].map((dd) => dd.getBoundingClientRect());
      return lines.slice(1).map((line, at) => Math.round(line.top - lines[at].bottom));
    });
    expect(gaps.length).toBeGreaterThan(0);
    expect(gaps.filter((gap) => gap < 3), `gaps ${gaps}`).toEqual([]);
  });
  await check("Sobre la clave sale un tooltip con el locator entero, que empieza por «E:\\cd-throwaway\\claude-code-home\\.claude.json#».", async () => {
    await page.mouse.move(600, 4);
    await key.hover();
    await expect(page.getByRole("tooltip")).toHaveText(/^E:\\cd-throwaway\\claude-code-home\\\.claude\.json#.+cd-throwaway-leftover/);
  });
  await check("Al copiarlo sale el aviso «Copied the locator.».", async () => {
    await dialog.getByRole("button", { name: "Copy the locator" }).click();
    await expect(page.locator(".notice", { hasText: "Copied the locator." })).toHaveText("Copied the locator.");
  });
  await check("Con la ventana estrecha, la clave pasa a la línea siguiente tras un punto o una barra, nunca en medio de una palabra.", async () => {
    await page.setViewportSize(SMALLEST);
    // The smallest window holds the key on one line: the dialog is narrowed
    // as a narrower window would lay it out.
    await dialog.evaluate((d) => (d.style.maxWidth = "420px"));
    const wrap = await key.evaluate((k) => {
      const pieces = [...k.querySelectorAll(".change-key-piece")];
      return {
        lines: new Set(pieces.map((piece) => Math.round(piece.getBoundingClientRect().top))).size,
        split: pieces.filter((piece) => piece.getClientRects().length > 1).map((piece) => piece.textContent),
        ends: pieces.slice(0, -1).map((piece) => piece.textContent!.at(-1)),
      };
    });
    expect(wrap.lines, "the key's lines").toBeGreaterThan(1);
    expect(wrap.split, "pieces broken inside").toEqual([]);
    expect(wrap.ends.filter((end) => end !== "." && end !== "/"), "a break after neither a dot nor a slash").toEqual([]);
  });
});

// At 1100 × 640 with Before you apply shown, the dialog scrolls as a whole and its buttons leave the view (#378).
uatStep("7.4", async ({ page, open, check }) => {
  await page.setViewportSize(SMALLEST);
  await open();
  await inventoryShown(page);
  const dialog = await removeLeftovers(page, "E:/cd-throwaway/many/project-gone");
  const section = (title: string) => dialog.locator(".dialog-section", { has: page.locator(".dialog-section-title", { hasText: new RegExp(`^${title}`) }) });
  const changes = section("What changes").locator(".dialog-section-body");
  const files = section("Files").locator(".dialog-section-body");
  const before = section("Before you apply");
  const fixed = [dialog.locator(".dialog-title"), dialog.locator(".dialog-backup"), ...["Cancel", "Remove 12 leftovers"].map((name) => dialog.locator(".dialog-foot").getByRole("button", { name }))];
  const tops = () => Promise.all([...fixed, before].map(async (each) => ((await each.count()) > 0 ? (await each.first().boundingBox())?.y : null)));
  const stillTops = await tops();

  await check("El diálogo cabe en la ventana.", async () => {
    const box = await dialog.boundingBox();
    expect(box!.y).toBeGreaterThanOrEqual(0);
    expect(box!.y + box!.height).toBeLessThanOrEqual(SMALLEST.height);
    expect(box!.x + box!.width).toBeLessThanOrEqual(SMALLEST.width);
    const [scroll, client] = await dialog.evaluate((d) => [d.scrollHeight, d.clientHeight]);
    expect(scroll, "the dialog scrolls as a whole").toBeLessThanOrEqual(client + 1);
  });
  await check("«What changes» y «Files» enseñan cada uno su título y parte de lo que tienen.", async () => {
    for (const [title, body] of [["What changes", changes], ["Files", files]] as const) {
      await expect(section(title).locator(".dialog-section-title")).toBeInViewport();
      expect((await body.boundingBox())!.height, `${title}'s body`).toBeGreaterThan(30);
      await expect(body).toBeInViewport();
    }
  });
  await check("Al desplazarte dentro de uno, solo se mueve su contenido, bajo su título.", async () => {
    const title = section("What changes").locator(".dialog-section-title");
    const titleTop = (await title.boundingBox())!.y;
    const filesTop = await files.evaluate((f) => f.scrollTop);
    await changes.hover();
    await page.mouse.wheel(0, 400);
    await expect.poll(() => changes.evaluate((c) => c.scrollTop)).toBeGreaterThan(0);
    expect(await files.evaluate((f) => f.scrollTop), "Files scrolled").toBe(filesTop);
    expect((await title.boundingBox())!.y, "What changes' title moved").toBe(titleTop);
    expect(await tops(), "the dialog's title, backup line or buttons moved").toEqual(stillTops);
  });
  await check("«What changes» llega abajo, al cambio de «cd-throwaway-gone-12».", async () => {
    await changes.hover();
    await page.mouse.wheel(0, 20_000);
    // The block is shorter than one change, so its bottom shows the end of the last change.
    const last = changes.locator(".change-row").last();
    await expect(last.locator(".change-item")).toHaveText("cd-throwaway-gone-12");
    await expect.poll(() => changes.evaluate((c) => c.scrollHeight - c.scrollTop - c.clientHeight)).toBeLessThanOrEqual(1);
    const [row, body] = await Promise.all([last.boundingBox(), changes.boundingBox()]);
    expect(row!.y + row!.height, "the last change's bottom").toBeLessThanOrEqual(body!.y + body!.height + 1);
    expect(row!.y + row!.height, "the last change's bottom").toBeGreaterThan(body!.y);
  });
  await check("El título «Remove 12 Entries», la línea gris del backup y los botones «Cancel» y «Remove 12 leftovers» se quedan quietos y siempre a la vista.", async () => {
    await files.hover();
    await page.mouse.wheel(0, 20_000);
    await expect.poll(() => files.evaluate((f) => f.scrollTop)).toBeGreaterThan(0);
    await expect(fixed[0]).toHaveText("Remove 12 Entries");
    expect(grey(await fixed[1].evaluate((line) => getComputedStyle(line).color)), "the backup line's colour").toBe(true);
    await expect(dialog.locator(".dialog-foot button")).toHaveText(["Cancel", "Remove 12 leftovers"]);
    expect(await tops(), "after scrolling both blocks").toEqual(stillTops);
    for (const each of fixed) await expect(each).toBeInViewport({ ratio: 1 });
  });
  await check("Cuando sale, el bloque «Before you apply» también se queda quieto y a la vista, entre «What changes» y «Files».", async () => {
    if ((await before.count()) === 0) return;
    const [box, changesBox, filesTitle] = await Promise.all([before.boundingBox(), changes.boundingBox(), section("Files").locator(".dialog-section-title").boundingBox()]);
    expect(box!.y).toBeGreaterThanOrEqual(changesBox!.y + changesBox!.height - 1);
    expect(box!.y + box!.height).toBeLessThanOrEqual(filesTitle!.y + 1);
    await expect(before).toBeInViewport({ ratio: 1 });
  });
  await check("Con el foco del teclado, el bloque «What changes» tiene un anillo violeta.", async () => {
    await changes.evaluate((c) => (c.scrollTop = 0));
    await dialog.locator(".dialog-title").click();
    for (let presses = 0; presses < 20 && !(await changes.evaluate((c) => c === document.activeElement)); presses++) await page.keyboard.press("Tab");
    await expect(changes).toBeFocused();
    const ring = await changes.evaluate((c) => ({ style: getComputedStyle(c).outlineStyle, colour: getComputedStyle(c).outlineColor, shadow: getComputedStyle(c).boxShadow }));
    expect((ring.style !== "none" && violet(ring.colour)) || violet(ring.shadow), JSON.stringify(ring)).toBe(true);
  });
  await check("Con el foco, «What changes» se desplaza con la flecha abajo.", async () => {
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ArrowDown");
    await expect.poll(() => changes.evaluate((c) => c.scrollTop)).toBeGreaterThan(0);
  });
}, { bug: "#378" });
