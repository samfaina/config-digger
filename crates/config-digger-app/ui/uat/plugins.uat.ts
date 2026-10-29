// Section 27: a Cursor plugin whose first manifest does not parse, which
// Cursor reads past to the next one.
import { expect } from "@playwright/test";
import { inventoryShown, ROOT } from "./app";
import { uatStep } from "./step";

/** The throwaway local plugin of Cursor, in the fixtures' user folder. */
const KIT = String.raw`${ROOT}\claude-code-home\.cursor\plugins\local\cd-throwaway-kit`;

/** The Item of the server the plugin's next manifest declares. */
const NEXT = "plugin-cd-throwaway-kit-cd-throwaway-next";

uatStep("27.1", async ({ page, open, check }) => {
  await open();
  await inventoryShown(page);
  await page.locator(".agent-tile", { hasText: /^Cursor/ }).click();
  const row = page.locator(".item-row", { has: page.locator(".item-name", { hasText: NEXT }) });

  await check("El Item «plugin-cd-throwaway-kit-cd-throwaway-next» sale «Effective».", async () => {
    await expect(row.locator(".state-pill")).toHaveText("Effective");
  });
  await check(
    "Bajo su nombre sale «plugin · E:\\cd-throwaway\\claude-code-home\\.cursor\\plugins\\local\\cd-throwaway-kit\\.claude-plugin\\plugin.json#mcpServers.cd-throwaway-next».",
    async () => {
      await expect(row.locator(".item-location")).toHaveText(String.raw`plugin · ${KIT}\.claude-plugin\plugin.json#mcpServers.cd-throwaway-next`);
    },
  );
  const notice = page.getByRole("status").filter({ hasText: "couldn't be read" });
  await check("El aviso encima de los Items dice «1 location couldn't be read.».", async () => {
    await expect(notice).toContainText("1 location couldn't be read.");
  });

  await notice.getByRole("button", { name: "View" }).click();
  const card = page.locator(".entry-card.failure");
  await check(
    "La tarjeta de Read failure dice «Cursor cannot parse E:\\cd-throwaway\\claude-code-home\\.cursor\\plugins\\local\\cd-throwaway-kit\\.cursor-plugin\\plugin.json as strict JSON (line 1, column 30), so it reads no Entries from it.».",
    async () => {
      await expect(card).toHaveCount(1);
      await expect(card.locator(".reason")).toHaveText(
        String.raw`Cursor cannot parse ${KIT}\.cursor-plugin\plugin.json as strict JSON (line 1, column 30), so it reads no Entries from it.`,
      );
    },
  );
  await check(
    "La tarjeta dice también «Cursor reads the first it can of the plugin's `.claude-plugin/plugin.json` or `plugin.json` instead of this file.».",
    async () => {
      await expect(card.locator(".note")).toHaveText("Cursor reads the first it can of the plugin's `.claude-plugin/plugin.json` or `plugin.json` instead of this file.");
    },
  );
});
