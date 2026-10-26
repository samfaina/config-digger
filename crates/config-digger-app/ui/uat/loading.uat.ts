// Section 15: the loading states of the first read and of a read again. The
// fake backend answers at once, so each step holds the read's answer with the
// page's delay parameter while it looks at the state.
import { expect } from "@playwright/test";
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
