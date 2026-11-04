// Section 28: Claude Code's nested skills. The fixtures hold the user's
// folder, claude-code-home, as a Project the user added, in no git repo, so
// config-digger leaves out the folders below it and notes why. They hold no
// project-outside-git, whose checks stay with a person.
import { expect } from "@playwright/test";
import { inventoryShown, pickProject, rail, ROOT, row } from "./app";
import { uatStep } from "./step";

/** The user's folder in the fixtures, a Project the user added. */
const HOME = String.raw`${ROOT}\claude-code-home`;

/** The note of a Project whose nested skills folders config-digger leaves out. */
const NOTE =
  "Claude Code also loads the `.claude\\skills` folders below this Project, but config-digger does not read them here: outside a git repo, that would walk every folder below a home folder or a drive.";

/** Why the checks of project-outside-git stay with a person. */
const NOT_RECORDED = "the fixtures hold no project-outside-git, which the step adds by hand in the real app";

uatStep("28.2", async ({ page, open, check, leave }) => {
  await open();
  await inventoryShown(page);
  await rail(page, "Projects & homes");
  await page.getByRole("tab", { name: /^Projects · / }).click();

  await check(
    "En Projects & homes, bajo «E:\\cd-throwaway\\claude-code-home» sale la nota «Claude Code also loads the `.claude\\skills` folders below this Project, but config-digger does not read them here: outside a git repo, that would walk every folder below a home folder or a drive.».",
    async () => {
      await expect(row(page, HOME).locator(".place-sub", { hasText: NOTE })).toHaveText(NOTE);
    },
  );
  leave("La fila de «E:\\cd-throwaway\\project-outside-git» no lleva esa nota.", NOT_RECORDED);
  leave("Con E:\\cd-throwaway\\project-outside-git elegido, el Item «cd-throwaway-outside-git-skill» sale «Effective».", NOT_RECORDED);
  leave(
    "Bajo su nombre sale «project · E:\\cd-throwaway\\project-outside-git\\apps\\web\\.claude\\skills\\cd-throwaway-outside-git-skill».",
    NOT_RECORDED,
  );
  leave("Su Entry tiene el aviso «Claude Code loads it only once it reads or edits a file under `apps\\web` in the Project.».", NOT_RECORDED);

  await rail(page, "Inventory");
  await inventoryShown(page);
  await page.locator(".agent-tile", { hasText: "Claude Code" }).click();
  await pickProject(page, HOME);

  await check(
    "Con E:\\cd-throwaway\\claude-code-home elegido, un aviso encima de los Items dice «Claude Code also loads the `.claude\\skills` folders below this Project, but config-digger does not read them here: outside a git repo, that would walk every folder below a home folder or a drive.».",
    async () => {
      await expect(page.locator(".items").getByRole("status").filter({ hasText: NOTE })).toHaveText(NOTE);
    },
  );
  await check("Con E:\\cd-throwaway\\claude-code-home elegido, «cd-throwaway-home-nested-skill» no sale en la lista Items.", async () => {
    await expect(page.locator(".item-name", { hasText: "cd-throwaway-home-nested-skill" })).toHaveCount(0);
  });
});
