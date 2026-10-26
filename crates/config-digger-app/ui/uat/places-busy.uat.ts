// Section 10: the busy states of a list change on Projects & homes. Each step
// holds change_list's answer with the page's delay parameter while it looks at
// the spinner's notice and the row's buttons. The fake backend answers every
// change with the one it recorded, the Hide of project-hide, so the notice
// after a change and the list it leaves are those of that Hide: the checks of
// what another change leaves stay with a person.
import { expect, type Locator, type Page } from "@playwright/test";
import { rail } from "./app";
import { uatStep } from "./step";

const HELD = 2000;
const CHANGE_WRITES = "the fake backend answers every list change with its recorded Hide of project-hide, so what a real change leaves is seen in the real app";

/** A pattern matching `text` whole. */
const whole = (text: string) => new RegExp(`^${text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`);

/** The row of the open tab whose path is `path`. */
const row = (page: Page, path: string) => page.locator(".place-row", { has: page.locator(".place-name", { hasText: whole(path) }) });

/** The busy notice at the bottom of the screen. */
const busy = (page: Page) => page.locator(".notice.busy");

/** Asserts the busy notice: its spinner turns and it says `text`. */
async function spinning(page: Page, text: string) {
  await expect(busy(page)).toHaveText(text);
  await expect(busy(page).locator(".spinner")).not.toHaveCSS("animation-name", "none");
}

/** Asserts every button of `row` is disabled. */
async function allDisabled(row: Locator) {
  const buttons = row.getByRole("button");
  expect(await buttons.count()).toBeGreaterThan(0);
  for (const button of await buttons.all()) await expect(button).toBeDisabled();
}

/** Asserts the busy notice sits over the open dialog: the point at its middle is the notice's. */
async function overTheDialog(page: Page) {
  await expect(page.getByRole("dialog")).toBeVisible();
  const box = await busy(page).boundingBox();
  expect(box).not.toBeNull();
  const top = await page.evaluate(({ x, y }) => document.elementFromPoint(x, y)?.closest(".notice") !== null, { x: box!.x + box!.width / 2, y: box!.y + box!.height / 2 });
  expect(top).toBe(true);
}

/** Opens the group titled `title` on the Projects tab, if it is closed. */
async function openGroup(page: Page, title: string) {
  const toggle = page.locator(".place-group-toggle", { hasText: new RegExp(`^[▾▸]\\s*${title} · `) });
  if ((await toggle.getAttribute("aria-expanded")) !== "true") await toggle.click();
}

uatStep("10.11", async ({ page, open, check, leave }) => {
  const path = String.raw`E:\cd-throwaway\project-hide`;
  await open({ delay: { change_list: HELD } });
  await rail(page, "Projects & homes");
  await openGroup(page, "Hidden");
  const hidden = row(page, path);
  await hidden.getByRole("button", { name: "Unhide" }).click();

  const why = "the fixtures list project-hide as hidden already, so its row offers Unhide, not Hide";
  leave("Nada más pulsar Hide sale abajo, donde salen los avisos, un aviso con una rueda que gira y el texto «Hiding E:\\cd-throwaway\\project-hide…», aunque dure menos de un segundo.", why);
  leave("Mientras, los botones «Open in Inventory» y «Hide» de la fila se ven apagados.", why);
  leave("El aviso de la rueda sigue ahí hasta que el Project sale de la lista principal.", why);
  leave("Entonces lo sustituye el aviso «Hid E:\\cd-throwaway\\project-hide. Picking it in the Inventory still evaluates it.».", why);
  await check("Nada más pulsar Unhide sale abajo el aviso de la rueda con «Unhiding E:\\cd-throwaway\\project-hide…», aunque dure menos de un segundo.", async () => {
    await spinning(page, `Unhiding ${path}…`);
  });
  await check("Mientras, los botones de la fila se ven apagados.", async () => {
    await allDisabled(hidden);
  });
  leave("El aviso de la rueda sigue ahí hasta que el Project vuelve a la lista principal.", CHANGE_WRITES);
  await check("Entonces lo sustituye el aviso «E:\\cd-throwaway\\project-hide is listed again.».", async () => {
    await expect(page.locator(".notice.success")).toHaveText(`${path} is listed again.`);
    await expect(busy(page)).toHaveCount(0);
  });
});

uatStep("10.12", async ({ page, open, check, leave }) => {
  const path = String.raw`E:\cd-throwaway\project-extra`;
  await open({ delay: { change_list: HELD } });
  await rail(page, "Projects & homes");
  const extra = row(page, path);
  await extra.getByRole("button", { name: "Remove from list" }).click();

  await check("Remove from list saca abajo un aviso con una rueda que gira y el texto «Removing E:\\cd-throwaway\\project-extra from config-digger's list…».", async () => {
    await spinning(page, `Removing ${path} from config-digger's list…`);
  });
  await check("Mientras, los botones de la fila se ven apagados.", async () => {
    await allDisabled(extra);
  });
  leave("Cuando la fila desaparece, sale el aviso «Removed E:\\cd-throwaway\\project-extra from config-digger's list. Nothing on disk changed.».", CHANGE_WRITES);
  await expect(busy(page)).toHaveCount(0);

  await page.getByRole("button", { name: "Add Project" }).click();
  const dialog = page.getByRole("dialog");
  // The fake picker picks project-extra, as the tester does in Windows' picker.
  await dialog.getByRole("button", { name: "Choose folder…" }).click();
  await expect(dialog.getByRole("textbox", { name: "Path" })).toHaveValue(path);
  const add = dialog.locator(".dialog-foot").getByRole("button", { name: "Add", exact: true });
  await add.click();
  await check("Add saca, por encima del diálogo, el aviso de la rueda con «Adding E:\\cd-throwaway\\project-extra…».", async () => {
    await spinning(page, `Adding ${path}…`);
    await overTheDialog(page);
  });
  await check("Mientras, Add se ve apagado.", async () => {
    await expect(add).toBeDisabled();
  });
  leave("Cuando el diálogo se cierra, sale «Added E:\\cd-throwaway\\project-extra: every Agent is evaluated on it.».", CHANGE_WRITES);
  await check("Al final, project-extra vuelve a estar en la lista.", async () => {
    await expect(dialog).toHaveCount(0);
    await expect(extra).toBeVisible();
  });
});

uatStep("10.13", async ({ page, open, check, leave }) => {
  const home = String.raw`E:\cd-throwaway\codex-home`;
  await open({ delay: { change_list: HELD } });
  await rail(page, "Projects & homes");
  await page.getByRole("tab", { name: /^Agent homes/ }).click();

  const why = "the fixtures find codex-home through %CODEX_HOME%, so its row offers Hide, not Remove from list";
  leave("Remove from list saca abajo un aviso con una rueda que gira y el texto «Removing E:\\cd-throwaway\\codex-home of Codex from config-digger's list…».", why);
  leave("Mientras, los botones de la fila se ven apagados.", why);
  leave("Cuando la fila desaparece, sale el aviso «Removed E:\\cd-throwaway\\codex-home of Codex from config-digger's list. Nothing on disk changed.».", why);

  await page.getByRole("button", { name: "Add Agent home" }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByRole("combobox", { name: "Agent" }).click();
  await page.getByRole("option", { name: "Codex", exact: true }).click();
  // The fake picker picks project-extra: the folder is typed, as the tester's pick fills it.
  await dialog.getByRole("textbox", { name: "Folder" }).fill(home);
  const add = dialog.locator(".dialog-foot").getByRole("button", { name: "Add", exact: true });
  await add.click();
  await check("Add saca, por encima del diálogo, el aviso de la rueda con «Adding E:\\cd-throwaway\\codex-home as an Agent home of Codex…».", async () => {
    await spinning(page, `Adding ${home} as an Agent home of Codex…`);
    await overTheDialog(page);
  });
  await check("Mientras, Add se ve apagado.", async () => {
    await expect(add).toBeDisabled();
  });
  leave("Cuando el diálogo se cierra, sale «Added E:\\cd-throwaway\\codex-home as an Agent home of Codex.».", CHANGE_WRITES);
  await check("Al final, la fila de codex-home vuelve a estar bajo Codex.", async () => {
    await expect(dialog).toHaveCount(0);
    // The rows of a group follow its heading, up to the next one.
    const under = await page.locator(".places .scroll > *").evaluateAll(
      (elements, path) => {
        let agent = "";
        for (const element of elements) {
          if (element.matches("h3.agent-group")) agent = element.firstChild?.textContent?.trim() ?? "";
          else if (element.querySelector(".place-sub.mono")?.textContent?.trim() === path) return agent;
        }
        return null;
      },
      home,
    );
    expect(under).toBe("Codex");
  });
});
