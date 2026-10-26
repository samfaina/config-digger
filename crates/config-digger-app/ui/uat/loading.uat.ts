// Section 15: the loading states of the first read and of a read again. The
// fake backend answers at once, so each step holds the read's answer with the
// page's delay parameter while it looks at the state.
import { expect, type Page } from "@playwright/test";
import { SMALLEST } from "../scripts/fakeUi.ts";
import { inventoryShown, rail } from "./app";
import { uatStep } from "./step";

uatStep("15.1", async ({ page, open, check }) => {
  await open({ delay: { read_inventory: 3000 } });
  const loading = page.locator(".inventory .loading");
  await loading.waitFor();

  await check("Antes de que salgan los paneles, el cuerpo es un único panel.", async () => {
    await expect(page.locator(".inventory > *")).toHaveCount(2);
    await expect(page.locator(".inventory > :nth-child(2)")).toHaveClass("loading");
  });
  await check("En ese panel, un aviso con una rueda que gira dice «Reading the inventory…».", async () => {
    await expect(loading.getByRole("status")).toHaveText("Reading the inventory…");
    await expect(loading.locator(".spinner")).not.toHaveCSS("animation-name", "none");
  });
  await check("Bajo el aviso laten filas grises.", async () => {
    await expect(loading.locator(".placeholder-row").first()).toBeVisible();
    await expect(loading.locator(".placeholder-row").first()).not.toHaveCSS("animation-name", "none");
  });
  await check("Luego salen los paneles de «Agents», «Items» y «Details» con tus datos.", async () => {
    for (const title of ["Agents", "Items", "Details"]) await expect(page.locator(".panel-title", { hasText: new RegExp(`^${title}`) })).toBeVisible();
    await expect(page.locator(".item-row").first()).toBeVisible();
  });
});

uatStep("15.2", async ({ page, open, check }) => {
  await open({ delay: { read_inventory: 2500 } });
  await inventoryShown(page);
  const refresh = page.locator("header.banner .cta", { hasText: /^(Refresh|Reading)$/ });
  const bar = page.locator("header.banner .read-progress");
  await refresh.click();

  await check("Con Refresh, el botón dice «Reading».", async () => {
    await expect(refresh).toHaveText("Reading");
  });
  await check("Mientras el botón dice «Reading», por el borde de abajo de la cabecera corre una barra fina de lado a lado, aunque dure menos de un segundo.", async () => {
    await expect(bar).toBeVisible();
    await expect(refresh).toHaveText("Reading");
    await runsUnderTheHeader(page);
  });
  await check("Los Items siguen a la vista y el clic abre el Item en Details.", async () => {
    const name = (await page.locator(".item-row .item-name").first().textContent())!.trim();
    await page.locator(".item-row").first().click();
    await expect(page.locator(".panel.detail")).toContainText(name);
    await expect(bar).toBeVisible();
  });
  await expect(bar).toHaveCount(0);
  // The tester comes back to the window: the app reads again on the window's focus event.
  await page.evaluate(() => window.dispatchEvent(new FocusEvent("focus")));
  await check("Al volver a la ventana sale la misma barra.", async () => {
    await expect(bar).toBeVisible();
    await runsUnderTheHeader(page);
  });
  await check("Al volver a la ventana no sale el estado de carga de la primera lectura.", async () => {
    await expect(bar).toBeVisible();
    await expect(page.locator(".inventory .loading")).toHaveCount(0);
    await expect(page.locator(".item-row").first()).toBeVisible();
  });
});

/** The read-again bar: thin, along the header's bottom edge from side to side, and running. */
async function runsUnderTheHeader(page: Page) {
  const bar = page.locator("header.banner .read-progress");
  const [b, h] = [(await bar.boundingBox())!, (await page.locator("header.banner").boundingBox())!];
  expect(b.height).toBeLessThanOrEqual(6);
  expect(b.x - h.x).toBeLessThanOrEqual(16);
  expect(h.x + h.width - (b.x + b.width)).toBeLessThanOrEqual(16);
  expect(Math.abs(h.y + h.height - (b.y + b.height))).toBeLessThanOrEqual(16);
  await expect(bar.locator("span")).not.toHaveCSS("animation-name", "none");
}

uatStep("15.3", async ({ page, open, check }) => {
  await page.setViewportSize(SMALLEST);
  // Each screen reads once it opens; the inventory answers at once.
  await open({ delay: { read_places: 3000, read_history: 3000, read_settings: 3000 } });
  await inventoryShown(page);
  const screens = [
    { screen: "Projects & homes", text: "Reading the Projects and Agent homes…", data: page.locator(".places-list .place-row").first() },
    { screen: "History · Undo", text: "Reading History…", data: page.locator(".operation-row").first() },
    { screen: "Settings", text: "Reading where config-digger keeps its data…", data: page.locator(".settings-data .place-row").first() },
  ];
  const seen: ({ screen: string } & Awaited<ReturnType<typeof loadingState>>)[] = [];
  const historyTitle = page.locator(".history .banner-title");
  let titleWhileReading = "";
  for (const { screen } of screens) {
    await rail(page, screen);
    seen.push({ screen, ...(await loadingState(page)) });
    if (screen === "History · Undo") titleWhileReading = (await historyTitle.textContent())!;
  }

  await check("Cada lista empieza con un aviso con una rueda que gira y filas grises que laten debajo, aunque dure menos de un segundo.", async () => {
    for (const state of seen) {
      expect(state.spinner, state.screen).not.toBe("none");
      expect(state.rows, state.screen).toBeGreaterThan(0);
      expect(state.rowAnimation, state.screen).not.toBe("none");
      expect(state.rowsBelow, state.screen).toBe(true);
    }
  });
  for (const [at, text] of [
    [0, "En Projects & homes, el aviso dice «Reading the Projects and Agent homes…»."],
    [1, "En History, el aviso dice «Reading History…»."],
    [2, "En Settings, el aviso dice «Reading where config-digger keeps its data…»."],
  ] as const) {
    await check(text, async () => {
      expect(seen[at].text).toBe(screens[at].text);
    });
  }
  await check("En History, mientras sale el aviso, el título de la cabecera dice «Operations», sin ningún número.", async () => {
    expect(titleWhileReading).toBe("Operations");
  });
  await check("Luego salen sus datos.", async () => {
    for (const { screen, data } of screens) {
      await rail(page, screen);
      await expect(data, screen).toBeVisible({ timeout: 10_000 });
      await expect(page.locator("main .loading"), screen).toHaveCount(0);
    }
  });
  await check("Cuando salen los datos de History, su título dice cuántas Operations hay, con el número delante, como «3 Operations».", async () => {
    await rail(page, "History · Undo");
    await expect(page.locator(".operation-row").first()).toBeVisible();
    await expect(historyTitle).toHaveText(`${await page.locator(".operation-row").count()} Operations`);
  });
  await check("En ningún momento sale una línea de texto suelta en lugar del aviso.", async () => {
    for (const state of seen) expect(state.stray, state.screen).toEqual([]);
  });
});

/** What the screen's loading block shows: its notice's text, its spinner and grey rows, and any line of text beside it. */
async function loadingState(page: Page) {
  const loading = page.locator("main .loading");
  await loading.waitFor();
  return loading.evaluate((block) => {
    const notice = block.querySelector(".notice.busy")!;
    const rows = [...block.querySelectorAll(".placeholder-row")];
    return {
      text: notice.textContent!.trim(),
      spinner: getComputedStyle(notice.querySelector(".spinner")!).animationName,
      rows: rows.length,
      rowAnimation: rows.length > 0 ? getComputedStyle(rows[0]).animationName : "none",
      rowsBelow: rows.length > 0 && notice.getBoundingClientRect().bottom <= rows[0].getBoundingClientRect().top + 1,
      stray: [...block.parentElement!.children].filter((each) => each !== block && each.matches("p") && each.textContent!.trim()).map((each) => each.textContent!.trim()),
    };
  });
}
