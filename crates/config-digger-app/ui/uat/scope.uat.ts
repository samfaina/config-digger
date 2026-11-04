// Section 39: a Claude Code plugin installed at project scope from the git
// repo scope-repo and read from its `directory` marketplace's folder, in the
// Projects outside that Project. The fixtures hold claude-code-scope-home as
// a home the user added, with user settings that do not name the plugin.
import { expect, type Page } from "@playwright/test";
import { inventoryShown, itemRow, pickProject, ROOT } from "./app";
import { uatStep } from "./step";

/** The plugin's folder in its marketplace, which Claude Code reads it from. */
const PLUGIN = String.raw`${ROOT}\scope-market\plugins\cd-throwaway-scoped`;

/** The Items of the plugin's server and skill. */
const SERVER = "plugin:cd-throwaway-scoped:cd-throwaway-scoped-server";
const SKILL = "cd-throwaway-scoped:cd-throwaway-scoped-skill";

/** The State pill of the Item named `name`. */
function pill(page: Page, name: string) {
  return itemRow(page, name).locator(".state-pill");
}

/** Picks the Claude Code tile, the home claude-code-scope-home and the Project `path`. */
async function showScopeHome(page: Page, path: string) {
  await inventoryShown(page);
  await page.locator(".agent-tile", { hasText: "Claude Code" }).click();
  await page.locator(".home-tile", { hasText: "claude-code-scope-home" }).click();
  await pickProject(page, path);
}

uatStep("39.1", async ({ page, open, check }) => {
  await open();
  const repo = String.raw`${ROOT}\scope-repo`;
  await showScopeHome(page, repo);

  await check("Con E:\\cd-throwaway\\scope-repo elegido, el Item «plugin:cd-throwaway-scoped:cd-throwaway-scoped-server» sale «Effective».", async () => {
    await expect(pill(page, SERVER)).toHaveText("Effective");
  });
  await check("Con E:\\cd-throwaway\\scope-repo elegido, el Item «cd-throwaway-scoped:cd-throwaway-scoped-skill» sale «Effective».", async () => {
    await expect(pill(page, SKILL)).toHaveText("Effective");
  });

  await pickProject(page, String.raw`${ROOT}\scope-worktree`);
  await check("Con E:\\cd-throwaway\\scope-worktree elegido, el Item «plugin:cd-throwaway-scoped:cd-throwaway-scoped-server» sale «Effective».", async () => {
    await expect(pill(page, SERVER)).toHaveText("Effective");
  });
  await check(
    "Bajo su nombre sale «plugin · E:\\cd-throwaway\\scope-market\\plugins\\cd-throwaway-scoped\\.mcp.json#mcpServers.cd-throwaway-scoped-server».",
    async () => {
      await expect(itemRow(page, SERVER).locator(".item-location")).toHaveText(`plugin · ${PLUGIN}\\.mcp.json#mcpServers.cd-throwaway-scoped-server`);
    },
  );
  await check("Con E:\\cd-throwaway\\scope-worktree elegido, el Item «cd-throwaway-scoped:cd-throwaway-scoped-skill» sale «Effective».", async () => {
    await expect(pill(page, SKILL)).toHaveText("Effective");
  });
  await check(
    "Bajo su nombre sale «plugin · E:\\cd-throwaway\\scope-market\\plugins\\cd-throwaway-scoped\\skills\\cd-throwaway-scoped-skill».",
    async () => {
      await expect(itemRow(page, SKILL).locator(".item-location")).toHaveText(`plugin · ${PLUGIN}\\skills\\cd-throwaway-scoped-skill`);
    },
  );
});

uatStep("39.2", async ({ page, open, check }) => {
  await open();
  await showScopeHome(page, String.raw`${ROOT}\scope-on`);

  await check("Con E:\\cd-throwaway\\scope-on elegido, el Item «plugin:cd-throwaway-scoped:cd-throwaway-scoped-server» sale «Effective».", async () => {
    await expect(pill(page, SERVER)).toHaveText("Effective");
  });
  await check("Con E:\\cd-throwaway\\scope-on elegido, el Item «cd-throwaway-scoped:cd-throwaway-scoped-skill» sale «Effective».", async () => {
    await expect(pill(page, SKILL)).toHaveText("Effective");
  });
  await check(
    "Bajo su nombre sale «plugin · E:\\cd-throwaway\\scope-market\\plugins\\cd-throwaway-scoped\\skills\\cd-throwaway-scoped-skill».",
    async () => {
      await expect(itemRow(page, SKILL).locator(".item-location")).toHaveText(`plugin · ${PLUGIN}\\skills\\cd-throwaway-scoped-skill`);
    },
  );

  await pickProject(page, String.raw`${ROOT}\scope-off`);
  await check("Con E:\\cd-throwaway\\scope-off elegido, el Item «plugin:cd-throwaway-scoped:cd-throwaway-scoped-server» sale «Disabled».", async () => {
    await expect(pill(page, SERVER)).toHaveText("Disabled");
  });
  await check("Con E:\\cd-throwaway\\scope-off elegido, el Item «cd-throwaway-scoped:cd-throwaway-scoped-skill» sale «Disabled».", async () => {
    await expect(pill(page, SKILL)).toHaveText("Disabled");
  });
});

uatStep("39.3", async ({ page, open, check }) => {
  await open();
  await showScopeHome(page, String.raw`${ROOT}\scope-none`);
  // The home has no Item of its own: its ring counting none shows the Project's Inventory.
  await expect(page.getByLabel("0 of 0 Items in use")).toBeVisible();

  await check("No sale ningún Item «plugin:cd-throwaway-scoped:cd-throwaway-scoped-server».", async () => {
    await expect(itemRow(page, SERVER)).toHaveCount(0);
  });
  await check("No sale ningún Item «cd-throwaway-scoped:cd-throwaway-scoped-skill».", async () => {
    await expect(itemRow(page, SKILL)).toHaveCount(0);
  });
});
