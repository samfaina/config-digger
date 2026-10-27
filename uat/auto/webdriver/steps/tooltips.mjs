// Section 3: the app's tooltips, by mouse and by keyboard.
import { KEY, ref, unwrap } from "../driver.mjs";
import { focused, go, hoverFresh, press, tabTo, tooltip, wait } from "../lib.mjs";
import { act, dialog, done, inventoryOf, openItem, project, waitPreview } from "./common.mjs";

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
        .then(unwrap);
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
        .then(unwrap);
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
  {
    id: "3.4",
    async run(t) {
      const { s } = t;
      const rings = [];
      // Tabs to the first element `css` matches, whose text holds `text`, and
      // returns its tooltip, or null when no tab stop matches.
      const read = async (css, text = "") => {
        if (!(await tabToMatch(s, css, text))) return null;
        await wait(150);
        rings.push({ css, outline: (await focused(s))?.outline });
        return tooltip(s);
      };
      const desktopRow = (state) =>
        s
          .exec(
            `return [...document.querySelectorAll(".item-row")].find((r) => r.querySelector(".item-name")?.textContent.trim() === "cd-throwaway-desktop" && r.querySelector(".state-pill")?.firstChild?.textContent.trim() === arguments[0]) ?? null;`,
            state,
          )
          .then(unwrap);
      const reread = () => s.until(() => s.exec("return !document.querySelector('.notice.busy, .read-progress')"), { what: "the re-read" });

      await go(s, "Inventory");
      await s.click(await s.waitText(".agent-tile .agent-name", "All Agents"));
      await s.until(() => s.exec("return !document.querySelector('.loading')"), { what: "the Inventory read" });
      await s.hover({ x: 600, y: 4 });
      await s.exec("document.activeElement?.blur()");
      const time = await read(".banner-value");
      t.check(time && /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$/.test(time.text), "en la hora de «Inventory read» sale al momento un tooltip con la fecha y la hora exactas, con segundos", time?.text);
      const folder = await read(".single-home");
      t.check(folder && /^[A-Za-z]:\\/.test(folder.text), "en el nombre del home sale un tooltip con su carpeta entera", folder?.text ?? "ningún Agent tiene un solo home");

      const mcpJson = `${t.root}\\project\\.mcp.json`;
      await inventoryOf(t, "Claude Code", "claude-code-home");
      await project(t, `${t.root}\\project`);
      await openItem(t, "cd-throwaway-project");
      await press(s, "Remove", ".panel.detail");
      await waitPreview(s);
      const file = await read(".change-file-name .mono");
      t.eq(file?.text, mcpJson, `en «.mcp.json» sale un tooltip con «${mcpJson}»`);
      const approval = await read(".check.info", "approval");
      t.eq(approval?.text, mcpJson, `en la nota de la aprobación sale un tooltip con «${mcpJson}»`);
      await press(s, "Cancel", ".dialog");
      await s.until(() => s.exec("return !document.querySelector('.dialog')"), { what: "the dialog closed" });

      const config = `${t.root}\\paths-that-wrap\\in-the-smallest-window\\of-config-digger\\claude-desktop-home\\claude_desktop_config.json`;
      await inventoryOf(t, "Claude Desktop", "claude-desktop-home");
      await s.click(await s.until(() => desktopRow("Effective"), { what: "cd-throwaway-desktop Effective" }));
      await wait(400);
      await act(t, "Disable");
      await done(s);
      await reread();
      let enabled = false;
      try {
        await s.click(await s.until(() => desktopRow("Disabled"), { what: "cd-throwaway-desktop Disabled" }));
        await wait(400);
        const parked = await read(".panel.detail .tag[tabindex]");
        const [from, on] = parked?.text.split("\n") ?? [];
        t.eq(from, `Parked from ${config}#mcpServers.cd-throwaway-desktop`, `en la etiqueta sale un tooltip que empieza por «Parked from ${config}#mcpServers.cd-throwaway-desktop»`);
        t.check(on?.startsWith("on "), "la segunda línea de ese tooltip empieza por «on » y sigue con la fecha en que se aparcó", on);
        await t.shot("parked", "la etiqueta «parked from» tiene un anillo violeta y su tooltip encima o debajo");
        await act(t, "Enable");
        await done(s);
        enabled = true;
        await reread();
        t.check(await desktopRow("Effective"), "tras Enable, cd-throwaway-desktop vuelve a salir como «Effective»");
      } finally {
        // A parking left behind would count once more in 13.1 and 13.3.
        if (!enabled) await enableDisabled(t, desktopRow).catch(() => {});
      }

      const notViolet = rings.filter((ring) => !violet(ring.outline?.split(" ").slice(1).join(" ")));
      t.check(rings.length === 5 && notViolet.length === 0, "cada elemento al que llega el foco tiene un anillo violeta", rings.map((ring) => `${ring.css}: ${ring.outline}`).join("; "));
      await inventoryOf(t, "Claude Code", "claude-code-synced-home");
      await openItem(t, "cd-throwaway-synced:cd-throwaway-synced-skill");
      const notes = await s.exec(`return [...document.querySelectorAll(".panel.detail .action-note")].map((e) => e.textContent.trim());`);
      const why = "Remove is not offered: this Entry is managed, or a copy of the user's account, and config-digger only reads it.";
      t.check(notes.includes(why), `bajo los botones de la skill dice «${why}», sin pasar el ratón por ningún sitio`, notes.join(" | "));
    },
  },
];

const rgb = (c) => (c?.match(/\d+(\.\d+)?/g) ?? []).map(Number);
const violet = (c) => {
  const [r, g, b] = rgb(c);
  return b > g + 40 && r > g;
};

// Presses Tab until the focused element matches `css` and its text holds
// `text`, at most `max` times.
async function tabToMatch(s, css, text, max = 60) {
  for (let i = 0; i < max; i++) {
    if (await s.exec("const e = document.activeElement; return !!e?.matches(arguments[0]) && e.textContent.includes(arguments[1]);", css, text)) return true;
    await s.keys(KEY.Tab);
  }
  return false;
}

// Enables the Disabled cd-throwaway-desktop that 3.4 parked, when the step
// stopped before its own Enable.
async function enableDisabled(t, desktopRow) {
  const { s } = t;
  if (await dialog(s)) await press(s, "Cancel", ".dialog").catch(() => press(s, "Done", ".dialog"));
  const row = await desktopRow("Disabled");
  if (!row) return;
  await s.click(row);
  await wait(400);
  await act(t, "Enable");
  await done(s);
}
