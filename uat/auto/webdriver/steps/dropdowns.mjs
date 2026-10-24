// Section 2: the app's dropdowns, by mouse and by keyboard.
import { KEY, ref, unwrap } from "../driver.mjs";
import { choose, fieldValue, focused, go, panel, press, tab, tabTo, wait } from "../lib.mjs";
import { inventoryOf } from "./common.mjs";

const rgb = (c) => (c?.match(/\d+(\.\d+)?/g) ?? []).map(Number);
const dark = (c) => {
  const [r, g, b] = rgb(c);
  return r + g + b < 3 * 70;
};
const purple = (c) => {
  const [r, g, b] = rgb(c);
  return b > g + 30 && r > g;
};

// Whether an option or the field shows the Project at `path`: from step 1.3
// on, E:\cd-throwaway\project reads "<path> · 1 location couldn't be read".
const showsProject = (text, path) => text === path || (text ?? "").startsWith(`${path} · `);

async function field(s, label) {
  const v = await s.exec(`return [...document.querySelectorAll("[role=combobox][aria-label='" + arguments[0] + "']")].at(-1) ?? null;`, label);
  return unwrap(v);
}

// Moves the active option with the arrows until it shows the Project at `path`.
async function arrowTo(s, path) {
  for (const key of [KEY.Down, KEY.Up]) {
    for (let i = 0; i < 40; i++) {
      const p = await panel(s);
      if (showsProject(p?.options.find((o) => o.active)?.text, path)) return true;
      await s.keys(key);
    }
  }
  return false;
}

// Types into the open panel, as the tester does without clicking first. When
// the keys reach no search box, the step says so once and clicks the box, as
// a tester would, so its other checks still run.
async function typeInPanel(t, text) {
  const { s } = t;
  const before = (await panel(s))?.search?.value ?? "";
  await s.keys(text);
  await wait(200);
  const after = (await panel(s))?.search?.value ?? "";
  if (after !== before + text) {
    t.note("Al abrir el panel, el foco se queda en el campo y no pasa a la caja «Search Projects…»: lo escrito no llega. Select.tsx enfoca la caja en su useLayoutEffect, cuando el panel aún tiene visibility: hidden, y un elemento oculto no recibe el foco. Con un clic en la caja, el filtro funciona.");
    const box = await s.find(".select-panel input");
    await s.click(box);
    await s.keys(text);
    await wait(200);
    return false;
  }
  return true;
}

export default [
  {
    id: "2.1",
    async run(t) {
      const { s } = t;
      await inventoryOf(t, "Claude Code", "claude-code-home");
      const projectField = await field(s, "Project");
      const before = (await s.text(projectField)).trim();
      await s.click(projectField);
      const p = await s.until(() => panel(s), { what: "the panel" });
      t.check(dark(p.bg), "el panel es oscuro, como los paneles de la app", p.bg);
      t.check(parseFloat(p.radius) > 0 && p.shadow !== "none", "el panel tiene esquinas redondeadas y sombra", `${p.radius}; ${p.shadow}`);
      const chosen = p.options.find((o) => o.chosen);
      t.check(chosen && purple(chosen.color), "el valor actual sale en morado", chosen && `${chosen.text}: ${chosen.color}`);
      const other = p.options.find((o) => !o.chosen && !o.disabled && o.text.startsWith(t.root) && o.text !== before);
      const otherEl = await s
        .exec(`return [...document.querySelectorAll(".select-panel [role=option]")].find((o) => o.textContent.trim() === arguments[0]) ?? null;`, other.text)
        .then(unwrap);
      await s.hover(otherEl);
      await wait(150);
      const lit = (await panel(s)).options.find((o) => o.text === other.text);
      t.check(lit.active && lit.bg !== "rgba(0, 0, 0, 0)", "la opción bajo el ratón se ilumina", `${lit.active} ${lit.bg}`);
      t.check(p.groups.includes("MISSING PROJECTS"), "los Projects que faltan salen bajo el título «MISSING PROJECTS»", p.groups.join(", "));
      await t.shot("panel", "el panel es oscuro, con esquinas redondeadas y sombra; el valor actual en morado; la opción bajo el ratón iluminada");
      await s.click(otherEl);
      await wait(300);
      t.check(!(await panel(s)), "al hacer clic, el panel se cierra");
      t.eq((await s.text(projectField)).trim(), other.text, "el campo muestra el Project elegido");
    },
  },
  {
    id: "2.2",
    async run(t) {
      const { s } = t;
      await inventoryOf(t, "Claude Code", "claude-code-home");
      await choose(s, "Project", `${t.root}\\project`);
      await s.hover({ x: 2, y: 2 });
      // The click of choose left the focus on the field, without a keyboard ring.
      await s.exec("document.activeElement?.blur()");
      await tabTo(s, (e) => e.role === "combobox" && e.label === "Project");
      await wait(300);
      const f = await focused(s);
      t.check(f && (purple(f.border) || purple(f.outline) || /rgb/.test(f.shadow)), "Tab lleva el foco al campo Project, con borde morado", f && `${f.border}; ${f.outline}`);
      await t.shot("foco", "el campo Project con el foco tiene el borde morado");
      const start = await fieldValue(s, "Project");
      await s.keys([KEY.Alt, KEY.Down]);
      let p = await s.until(() => panel(s), { what: "the panel" });
      t.eq(p.options.find((o) => o.active)?.text, start, "Alt+Flecha abajo abre el panel sobre el valor actual");
      await s.keys(KEY.Down);
      const down = (await panel(s)).options.find((o) => o.active)?.text;
      await s.keys(KEY.Up);
      const up = (await panel(s)).options.find((o) => o.active)?.text;
      t.check(down !== start && up === start, "las flechas mueven la opción iluminada", `${start} → ${down} → ${up}`);
      const reached = await typeInPanel(t, "hide");
      p = await panel(s);
      t.check(reached && p.search?.value === "hide", "lo que escribes sale en la caja «Search Projects…» de arriba del panel", p.search?.value);
      t.check(p.options.length === 1 && p.options[0].text === `${t.root}\\project-hide` && p.options[0].active, `la caja deja solo «${t.root}\\project-hide», iluminado`, p.options.map((o) => o.text).join(", "));
      await s.keys(KEY.Escape);
      await wait(200);
      const esc = await focused(s);
      t.check(!(await panel(s)) && (await fieldValue(s, "Project")) === start && esc?.label === "Project", "Escape cierra el panel sin cambiar el Project, y el foco sigue en el campo");
      await s.keys(KEY.Space);
      await s.until(() => panel(s), { what: "the panel" });
      await arrowTo(s, `${t.root}\\project`);
      await s.keys(KEY.Enter);
      await wait(400);
      t.check(!(await panel(s)) && showsProject(await fieldValue(s, "Project"), `${t.root}\\project`), "Enter elige la opción iluminada y cierra el panel");
      await s.keys(KEY.Down);
      await wait(600);
      const next = await fieldValue(s, "Project");
      const closed = !(await panel(s));
      t.check(closed && !showsProject(next, `${t.root}\\project`), "con el panel cerrado, Flecha abajo pasa al Project siguiente sin abrir el panel", next);
      await s.keys(KEY.Up);
      await s.until(() => s.exec("return !document.querySelector('.loading') && !document.querySelector('.read-progress')"), { what: "the read" });
      await wait(500);
      t.check(!(await panel(s)) && showsProject(await fieldValue(s, "Project"), `${t.root}\\project`), `Flecha arriba vuelve a «${t.root}\\project» sin abrir el panel`);
      const items = await s.exec("return [...document.querySelectorAll('.item-row .item-name')].map((e) => e.textContent.trim())");
      t.check(items.includes("cd-throwaway-project"), "cada flecha carga el Project: la lista de Items muestra «cd-throwaway-project»", items.join(", "));
    },
  },
  {
    id: "2.5",
    async run(t) {
      const { s } = t;
      await go(s, "Inventory");
      const projectField = await field(s, "Project");
      const before = await fieldValue(s, "Project");
      await s.click(projectField);
      let p = await s.until(() => panel(s), { what: "the panel" });
      t.check(p.search && p.search.placeholder === "Search Projects…" && p.search.value === "" && p.search.focused, "el panel se abre con la caja «Search Projects…» arriba, vacía y con el cursor dentro", JSON.stringify(p.search));
      const all = p.options.length;
      await typeInPanel(t, "EXTRA");
      p = await panel(s);
      t.check(p.options.length === 1 && p.options[0].text === `${t.root}\\project-extra` && p.options[0].active, `con EXTRA queda solo «${t.root}\\project-extra», iluminado`, p.options.map((o) => o.text).join(", "));
      await s.keys(KEY.Backspace.repeat(5));
      await wait(200);
      t.eq((await panel(s)).options.length, all, "al borrar vuelven todas las opciones");
      await s.keys("throwaway");
      await wait(200);
      p = await panel(s);
      t.check(p.options.every((o) => o.text.replaceAll("/", "\\").startsWith(t.root)), `con throwaway quedan los Projects de «${t.root}»`, p.options.map((o) => o.text).join(", "));
      t.check(p.groups.includes("MISSING PROJECTS"), "los que faltan salen bajo «MISSING PROJECTS»");
      t.check(!p.options.some((o) => o.text === "No Project"), "con throwaway, «No Project» no sale");
      t.eq(p.options.find((o) => o.active)?.text, `${t.root}\\project-extra`, `con throwaway, «${t.root}\\project-extra» sigue iluminado`);
      await s.keys(KEY.Down);
      p = await panel(s);
      const at = p.options.findIndex((o) => o.text === `${t.root}\\project-extra`);
      const lit = p.options.find((o) => o.active)?.text;
      t.eq(lit, p.options[at + 1]?.text, "Flecha abajo ilumina el Project de debajo");
      await s.keys(KEY.Enter);
      await wait(400);
      t.check(!(await panel(s)), "Enter lo elige y el panel se cierra");
      t.eq(await fieldValue(s, "Project"), lit, "el campo muestra el Project elegido");
      const f = await focused(s);
      t.check(f?.label === "Project" && (purple(f.border) || purple(f.outline)), "el foco vuelve al campo, con borde morado", f && `${f.label} ${f.border}`);
      await s.click(projectField);
      p = await s.until(() => panel(s), { what: "the panel" });
      t.eq(p.search?.value, "", "al abrir otra vez, la caja está vacía");
      await typeInPanel(t, "zz");
      t.eq((await panel(s)).empty, "No match", "con zz, el panel dice «No match»");
      const chosen = await fieldValue(s, "Project");
      await s.keys(KEY.Escape);
      await wait(200);
      t.check(!(await panel(s)) && (await fieldValue(s, "Project")) === chosen, "Escape cierra el panel sin cambiar el Project");
      void before;
    },
  },
  {
    id: "2.3",
    async run(t) {
      const { s } = t;
      await s.setWindow(1100, 640);
      await go(s, "Inventory");
      await s.click(await s.waitText(".agent-tile .agent-name", "All Agents"));
      await wait(400);
      const row = await s
        .exec(`return [...document.querySelectorAll(".home-row")].find((r) => r.textContent.includes("Claude Code") && r.querySelector("[role=combobox]")) ?? null;`)
        .then(unwrap);
      if (!t.check(row, "la fila de Claude Code de Agent home tiene su desplegable (dos homes)")) return;
      await s.exec("arguments[0].scrollIntoView({ block: 'nearest' });", ref(row));
      const combo = await s.find("[role=combobox]", row);
      await s.click(combo);
      let p = await s.until(() => panel(s), { what: "the panel" });
      const fieldRect = await s.rect(combo);
      await t.shot("home", "el panel de la fila de Claude Code sale entero debajo del campo");
      t.check(p.rect.top >= fieldRect.y + fieldRect.height - 2, "el panel de la fila de Claude Code se abre debajo del campo", `${p.rect.top} frente a ${fieldRect.y + fieldRect.height}`);
      t.check(p.rect.bottom <= p.vh && p.rect.left >= 0 && p.rect.right <= p.vw, "ese panel sale entero, sin pasar del borde de la ventana", JSON.stringify(p.rect));
      await s.keys(KEY.Escape);
      await go(s, "Projects & homes");
      await tab(s, "Agent homes");
      await press(s, "Add Agent home", ".places");
      const agent = await field(s, "Agent");
      await s.click(agent);
      p = await s.until(() => panel(s), { what: "the Agent panel" });
      const agentRect = await s.rect(agent);
      t.check(p.rect.top >= agentRect.y + agentRect.height - 2 && p.rect.bottom <= p.vh, "el panel de Agent se abre entero debajo de su campo", JSON.stringify(p.rect));
      await s.keys(KEY.Escape);
      await press(s, "Cancel", ".dialog");
    },
  },
  {
    id: "2.4",
    async run(t) {
      const { s } = t;
      await go(s, "Projects & homes");
      await tab(s, "Agent homes");
      await press(s, "Add Agent home", ".places");
      const agent = await field(s, "Agent");
      t.eq((await s.text(agent)).trim(), "Choose an Agent…", "el campo empieza en «Choose an Agent…»");
      await s.click(agent);
      let p = await s.until(() => panel(s), { what: "the panel" });
      const none = p.options.find((o) => o.text === "Choose an Agent…");
      const [r, g, b] = rgb(none?.color);
      t.check(none?.disabled && Math.abs(r - g) < 25 && Math.abs(g - b) < 25 && r < 200, "«Choose an Agent…» sale en gris dentro del panel", none && `${none.color}`);
      const noneEl = await s
        .exec(`return [...document.querySelectorAll(".select-panel [role=option]")].find((o) => o.textContent.trim() === "Choose an Agent…");`)
        .then(unwrap);
      await s.click(noneEl);
      await wait(200);
      p = await panel(s);
      t.check(p && (await s.text(agent)).trim() === "Choose an Agent…", "el clic en esa opción no hace nada y el panel sigue abierto");
      const codex = await s
        .exec(`return [...document.querySelectorAll(".select-panel [role=option]")].find((o) => o.textContent.trim() === "Codex");`)
        .then(unwrap);
      await s.click(codex);
      await wait(200);
      t.check(!(await panel(s)) && (await s.text(agent)).trim() === "Codex", "el clic en «Codex» cierra el panel y el campo muestra «Codex»");
      await press(s, "Cancel", ".dialog");
    },
  },
];
