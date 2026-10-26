// Section 5: the notices at the bottom of the screen. Their colours and shape
// are read from their styles, their 4 s from the page's clock, paused once the
// screen is shown, and their motion from the keyframes of their animations.
// Whether that motion looks smooth on screen is left to a person.
import { expect, type Locator, type Page } from "@playwright/test";
import { away, inventoryShown, pauseClock, rail, selectItem } from "./app";
import { leans, lightness, lilac, white } from "./look";
import { uatStep } from "./step";

const COPIED = "Copied the path.";

/** A keyframe of a notice's animation, by what moves it or fades it. */
type Frame = { opacity?: string; translate?: string; transform?: string };

/** The look of `notice`, beside an Entry card and a panel, and the keyframes of its animations. */
function lookOf(notice: Locator) {
  return notice.evaluate((n) => {
    const c = getComputedStyle(n);
    const card = document.querySelector(".entry-card") ?? document.querySelector(".panel")!;
    return {
      radius: parseFloat(c.borderTopLeftRadius),
      height: n.getBoundingClientRect().height,
      background: c.backgroundColor,
      edge: c.borderLeftColor,
      edgeWidth: parseFloat(c.borderLeftWidth),
      color: c.color,
      icon: getComputedStyle(n.querySelector(".icon")!).color,
      cardRadius: parseFloat(getComputedStyle(card).borderTopLeftRadius),
      cardBackground: getComputedStyle(card).backgroundColor,
      panelBackground: getComputedStyle(document.querySelector(".panel")!).backgroundColor,
    };
  });
}

/** The name and keyframes of each animation of `notice`. */
function animationsOf(notice: Locator) {
  return notice.evaluate((n) =>
    n.getAnimations().map((animation) => ({
      name: (animation as CSSAnimation).animationName,
      frames: (animation.effect as KeyframeEffect).getKeyframes().map(({ opacity, translate, transform }) => ({ opacity, translate, transform }) as Frame),
    })),
  );
}

/** Whether a frame moves the notice off its place: a translate or transform other than none or zero. */
const offset = (frame: Frame) =>
  [frame.translate, frame.transform].some((value) => value !== undefined && value !== "none" && !/^(0(px)?\s*)+$/.test(value));
/** Keyframes that change the opacity, from their first frame to their last. */
const fades = (frames: Frame[]) => frames.length >= 2 && Number(frames[0].opacity ?? 1) !== Number(frames.at(-1)!.opacity ?? 1);

/** The Inventory with cd-throwaway-user open, on a clock paused for the notices' timers, with the clipboard allowed. */
async function copyPathShown(page: Page, open: () => Promise<void>) {
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.clock.install();
  await open();
  await inventoryShown(page);
  await selectItem(page, "cd-throwaway-user");
  await pauseClock(page);
  return page.locator(".panel.detail .entry-location").first().getByRole("button", { name: "Copy path" });
}

uatStep("5.1", async ({ page, open, check, leave }) => {
  const copyPath = await copyPathShown(page, open);
  await copyPath.click();
  await away(page);
  const notice = page.locator(".notice", { hasText: COPIED });
  await notice.waitFor();
  const look = await lookOf(notice);

  await check("Sale un recuadro con las esquinas redondeadas de las tarjetas de Entry, no una píldora.", async () => {
    expect(look.radius, "radius").toBeLessThan(look.height / 2 - 2);
    expect(Math.abs(look.radius - look.cardRadius), `radius ${look.radius} against the card's ${look.cardRadius}`).toBeLessThanOrEqual(2);
  });
  await check("Su fondo es verde y destaca, más claro que el panel y las tarjetas.", async () => {
    expect(leans(look.background, 1), look.background).toBe(true);
    expect(lightness(look.background), `${look.background} against the panel's ${look.panelBackground}`).toBeGreaterThan(lightness(look.panelBackground));
    expect(lightness(look.background), `${look.background} against the card's ${look.cardBackground}`).toBeGreaterThan(lightness(look.cardBackground));
  });
  await check("Tiene un borde verde a la izquierda y un icono verde de visto.", async () => {
    expect(look.edgeWidth, "the left border's width").toBeGreaterThan(0);
    expect(leans(look.edge, 1), look.edge).toBe(true);
    expect(leans(look.icon, 1), look.icon).toBe(true);
    await expect(notice.locator(".icon")).toBeVisible();
  });
  await check("Su texto, en blanco, dice «Copied the path.».", async () => {
    await expect(notice).toHaveText(COPIED);
    expect(white(look.color), look.color).toBe(true);
  });
  leave("Entra subiendo un poco mientras aparece, en un instante y sin saltos.", "whether the motion looks smooth needs a person");
  await check("Se queda unos 4 segundos y se va bajando un poco mientras se desvanece.", async () => {
    await page.clock.runFor(3900);
    await expect(notice).toBeVisible();
    await expect(notice).not.toHaveClass(/leaving/);
    await page.clock.runFor(200);
    await expect(notice).toHaveClass(/leaving/);
    const leaving = await animationsOf(notice);
    expect(
      leaving.some(({ frames }) => offset(frames.at(-1)!) && fades(frames)),
      JSON.stringify(leaving),
    ).toBe(true);
    await page.clock.runFor(200);
    await expect(notice).toHaveCount(0);
  });

  await copyPath.click();
  await notice.hover();
  await check("Con el ratón encima no se va.", async () => {
    await page.clock.runFor(10_000);
    await expect(notice).toBeVisible();
    await expect(notice).not.toHaveClass(/leaving/);
  });
  await check("Unos 4 segundos después de apartar el ratón, se va igual.", async () => {
    await away(page);
    await page.clock.runFor(3900);
    await expect(notice).not.toHaveClass(/leaving/);
    await page.clock.runFor(200);
    await expect(notice).toHaveClass(/leaving/);
    await page.clock.runFor(200);
    await expect(notice).toHaveCount(0);
  });
});

uatStep("5.2", async ({ page, open, check }) => {
  // The fixtures' Logs folder holds nothing, so open_data_folder answers that
  // it opened nothing. Opening a location answers an error here, so a notice
  // of the UI opening one anyway would be an error.
  await page.clock.install();
  await open({ fail: ["open_location", "reveal_location", "open_backup_folder"] });
  await inventoryShown(page);
  await rail(page, "Settings");
  const logs = page.locator(".settings-data .place-row", { has: page.locator(".place-name", { hasText: /^Logs$/ }) });
  await logs.waitFor();
  await pauseClock(page);
  await logs.getByRole("button", { name: "Open folder" }).click();
  await away(page);
  const notice = page.locator(".notice").last();
  await notice.waitFor();
  const look = await lookOf(notice);
  const arriving = await animationsOf(notice);

  await check("No se abre el Explorador.", async () => {
    await expect(notice).toHaveClass(/\binformation\b/);
    await expect(page.locator(".notice.error")).toHaveCount(0);
  });
  await check("Sale un recuadro con la forma del paso 5.1, de fondo azul.", async () => {
    expect(look.radius, "radius").toBeLessThan(look.height / 2 - 2);
    expect(Math.abs(look.radius - look.cardRadius), `radius ${look.radius} against the card's ${look.cardRadius}`).toBeLessThanOrEqual(2);
    expect(leans(look.background, 2), look.background).toBe(true);
  });
  await check("Tiene un borde lila claro a la izquierda y un icono «i» lila claro.", async () => {
    expect(look.edgeWidth, "the left border's width").toBeGreaterThan(0);
    expect(lilac(look.edge), look.edge).toBe(true);
    expect(lilac(look.icon), look.icon).toBe(true);
    await expect(notice.locator(".icon")).toBeVisible();
  });
  await check("Su texto, en blanco, dice «config-digger has kept nothing there yet.».", async () => {
    await expect(notice).toHaveText("config-digger has kept nothing there yet.");
    expect(white(look.color), look.color).toBe(true);
  });
  await check("Entra, se queda unos 4 segundos y se va como el del paso 5.1.", async () => {
    expect(arriving.map(({ name }) => name)).toContain("float-in");
    await page.clock.runFor(3900);
    await expect(notice).not.toHaveClass(/leaving/);
    await page.clock.runFor(200);
    await expect(notice).toHaveClass(/leaving/);
    expect((await animationsOf(notice)).map(({ name }) => name)).toContain("float-out");
    await page.clock.runFor(200);
    await expect(page.locator(".notice")).toHaveCount(0);
  });
});

uatStep("5.4", async ({ page, open, check }) => {
  // Windows' animation effects turned off reach WebView2 as prefers-reduced-motion.
  await page.emulateMedia({ reducedMotion: "reduce" });
  const copyPath = await copyPathShown(page, open);
  await copyPath.click();
  await away(page);
  const notice = page.locator(".notice", { hasText: COPIED });
  await notice.waitFor();
  const arriving = await animationsOf(notice);
  await page.clock.runFor(4100);
  await expect(notice).toHaveClass(/leaving/);
  const leaving = await animationsOf(notice);

  await check("El aviso «Copied the path.» aparece y se va con un fundido.", async () => {
    await expect(notice).toHaveText(COPIED);
    expect(arriving.some(({ frames }) => fades(frames)), JSON.stringify(arriving)).toBe(true);
    expect(leaving.some(({ frames }) => fades(frames)), JSON.stringify(leaving)).toBe(true);
  });
  await check("El aviso se queda en el mismo sitio, sin subir ni bajar.", async () => {
    const frames = [...arriving, ...leaving].flatMap((animation) => animation.frames);
    expect(frames.length, "keyframes").toBeGreaterThan(0);
    expect(frames.filter(offset), JSON.stringify(frames)).toEqual([]);
  });
});
