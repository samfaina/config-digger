// Section 15: the loading states of the first read and of a re-read. The
// reads take a few milliseconds here, so each step holds the read's answer
// with a delay (driver.mjs, Session.delay) to look at the state it shows.
import { go, press, refocus, wait } from "../lib.mjs";
import { inventoryOf, openItem } from "./common.mjs";

// What the main area shows: its blocks, the loading block's text, its grey
// rows and whether they and the spinner move.
export function loadingState(s, scope = "main") {
  return s.exec(
    `const root = document.querySelector(arguments[0]);
     const loading = root?.querySelector(".loading");
     const anim = (el) => el ? getComputedStyle(el).animationName : null;
     return {
       loading: loading ? loading.innerText.trim() : null,
       rows: loading ? loading.querySelectorAll(".placeholder-row").length : 0,
       rowAnim: anim(loading?.querySelector(".placeholder-row")),
       spinner: anim(loading?.querySelector(".spinner")),
       blocks: root ? [...root.querySelectorAll(":scope > div > *, :scope > div > .panes > *, :scope > div > .panes > .column > *")].map((e) => e.className) : [],
       titles: [...(root?.querySelectorAll(".panel-title") ?? [])].map((e) => e.firstChild?.textContent.trim()),
     };`,
    scope,
  );
}

export default [
  {
    id: "15.1",
    note: "La app se abre recargando su página en la sesión de WebDriver: la primera lectura es la misma que al abrirla desde el menú Inicio.",
    async run(t) {
      const { s } = t;
      await s.instrument();
      await s.delay({ read_inventory: 2500 });
      await s.reload();
      const during = await s.until(async () => {
        const state = await loadingState(s).catch(() => null);
        return state?.loading ? state : null;
      }, { what: "the first read's loading state" });
      await t.shot("leyendo", "la rueda gira y las filas grises laten");
      const blocks = await s.exec(`return [...document.querySelector(".inventory").children].map((e) => e.className);`);
      t.check(blocks.length === 2 && blocks[1] === "loading", "antes de los paneles, el cuerpo es un único panel", blocks.join(", "));
      t.eq(during.loading, "Reading the inventory…", "el aviso dice «Reading the inventory…»");
      t.check(during.spinner && during.spinner !== "none", "el aviso lleva una rueda que gira", during.spinner);
      t.check(during.rows > 0 && during.rowAnim !== "none", "bajo el aviso laten filas grises", `${during.rows} filas, animación ${during.rowAnim}`);
      await s.delay({});
      const after = await s.until(async () => {
        const state = await loadingState(s);
        return !state.loading && state.titles.includes("Details") ? state : null;
      }, { what: "the panels" });
      for (const title of ["Agents", "Items", "Details"]) t.check(after.titles.includes(title), `luego sale el panel «${title}»`);
    },
  },
  {
    id: "15.2",
    note: "La vuelta a la ventana se simula con el evento focus de la ventana, el que escucha la app.",
    async run(t) {
      const { s } = t;
      // The home of 1.2, whose Items the read keeps in view.
      await inventoryOf(t, "Claude Code", "claude-code-home");
      await s.instrument();
      await s.delay({ read_inventory: 2500 });
      const refresh = await press(s, "Refresh", "header");
      await wait(200);
      t.eq((await s.text(refresh)).trim().toLowerCase(), "reading", "con Refresh, el botón dice «Reading»");
      const bar = await s.exec(`
        const bar = document.querySelector("header .read-progress, .read-progress");
        const header = document.querySelector("header.banner");
        if (!bar) return null;
        const b = bar.getBoundingClientRect(), h = header.getBoundingClientRect();
        return { left: b.left - h.left, right: h.right - b.right, bottomGap: Math.abs(h.bottom - b.bottom), height: b.height, anim: getComputedStyle(bar.firstElementChild ?? bar, "::before").animationName + "/" + getComputedStyle(bar.firstElementChild ?? bar).animationName };`);
      t.check(bar && bar.left <= 16 && bar.right <= 16 && bar.bottomGap <= 16 && bar.height <= 6, "por el borde de abajo de la cabecera corre una barra fina de lado a lado", JSON.stringify(bar));
      await t.shot("barra", "la barra fina corre por el borde de abajo de la cabecera");
      const items = await s.exec("return document.querySelectorAll('.item-row').length");
      t.check(items > 0, "los Items siguen a la vista mientras la barra corre", `${items} filas`);
      const name = await s.exec("return document.querySelector('.item-row .item-name')?.textContent.trim()");
      const details = await openItem(t, name);
      const stillReading = await s.exec("return !!document.querySelector('.read-progress')");
      t.check(details.includes(name) && stillReading, "el clic durante la lectura abre el Item en Details", `${name}, barra ${stillReading}`);
      await s.until(() => s.exec("return !document.querySelector('.read-progress')"), { what: "the read's end" });
      await refocus(s);
      await wait(200);
      const back = await s.exec("return { bar: !!document.querySelector('.read-progress'), loading: !!document.querySelector('.loading') }");
      t.check(back.bar, "al volver a la ventana sale la misma barra");
      t.check(!back.loading, "al volver a la ventana no sale el estado de carga de la primera lectura");
      await s.delay({});
      await s.until(() => s.exec("return !document.querySelector('.read-progress')"), { what: "the read's end" });
    },
  },
  {
    id: "15.3",
    note: "La app se abre recargando su página, con las lecturas retenidas 3 s, para ver cada pantalla mientras lee.",
    async run(t) {
      const { s } = t;
      await s.setWindow(1100, 640);
      await s.instrument();
      await s.delay({ read_places: 4000, read_history: 4000, read_settings: 4000, read_about: 4000 });
      await s.reload();
      await s.waitFor(".rail button[aria-label='Settings']");
      const screens = [
        ["Projects & homes", "Reading the Projects and Agent homes…"],
        ["History · Undo", "Reading History…"],
        ["Settings", "Reading where config-digger keeps its data…"],
      ];
      const historyTitle = () => s.exec(`return document.querySelector(".history .banner-title")?.textContent;`);
      for (const [screen, text] of screens) {
        await go(s, screen);
        const state = await s.until(async () => {
          const st = await loadingState(s);
          return st.loading ? st : null;
        }, { what: `the loading state of ${screen}`, timeout: 3000 }).catch(() => null);
        await t.shot(screen.split(" ")[0].toLowerCase(), `${screen}: rueda que gira y filas grises que laten`);
        t.eq(state?.loading, text, `en ${screen}, el aviso dice «${text}»`);
        t.check(state && state.spinner !== "none" && state.rows > 0 && state.rowAnim !== "none", `en ${screen}, rueda que gira y filas grises que laten debajo`, JSON.stringify(state));
        const stray = await s.exec(`
          const loading = document.querySelector("main .loading");
          const owner = loading?.parentElement;
          return owner ? [...owner.children].filter((e) => e !== loading && e.matches("p") && e.textContent.trim()).map((e) => e.textContent.trim()) : [];`);
        t.check(stray.length === 0, `en ${screen}, no sale una línea de texto suelta en lugar del aviso`, stray.join(" | "));
        if (screen === "History · Undo") {
          const title = await historyTitle();
          t.eq(title, "Operations", "en History, mientras sale el aviso, el título de la cabecera dice «Operations», sin ningún número");
        }
      }
      await s.delay({});
      await wait(4500);
      for (const [screen] of screens) {
        await go(s, screen);
        const done = await s.until(async () => !(await loadingState(s)).loading, { what: `${screen} read`, timeout: 8000 }).catch(() => false);
        t.check(done, `luego salen los datos de ${screen}`);
        if (screen === "History · Undo") {
          const title = await historyTitle();
          t.check(/^\d+ Operations?$/.test(title ?? ""), "cuando salen los datos de History, su título dice cuántas Operations hay, con el número delante", title);
        }
      }
      await go(s, "Inventory");
    },
  },
];
