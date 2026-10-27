// Section 10: the Projects & homes screen, on the fixtures. They were recorded
// after the throwaway data's walk, so project-hide is hidden and
// claude-code-many-home is added, and a write changes nothing: the checks of
// a step that need the state a real write leaves are left to the WebDriver
// walk's tester.
import { expect, type Locator, type Page } from "@playwright/test";
import { SMALLEST } from "../scripts/fakeUi.ts";
import { group, inventoryShown, NO_WRITE, openGroup, rail, ROOT, row, whole, wrapsAtSeparators } from "./app";
import { uatStep } from "./step";

/** Why the checks of project-gone's Inventory stay with a person. */
const NO_LEFTOVERS =
  "the fixtures hold no inventory of project-gone, so the fake backend answers that of no Project, which has no Unread Item and no leftover";

/** The Projects tab, as groups: each heading's text, whether it is open, and its rows. */
function projectGroups(page: Page) {
  return page.locator(".places-list .scroll").evaluate((list) => {
    const groups: { title: string; open: boolean; rows: { name: string; sub: string; count: string | null }[] }[] = [];
    for (const el of list.children) {
      if (el.matches("h3.place-group")) {
        const toggle = el.querySelector(".place-group-toggle");
        groups.push({ title: el.textContent!.trim(), open: toggle ? toggle.getAttribute("aria-expanded") === "true" : true, rows: [] });
      } else if (el.matches(".place-row") && groups.length > 0) {
        groups.at(-1)!.rows.push({
          name: el.querySelector(".place-name")!.textContent!.trim(),
          sub: el.querySelector(".place-sub")!.textContent!.trim(),
          count: el.querySelector(".place-count")?.textContent!.trim() ?? null,
        });
      }
    }
    return groups;
  });
}

/** How many lines of text `element` takes up. */
function lines(element: Locator): Promise<number> {
  return element.evaluate((el) => {
    const range = document.createRange();
    range.selectNodeContents(el);
    return new Set([...range.getClientRects()].filter((rect) => rect.width > 0).map((rect) => Math.round(rect.top))).size;
  });
}

/** Opens the Projects tab of Projects & homes and waits for its rows. */
async function projectsTab(page: Page) {
  await rail(page, "Projects & homes");
  await page.getByRole("tab", { name: /^Projects · / }).click();
  await page.locator(".places .scroll .place-row").first().waitFor();
}

uatStep("10.1", async ({ page, open, check }) => {
  await open();
  await inventoryShown(page);
  const icon = page.getByRole("button", { name: "Projects & homes", exact: true });
  await icon.hover();

  await check("El tooltip dice «Projects & homes».", async () => {
    await expect(page.getByRole("tooltip")).toHaveText("Projects & homes");
  });
  await icon.click();
  await check("Arriba salen las pestañas «Projects · N» y «Agent homes · N».", async () => {
    await expect(page.getByRole("tab")).toHaveText([/^Projects · \d+$/, /^Agent homes · \d+$/]);
  });
  await check("El botón de arriba es «Add Project» o «Add Agent home», según la pestaña abierta.", async () => {
    const add = page.locator(".places header .cta");
    await page.getByRole("tab", { name: /^Projects · / }).click();
    await expect(add).toHaveText("Add Project");
    await page.getByRole("tab", { name: /^Agent homes · / }).click();
    await expect(add).toHaveText("Add Agent home");
  });
});

uatStep("10.5", async ({ page, open, check, leave }) => {
  await open();
  await projectsTab(page);
  // project-hide is hidden in the fixtures: its row shows in the Hidden group.
  await openGroup(page, "Hidden");
  const groups = await projectGroups(page);
  const rows = groups.flatMap((each) => each.rows);
  const missing = groups.find((each) => /^▾? ?Missing · \d+$/.test(each.title))!;

  await check("Salen «E:\\cd-throwaway\\project» y «E:\\cd-throwaway\\project-hide».", async () => {
    expect(rows.map((each) => each.name)).toEqual(expect.arrayContaining([`${ROOT}\\project`, `${ROOT}\\project-hide`]));
  });
  leave(
    "Cada uno tiene la línea «Folder · Known to Claude Code».",
    "the fixtures have project-hide hidden, and a hidden row says only «Known to Claude Code»",
  );
  await check("«E:\\cd-throwaway\\codex-home» y «E:\\cd-throwaway\\paths-that-wrap\\in-the-smallest-window\\of-config-digger\\claude-desktop-home» no salen como Projects.", async () => {
    expect(rows.map((each) => each.name)).not.toContain(`${ROOT}\\codex-home`);
    expect(rows.map((each) => each.name)).not.toContain(`${ROOT}\\paths-that-wrap\\in-the-smallest-window\\of-config-digger\\claude-desktop-home`);
  });
  leave(
    "El grupo «▾ Missing · 1» está abierto.",
    "the fixtures add claude-code-many-home, whose missing Project makes the group «Missing · 2»; the walk checks it on fresh data",
  );
  await check("En ese grupo sale «E:/cd-throwaway/project-gone», con la línea «The folder no longer exists · known to Claude Code» y «1 leftover».", async () => {
    expect(missing.open).toBe(true);
    const gone = missing.rows.find((each) => whole("E:/cd-throwaway/project-gone").test(each.name));
    expect(gone).toMatchObject({ sub: "The folder no longer exists · known to Claude Code", count: "1 leftover" });
  });
  await check("«E:/cd-throwaway/project-deleted» no sale en el grupo «Missing · 1».", async () => {
    expect(missing.rows.filter((each) => whole("E:/cd-throwaway/project-deleted").test(each.name))).toEqual([]);
  });
  await check("Tras Missing sale el grupo «▸ Missing, no leftovers · 1», cerrado.", async () => {
    expect(groups[groups.indexOf(missing) + 1]).toMatchObject({ title: "▸ Missing, no leftovers · 1", open: false });
  });
});

uatStep("10.6", async ({ page, open, check, leave }) => {
  await open();
  await projectsTab(page);
  await row(page, "E:/cd-throwaway/project-gone").getByRole("button", { name: "Show leftovers" }).click();

  await check("Se abre el Inventario.", async () => {
    await expect(page.locator(".inventory")).toBeVisible();
    await expect(page.locator(".inventory .loading")).toHaveCount(0);
  });
  await check("El campo Project dice «E:/cd-throwaway/project-gone».", async () => {
    await expect(page.getByRole("combobox", { name: "Project" })).toHaveText(whole("E:/cd-throwaway/project-gone"));
  });
  await check("Está elegido «All Agents».", async () => {
    await expect(page.locator(".inventory .banner-title")).toHaveText("All Agents");
  });
  leave("El filtro de State es «Unread».", NO_LEFTOVERS);
  leave("La lista de Items muestra «cd-throwaway-leftover».", NO_LEFTOVERS);
});

uatStep("10.14", async ({ page, open, check, leave }) => {
  await open({ delay: { read_inventory: 3000, switch_project: 3000 } });
  await projectsTab(page);
  await row(page, "E:/cd-throwaway/project-gone").getByRole("button", { name: "Show leftovers" }).click();
  const loading = page.locator(".inventory .loading");
  await loading.waitFor();

  await check("Nada más pulsar, antes que los paneles, el Inventario ocupa su cuerpo con un único panel, aunque dure menos de un segundo.", async () => {
    await expect(page.locator(".inventory > *")).toHaveCount(2);
    await expect(page.locator(".inventory > :nth-child(2)")).toHaveClass("loading");
  });
  await check("Arriba del panel, un aviso con una rueda que gira dice «Reading the inventory…».", async () => {
    await expect(loading.locator(".notice.busy")).toHaveText("Reading the inventory…");
    await expect(loading.locator(".notice.busy .spinner")).not.toHaveCSS("animation-name", "none");
    const [busy, rows] = [await loading.locator(".notice.busy").boundingBox(), await loading.locator(".placeholder-rows").boundingBox()];
    expect(busy!.y + busy!.height).toBeLessThanOrEqual(rows!.y + 1);
  });
  await check("Debajo laten filas grises con la forma de las filas de Items.", async () => {
    const placeholder = loading.locator(".placeholder-row").first();
    await expect(placeholder).toBeVisible();
    await expect(placeholder).not.toHaveCSS("animation-name", "none");
    // An Item row's shape: an icon beside a name and a shorter line under it.
    await expect(placeholder.locator(".placeholder-icon")).toBeVisible();
    await expect(placeholder.locator(".placeholder-line")).toHaveCount(2);
  });
  await check("Bajo la cabecera no salen los paneles de Agents, Items y Details, ni vacíos ni con sus cuentas a 0.", async () => {
    await expect(page.locator(".inventory .panel-title")).toHaveCount(0);
  });
  leave("Al acabar la lectura, el panel deja sitio a los paneles, con «cd-throwaway-leftover» en la lista de Items.", NO_LEFTOVERS);
});

uatStep("10.10", async ({ page, open, check }) => {
  await page.setViewportSize(SMALLEST);
  await open();
  await inventoryShown(page);
  await rail(page, "Projects & homes");
  const tabs = [
    {
      name: "Projects",
      add: "Add Project",
      about: "Projects come from the Agents' own registries plus the folders you add. config-digger never scans the disk.",
    },
    {
      name: "Agent homes",
      add: "Add Agent home",
      about: "Each Agent reads its user configuration from one or more homes. Entries in different homes never compete.",
    },
  ];
  const each = async (assert: (tab: (typeof tabs)[number]) => Promise<void>) => {
    for (const tab of tabs) {
      await page.getByRole("tab", { name: new RegExp(`^${tab.name} · `) }).click();
      await assert(tab);
    }
  };
  const about = page.locator(".places-about");

  await check("En las dos pestañas, el título «Projects & homes» ocupa una línea.", () =>
    each(async () => {
      await expect(page.locator(".places .banner-title")).toHaveText("Projects & homes");
      expect(await lines(page.locator(".places .banner-title"))).toBe(1);
    }),
  );
  await check("Cada pestaña, «Projects · N» y «Agent homes · N», ocupa una línea.", () =>
    each(async () => {
      for (const tab of await page.getByRole("tab").all()) expect(await lines(tab), (await tab.textContent())!).toBe(1);
    }),
  );
  await check("El botón «Add Project» o «Add Agent home» ocupa una línea.", () =>
    each(async ({ add }) => {
      const button = page.locator(".places header .cta");
      await expect(button).toHaveText(add);
      expect(await lines(button)).toBe(1);
    }),
  );
  await check("La descripción sale encima de la lista, no en la cabecera.", () =>
    each(async () => {
      expect(await page.locator(".places header .places-about").count()).toBe(0);
      const header = (await page.locator(".places header").boundingBox())!;
      const [text, list] = [(await about.boundingBox())!, (await page.locator(".places-list .scroll").boundingBox())!];
      expect(text.y).toBeGreaterThanOrEqual(header.y + header.height);
      expect(text.y + text.height).toBeLessThanOrEqual(list.y + 1);
    }),
  );
  await check(
    "En Projects, la descripción dice «Projects come from the Agents' own registries plus the folders you add. config-digger never scans the disk.».",
    async () => {
      await page.getByRole("tab", { name: /^Projects · / }).click();
      await expect(about).toHaveText(tabs[0].about);
    },
  );
  await check(
    "En Agent homes, la descripción dice «Each Agent reads its user configuration from one or more homes. Entries in different homes never compete.».",
    async () => {
      await page.getByRole("tab", { name: /^Agent homes · / }).click();
      await expect(about).toHaveText(tabs[1].about);
    },
  );
  await check("En Projects, cada ruta que ocupa más de una línea se parte justo después de una «\\», nunca dentro del nombre de una carpeta.", async () => {
    await page.getByRole("tab", { name: /^Projects · / }).click();
    await row(page, String.raw`${ROOT}\paths-that-wrap\in-the-smallest-window\of-config-digger\project-long`).scrollIntoViewIfNeeded();
    await wrapsAtSeparators(page.locator(".places-list .place-name.mono"));
  });
  await check(
    "En Agent homes, cada carpeta o nombre de home que ocupa más de una línea se parte justo después de una «\\», nunca dentro del nombre de una carpeta.",
    async () => {
      await page.getByRole("tab", { name: /^Agent homes · / }).click();
      await wrapsAtSeparators(page.locator(".places-list .place-name, .places-list .place-sub.mono"));
    },
  );
  await check("Ninguna ruta se sale de su fila ni hace que la página se desplace de lado.", () =>
    each(async () => {
      const overflow = await page.locator(".places-list .place-row").evaluateAll((rows) =>
        rows.flatMap((row) => {
          const edge = row.getBoundingClientRect().right;
          return [...row.querySelectorAll(".mono")].filter((path) => path.getBoundingClientRect().right > edge + 1).map((path) => path.textContent);
        }),
      );
      expect(overflow).toEqual([]);
      expect(await page.evaluate(() => document.scrollingElement!.scrollWidth <= innerWidth)).toBe(true);
    }),
  );
});

uatStep("10.15", async ({ page, open, check, leave }) => {
  await open();
  await projectsTab(page);
  const search = page.getByRole("textbox", { name: "Search Projects" });
  const hide = `${ROOT}\\project-hide`;
  // project-hide is already hidden in the fixtures, as Hide leaves it.

  await search.click();
  await page.keyboard.type("HIDE");
  await openGroup(page, "Hidden");
  await check("Con HIDE, el grupo de Projects dice «Projects · 0».", async () => {
    await expect(page.locator("h3.place-group").first()).toHaveText("Projects · 0");
  });
  await check("Con HIDE, la fila «No Project» no sale.", async () => {
    await expect(row(page, "No Project")).toHaveCount(0);
  });
  await check("Con HIDE, el grupo «Hidden · 1» muestra «E:\\cd-throwaway\\project-hide».", async () => {
    const groups = await projectGroups(page);
    expect(groups.find((each) => each.title === "▾ Hidden · 1")?.rows.map((each) => each.name)).toEqual([hide]);
  });

  await search.fill("gone");
  await check("Con gone, el grupo de Projects dice «Projects · 0».", async () => {
    await expect(page.locator("h3.place-group").first()).toHaveText("Projects · 0");
  });
  await check("Con gone, el grupo «Hidden» no sale.", async () => {
    await expect(group(page, "Hidden")).toHaveCount(0);
  });
  leave(
    "Con gone, el grupo «Missing, no leftovers · 1» se abre solo.",
    `project-gone joins that group once step 10.7 removes its leftover; ${NO_WRITE}`,
  );
  leave(
    "Con gone, ese grupo muestra solo «E:/cd-throwaway/project-gone», sin leftovers desde el paso 10.7.",
    `it needs step 10.7's Remove; ${NO_WRITE}`,
  );

  await search.fill("zz");
  await check("Con zz, la lista dice «No Project path holds “zz”.».", async () => {
    await expect(page.locator(".places .scroll .empty")).toHaveText("No Project path holds “zz”.");
  });

  await search.fill("");
  await check("Al borrar, vuelven todas las filas y «No Project».", async () => {
    await expect(row(page, "No Project")).toBeVisible();
    await expect(row(page, `${ROOT}\\project`)).toBeVisible();
    await expect(row(page, `${ROOT}\\project-extra`)).toBeVisible();
    await expect(row(page, "E:/cd-throwaway/project-gone")).toBeVisible();
    await expect(group(page, "Hidden")).toBeVisible();
  });
  leave("Unhide devuelve project-hide a la lista principal.", NO_WRITE);

  await page.getByRole("tab", { name: /^Agent homes · / }).click();
  await check("La pestaña Agent homes no tiene caja de búsqueda.", async () => {
    await expect(page.locator(".places .agent-group").first()).toBeVisible();
    await expect(page.getByRole("textbox", { name: "Search Projects" })).toHaveCount(0);
  });
});
