// Section 3: the app's tooltips, by mouse and by keyboard.
import { KEY, ref } from "../driver.mjs";
import { focused, go, hoverFresh, tabTo, tooltip, wait } from "../lib.mjs";
import { inventoryOf, openItem } from "./common.mjs";

const inside = (tip) => tip && tip.rect.left >= 0 && tip.rect.top >= 0 && tip.rect.right <= tip.vw && tip.rect.bottom <= tip.vh;

export default [
  {
    id: "3.1",
    async run(t) {
      const { s } = t;
      await inventoryOf(t, "Claude Code", "claude-code-home");
      await s.hover({ x: 600, y: 10 });
      const rail = await s.waitFor(".rail button[aria-label='Inventory']");
      await s.hover(rail);
      await wait(200);
      const early = await tooltip(s);
      await wait(700);
      const tip = await tooltip(s);
      t.check(!early && tip?.text === "Inventory", "tras medio segundo sale un recuadro con el texto «Inventory»", `a 200 ms: ${early?.text ?? "nada"}; a 900 ms: ${tip?.text ?? "nada"}`);
      const [r, g, b] = (tip?.bg.match(/\d+/g) ?? []).map(Number);
      t.check(tip && r + g + b < 210, "el recuadro es oscuro", tip?.bg);
      t.check(tip && parseFloat(tip.radius) > 0 && !tip.border.startsWith("0px") && tip.shadow !== "none", "el recuadro tiene esquinas redondeadas, borde fino y sombra", tip && `${tip.radius}; ${tip.border}; ${tip.shadow}`);
      await t.shot("inventory", "el tooltip es un recuadro oscuro con esquinas redondeadas, borde fino y sombra, como los paneles");
      await s.hover({ x: 600, y: 300 });
      await wait(400);
      t.check(!(await tooltip(s)), "el recuadro desaparece al apartar el ratón");
      await openItem(t, "cd-throwaway-user");
      const copy = await s.waitFor(".panel.detail button[aria-label='Copy the configuration']");
      await hoverFresh(s, copy);
      await wait(900);
      t.eq((await tooltip(s))?.text, "Copy the configuration, with every Secret masked", "el botón de copiar muestra «Copy the configuration, with every Secret masked»");
      // The rows that show a location are the Items list's: each row's grey
      // line is its Effective Entry's location.
      const loc = await s
        .exec(`return [...document.querySelectorAll(".item-row")].find((r) => r.querySelector(".item-name")?.textContent.trim() === "cd-throwaway-user")?.querySelector(".item-location") ?? null;`)
        .then((v) => (v ? Object.values(v)[0] : null));
      if (t.check(loc, "la fila tiene su ubicación")) {
        await hoverFresh(s, loc);
        await wait(900);
        const full = await tooltip(s);
        t.eq(full?.text, `${t.root}\\claude-code-home\\.claude.json#mcpServers.cd-throwaway-user`, "la fila muestra su ruta entera");
      }
      const titled = await s.exec(`return [...document.querySelectorAll("[title]")].filter((e) => !(e instanceof SVGElement)).map((e) => e.outerHTML.slice(0, 80));`);
      t.check(titled.length === 0, "no sale la etiqueta amarilla o blanca del sistema: ningún elemento tiene title", titled.join(" | "));
    },
  },
  {
    id: "3.2",
    async run(t) {
      const { s } = t;
      await s.hover({ x: 600, y: 300 });
      await s.exec("document.activeElement?.blur()");
      const f = await tabTo(s, (e) => e.label === "Inventory");
      const tip = await tooltip(s);
      t.check(f && tip?.text === "Inventory", "el tooltip «Inventory» sale en cuanto el botón recibe el foco, sin pausa", tip?.text);
      await s.keys(KEY.Escape);
      await wait(150);
      const after = await focused(s);
      t.check(!(await tooltip(s)) && after?.label === "Inventory", "Escape lo oculta y el foco sigue en el botón");
      await s.keys(KEY.Tab);
      await wait(150);
      t.eq((await tooltip(s))?.text, "History · Undo", "con Tab sale el tooltip «History · Undo» del botón siguiente");
    },
  },
  {
    id: "3.3",
    async run(t) {
      const { s } = t;
      await s.setWindow(1100, 640);
      await go(s, "Inventory");
      const about = await s.waitFor(".rail button[aria-label='About']");
      await hoverFresh(s, about);
      await wait(900);
      const tip = await tooltip(s);
      const button = await s.rect(about);
      t.check(inside(tip), "el tooltip de About queda entero dentro de la ventana", JSON.stringify(tip?.rect));
      t.check(tip && tip.rect.bottom <= button.y + 2 && tip.rect.left >= 0, "el de About sale encima del botón, sin pasar del borde izquierdo", JSON.stringify({ tip: tip?.rect, button }));
      await t.shot("about", "el tooltip de About sale entero, encima del botón");
      await inventoryOf(t, "Claude Code", "claude-code-home");
      const row = await s
        .exec(`return [...document.querySelectorAll(".item-row")].find((r) => r.querySelector(".item-name")?.textContent.trim() === "cd-throwaway-user-skill")?.querySelector(".item-location") ?? null;`)
        .then((v) => (v ? Object.values(v)[0] : null));
      await s.exec("arguments[0].scrollIntoView({ block: 'nearest' });", ref(row));
      await hoverFresh(s, row);
      await wait(900);
      const skill = await tooltip(s);
      t.check(inside(skill), "el tooltip de la fila queda entero dentro de la ventana", JSON.stringify(skill?.rect));
      t.check(skill && skill.text.includes("cd-throwaway-user-skill") && /[A-Z]:\\/.test(skill.text), "el de la fila muestra la ruta entera de la skill", skill?.text);
      t.check(skill && skill.lines > 1, "partida en varias líneas", `${skill?.lines} líneas`);
      await t.shot("skill", "el tooltip de la fila sale entero, con la ruta partida en varias líneas");
    },
  },
];
