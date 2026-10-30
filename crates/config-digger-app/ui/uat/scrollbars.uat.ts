// Section 4: the app's scrollbars, and the Inventory's blocks in the smallest
// window. The thin bar is the app's ::-webkit-scrollbar rules, which no
// computed style of an element shows: the tests read the rules from the page's
// style sheets, measure the bar's width as the gutter it takes, and drag its
// thumb with the mouse.
import { expect, type Locator, type Page } from "@playwright/test";
import { SMALLEST } from "../scripts/fakeUi.ts";
import { away, inventoryShown, rail, ROOT, WRAPS } from "./app";
import { grey, lightness } from "./look";
import { uatStep } from "./step";

/** The home step 4.1 adds, which the fixtures hold. */
const MANY_HOME = String.raw`${ROOT}\claude-code-many-home`;

/** How a scrolling element scrolls: whether it overflows, its bar's width, its scroll and its box. */
function scrolling(scroller: Locator) {
  return scroller.evaluate((element: HTMLElement) => {
    const style = getComputedStyle(element);
    return {
      overflows: element.scrollHeight > element.clientHeight + 1,
      bar: element.offsetWidth - element.clientWidth - parseFloat(style.borderLeftWidth) - parseFloat(style.borderRightWidth),
      top: element.scrollTop,
      max: element.scrollHeight - element.clientHeight,
      // Chromium drops the ::-webkit-scrollbar rules of an element where either applies.
      standard: `${style.scrollbarWidth} ${style.scrollbarColor}`,
    };
  });
}

/** The colours the app's ::-webkit-scrollbar rules give the track, the thumb and the thumb under the mouse, as `rgb(…)`. */
function scrollbarRules(page: Page) {
  return page.evaluate(() => {
    const rules = [...document.styleSheets].flatMap((sheet) => [...sheet.cssRules]).filter((rule) => rule instanceof CSSStyleRule);
    const colour = (selector: string) => {
      const rule = rules.find((each) => each.selectorText.split(",").map((part) => part.trim()).includes(selector));
      if (rule === undefined) return null;
      const probe = document.createElement("span");
      probe.style.backgroundColor = rule.style.backgroundColor;
      document.body.append(probe);
      const resolved = getComputedStyle(probe).backgroundColor;
      probe.remove();
      return resolved;
    };
    return {
      track: colour("::-webkit-scrollbar-track"),
      thumb: colour("::-webkit-scrollbar-thumb"),
      hover: colour("::-webkit-scrollbar-thumb:hover"),
      panel: getComputedStyle(document.querySelector(".panel") ?? document.body).backgroundColor,
    };
  });
}

/** Drags the thumb of `scroller`'s bar from its top to below the bar's bottom. */
async function dragThumb(page: Page, scroller: Locator) {
  await scroller.evaluate((element) => (element.scrollTop = 0));
  const box = await scroller.boundingBox();
  if (box === null) throw new Error("The scrolling list has no box.");
  const x = box.x + box.width - 5;
  await page.mouse.move(x, box.y + 8);
  await page.mouse.down();
  await page.mouse.move(x, box.y + box.height + 40, { steps: 10 });
  await page.mouse.up();
}

/** The bar checks every step of section 4 makes of `scroller`, by the plan's texts. */
async function bar(
  page: Page,
  scroller: Locator,
  check: (text: string, assert: () => Promise<void>) => Promise<void>,
  texts: { thin: string; thumb: string; hover: string; drag: string },
) {
  await check(texts.thin, async () => {
    const shown = await scrolling(scroller);
    expect(shown.overflows).toBe(true);
    expect(shown.standard).toBe("auto auto");
    // The app's 10 px, not the system's 17 px; at a display scale of 125 % it measures 9.
    expect(shown.bar).toBeGreaterThanOrEqual(9);
    expect(shown.bar).toBeLessThanOrEqual(10);
    expect((await scrollbarRules(page)).track).toBe("rgba(0, 0, 0, 0)");
  });
  await check(texts.thumb, async () => {
    const { thumb, panel } = await scrollbarRules(page);
    expect(grey(thumb), thumb ?? "no thumb rule").toBe(true);
    expect(lightness(thumb)).toBeGreaterThan(lightness(panel));
    expect(lightness(thumb)).toBeLessThan(3 * 128);
  });
  await check(texts.hover, async () => {
    const { thumb, hover } = await scrollbarRules(page);
    expect(lightness(hover)).toBeGreaterThan(lightness(thumb));
  });
  await check(texts.drag, async () => {
    await dragThumb(page, scroller);
    await expect.poll(async () => {
      const { top, max } = await scrolling(scroller);
      return top >= max - 2;
    }).toBe(true);
  });
}

/** Opens the Inventory of Claude Code's claude-code-many-home, and returns its Items list. */
async function manyHome(page: Page): Promise<Locator> {
  await page.locator(".agent-tile", { hasText: /^Claude Code/ }).click();
  await page.locator(".home-tile", { has: page.locator(".home-name", { hasText: /claude-code-many-home$/ }) }).click();
  await expect(page.locator(".item-name", { hasText: /^cd-throwaway-many-01$/ })).toBeVisible();
  return page.locator(".panel.items .scroll");
}

/** The width of the Items list's bar on claude-code-many-home, which the other bars match. */
async function itemsBar(page: Page): Promise<number> {
  await rail(page, "Inventory");
  return (await scrolling(await manyHome(page))).bar;
}

/** Two bars are the same when their widths match within a pixel and the same rules draw them. */
async function sameBar(scroller: Locator, items: number) {
  const shown = await scrolling(scroller);
  expect(Math.abs(shown.bar - items)).toBeLessThanOrEqual(1);
  expect(shown.standard).toBe("auto auto");
}

uatStep("4.1", async ({ page, open, check }) => {
  await page.setViewportSize(SMALLEST);
  await open();
  await inventoryShown(page);
  const items = await manyHome(page);

  await bar(page, items, check, {
    thin: "La barra es fina, sin el carril gris claro del sistema.",
    thumb: "El tirador es gris tenue.",
    hover: "Bajo el ratón, el tirador se aclara.",
    drag: "El tirador se arrastra con normalidad.",
  });
  await check("Al final de la lista sale «cd-throwaway-many-30».", async () => {
    const last = items.locator(".item-name", { hasText: /^cd-throwaway-many-30$/ });
    const [list, row] = [await items.boundingBox(), await last.boundingBox()];
    expect(row!.y).toBeGreaterThanOrEqual(list!.y - 1);
    expect(row!.y + row!.height).toBeLessThanOrEqual(list!.y + list!.height + 1);
  });
});

uatStep("4.2", async ({ page, open, check }) => {
  await page.setViewportSize(SMALLEST);
  await open();
  await inventoryShown(page);
  const items = await itemsBar(page);
  await rail(page, "Projects & homes");
  await page.getByRole("tab", { name: /^Projects/ }).click();
  const list = page.locator(".places-list .scroll");

  await check("La lista tiene los 30 Projects de «E:\\cd-throwaway\\many» y no cabe entera.", async () => {
    // The 30 numbered ones, not many/project-gone, which is Missing.
    await expect(list.locator(".place-name", { hasText: /^E:\\cd-throwaway\\many\\project-\d\d$/ })).toHaveCount(30);
    expect((await scrolling(list)).overflows).toBe(true);
  });
  await bar(page, list, check, {
    thin: "La barra es fina, sin el carril gris claro del sistema.",
    thumb: "El tirador es gris tenue.",
    hover: "Bajo el ratón, el tirador se aclara.",
    drag: "El tirador se arrastra con normalidad.",
  });
  await check("La barra es igual a la de la lista de Items del Inventario.", () => sameBar(list, items));
});

uatStep("4.4", async ({ page, open, check }) => {
  await page.setViewportSize(SMALLEST);
  await open();
  await inventoryShown(page);
  const items = await itemsBar(page);
  await rail(page, "About");
  await page.getByRole("button", { name: "Third-party licenses" }).first().click();
  const dialog = page.getByRole("dialog");
  const list = dialog.locator(".licenses-list");
  await list.waitFor();
  const first = list.locator("details.license").first();
  await first.locator("summary").click();
  const text = first.locator("pre.license-text");

  await check("La lista de licencias y el texto de la licencia se desplazan cada uno con su barra.", async () => {
    expect((await scrolling(list)).overflows).toBe(true);
    expect((await scrolling(text)).overflows).toBe(true);
  });
  // One check names both bars: each of them meets it.
  const both = async (assert: (scroller: Locator) => Promise<void>) => {
    await assert(list);
    await assert(text);
  };
  await check("Cada barra es fina, sin el carril gris claro del sistema.", () =>
    both(async (scroller) => {
      const shown = await scrolling(scroller);
      expect(shown.standard).toBe("auto auto");
      expect(shown.bar).toBeGreaterThanOrEqual(9);
      expect(shown.bar).toBeLessThanOrEqual(10);
      expect((await scrollbarRules(page)).track).toBe("rgba(0, 0, 0, 0)");
    }),
  );
  await check("El tirador es gris tenue.", async () => {
    const { thumb, panel } = await scrollbarRules(page);
    expect(grey(thumb), thumb ?? "no thumb rule").toBe(true);
    expect(lightness(thumb)).toBeGreaterThan(lightness(panel));
  });
  await check("Bajo el ratón, el tirador se aclara.", async () => {
    const { thumb, hover } = await scrollbarRules(page);
    expect(lightness(hover)).toBeGreaterThan(lightness(thumb));
  });
  await check("El tirador se arrastra con normalidad.", () =>
    both(async (scroller) => {
      await scroller.scrollIntoViewIfNeeded();
      await dragThumb(page, scroller);
      await expect.poll(async () => {
        const { top, max } = await scrolling(scroller);
        return top >= max - 2;
      }).toBe(true);
    }),
  );
  await check("Las dos barras son iguales a la de la lista de Items del Inventario.", () => both((scroller) => sameBar(scroller, items)));
});

uatStep("4.5", async ({ page, open, check }) => {
  await page.setViewportSize(SMALLEST);
  await open();
  await inventoryShown(page);
  const items = await itemsBar(page);
  await page.getByRole("combobox", { name: "Project" }).click();
  const options = page.locator(".select-panel .select-list");
  await options.waitFor();

  await check("El panel no cabe entero en la ventana y se desplaza con su propia barra.", async () => {
    expect((await scrolling(options)).overflows).toBe(true);
    const panel = await page.locator(".select-panel").boundingBox();
    expect(panel!.y + panel!.height).toBeLessThanOrEqual(SMALLEST.height);
  });
  await bar(page, options, check, {
    thin: "La barra es fina, sin el carril gris claro del sistema.",
    thumb: "El tirador es gris tenue.",
    hover: "Bajo el ratón, el tirador se aclara.",
    drag: "El tirador se arrastra con normalidad.",
  });
  await check("La barra es igual a la de la lista de Items del Inventario.", () => sameBar(options, items));
  await page.keyboard.press("Escape");
});

/** A box on the page, by its edges. */
type Box = { left: number; top: number; right: number; bottom: number };
/** Whether two boxes share more than a pixel each way. */
const overlap = (a: Box, b: Box) => Math.min(a.right, b.right) - Math.max(a.left, b.left) > 1 && Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top) > 1;

/**
 * The boxes of the text the Inventory shows, each clipped by the blocks that
 * scroll or hide it, with its text node's number: a text cut with «…» has a
 * box for what shows and one for the whole text.
 */
function textBoxes(page: Page) {
  return page.evaluate(() => {
    const boxes: { node: number; text: string; left: number; top: number; right: number; bottom: number }[] = [];
    let nodes = 0;
    const walker = document.createTreeWalker(document.querySelector("main")!, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
      if (!node.textContent?.trim()) continue;
      nodes += 1;
      const range = document.createRange();
      range.selectNodeContents(node);
      for (const rect of range.getClientRects()) {
        let box = { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom };
        for (let parent = node.parentElement; parent !== null; parent = parent.parentElement) {
          const style = getComputedStyle(parent);
          if (style.visibility === "hidden" || style.display === "none") box = { ...box, right: box.left };
          if (style.overflowX !== "visible" || style.overflowY !== "visible") {
            const clip = parent.getBoundingClientRect();
            box = { left: Math.max(box.left, clip.left), top: Math.max(box.top, clip.top), right: Math.min(box.right, clip.right), bottom: Math.min(box.bottom, clip.bottom) };
          }
        }
        if (box.right - box.left > 1 && box.bottom - box.top > 1) boxes.push({ node: nodes, text: node.textContent.trim().slice(0, 40), ...box });
      }
    }
    return boxes;
  });
}

uatStep("4.6", async ({ page, open, check }) => {
  await page.setViewportSize(SMALLEST);
  await open();
  await inventoryShown(page);
  await page.locator(".agent-tile", { hasText: /^All Agents/ }).click();
  const blocks = { agents: page.locator(".panel.agents"), states: page.locator(".panel.states"), homes: page.locator(".panel.homes") };
  const rows = { agents: blocks.agents.locator(".scroll"), states: blocks.states.locator(".bars"), homes: blocks.homes.locator(".home-list") };
  /** How many of a block's rows show whole inside it. */
  const shownRows = async (block: Locator, list: Locator) =>
    list.evaluate(
      (element, panel) => {
        const box = panel.getBoundingClientRect();
        return [...element.children].filter((row) => {
          const own = row.getBoundingClientRect();
          return own.height > 0 && own.top >= box.top - 1 && own.bottom <= box.bottom + 1;
        }).length;
      },
      (await block.elementHandle())!,
    );

  await check("«Agents» muestra su título y al menos dos Agents.", async () => {
    await expect(blocks.agents.locator(".panel-title")).toHaveText("Agents");
    expect(await shownRows(blocks.agents, rows.agents)).toBeGreaterThanOrEqual(2);
  });
  await check("«Entries by State» y «Agent home» muestran cada uno su título y al menos una fila.", async () => {
    await expect(blocks.states.locator(".panel-title")).toHaveText(/^Entries by State/);
    await expect(blocks.homes.locator(".panel-title")).toHaveText("Agent home");
    expect(await shownRows(blocks.states, rows.states)).toBeGreaterThanOrEqual(1);
    expect(await shownRows(blocks.homes, rows.homes)).toBeGreaterThanOrEqual(1);
  });
  await check("Al desplazarte dentro de un bloque llegas a su última fila, y los otros dos bloques no se mueven.", async () => {
    const tops = async () => Promise.all(Object.values(blocks).map(async (block) => (await block.boundingBox())!.y));
    const before = await tops();
    for (const list of Object.values(rows)) {
      const box = (await list.boundingBox())!;
      await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
      await page.mouse.wheel(0, 2000);
      await expect
        .poll(() =>
          list.evaluate((element) => {
            const last = element.lastElementChild?.getBoundingClientRect();
            const box = element.getBoundingClientRect();
            return last === undefined || (last.top >= box.top - 1 && last.bottom <= box.bottom + 1);
          }),
        )
        .toBe(true);
      expect(await tops()).toEqual(before);
    }
  });
  await check("Cada fila del bloque Agent home cabe entera en él, también tras desplazarte.", async () => {
    const heights = await rows.homes.evaluate((list) => ({
      list: list.clientHeight,
      rows: [...list.querySelectorAll(".home-row")].map((row) => [row.textContent, row.getBoundingClientRect().height] as const),
    }));
    expect(heights.rows.length).toBeGreaterThanOrEqual(2);
    for (const [text, height] of heights.rows) expect(height, text!).toBeLessThanOrEqual(heights.list);
  });
  await check(
    String.raw`El home de Claude Desktop acaba entero en «\claude-desktop-home», y su tooltip muestra «E:\cd-throwaway\paths-that-wrap\in-the-smallest-window\of-config-digger\claude-desktop-home» entero.`,
    async () => {
      const name = blocks.homes.locator(".home-row", { hasText: /^Claude Desktop/ }).locator(".single-home");
      const tail = name.locator(".home-name-tail");
      await expect(tail).toHaveText(String.raw`\claude-desktop-home`);
      expect(await tail.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);
      // A scroll hides a tooltip, and the main thread may get the last scroll of the
      // blocks' wheels only after the hover: the hover is then made again.
      await expect(async () => {
        await away(page);
        await name.hover();
        await expect(page.getByRole("tooltip")).toHaveText(String.raw`${WRAPS}\claude-desktop-home`, { timeout: 2000 });
      }).toPass();
    },
  );
  await check("«Claude Code» y «claude-code-many-home» se eligen con un clic.", async () => {
    const agent = page.locator(".agent-tile", { hasText: /^Claude Code/ });
    await agent.click();
    await expect(agent).toHaveAttribute("aria-pressed", "true");
    const home = page.locator(".home-tile", { has: page.locator(".home-name", { hasText: /claude-code-many-home$/ }) });
    await home.click();
    await expect(home).toHaveAttribute("aria-pressed", "true");
  });
  // Each Claude Code tile's name: its parts' text, and whether each shows whole or is cut.
  const names = () =>
    blocks.homes.locator(".home-name").evaluateAll((names) =>
      names.map((name) => {
        const part = (css: string) => {
          const element = name.querySelector<HTMLElement>(css);
          return element && { text: element.textContent!, cut: element.scrollWidth > element.clientWidth, ellipsis: getComputedStyle(element).textOverflow };
        };
        return { head: part(".home-name-head"), tail: part(".home-name-tail")! };
      }),
    );
  const tiles = await names();
  // No Claude Code home of the fixtures is a Project's Last-opened one or has a
  // Read failure of its own, so each tile is given the tag and the badge as
  // HomesPane draws them, to read its name beside them.
  await blocks.homes.locator(".home-tile").evaluateAll((buttons) => {
    for (const button of buttons) button.insertAdjacentHTML("beforeend", '<span class="tag marked">last opened here</span><span class="badge marked">1</span>');
  });
  const marked = await names();
  await blocks.homes.locator(".marked").evaluateAll((marks) => marks.forEach((mark) => mark.remove()));
  await check("Cada ficha de Agent home muestra entero el final de su carpeta, como «claude-code-many-home» o «claude-code-synced-home», y ninguna se lee igual que otra.", async () => {
    expect(tiles.map(({ tail }) => tail.text)).toEqual(expect.arrayContaining([String.raw`\claude-code-shape-home`, String.raw`\claude-code-synced-home`, String.raw`\claude-code-many-home`]));
    for (const { tail } of [...tiles, ...marked]) expect(tail.cut, tail.text).toBe(false);
    expect(new Set(tiles.map(({ tail }) => tail.text)).size).toBe(tiles.length);
  });
  await check("Un nombre que no cabe en su ficha se corta con «…» en la parte de la carpeta que va antes de ese final.", async () => {
    const long = [...tiles, ...marked].filter(({ head }) => head !== null);
    expect(long).toHaveLength(6);
    for (const { head } of long) expect(head).toEqual({ text: ROOT, cut: true, ellipsis: "ellipsis" });
  });
  await check("Un nombre que cabe en su ficha, como «default» cuando sale, se ve entero, sin «…».", async () => {
    // The fixtures' default Claude Code home is named default.
    expect(tiles.find(({ tail }) => tail.text === "default")).toEqual({ head: null, tail: { text: "default", cut: false, ellipsis: "ellipsis" } });
  });

  const project = page.getByRole("combobox", { name: "Project" });
  await project.click();
  await page.getByRole("option", { name: String.raw`${ROOT}\many\project-01`, exact: true }).click();
  await expect(project).toHaveText(String.raw`${ROOT}\many\project-01`);
  await away(page);
  await project.hover();
  await check("El tooltip del campo Project muestra «E:\\cd-throwaway\\many\\project-01» entero.", async () => {
    const tooltip = page.getByRole("tooltip");
    await expect(tooltip).toHaveText(String.raw`${ROOT}\many\project-01`);
    expect(await tooltip.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);
  });

  const row = page.locator(".item-row", { has: page.locator(".item-name", { hasText: /^cd-throwaway-many-01$/ }) });
  await row.click();
  await page.locator(".entry-card").first().waitFor();

  await check("La lista de Items y el panel de la derecha tienen más o menos el mismo ancho.", async () => {
    const [items, detail] = [await page.locator(".panel.items").boundingBox(), await page.locator(".panel.detail").boundingBox()];
    expect(Math.abs(items!.width - detail!.width) / items!.width).toBeLessThan(0.2);
  });
  await check("La columna de la izquierda es algo más estrecha que cada uno de ellos.", async () => {
    const [column, items, detail] = await Promise.all([".inventory .column", ".panel.items", ".panel.detail"].map(async (css) => (await page.locator(css).boundingBox())!.width));
    expect(column).toBeLessThan(Math.min(items, detail));
  });
  await check("Ningún texto se monta sobre otro.", async () => {
    const boxes = await textBoxes(page);
    const overlapping = boxes.flatMap((a, at) => boxes.slice(at + 1).filter((b) => b.node !== a.node && overlap(a, b)).map((b) => `«${a.text}» y «${b.text}»`));
    expect(overlapping).toEqual([]);
  });
  const part = async (css: string) => (await row.locator(css).boundingBox())!;
  await check("En cada fila de Items, el nombre y, debajo, la ubicación van junto al icono.", async () => {
    const [icon, name, location] = [await part(".item-icon"), await part(".item-name"), await part(".item-location")];
    expect(name.x - (icon.x + icon.width)).toBeLessThan(24);
    expect(location.y).toBeGreaterThan(name.y);
    expect(Math.abs(location.x - name.x)).toBeLessThan(2);
  });
  await check("El número de Entries y el State van en otra línea, bajo el nombre y la ubicación.", async () => {
    const [location, side] = [await part(".item-location"), await part(".item-side")];
    expect(side.y).toBeGreaterThanOrEqual(location.y + location.height - 2);
  });
  await check("La ubicación acaba en «…» y su tooltip muestra la ruta entera.", async () => {
    const location = row.locator(".item-location");
    await expect(location).toHaveCSS("text-overflow", "ellipsis");
    expect(await location.evaluate((element) => element.scrollWidth > element.clientWidth)).toBe(true);
    await away(page);
    await location.hover();
    await expect(page.getByRole("tooltip")).toHaveText(new RegExp(`^${MANY_HOME.replaceAll("\\", "\\\\")}\\\\`));
  });
});
