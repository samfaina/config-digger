// Section 4: the app's scrollbars, and the Inventory's blocks in the
// smallest window. What a style can show is checked here: the list overflows,
// its scrollbar is the app's 10 px one rather than the system's 17 px, the
// thumb drags to the end. How the thumb looks, and how it lightens under the
// mouse, is judged from screenshots.
import { KEY, ref, unwrap } from "../driver.mjs";
import { go, hoverFresh, panel, press, tab, tooltip, wait } from "../lib.mjs";
import { act, addHome, done, inventoryOf, openItem, project } from "./common.mjs";

// The scrolling element matching `css` (the last one when several match).
async function scroller(s, css) {
  const v = await s.exec(`return [...document.querySelectorAll(arguments[0])].at(-1) ?? null;`, css);
  return unwrap(v);
}

function bar(s, el) {
  return s.exec(
    `const e = arguments[0], r = e.getBoundingClientRect();
     return { overflows: e.scrollHeight > e.clientHeight + 1, gutter: e.offsetWidth - e.clientWidth - parseFloat(getComputedStyle(e).borderLeftWidth) - parseFloat(getComputedStyle(e).borderRightWidth),
              top: e.scrollTop, max: e.scrollHeight - e.clientHeight, rect: { left: r.left, top: r.top, right: r.right, bottom: r.bottom, height: r.height }, ch: e.clientHeight, sh: e.scrollHeight, vh: innerHeight };`,
    ref(el),
  );
}

// Checks one scrolling area as the steps of section 4 do, and returns its
// gutter width so a step can compare it with the Items list's.
async function checkBar(t, css, what, { last } = {}) {
  const { s } = t;
  const el = await s.until(() => scroller(s, css), { what: css });
  await s.exec("arguments[0].scrollTop = 0;", ref(el));
  let b = await bar(s, el);
  t.check(b.overflows, `${what} no cabe entera y se desplaza con su barra`, `${b.sh} px de contenido en ${b.ch} px`);
  t.check(b.gutter > 0 && b.gutter <= 10, `la barra de ${what} es fina, la de la app (10 px), no la del sistema`, `${b.gutter} px`);
  const thumb = { x: Math.round(b.rect.right - 5), y: Math.round(b.rect.top + Math.max(8, (b.ch * b.ch) / b.sh / 2)) };
  await s.hover({ x: Math.round(b.rect.left + 20), y: thumb.y });
  await t.shot(`${what.split(" ")[0]}-quieta`, `la barra de ${what}: fina, sin el carril gris claro del sistema, tirador gris tenue`);
  await s.hover(thumb);
  await wait(200);
  await t.shot(`${what.split(" ")[0]}-encima`, `bajo el ratón, el tirador de ${what} se aclara`);
  await s.cmd("POST", "/actions", {
    actions: [
      {
        type: "pointer",
        id: "mouse",
        parameters: { pointerType: "mouse" },
        actions: [
          { type: "pointerMove", duration: 50, origin: "viewport", x: thumb.x, y: thumb.y },
          { type: "pointerDown", button: 0 },
          { type: "pointerMove", duration: 400, origin: "viewport", x: thumb.x, y: Math.round(Math.min(b.rect.bottom + 40, b.vh - 2)) },
          { type: "pointerUp", button: 0 },
        ],
      },
    ],
  });
  await wait(300);
  b = await bar(s, el);
  t.check(b.top >= b.max - 2, `el tirador de ${what} se arrastra hasta abajo`, `${b.top} de ${b.max}`);
  if (last) {
    const seen = await s.exec(
      `const box = arguments[0].getBoundingClientRect();
       const e = [...arguments[0].querySelectorAll("*")].find((x) => x.children.length === 0 && x.textContent.trim() === arguments[1]);
       if (!e) return false; const r = e.getBoundingClientRect(); return r.top >= box.top - 1 && r.bottom <= box.bottom + 1;`,
      ref(el),
      last,
    );
    t.check(seen, `al final de ${what} sale «${last}»`);
  }
  await s.hover({ x: 300, y: 10 });
  return b.gutter;
}

// Two scrollbars are the same when their gutters match within a pixel: at a
// display scale of 125 %, 10 px may measure 9.
function sameBar(t, gutter, what) {
  t.check(Math.abs(gutter - t.shared.itemsGutter) <= 1, what, `${gutter} px frente a ${t.shared.itemsGutter} px`);
}

export default [
  {
    id: "4.1",
    async run(t) {
      const { s } = t;
      await s.setWindow(1100, 640);
      await addHome(t, "Claude Code", `${t.root}\\claude-code-many-home`, { checks: false });
      await inventoryOf(t, "Claude Code", "claude-code-many-home");
      t.shared.itemsGutter = await checkBar(t, ".panel.items .scroll", "la lista de Items", { last: "cd-throwaway-many-30" });
    },
  },
  {
    id: "4.2",
    async run(t) {
      const { s } = t;
      await go(s, "Projects & homes");
      await tab(s, "Projects");
      // The 30 numbered ones, not many\project-gone, which is Missing.
      const many = await s.exec(
        `return [...document.querySelectorAll(".place-name")].filter((e) => { const n = e.textContent.trim(); return n.startsWith(arguments[0]) && /^\\d\\d$/.test(n.slice(arguments[0].length)); }).length;`,
        `${t.root}\\many\\project-`,
      );
      t.eq(many, 30, `la lista tiene los 30 Projects de «${t.root}\\many»`);
      const gutter = await checkBar(t, ".places-list .scroll", "la lista de Projects");
      sameBar(t, gutter, "la barra es igual a la de la lista de Items del Inventario");
    },
  },
  {
    id: "4.3",
    async run(t) {
      const { s } = t;
      await inventoryOf(t, "Claude Code", "claude-code-many-home");
      await project(t, `${t.root}\\many\\project-01`);
      for (let n = 1; n <= 12; n++) {
        await openItem(t, `cd-throwaway-many-${String(n).padStart(2, "0")}`);
        await act(t, "Disable");
        await done(s);
        await s.until(() => s.exec("return !document.querySelector('.notice.busy, .read-progress')"), { what: "the re-read" });
      }
      await go(s, "History · Undo");
      await s.until(() => s.exec("return !document.querySelector('main .loading')"), { what: "History" });
      const rows = await s.exec(`return [...document.querySelectorAll(".operation-row .operation-title")].map((e) => e.textContent.trim());`);
      const disables = rows.filter((r) => r.startsWith("Disable")).length;
      t.check(disables >= 12, "la lista tiene 12 Operations «Disable»", `${disables} de ${rows.length} filas`);
      const gutter = await checkBar(t, ".history-list .scroll", "la lista de Operations");
      sameBar(t, gutter, "la barra es igual a la de la lista de Items del Inventario");
    },
  },
  {
    id: "4.4",
    async run(t) {
      const { s } = t;
      await go(s, "About");
      await press(s, "Third-party licenses", "main");
      await s.waitFor(".dialog");
      const list = await checkBar(t, ".dialog .licenses-list", "la lista de licencias");
      sameBar(t, list, "la barra de la lista de licencias es igual a la de la lista de Items");
      const first = await s
        .exec(`return document.querySelector(".dialog details.license summary") ?? null;`)
        .then(unwrap);
      if (t.check(first, "la primera licencia se despliega")) {
        await s.exec("arguments[0].scrollIntoView({ block: 'start' });", ref(first));
        await s.click(first);
        await wait(300);
        const text = await checkBar(t, ".dialog details[open] pre.license-text", "el texto de la licencia");
        sameBar(t, text, "la barra del texto de la licencia es igual a la de la lista de Items");
      }
      await press(s, "Close", ".dialog");
    },
  },
  {
    id: "4.5",
    async run(t) {
      const { s } = t;
      await inventoryOf(t, "Claude Code", "claude-code-many-home");
      const field = await s.find("[role=combobox][aria-label=Project]");
      await s.click(field);
      const p = await s.until(() => panel(s), { what: "the panel" });
      t.check(p.scrolls && p.rect.bottom <= p.vh, "el panel no cabe entero en la ventana y se desplaza con su propia barra", JSON.stringify(p.rect));
      const gutter = await checkBar(t, ".select-panel .select-list", "el panel del desplegable");
      sameBar(t, gutter, "la barra es igual a la de la lista de Items del Inventario");
      await s.keys(KEY.Escape);
    },
  },
  {
    id: "4.6",
    async run(t) {
      const { s } = t;
      await s.setWindow(1100, 640);
      await go(s, "Inventory");
      await s.click(await s.waitText(".agent-tile .agent-name", "All Agents"));
      await wait(400);
      const blocks = await s.exec(`
        const block = (css, title) => {
          const p = document.querySelector(css); if (!p) return null;
          const sc = p.querySelector(".scroll, .bars, .home-list") ?? p;
          const r = p.getBoundingClientRect();
          return { title: p.querySelector(".panel-title")?.firstChild?.textContent.trim(), rows: sc.children.length, visibleRows: [...sc.children].filter((c) => { const b = c.getBoundingClientRect(); return b.top >= r.top && b.bottom <= r.bottom + 1; }).length, overflows: sc.scrollHeight > sc.clientHeight + 1, top: r.top, bottom: r.bottom };
        };
        return { agents: block(".panel.agents"), states: block(".panel.states"), homes: block(".panel.homes") };`);
      t.check(blocks.agents?.title === "Agents" && blocks.agents.visibleRows >= 2, "«Agents» muestra su título y al menos dos Agents", JSON.stringify(blocks.agents));
      t.check(blocks.states?.title === "Entries by State" && blocks.states.visibleRows >= 1 && blocks.homes?.title === "Agent home" && blocks.homes.visibleRows >= 1, "«Entries by State» y «Agent home» muestran cada uno su título y al menos una fila", JSON.stringify([blocks.states, blocks.homes]));
      const moved = await s.exec(`
        const tops = () => [...document.querySelectorAll(".panel.agents, .panel.states, .panel.homes")].map((p) => Math.round(p.getBoundingClientRect().top));
        const before = tops();
        const out = [];
        for (const css of [".panel.agents .scroll", ".panel.states .bars", ".panel.homes .home-list"]) {
          const e = document.querySelector(css); if (!e) { out.push(null); continue; }
          e.scrollTop = e.scrollHeight;
          const last = e.lastElementChild?.getBoundingClientRect(), box = e.getBoundingClientRect();
          out.push({ reached: !last || last.bottom <= box.bottom + 1, top: e.scrollTop });
        }
        return { out, same: JSON.stringify(before) === JSON.stringify(tops()) };`);
      t.check(moved.out.every((x) => x === null || x.reached) && moved.same, "al desplazarte dentro de un bloque llegas a su última fila, y los otros no se mueven", JSON.stringify(moved));
      await inventoryOf(t, "Claude Code", "claude-code-many-home");
      t.check(true, "«Claude Code» y «claude-code-many-home» se eligen con un clic");
      await project(t, `${t.root}\\many\\project-01`);
      const projectField = await s.find("[role=combobox][aria-label=Project]");
      await s.hover(projectField);
      await wait(900);
      t.eq((await tooltip(s))?.text, `${t.root}\\many\\project-01`, `el tooltip del campo Project muestra «${t.root}\\many\\project-01» entero`);
      await openItem(t, "cd-throwaway-many-01");
      const layout = await s.exec(`
        const w = (css) => document.querySelector(css)?.getBoundingClientRect().width;
        const row = [...document.querySelectorAll(".item-row")].find((r) => r.querySelector(".item-name")?.textContent.trim() === "cd-throwaway-many-01");
        const rr = (e) => e?.getBoundingClientRect();
        const name = rr(row.querySelector(".item-name")), loc = rr(row.querySelector(".item-location")), icon = rr(row.querySelector(".item-icon")), side = rr(row.querySelector(".item-side"));
        const loce = row.querySelector(".item-location");
        const overlaps = [];
        const texts = [...document.querySelectorAll("main .panel *")].filter((e) => e.children.length === 0 && e.textContent.trim() && e.getClientRects().length);
        return { items: w(".panel.items"), detail: w(".panel.detail"), column: w(".inventory .column"),
          nameNextToIcon: Math.abs(name.left - icon.right) < 24 && loc.top > name.top && Math.abs(loc.left - name.left) < 2,
          sideBelow: side.top >= loc.bottom - 2, ellipsis: loce.scrollWidth > loce.clientWidth && getComputedStyle(loce).textOverflow === "ellipsis" };`);
      t.check(Math.abs(layout.items - layout.detail) / layout.items < 0.2, "la lista de Items y el panel de la derecha tienen más o menos el mismo ancho", `${Math.round(layout.items)} y ${Math.round(layout.detail)}`);
      t.check(layout.column < Math.min(layout.items, layout.detail), "la columna de la izquierda es algo más estrecha", `${Math.round(layout.column)}`);
      t.check(layout.nameNextToIcon, "en cada fila de Items, el nombre y debajo la ubicación van junto al icono");
      t.check(layout.sideBelow, "el número de Entries y el State van en otra línea, bajo el nombre y la ubicación");
      t.check(layout.ellipsis, "la ubicación acaba en «…»");
      const loc = await s
        .exec(`return [...document.querySelectorAll(".item-row")].find((r) => r.querySelector(".item-name")?.textContent.trim() === "cd-throwaway-many-01").querySelector(".item-location");`)
        .then(unwrap);
      await hoverFresh(s, loc);
      await wait(900);
      const tip = await tooltip(s);
      t.check(tip?.text.startsWith(`${t.root}\\claude-code-many-home\\`), "su tooltip muestra la ruta entera", tip?.text);
      await t.shot("bloques", "ningún texto se monta sobre otro");
    },
  },
];
