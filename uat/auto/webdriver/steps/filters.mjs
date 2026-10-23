// Section 6: a row of Entries by State, chosen as a filter.
import { press, wait } from "../lib.mjs";
import { inventoryOf } from "./common.mjs";

function look(s, css) {
  return s.exec(
    `const e = document.querySelector(arguments[0]); if (!e) return null; const c = getComputedStyle(e);
     const track = e.querySelector(".bar-track");
     return { pressed: e.getAttribute("aria-pressed"), bg: c.backgroundColor, image: c.backgroundImage, shadow: c.boxShadow, track: track && getComputedStyle(track).backgroundColor };`,
    css,
  );
}

const items = (s) => s.exec("return [...document.querySelectorAll('.item-row .item-name')].map((e) => e.textContent.trim())");

export default [
  {
    id: "6.1",
    async run(t) {
      const { s } = t;
      await inventoryOf(t, "Claude Code", "claude-code-home");
      const all = await items(s);
      const tile = await look(s, ".agent-tile.selected");
      const row = await s.waitFor(".panel.states .bar");
      const rest = await look(s, ".panel.states .bar");
      await s.hover(row);
      await wait(250);
      const hovered = await look(s, ".panel.states .bar");
      t.check(hovered.image === "none" && hovered.bg !== rest.bg, "bajo el ratón, la fila solo toma un fondo gris un poco más claro", `${rest.bg} → ${hovered.bg}, ${hovered.image}`);
      await s.click(row);
      await wait(300);
      const chosen = await look(s, ".panel.states .bar");
      const chosenTile = await look(s, ".agent-tile.selected");
      t.check(chosen.pressed === "true" && chosen.image.includes("gradient") && chosen.image === chosenTile.image && chosen.shadow === chosenTile.shadow, "al pulsarla, toma el mismo degradado lila con halo que la tarjeta «All Agents»", `${chosen.image} / ${chosen.shadow}; tarjeta ${tile.image}`);
      await s.hover({ x: 600, y: 4 });
      await wait(250);
      const away = await look(s, ".panel.states .bar");
      t.check(away.image === chosen.image, "la fila conserva el degradado al apartar el ratón");
      t.check(away.track !== rest.track, "su carril se oscurece", `${rest.track} → ${away.track}`);
      await t.shot("elegida", "la barra de color y la cuenta se leen bien sobre el degradado lila");
      const state = await s.exec("return document.querySelector('.panel.states .bar .bar-label').textContent.trim()");
      const shown = await s.exec(`return [...document.querySelectorAll(".item-row .state-pill")].map((e) => e.firstChild.textContent.trim());`);
      t.check(shown.length > 0 && shown.every((x) => x === state), `la lista de Items solo muestra los de ese State (${state})`, shown.join(", "));
      await press(s, "clear", ".panel.states");
      await wait(300);
      const cleared = await look(s, ".panel.states .bar");
      t.check(cleared.pressed === "false" && cleared.image === "none", "con clear, la fila vuelve a no tener fondo", `${cleared.image} ${cleared.bg}`);
      t.eq((await items(s)).join(","), all.join(","), "con clear, la lista muestra otra vez todos los Items");
    },
  },
];
