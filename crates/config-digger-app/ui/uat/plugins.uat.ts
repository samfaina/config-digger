// Section 27: a Cursor plugin whose first manifest does not parse, which
// Cursor reads past to the next one. Section 31: a VS Code agent plugin
// whose manifest does not parse, which VS Code reads around the error.
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

/** The throwaway agent plugin of VS Code, in the fixtures' user folder. */
const VSPLUGIN = String.raw`${ROOT}\claude-code-home\.vscode\agent-plugins\cd-throwaway.invalid\cd-throwaway\cd-throwaway-vsplugin`;

/** The Item of the server the plugin's broken manifest names a file of. */
const SERVER = "cd-throwaway-vsplugin-server";

uatStep("31.1", async ({ page, open, check }) => {
  await open();
  await inventoryShown(page);
  await page.locator(".agent-tile", { hasText: /^VS Code/ }).click();
  const row = page.locator(".item-row", { has: page.locator(".item-name", { hasText: SERVER }) });

  await check("El Item «cd-throwaway-vsplugin-server» sale «Effective».", async () => {
    await expect(row.locator(".state-pill")).toHaveText("Effective");
  });
  await check(
    "Bajo su nombre sale «plugin · E:\\cd-throwaway\\claude-code-home\\.vscode\\agent-plugins\\cd-throwaway.invalid\\cd-throwaway\\cd-throwaway-vsplugin\\servers.json#mcpServers.cd-throwaway-vsplugin-server».",
    async () => {
      await expect(row.locator(".item-location")).toHaveText(String.raw`plugin · ${VSPLUGIN}\servers.json#mcpServers.cd-throwaway-vsplugin-server`);
    },
  );
  await check("No sale ningún Item «cd-throwaway-vsplugin-root».", async () => {
    await expect(page.locator(".item-name", { hasText: "cd-throwaway-vsplugin-root" })).toHaveCount(0);
  });

  const notice = page.getByRole("status").filter({ hasText: "couldn't be read" });
  await check("El aviso encima de los Items dice «1 location couldn't be read.».", async () => {
    await expect(notice).toContainText("1 location couldn't be read.");
  });

  await notice.getByRole("button", { name: "View" }).click();
  const card = page.locator(".entry-card.failure");
  await check(
    "La tarjeta de Read failure dice «VS Code cannot parse E:\\cd-throwaway\\claude-code-home\\.vscode\\agent-plugins\\cd-throwaway.invalid\\cd-throwaway\\cd-throwaway-vsplugin\\.claude-plugin\\plugin.json as JSONC with trailing commas (line 1, column 67).».",
    async () => {
      await expect(card).toHaveCount(1);
      await expect(card.locator(".reason")).toHaveText(
        String.raw`VS Code cannot parse ${VSPLUGIN}\.claude-plugin\plugin.json as JSONC with trailing commas (line 1, column 67).`,
      );
    },
  );
  await check("La tarjeta dice también «VS Code reads what is around the error, and config-digger lists what it keeps.».", async () => {
    await expect(card.locator(".note")).toHaveText("VS Code reads what is around the error, and config-digger lists what it keeps.");
  });
});
