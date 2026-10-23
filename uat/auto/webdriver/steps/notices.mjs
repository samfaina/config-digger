// Section 5: the notices at the bottom of the screen. Their colours, shape,
// text and timing are checked from styles and clocks; the motion is checked
// from the keyframes of their animations. Whether that motion looks smooth
// on screen is left to a person (docs/research/uat-automation.md, section 9).
import { ref } from "../driver.mjs";
import { go, hoverFresh, notices, press, wait } from "../lib.mjs";
import { inventoryOf, openItem } from "./common.mjs";

const rgb = (c) => (c?.match(/\d+(\.\d+)?/g) ?? []).map(Number);
const lum = (c) => {
  const [r, g, b] = rgb(c);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};

// The newest notice: its look, and the keyframes of its animations.
export function noticeLook(s) {
  return s.exec(`
    const n = [...document.querySelectorAll(".notice")].at(-1);
    if (!n) return null;
    const c = getComputedStyle(n), r = n.getBoundingClientRect();
    const icon = n.querySelector("svg, .icon");
    const card = document.querySelector(".entry-card") ?? document.querySelector(".panel");
    return {
      cls: n.className, text: n.textContent.trim(), bg: c.backgroundColor, radius: parseFloat(c.borderTopLeftRadius), height: r.height,
      edge: c.borderLeftWidth + " " + c.borderLeftStyle + " " + c.borderLeftColor, edgeColor: c.borderLeftColor, color: c.color,
      icon: icon && getComputedStyle(icon).color, iconName: icon?.getAttribute("data-icon") ?? icon?.outerHTML.slice(0, 60),
      cardRadius: parseFloat(getComputedStyle(document.querySelector(".entry-card") ?? n).borderTopLeftRadius),
      panelBg: getComputedStyle(document.querySelector(".panel")).backgroundColor,
      cardBg: getComputedStyle(card).backgroundColor,
      centred: Math.abs((r.left + r.right) / 2 - innerWidth / 2) < 80 && r.bottom > innerHeight * 0.7,
      keyframes: n.getAnimations().map((a) => ({ name: a.animationName ?? a.id, duration: a.effect.getTiming().duration, frames: a.effect.getKeyframes().map((k) => ({ transform: k.transform, translate: k.translate, opacity: k.opacity })) })),
    };`);
}

// Waits for the notice with `text` to go, and returns how long it stayed.
async function lifetime(s, text, from) {
  await s.until(async () => !(await notices(s)).some((n) => n.text.includes(text)), { what: `the notice "${text}" gone`, timeout: 20000 });
  return Date.now() - from;
}

// The keyframes of the notice leaving, caught while it leaves.
async function leaving(s, text) {
  return s.until(
    () =>
      s.exec(
        `const n = [...document.querySelectorAll(".notice")].find((e) => e.textContent.includes(arguments[0]) && e.classList.contains("leaving"));
         return n ? n.getAnimations().map((a) => ({ name: a.animationName, frames: a.effect.getKeyframes().map((k) => ({ transform: k.transform, translate: k.translate, opacity: k.opacity })) })) : null;`,
        text,
      ),
    { what: "the notice leaving", timeout: 20000 },
  );
}

async function copyPath(t) {
  const { s } = t;
  await inventoryOf(t, "Claude Code", "claude-code-home");
  await openItem(t, "cd-throwaway-user");
  const button = await s.until(
    () =>
      s
        .exec(`return [...document.querySelectorAll(".panel.detail .entry-location button")].find((b) => b.textContent.trim().toLowerCase() === "copy path") ?? null;`)
        .then((v) => (v ? Object.values(v)[0] : null)),
    { what: "Copy path" },
  );
  return button;
}

// The notices move with the translate property: a frame has an offset when
// its translate, or its transform, is set and not zero.
const offset = (f) =>
  [f.translate, f.transform].some((v) => v && v !== "none" && !/^(0(px)?\s*)+$/.test(v) && !/^translateY?\(0(px)?(, 0(px)?)?\)$/.test(v));
// From an offset to none, or the other way when leaving: Web Animations
// lists a missing end frame as the element's own style.
const movesUp = (frames) => frames.length >= 2 && offset(frames[0]) && !offset(frames.at(-1));
const fades = (frames) => frames.length >= 2 && Number(frames[0].opacity ?? 1) !== Number(frames.at(-1).opacity ?? 1);

export default [
  {
    id: "5.1",
    manual: "Que el aviso entre y se vaya sin saltos: las pruebas miran su duración y sus fotogramas, no cómo se ve moverse.",
    async run(t) {
      const { s } = t;
      const button = await copyPath(t);
      await s.click(button);
      const from = Date.now();
      await s.hover({ x: 600, y: 4 });
      const look = await s.until(() => noticeLook(s), { what: "the notice" });
      t.check(look.radius < look.height / 2 - 2 && Math.abs(look.radius - look.cardRadius) <= 2, "sale un recuadro con las esquinas redondeadas de las tarjetas de Entry, no una píldora", `radio ${look.radius}, alto ${Math.round(look.height)}, tarjeta ${look.cardRadius}`);
      const [r, g, b] = rgb(look.bg);
      t.check(g > r && g > b && lum(look.bg) > lum(look.panelBg) && lum(look.bg) > lum(look.cardBg), "su fondo es verde y destaca, más claro que el panel y las tarjetas", `${look.bg} frente a ${look.panelBg} y ${look.cardBg}`);
      const [er, eg, eb] = rgb(look.edgeColor);
      t.check(eg > er && eg > eb && !look.edge.startsWith("0px"), "tiene un borde verde a la izquierda", look.edge);
      const [ir, ig, ib] = rgb(look.icon);
      t.check(look.icon && ig > ir && ig > ib, "tiene un icono verde de visto", look.icon);
      const [cr, cg, cb] = rgb(look.color);
      t.check(cr > 230 && cg > 230 && cb > 230, "su texto es blanco", look.color);
      t.eq(look.text, "Copied the path.", "su texto dice «Copied the path.»");
      t.check(look.keyframes.some((k) => movesUp(k.frames) && fades(k.frames) && k.duration <= 400), "entra subiendo un poco mientras aparece, en un instante", JSON.stringify(look.keyframes));
      await t.shot("aviso", "recuadro verde con borde verde a la izquierda, icono de visto verde y texto blanco");
      const out = await leaving(s, "Copied the path.");
      t.check(out.some((k) => offset(k.frames.at(-1)) && fades(k.frames)), "se va bajando un poco mientras se desvanece", JSON.stringify(out));
      const stayed = await lifetime(s, "Copied the path.", from);
      t.check(stayed > 3500 && stayed < 5500, "se queda unos 4 segundos", `${stayed} ms`);
      await s.click(button);
      const el = await s.until(() => s.find(".notice").catch(() => null), { what: "the notice" });
      await s.hover(el);
      await wait(6000);
      t.check((await notices(s)).some((n) => n.text === "Copied the path."), "con el ratón encima no se va", "6 s con el ratón encima");
      await s.hover({ x: 600, y: 4 });
      const after = await lifetime(s, "Copied the path.", Date.now());
      t.check(after > 3500 && after < 5500, "unos 4 segundos después de apartar el ratón, se va igual", `${after} ms`);
    },
  },
  {
    id: "5.2",
    async run(t) {
      const { s } = t;
      const out = t.ps(t.step.commands[0].text);
      const empty = out.split(/\r?\n/).map((l) => l.split(": ")).find(([, v]) => v === "False");
      if (!empty) {
        t.blocked = "las cuatro carpetas de datos existen";
        return;
      }
      const name = empty[0];
      const explorers = () => Number(t.ps("@(Get-Process explorer -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 }).Count"));
      const windows = () => t.ps("@((New-Object -ComObject Shell.Application).Windows()).Count");
      const before = windows();
      await go(s, "Settings");
      const button = await s.until(
        () =>
          s
            .exec(
              `const row = [...document.querySelectorAll(".settings-data .place-row")].find((r) => r.textContent.includes(arguments[0]));
               return row ? [...row.querySelectorAll("button")].find((b) => b.textContent.trim() === "Open folder") ?? null : null;`,
              name,
            )
            .then((v) => (v ? Object.values(v)[0] : null)),
        { what: `Open folder of ${name}` },
      );
      await s.click(button);
      const from = Date.now();
      await s.hover({ x: 600, y: 4 });
      const look = await s.until(() => noticeLook(s), { what: "the notice" });
      await wait(800);
      t.check(windows() === before, "no se abre el Explorador", `${before} ventanas del Explorador antes y ${windows()} después`);
      void explorers;
      t.check(look.radius < look.height / 2 - 2, "sale un recuadro con la forma del paso 5.1");
      const [r, g, b] = rgb(look.bg);
      t.check(b > r && b > g - 10, "de fondo azul", look.bg);
      const [er, eg, eb] = rgb(look.edgeColor);
      t.check(eb > eg && er > eg - 20 && er > 150, "tiene un borde lila claro a la izquierda", look.edge);
      const [ir, ig, ib] = rgb(look.icon);
      t.check(look.icon && ib > ig && ir > 150, "y un icono «i» lila claro", look.icon);
      t.eq(look.text, "config-digger has kept nothing there yet.", "su texto dice «config-digger has kept nothing there yet.»");
      t.check(look.keyframes.some((k) => movesUp(k.frames) && fades(k.frames)), "entra como el del paso 5.1");
      await t.shot("aviso", "recuadro azul con borde e icono «i» lila claro y texto blanco");
      const stayed = await lifetime(s, "config-digger has kept nothing there yet.", from);
      t.check(stayed > 3500 && stayed < 5500, "se queda unos 4 segundos y se va como el del paso 5.1", `${stayed} ms`);
    },
  },
  {
    id: "5.3",
    async run(t) {
      const { s } = t;
      await go(s, "Settings");
      const toggle = await s.waitFor("button[role=switch][aria-label='Check for updates on startup']");
      const state = () => s.attr(toggle, "aria-checked");
      const start = await state();
      await s.click(toggle);
      await wait(500);
      await s.click(toggle);
      await wait(500);
      t.eq(await state(), start, "el interruptor queda como estaba tras dos pulsaciones");
      t.ps(t.step.commands[0].text);
      try {
        await s.click(toggle);
        const from = Date.now();
        await s.hover({ x: 600, y: 4 });
        const look = await s.until(async () => {
          const l = await noticeLook(s);
          return l?.cls.includes("error") ? l : null;
        }, { what: "the error notice" });
        const [r, g, b] = rgb(look.bg);
        t.check(r > g + 30 && r > b, "sale un recuadro con la forma del paso 5.1, de fondo rojo", look.bg);
        const [er, eg, eb] = rgb(look.edgeColor);
        t.check(er > 220 && eg > 140 && eb > 140 && er > eg + 40, "tiene un borde rosa claro a la izquierda", look.edge);
        const [ir, ig, ib] = rgb(look.icon);
        t.check(look.icon && ir > 220 && ig > 140 && ib > 140 && ir > ig + 40, "y un icono «!» rosa claro", look.icon);
        t.check(look.text.startsWith("The setting couldn't be saved: cannot write ") && /(Acceso denegado|Access is denied)\. \(os error 5\)$/.test(look.text), "su texto dice «The setting couldn't be saved: cannot write …» y acaba en un error de acceso denegado", look.text);
        await t.shot("aviso", "recuadro rojo con borde e icono «!» rosa claro y texto blanco");
        const stayed = await lifetime(s, "The setting couldn't be saved", from);
        t.check(stayed > 3500 && stayed < 5500, "se queda unos 4 segundos y se va como el del paso 5.1", `${stayed} ms`);
        t.eq(await state(), start, "el interruptor vuelve a como estaba");
      } finally {
        t.ps(t.step.commands[1].text);
      }
    },
  },
  {
    id: "5.4",
    note: "«Efectos de animación» de Windows se emula con prefers-reduced-motion: reduce por CDP, la preferencia que WebView2 toma de ese ajuste. El ajuste de Windows del tester no se toca.",
    async run(t) {
      const { s } = t;
      const button = await copyPath(t);
      await s.cdp("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-motion", value: "reduce" }] });
      try {
        t.check(await s.exec("return matchMedia('(prefers-reduced-motion: reduce)').matches"), "la app ve los efectos de animación apagados");
        await s.click(button);
        await s.hover({ x: 600, y: 4 });
        const look = await s.until(() => noticeLook(s), { what: "the notice" });
        t.eq(look.text, "Copied the path.", "sale el aviso «Copied the path.»");
        const frames = look.keyframes.flatMap((k) => k.frames);
        t.check(look.keyframes.some((k) => fades(k.frames)), "el aviso aparece con un fundido", JSON.stringify(look.keyframes));
        t.check(frames.every((f) => !offset(f)), "se queda en el mismo sitio, sin subir", JSON.stringify(frames));
        const out = await leaving(s, "Copied the path.");
        t.check(out.some((k) => fades(k.frames)) && out.flatMap((k) => k.frames).every((f) => !offset(f)), "se va con un fundido, sin bajar", JSON.stringify(out));
      } finally {
        await s.cdp("Emulation.setEmulatedMedia", { features: [] });
      }
    },
  },
];
