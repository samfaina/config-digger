// Section 10: the Projects & homes screen, with real list changes on the
// throwaway profile.
import { dirname, join } from "node:path";
import { KEY, ref, unwrap } from "../driver.mjs";
import { buttonEl, choose, fieldValue, go, notices, panel, press, rowButton, tab, typeInto, wait, waitNotice } from "../lib.mjs";
import { act, addHome, checkBeforeYouApply, confirm, DESKTOP_HOME, dialog, dialogParts, done, homeGroups, inventoryOf, openItem, operationId, pickFolder, slashPath, waitPreview } from "./common.mjs";
import { loadingState } from "./loading.mjs";

// The Projects tab, as groups: each heading's text, whether it is open, and
// its rows with their texts, tags and buttons.
export function projectGroups(s) {
  return s.exec(`
    const groups = [];
    for (const el of document.querySelectorAll(".places .scroll > *")) {
      if (el.matches("h3.place-group")) {
        const toggle = el.querySelector(".place-group-toggle");
        groups.push({ title: el.textContent.trim(), open: toggle ? toggle.getAttribute("aria-expanded") === "true" : true, rows: [] });
      } else if (el.matches(".place-row") && groups.length) {
        groups.at(-1).rows.push({
          name: el.querySelector(".place-name")?.textContent.trim(),
          sub: el.querySelector(".place-sub")?.textContent.trim(),
          subs: [...el.querySelectorAll(".place-sub")].map((e) => e.textContent.trim()),
          tags: [...el.querySelectorAll(".tags .tag")].map((e) => e.textContent.trim()),
          count: el.querySelector(".place-count")?.textContent.trim(),
          buttons: [...el.querySelectorAll("button")].map((b) => ({ text: b.textContent.trim(), disabled: b.disabled })),
          opacity: getComputedStyle(el).opacity,
          color: getComputedStyle(el.querySelector(".place-name") ?? el).color,
        });
      } else if (el.matches("p.empty") && groups.length) groups.at(-1).empty = el.textContent.trim();
    }
    return groups;`);
}

const inGroup = (groups, title, name) => groups.find((g) => g.title.startsWith(title))?.rows.find((r) => r.name === name);

// The row named `name` of the open tab, or null.
const placeRow = (s, name) =>
  s.exec(`return [...document.querySelectorAll(".place-row")].find((r) => r.querySelector(".place-name")?.textContent.trim() === arguments[0]) ?? null;`, name).then(unwrap);

// The count on the row named `name` of the open tab, such as "2 Entries" or "1 leftover".
const rowCount = (s, name) =>
  s.exec(
    `return [...document.querySelectorAll(".place-row")].find((r) => r.querySelector(".place-name")?.textContent.trim() === arguments[0])?.querySelector(".place-count")?.textContent.trim() ?? null;`,
    name,
  );

// The Missing group, of the missing Projects with leftovers, and the one of those without.
const missingGroup = (groups) => groups.find((g) => /Missing · \d/.test(g.title));
const noLeftoversGroup = (groups) => groups.find((g) => g.title.includes("Missing, no leftovers · "));

// Opens the group titled `title`, such as "Missing" or "Missing, no leftovers", if it is closed.
// Fails when no group has that title, once the tab has had time to read its list.
export async function openGroup(s, title, opts) {
  const toggle = await s.until(
    () =>
      s
        .exec(`return [...document.querySelectorAll(".place-group-toggle")].find((b) => b.textContent.replace(/^[▾▸]\\s*/, "").startsWith(arguments[0] + " · ")) ?? null;`, title)
        .then(unwrap),
    { what: `the group "${title}"`, ...opts },
  );
  if ((await s.attr(toggle, "aria-expanded")) !== "true") {
    await s.click(toggle);
    await wait(200);
  }
}

// What is wrong with how the paths of a tab wrap, each path as its lines laid
// out: none takes more than one line, so none shows where it wraps, or a line
// but the last ends inside a folder's name rather than right after a `\` or a
// `/`. Undefined when a path wraps and each wraps only at a separator. The
// fake UI's tests have the same check, `wrapFault` in
// crates/config-digger-app/ui/uat/wraps.ts.
export function wrapFault(paths) {
  if (!paths.some((lines) => lines.length > 1)) return `ninguna de las ${paths.length} rutas ocupa más de una línea, así que ninguna muestra dónde se parte`;
  const midWord = paths.find((lines) => lines.slice(0, -1).some((line) => !/[\\/]$/.test(line)));
  return midWord && `una ruta se parte dentro del nombre de una carpeta: ${midWord.join(" ⏎ ")}`;
}

// How many lines the text of an element takes, from the boxes of its
// contents, each `{ text, top }`, `text` true for a box of a text node. The
// boxes of other elements, such as an Add button's icon, a flex item centred
// on its own, don't count, and a top within 2 px of a line's first is on that
// line, the tolerance `linesOf` reads the paths with. Step 10.10 runs its
// source in the page, so it uses nothing from this module.
export function textLines(boxes) {
  const tops = boxes.filter((box) => box.text).map((box) => box.top).sort((a, b) => a - b);
  let lines = 0, lineTop;
  for (const top of tops) if (lineTop === undefined || top - lineTop > 2) [lines, lineTop] = [lines + 1, top];
  return lines;
}

// Presses `button` in the row of `path`, holds `command` so its busy state
// shows, and returns what the busy notice and the row's buttons showed.
async function busyPress(t, path, button, command, busyText) {
  const { s } = t;
  await s.instrument();
  await s.delay({ [command]: 1500 });
  const el = await s.until(() => rowButton(s, path, button), { what: `${button} of ${path}` });
  await s.click(el);
  const busy = await waitNotice(s, busyText, { timeout: 1500 }).catch(() => null);
  const row = await s.exec(
    `const row = [...document.querySelectorAll(".place-row")].find((r) => r.querySelector(".place-name")?.textContent.trim() === arguments[0]);
     return row ? [...row.querySelectorAll("button")].map((b) => ({ text: b.textContent.trim(), disabled: b.disabled })) : null;`,
    path,
  );
  await s.delay({});
  return { busy, row };
}

const allDisabled = (buttons) => Array.isArray(buttons) && buttons.length > 0 && buttons.every((b) => b.disabled);

// Scrolls the first element matching `css` whose text holds `text` to the
// middle of what scrolls it, so that the shot taken next shows it.
async function scrollTo(s, css, text = "") {
  await s.exec(`[...document.querySelectorAll(arguments[0])].find((e) => e.textContent.includes(arguments[1]))?.scrollIntoView({ block: "center" });`, css, text);
  await wait(200);
}

// The red, green, blue and alpha of a computed colour.
const rgb = (colour) => (colour.match(/\d+(\.\d+)?/g) ?? []).map(Number);

// A grey: its red, green and blue within 20 of each other.
const grey = (colour) => {
  const [r, g, b] = rgb(colour);
  return Math.abs(r - g) < 20 && Math.abs(g - b) < 20;
};

// Checks that each tag `css` matches is a filled grey pill with a grey
// outline and nearly white text, as steps 10.16 and 10.21 expect.
async function checkTagLook(t, css) {
  const looks = await t.s.exec(
    `return [...document.querySelectorAll(arguments[0])].map((tag) => {
       const c = getComputedStyle(tag);
       return { text: tag.textContent.trim(), bg: c.backgroundColor, border: c.borderTopWidth + " " + c.borderTopStyle, edge: c.borderTopColor, color: c.color };
     });`,
    css,
  );
  const each = (ok, what) => {
    const wrong = looks.filter((look) => !ok(look));
    t.check(looks.length > 0 && wrong.length === 0, what, looks.length === 0 ? "ninguna etiqueta" : wrong.map((look) => `${look.text}: ${JSON.stringify(look)}`).join("; "));
  };
  each((look) => (rgb(look.bg)[3] ?? 1) > 0 && grey(look.bg), "cada etiqueta es una píldora rellena de gris");
  each((look) => look.border.includes("solid") && !look.border.startsWith("0px") && grey(look.edge), "cada etiqueta tiene un borde gris alrededor");
  each((look) => rgb(look.color).slice(0, 3).every((channel) => channel > 200), "el texto de cada etiqueta es casi blanco");
}

export default [
  {
    id: "10.1",
    async run(t) {
      const { s } = t;
      await go(s, "Inventory");
      const icon = await s.waitFor(".rail button[aria-label='Projects & homes']");
      await s.hover(icon);
      const tip = await s
        .until(() => s.exec(`return [...document.querySelectorAll(".tooltip")].find((e) => !e.hidden && getComputedStyle(e).visibility !== "hidden")?.textContent ?? null;`), {
          what: "the tooltip",
          timeout: 3000,
        })
        .catch(() => null);
      t.eq(tip, "Projects & homes", "el tooltip dice «Projects & homes»");
      await s.click(icon);
      await wait(300);
      const tabs = await s.exec(`return [...document.querySelectorAll("[role=tab]")].map((e) => e.textContent.trim());`);
      t.check(tabs.some((x) => /^Projects · \d+$/.test(x)) && tabs.some((x) => /^Agent homes · \d+$/.test(x)), "salen las pestañas «Projects · N» y «Agent homes · N»", tabs.join(", "));
      await tab(s, "Projects");
      t.check(await buttonEl(s, "Add Project", ".places"), "con Projects, el botón de arriba es «Add Project»");
      await tab(s, "Agent homes");
      t.check(await buttonEl(s, "Add Agent home", ".places"), "con Agent homes, el botón de arriba es «Add Agent home»");
    },
  },
  {
    id: "10.2",
    async run(t) {
      await addHome(t, "Codex", `${t.root}\\codex-home`);
    },
  },
  {
    id: "10.3",
    async run(t) {
      await addHome(t, "Claude Desktop", `${t.root}\\${DESKTOP_HOME}`);
    },
  },
  {
    id: "10.4",
    async run(t) {
      const out = t.ps(t.step.commands[0].text + " | Out-String -Width 300");
      const rows = out
        .split(/\r?\n/)
        .slice(2)
        .map((l) => l.trim().split(/\s+/))
        .filter((r) => r.length === 2);
      const homes = new Map(rows.map(([agent, folder]) => [folder, agent]));
      const paths = [];
      for (const [agent, folder] of [
        ["claude-code", "claude-code-home"],
        ["codex", "codex-home"],
        ["claude-desktop", DESKTOP_HOME],
      ]) {
        const path = `${t.root}\\${folder}`;
        paths.push(path);
        t.eq(homes.get(path), agent, `una fila dice «${agent}» con «${path}»`);
      }
      // Step 1.3 adds claude-code-shape-home too, which is none of those folders.
      const extra = rows.filter(([, folder]) => paths.includes(folder)).length;
      t.eq(extra, 3, "ninguna de esas carpetas sale con otro Agent");
    },
  },
  {
    id: "10.16",
    async run(t) {
      const { s } = t;
      await go(s, "Projects & homes");
      await tab(s, "Agent homes");
      const groups = await homeGroups(s);
      for (const [agent, folder] of [
        ["Claude Code", "claude-code-home"],
        ["Codex", "codex-home"],
        ["Claude Desktop", DESKTOP_HOME],
      ]) {
        const row = groups.find((g) => g.agent === agent)?.rows.find((r) => r.name === `${t.root}\\${folder}`);
        t.check(row && row.tags.join("|") === `${agent}|added by you`, `la fila de ${folder} tiene dos etiquetas: «${agent}» y «added by you»`, row?.tags.join(", "));
      }
      // A Mirrored home's read-only tag has a tone of its own.
      await checkTagLook(t, ".places .tag:not(.read-only)");
      await t.shot("etiquetas", "las etiquetas son píldoras grises con borde gris claro y texto casi blanco");
    },
  },
  {
    id: "10.20",
    async run(t) {
      const { s } = t;
      const path = `${t.root}\\project-hide`;
      await go(s, "Projects & homes");
      await tab(s, "Projects");
      if (!(await rowButton(s, path, "Hide"))) {
        // A run of this step alone may find project-hide still hidden.
        await openGroup(s, "Hidden");
        await s.click(await s.until(() => rowButton(s, path, "Unhide"), { what: "Unhide" }));
        await waitNotice(s, `${path} is listed again.`);
      }
      await s.click(await s.until(() => rowButton(s, path, "Hide"), { what: "Hide" }));
      await waitNotice(s, `Hid ${path}`);
      await openGroup(s, "Hidden");
      // The border, background and tags of `row`, as drawn.
      const look = (row) =>
        s.exec(
          `const c = getComputedStyle(arguments[0]);
           return { border: c.borderTopStyle, edge: c.borderTopColor, bg: c.backgroundColor, tags: [...arguments[0].querySelectorAll(".tag")].map((e) => e.textContent.trim()) };`,
          ref(row),
        );
      const hiddenRow = await s.until(() => placeRow(s, path), { what: `the hidden row of ${path}` });
      const noneRow = await s.until(() => placeRow(s, "No Project"), { what: "the row No Project" });
      const hidden = await look(hiddenRow);
      const none = await look(noneRow);
      const clear = (colour) => /rgba\(.*,\s*0\)$/.test(colour) || colour === "transparent";
      t.check(hidden.border === "dashed" && clear(hidden.bg), `la fila de «${path}» en Hidden tiene un borde discontinuo sobre el panel`, `${hidden.border} ${hidden.bg}`);
      t.eq(hidden.tags.join("|"), "known to Claude Code|hidden", `la fila de «${path}» en Hidden lleva las etiquetas «known to Claude Code» y «hidden»`);
      t.check(clear(none.edge), "la fila «No Project» no tiene ningún borde visible", `${none.border} ${none.edge}`);
      t.check(clear(none.bg), "la fila «No Project» no tiene el fondo de las otras filas", none.bg);
      t.check(none.tags.length === 0, "la fila «No Project» no lleva ninguna etiqueta", none.tags.join(", "));
      await s.exec("arguments[0].scrollIntoView({ block: 'nearest' });", ref(hiddenRow));
      await wait(200);
      await t.shot("oculta", "el nombre de la fila oculta se lee claro", "la etiqueta «known to Claude Code» de la fila oculta se lee clara", "la etiqueta «hidden» se lee clara");
      await s.exec("arguments[0].scrollIntoView({ block: 'nearest' });", ref(noneRow));
      await wait(200);
      await t.shot("no-project", "el nombre «No Project» se lee claro", "el texto bajo «No Project» se lee claro");
      await s.click(await s.until(() => rowButton(s, path, "Unhide"), { what: "Unhide" }));
      const listed = await waitNotice(s, `${path} is listed again`);
      t.eq(listed.text, `${path} is listed again.`, `Unhide da el aviso «${path} is listed again.»`);
    },
  },
  {
    id: "10.19",
    async run(t) {
      const { s } = t;
      const home = `${t.root}\\claude-code-home`;
      await go(s, "Projects & homes");
      await tab(s, "Agent homes");
      const count = await s.until(() => rowCount(s, home), { what: `the count of ${home}` }).catch(() => null);
      t.eq(count, "2 Entries", `la fila de «${home}» dice «2 Entries»`);
      await s.click(await s.until(() => rowButton(s, home, "Show in Inventory"), { what: "Show in Inventory" }));
      await s.until(() => s.exec("return !!document.querySelector('.inventory') && !document.querySelector('.loading')"), { what: "the Inventory" });
      await wait(300);
      const state = await s.exec(`return {
        home: [...document.querySelectorAll(".home-tile")].find((b) => b.getAttribute("aria-pressed") === "true")?.querySelector(".home-name")?.textContent.trim() ?? null,
        items: [...document.querySelectorAll(".item-row .item-name")].map((e) => e.textContent.trim()),
      };`);
      const project = await fieldValue(s, "Project");
      t.eq(project, "No Project", "el campo Project del Inventario dice «No Project»");
      t.check(state.home?.endsWith("claude-code-home"), `la ficha elegida del panel Agent home es «${home}»`, state.home);
      t.eq([...state.items].sort().join(", "), "cd-throwaway-user, cd-throwaway-user-skill", "la lista de Items muestra «cd-throwaway-user» y «cd-throwaway-user-skill», y ningún otro Item");
      t.check(!state.items.some((name) => name === "cd-throwaway-local" || name === "cd-throwaway-leftover"), "«cd-throwaway-local» y «cd-throwaway-leftover» no salen en la lista", state.items.join(", "));
    },
  },
  {
    id: "10.5",
    async run(t) {
      const { s } = t;
      await go(s, "Projects & homes");
      await tab(s, "Projects");
      const groups = await projectGroups(s);
      for (const name of ["project", "project-hide"]) {
        const row = inGroup(groups, "Projects", `${t.root}\\${name}`);
        t.check(row, `sale «${t.root}\\${name}»`);
        t.eq(row?.subs.join(" | "), "Folder", `${name} tiene la línea «Folder»`);
        t.eq(row?.tags.join("|"), "known to Claude Code", `${name} lleva la etiqueta «known to Claude Code»`);
      }
      const all = groups.flatMap((g) => g.rows.map((r) => r.name));
      for (const name of ["codex-home", DESKTOP_HOME]) t.check(!all.includes(`${t.root}\\${name}`), `«${t.root}\\${name}» no sale como Project`);
      const missing = missingGroup(groups);
      t.eq(missing?.title, "▾ Missing · 1", "el grupo «▾ Missing · 1»");
      t.check(missing?.open, "el grupo Missing está abierto");
      const gone = missing?.rows[0];
      t.eq(gone?.name, `${t.root}\\project-gone`, `en Missing sale «${t.root}\\project-gone»`);
      t.eq(gone?.subs.join(" | "), "The folder no longer exists", "con la línea «The folder no longer exists»");
      t.eq(gone?.tags.join("|"), "known to Claude Code", "con la etiqueta «known to Claude Code»");
      t.eq(gone?.count, "1 leftover", "y «1 leftover»");
      t.check(!missing?.rows.some((r) => /project-deleted$/.test(r.name)), `«${t.root}\\project-deleted» no sale en el grupo «Missing · 1»`, missing?.rows.map((r) => r.name).join(", "));
      const next = groups[groups.indexOf(missing) + 1];
      t.check(next?.title === "▸ Missing, no leftovers · 1" && !next.open, "tras Missing sale el grupo «▸ Missing, no leftovers · 1», cerrado", next && `${next.title} ${next.open ? "abierto" : "cerrado"}`);
    },
  },
  {
    id: "10.17",
    async run(t) {
      const { s } = t;
      const search = "input[aria-label='Search Projects']";
      await go(s, "Projects & homes");
      await tab(s, "Projects");
      await typeInto(s, search, "deleted");
      let noLeftovers = noLeftoversGroup(await projectGroups(s));
      const row = noLeftovers?.rows.find((r) => /project-deleted$/.test(r.name));
      const deleted = slashPath(t, "project-deleted");
      t.check(noLeftovers?.title === "Missing, no leftovers · 1" && noLeftovers.open, "con deleted, el grupo «Missing, no leftovers · 1» se abre solo", noLeftovers && `${noLeftovers.title} ${noLeftovers.open ? "abierto" : "cerrado"}`);
      t.check(row && t.path(row.name, deleted), `con deleted, ese grupo muestra «${deleted}»`, noLeftovers?.rows.map((r) => r.name).join(", "));
      t.check(
        row?.subs.join(" | ") === "The folder no longer exists" && row.count === "No leftovers" && row.tags.join("|") === "known to Claude Code",
        "la fila de project-deleted dice «The folder no longer exists» y «No leftovers», y lleva la etiqueta «known to Claude Code»",
        row && `${row.subs.join(" | ")}; ${row.count}; ${row.tags.join(", ")}`,
      );
      t.eq(row?.buttons.map((b) => b.text).join(", "), "Hide", "el único botón de la fila es «Hide»");
      await typeInto(s, search, "");
      noLeftovers = noLeftoversGroup(await projectGroups(s));
      t.check(noLeftovers?.title === "▸ Missing, no leftovers · 1" && !noLeftovers.open, "al borrar lo escrito, el grupo vuelve a «▸ Missing, no leftovers · 1», cerrado", noLeftovers && `${noLeftovers.title} ${noLeftovers.open ? "abierto" : "cerrado"}`);
      await openGroup(s, "Missing, no leftovers");
      const path = row?.name ?? deleted;
      await s.click(await s.until(() => rowButton(s, path, "Hide"), { what: "Hide" }));
      const hid = await waitNotice(s, `Hid ${path}`);
      t.eq(hid.text, `Hid ${path}. Picking it in the Inventory still evaluates it.`, `Hide da el aviso «Hid ${path}. Picking it in the Inventory still evaluates it.»`);
      let groups = await projectGroups(s);
      t.check(!noLeftoversGroup(groups), "tras Hide, el grupo «Missing, no leftovers» ya no sale", groups.map((g) => g.title).join(", "));
      await openGroup(s, "Hidden");
      groups = await projectGroups(s);
      const hidden = groups.find((g) => g.title.includes("Hidden · 1"))?.rows.find((r) => r.name === path);
      t.eq(hidden?.tags.join("|"), "known to Claude Code|missing|hidden", "el grupo «Hidden · 1» muestra project-deleted con las etiquetas «known to Claude Code», «missing» y «hidden»");
      await s.click(await s.until(() => rowButton(s, path, "Unhide"), { what: "Unhide" }));
      const listed = await waitNotice(s, `${path} is listed again`);
      t.eq(listed.text, `${path} is listed again.`, `Unhide da el aviso «${path} is listed again.»`);
      await openGroup(s, "Missing, no leftovers");
      noLeftovers = noLeftoversGroup(await projectGroups(s));
      t.check(noLeftovers?.title.endsWith("Missing, no leftovers · 1") && noLeftovers.rows.some((r) => r.name === path), "tras Unhide, project-deleted vuelve al grupo «Missing, no leftovers · 1»", noLeftovers?.title);
      await go(s, "Inventory");
      await s.until(() => s.exec("return !document.querySelector('.loading')"), { what: "the Inventory read" });
      await s.click(await s.exec(`return [...document.querySelectorAll("[role=combobox][aria-label=Project]")].at(-1) ?? null;`).then(unwrap));
      await s.until(() => panel(s), { what: "the panel" });
      const field = await s.exec(`return {
        groups: [...document.querySelectorAll(".select-panel [role=group]")].map((g) => ({ title: g.querySelector(".select-group")?.innerText.trim(), options: [...g.querySelectorAll("[role=option]")].map((o) => o.textContent.trim()) })),
        options: [...document.querySelectorAll(".select-panel [role=option]")].map((o) => o.textContent.trim()),
      };`);
      const missing = field.groups.find((g) => g.title === "MISSING PROJECTS");
      const gone = slashPath(t, "project-gone");
      t.check(missing?.options.some((o) => t.path(o.split(" · ")[0], gone)), `en el campo Project, el grupo «MISSING PROJECTS» muestra «${gone}»`, JSON.stringify(field.groups));
      t.check(!field.options.some((o) => /project-deleted/.test(o)), `«${deleted}» no sale en el campo Project`, field.options.join(", "));
      await s.keys(KEY.Escape);
      await wait(200);
    },
  },
  {
    id: "10.6",
    async run(t) {
      const { s } = t;
      const gone = await goneRow(t);
      await s.click(await s.until(() => rowButton(s, gone, "Show leftovers"), { what: "Show leftovers" }));
      await s.until(() => s.exec("return !!document.querySelector('.inventory') && !document.querySelector('.loading')"), { what: "the Inventory" });
      await wait(300);
      const state = await s.exec(`return {
        project: document.querySelector("[role=combobox][aria-label=Project]")?.textContent.trim(),
        agent: document.querySelector(".banner-title")?.textContent.trim(),
        pressed: [...document.querySelectorAll(".bar[aria-pressed=true] .bar-label")].map((e) => e.textContent.trim()),
        items: [...document.querySelectorAll(".item-row .item-name")].map((e) => e.textContent.trim()),
      };`);
      t.check(state.agent !== undefined, "se abre el Inventario");
      const goneAt = `${t.root}\\project-gone`;
      t.eq(state.project, goneAt, `el campo Project dice «${goneAt}»`);
      t.eq(state.agent, "All Agents", "está elegido «All Agents»");
      t.eq(state.pressed.join(","), "Unread", "el filtro de State es «Unread»");
      t.check(state.items.includes("cd-throwaway-leftover"), "la lista de Items muestra «cd-throwaway-leftover»", state.items.join(", "));
    },
  },
  {
    id: "10.14",
    async run(t) {
      const { s } = t;
      const gone = await goneRow(t);
      await s.instrument();
      await s.delay({ read_inventory: 2000, switch_project: 2000 });
      await s.click(await s.until(() => rowButton(s, gone, "Show leftovers"), { what: "Show leftovers" }));
      const during = await s.until(async () => {
        const st = await loadingState(s);
        return st.loading ? st : null;
      }, { what: "the loading state", timeout: 2000 }).catch(() => null);
      const blocks = await s.exec(`const inv = document.querySelector(".inventory"); return inv ? [...inv.children].map((e) => e.className) : [];`);
      await t.shot("leyendo", "las filas grises laten con la forma de las filas de Items");
      t.check(blocks.length === 2 && blocks[1] === "loading", "nada más pulsar, el Inventario ocupa su cuerpo con un único panel", blocks.join(", "));
      t.eq(during?.loading, "Reading the inventory…", "un aviso con una rueda que gira dice «Reading the inventory…»");
      t.check(during && during.spinner !== "none" && during.rows > 0 && during.rowAnim !== "none", "debajo laten filas grises", JSON.stringify(during));
      t.check(!blocks.some((b) => b.includes("panes")), "bajo la cabecera no salen los paneles de Agents, Items y Details");
      await s.delay({});
      const items = await s.until(
        async () => (await s.exec("return document.querySelector('.loading') ? null : [...document.querySelectorAll('.item-row .item-name')].map((e) => e.textContent.trim())")),
        { what: "the Items" },
      );
      t.check(items.includes("cd-throwaway-leftover"), "al acabar la lectura sale «cd-throwaway-leftover» en la lista de Items", items.join(", "));
    },
  },
  {
    id: "10.7",
    async run(t) {
      const { s } = t;
      const gone = await goneRow(t);
      await s.click(await s.until(() => rowButton(s, gone, "Remove leftovers"), { what: "Remove leftovers" }));
      const d = await waitPreview(s);
      t.eq(d.title, "Remove 1 Entry", "el diálogo se titula «Remove 1 Entry»");
      const parts = await dialogParts(s);
      t.check(parts.files?.title === "Files · 1", "la sección «Files · 1»", parts.files?.title);
      t.check(parts.files?.paths.includes(`${t.root}\\claude-code-home\\.claude.json`), `el diff es de «${t.root}\\claude-code-home\\.claude.json»`, parts.files?.paths.join(", "));
      const red = parts.files?.removed.filter((l) => l.text.includes("cd-throwaway-leftover")) ?? [];
      const reddish = (c) => {
        const [r, g, b] = (c.match(/\d+/g) ?? []).map(Number);
        return r > g + 40 && r > b + 40;
      };
      t.check(red.length > 0 && red.every((l) => reddish(l.color) || reddish(l.bg)), "el diff marca en rojo las líneas de «cd-throwaway-leftover»", JSON.stringify(red.slice(0, 2)));
      await scrollTo(s, ".dialog .diff-removed", "cd-throwaway-leftover");
      await t.shot("diff", "las líneas de cd-throwaway-leftover salen en rojo");
      const outside = parts.paths.filter((p) => !p.replaceAll("/", "\\").toLowerCase().startsWith(t.root.toLowerCase()));
      t.check(outside.length === 0, `ninguna casilla ni aviso nombra una ruta fuera de ${t.root}`, outside.join(", "));
      checkBeforeYouApply(t, parts.before);
      if (parts.before.length) await scrollTo(s, ".dialog .check");
      if (parts.before.length) await t.shot("before-you-apply", "si hay aviso: bloque ámbar con borde ámbar a la izquierda, icono junto a su casilla, primera línea en negrita, rutas en monospace en su línea; si hay nota: bloque azul con borde azul claro e icono de información");
      t.check(parts.foot && /^Each touched file is copied to .+ before anything is written\.$/.test(parts.foot.text), "la nota del pie dice «Each touched file is copied to … before anything is written.»", parts.foot?.text);
      if (parts.foot) {
        const [r, g, b] = (parts.foot.color.match(/\d+/g) ?? []).map(Number);
        t.check(r >= 160 && g >= 160 && b >= 160, "la nota del pie va en gris claro, no en gris oscuro", parts.foot.color);
        if (parts.textSize) t.check(Math.abs(parseFloat(parts.foot.size) - parseFloat(parts.textSize)) <= 1.5, "la nota del pie tiene casi el tamaño del texto de las casillas", `${parts.foot.size} frente a ${parts.textSize}`);
      }
      await scrollTo(s, ".dialog .dialog-backup");
      await t.shot("pie", "la nota del pie va en gris claro, casi blanco, del tamaño del texto de las casillas");
      const result = await confirm(s, "Remove");
      t.check(/Removed 1 Entry\. Operation \S+: verified\./.test(result.text), "el resultado dice «Removed 1 Entry. Operation <id>: verified.»", result.text.split("\n").find((l) => l.includes("Operation")));
      t.shared.removeLeftoversOperation = operationId(result.text.match(/Removed 1 Entry\. Operation \S+:/)?.[0] ?? "");
      await s.instrument();
      await s.delay({ read_places: 1500 });
      await done(s);
      const busy = await waitNotice(s, "Reading the Projects and Agent homes again…", { timeout: 1500 }).catch(() => null);
      const row = await s.exec(
        `const row = [...document.querySelectorAll(".place-row")].find((r) => r.querySelector(".place-name")?.textContent.trim() === arguments[0]);
         return row ? [...row.querySelectorAll("button")].map((b) => ({ text: b.textContent.trim(), disabled: b.disabled })) : null;`,
        gone,
      );
      t.check(busy && busy.cls.includes("busy"), "tras Done sale abajo un aviso con rueda «Reading the Projects and Agent homes again…»", busy?.cls);
      t.check(allDisabled(row), "mientras, los botones de la fila de project-gone están apagados", JSON.stringify(row));
      await s.delay({});
      const after = await s
        .until(async () => {
          const groups = await projectGroups(s);
          return !missingGroup(groups) && noLeftoversGroup(groups) ? groups : null;
        }, { what: "project-gone among the missing Projects without leftovers" })
        .catch(() => null);
      const noLeftovers = after && noLeftoversGroup(after);
      const titles = after?.map((g) => `${g.title} ${g.open ? "abierto" : "cerrado"}`).join(", ");
      t.check(after && !missingGroup(after), "luego el grupo «Missing» ya no sale", titles);
      t.check(noLeftovers?.title === "▸ Missing, no leftovers · 2" && !noLeftovers.open, "en su lugar sale el grupo «▸ Missing, no leftovers · 2», cerrado", titles);
      await openGroup(s, "Missing, no leftovers");
      const left = noLeftoversGroup(await projectGroups(s))?.rows.find((r) => r.name === gone);
      t.check(
        left?.count === "No leftovers" && left.buttons.some((b) => b.text === "Hide"),
        "abierto, en ese grupo la fila de project-gone dice «No leftovers» y ofrece «Hide»",
        left && `${left.count}; ${left.buttons.map((b) => b.text).join(", ")}`,
      );
    },
    note: "El id de la Operation queda guardado para el paso 11.6.",
  },
  {
    id: "10.8",
    async run(t) {
      const { s } = t;
      const path = `${t.root}\\project-hide`;
      await go(s, "Projects & homes");
      await tab(s, "Projects");
      await s.click(await s.until(() => rowButton(s, path, "Hide"), { what: "Hide" }));
      const hid = await waitNotice(s, `Hid ${path}`);
      t.eq(hid.text, `Hid ${path}. Picking it in the Inventory still evaluates it.`, `Hide da el aviso «Hid ${path}. Picking it in the Inventory still evaluates it.»`);
      let groups = await projectGroups(s);
      t.check(!inGroup(groups, "Projects", path), "el Project sale de la lista principal");
      const hidden = groups.find((g) => g.title.includes("Hidden"));
      t.check(hidden && hidden.title.includes("Hidden · 1") && !hidden.open, "el grupo «Hidden · 1» empieza cerrado", hidden && `${hidden.title} ${hidden.open ? "abierto" : "cerrado"}`);
      await openGroup(s, "Hidden");
      groups = await projectGroups(s);
      const row = inGroup(groups, "▾ Hidden", path) ?? groups.find((g) => g.title.includes("Hidden"))?.rows.find((r) => r.name === path);
      t.check(row, "abierto, el grupo muestra el Project");
      // The list ends at the Hidden group: the shot waits for the notices
      // over it to go, the Undo of 10.7 lasting 10 s.
      await s.until(async () => (await notices(s)).length === 0, { what: "the notices gone", timeout: 12000 }).catch(() => {});
      await scrollTo(s, ".place-row", path);
      await t.shot("hidden", "el Project oculto sale en gris");
      await s.click(await s.until(() => rowButton(s, path, "Unhide"), { what: "Unhide" }));
      const listed = await waitNotice(s, `${path} is listed again`);
      t.eq(listed.text, `${path} is listed again.`, `Unhide da el aviso «${path} is listed again.»`);
      groups = await projectGroups(s);
      t.check(inGroup(groups, "Projects", path), "el Project vuelve a la lista principal");
    },
  },
  {
    id: "10.11",
    async run(t) {
      const { s } = t;
      const path = `${t.root}\\project-hide`;
      await go(s, "Projects & homes");
      await tab(s, "Projects");
      const hide = await busyPress(t, path, "Hide", "change_list", `Hiding ${path}…`);
      t.check(hide.busy?.cls.includes("busy"), `nada más pulsar Hide sale abajo un aviso con rueda «Hiding ${path}…»`, hide.busy?.text);
      t.check(allDisabled(hide.row), "mientras, los botones «Open in Inventory» y «Hide» de la fila se ven apagados", JSON.stringify(hide.row));
      const hid = await waitNotice(s, `Hid ${path}`);
      const stillListed = inGroup(await projectGroups(s), "Projects", path);
      t.check(!stillListed && !(await notices(s)).some((n) => n.cls.includes("busy")), "el aviso de la rueda sigue hasta que el Project sale de la lista y entonces lo sustituye «Hid …»", hid.text);
      await openGroup(s, "Hidden");
      const unhide = await busyPress(t, path, "Unhide", "change_list", `Unhiding ${path}…`);
      t.check(unhide.busy?.cls.includes("busy"), `nada más pulsar Unhide sale abajo el aviso de la rueda con «Unhiding ${path}…»`, unhide.busy?.text);
      t.check(allDisabled(unhide.row), "mientras, los botones de la fila se ven apagados", JSON.stringify(unhide.row));
      const listed = await waitNotice(s, `${path} is listed again.`);
      t.check(inGroup(await projectGroups(s), "Projects", path), "el aviso de la rueda sigue hasta que el Project vuelve a la lista, y lo sustituye «… is listed again.»", listed.text);
    },
  },
  {
    id: "10.9",
    async run(t) {
      const { s } = t;
      const missing = `${t.root}\\no-existe`;
      const extra = `${t.root}\\project-extra`;
      await go(s, "Projects & homes");
      await tab(s, "Projects");
      await press(s, "Add Project", ".places");
      await typeInto(s, "input[aria-label=Path]", missing);
      await wait(800);
      const add = await buttonEl(s, "Add", ".dialog .dialog-foot");
      if (!(await s.attr(add, "disabled"))) await s.click(add);
      const error = await s
        .until(() => s.exec(`return [...document.querySelectorAll(".dialog .alert")].map((e) => e.textContent.trim()).join(" | ") || null;`), { what: "the error", timeout: 3000 })
        .catch(() => null);
      t.eq(error, `${missing} is not an existing folder or .code-workspace file`, `con no-existe, el diálogo muestra «${missing} is not an existing folder or .code-workspace file»`);
      await typeInto(s, "input[aria-label=Path]", "");
      await pickFolder(t, "Choose folder", "input[aria-label=Path]", extra);
      await press(s, "Add", ".dialog .dialog-foot");
      const added = await waitNotice(s, `Added ${extra}`);
      t.eq(added.text, `Added ${extra}: every Agent is evaluated on it.`, `sale el aviso «Added ${extra}: every Agent is evaluated on it.»`);
      const row = (await projectGroups(s)).flatMap((g) => g.rows).find((r) => r.name === extra);
      t.check(
        row?.subs.join(" | ") === "Folder" && row.tags.join("|") === "added by you",
        "la fila dice «Folder» y lleva solo la etiqueta «added by you»",
        row && `${row.subs.join(" | ")}; ${row.tags.join(", ")}`,
      );
      t.eq(row?.buttons.map((b) => b.text).join(", "), "Open in Inventory, Remove from list", "la fila tiene «Open in Inventory» y «Remove from list»");
    },
  },
  {
    id: "10.21",
    async run(t) {
      const { s } = t;
      await go(s, "Projects & homes");
      await tab(s, "Projects");
      // After 10.7, project-gone has no leftovers: its row is in the closed
      // group «Missing, no leftovers».
      if (noLeftoversGroup(await projectGroups(s))) await openGroup(s, "Missing, no leftovers");
      const groups = await projectGroups(s);
      const project = inGroup(groups, "Projects", `${t.root}\\project`);
      const extra = inGroup(groups, "Projects", `${t.root}\\project-extra`);
      const gone = [missingGroup(groups), noLeftoversGroup(groups)].flatMap((g) => g?.rows ?? []).find((r) => r.name?.replaceAll("/", "\\") === `${t.root}\\project-gone`);
      t.eq(project?.tags.join("|"), "known to Claude Code", `la fila de «${t.root}\\project» lleva la etiqueta «known to Claude Code»`);
      t.eq(extra?.tags.join("|"), "added by you", `la fila de «${t.root}\\project-extra» lleva solo la etiqueta «added by you»`);
      t.eq(gone?.tags.join("|"), "known to Claude Code", `la fila de «${slashPath(t, "project-gone")}» lleva la etiqueta «known to Claude Code»`);
      const lines = [project, extra, gone].flatMap((row) => row?.subs ?? []);
      t.check(
        !lines.some((line) => /known to|added by you/i.test(line)),
        "ninguna de las tres filas dice «Known to Claude Code» ni «Added by you» en una línea gris bajo la ruta",
        lines.join(" | "),
      );
      await checkTagLook(t, ".places .place-row .tags .tag");
      await s.until(async () => (await notices(s)).length === 0, { what: "the notices gone", timeout: 6000 }).catch(() => {});
      await scrollTo(s, ".place-row", `${t.root}\\project-extra`);
      await t.shot("etiquetas", "las etiquetas de los Projects son píldoras grises con borde gris claro y texto casi blanco, como las de la pestaña Agent homes");
    },
  },
  {
    id: "10.12",
    async run(t) {
      const { s } = t;
      const extra = `${t.root}\\project-extra`;
      await go(s, "Projects & homes");
      await tab(s, "Projects");
      const removing = await busyPress(t, extra, "Remove from list", "change_list", `Removing ${extra} from config-digger's list…`);
      t.check(removing.busy?.cls.includes("busy"), `Remove from list saca un aviso con rueda «Removing ${extra} from config-digger's list…»`, removing.busy?.text);
      t.check(allDisabled(removing.row), "mientras, los botones de la fila se ven apagados", JSON.stringify(removing.row));
      const removed = await waitNotice(s, `Removed ${extra}`);
      t.eq(removed.text, `Removed ${extra} from config-digger's list. Nothing on disk changed.`, `sale «Removed ${extra} from config-digger's list. Nothing on disk changed.»`);
      t.check(!(await projectGroups(s)).flatMap((g) => g.rows).some((r) => r.name === extra), "la fila desaparece");
      await addBusy(t, "Add Project", "Choose folder", "input[aria-label=Path]", extra, `Adding ${extra}…`, `Added ${extra}: every Agent is evaluated on it.`);
      t.check((await projectGroups(s)).flatMap((g) => g.rows).some((r) => r.name === extra), "al final, project-extra vuelve a estar en la lista");
    },
  },
  {
    id: "10.13",
    async run(t) {
      const { s } = t;
      const home = `${t.root}\\codex-home`;
      await go(s, "Projects & homes");
      await tab(s, "Agent homes");
      const removing = await busyPress(t, home, "Remove from list", "change_list", `Removing ${home} of Codex from config-digger's list…`);
      t.check(removing.busy?.cls.includes("busy"), `Remove from list saca un aviso con rueda «Removing ${home} of Codex from config-digger's list…»`, removing.busy?.text);
      t.check(allDisabled(removing.row), "mientras, los botones de la fila se ven apagados", JSON.stringify(removing.row));
      const removed = await waitNotice(s, `Removed ${home}`);
      t.eq(removed.text, `Removed ${home} of Codex from config-digger's list. Nothing on disk changed.`, `sale «Removed ${home} of Codex from config-digger's list. Nothing on disk changed.»`);
      await addBusy(t, "Add Agent home", "Choose", "input[aria-label=Folder]", home, `Adding ${home} as an Agent home of Codex…`, `Added ${home} as an Agent home of Codex.`, "Codex");
      const groups = await homeGroups(s);
      t.check(groups.find((g) => g.agent === "Codex")?.rows.some((r) => r.name === home), "al final, la fila de codex-home vuelve a estar bajo Codex");
    },
  },
  {
    id: "10.10",
    async run(t) {
      const { s } = t;
      await s.setWindow(1100, 640);
      await go(s, "Projects & homes");
      for (const [name, add, about] of [
        ["Projects", "Add Project", "Projects come from the Agents' own registries plus the folders you add. config-digger never scans the disk."],
        ["Agent homes", "Add Agent home", "Each Agent reads its user configuration from one or more homes. Entries in different homes never compete."],
      ]) {
        await tab(s, name);
        const layout = await s.exec(`
          ${textLines}
          const boxes = (el) => {
            const out = [], range = document.createRange(), walker = document.createTreeWalker(el, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT);
            for (let node = walker.nextNode(); node; node = walker.nextNode()) {
              const text = node.nodeType === Node.TEXT_NODE;
              if (text) range.selectNodeContents(node);
              for (const box of (text ? range : node).getClientRects()) if (box.width > 0) out.push({ text, top: box.top });
            }
            return out;
          };
          const lines = (el) => el ? textLines(boxes(el)) : null;
          const header = document.querySelector(".places header, .places .banner");
          const aboutEl = document.querySelector(".places-about");
          const addEl = [...document.querySelectorAll(".places button")].find((b) => b.textContent.trim().startsWith("Add"));
          const addBoxes = addEl ? boxes(addEl) : null;
          return {
            title: lines(document.querySelector(".places .banner-title")),
            tabs: [...document.querySelectorAll("[role=tab]")].map((e) => [e.textContent.trim(), lines(e)]),
            add: addBoxes && textLines(addBoxes),
            addBoxes,
            aboutInHeader: header ? header.contains(aboutEl) : null,
            aboutAboveList: aboutEl ? aboutEl.getBoundingClientRect().bottom <= (document.querySelector(".places .scroll")?.getBoundingClientRect().top ?? 0) + 1 : false,
            about: aboutEl?.textContent.trim(),
            vp: [innerWidth, innerHeight],
          };`);
        t.eq(layout.title, 1, `en ${name}, el título «Projects & homes» ocupa una línea`);
        t.check(layout.tabs.every(([, n]) => n === 1), `en ${name}, cada pestaña ocupa una línea`, JSON.stringify(layout.tabs));
        t.check(layout.add === 1, `en ${name}, el botón «${add}» ocupa una línea`, `«${layout.add}», no «1»: ${JSON.stringify(layout.addBoxes)}`);
        t.check(layout.aboutInHeader === false && layout.aboutAboveList, `en ${name}, la descripción sale encima de la lista, no en la cabecera`);
        t.eq(layout.about, about, `en ${name}, la descripción dice «${about}»`);
        const paths = await s.exec(`
          const linesOf = (el) => {
            const lines = [], range = document.createRange(), walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
            for (let node = walker.nextNode(); node; node = walker.nextNode()) {
              for (let at = 0; at < node.textContent.length; at++) {
                range.setStart(node, at); range.setEnd(node, at + 1);
                const top = Math.round(range.getBoundingClientRect().top), line = lines.at(-1);
                if (line && Math.abs(line.top - top) <= 2) line.text += node.textContent[at]; else lines.push({ top, text: node.textContent[at] });
              }
            }
            return lines.map((line) => line.text);
          };
          const css = ${JSON.stringify(name === "Projects" ? ".places-list .place-name.mono" : ".places-list .place-name, .places-list .place-sub.mono")};
          const lines = [...document.querySelectorAll(css)].map(linesOf);
          const out = [...document.querySelectorAll(".places-list .place-row")].flatMap((row) => [...row.querySelectorAll(".mono")].filter((p) => p.getBoundingClientRect().right > row.getBoundingClientRect().right + 1).map((p) => p.textContent));
          return { lines, out, sideways: document.scrollingElement.scrollWidth > innerWidth };`);
        const fault = wrapFault(paths.lines);
        t.check(
          fault === undefined,
          name === "Projects"
            ? "en Projects, cada ruta que ocupa más de una línea se parte justo después de una «\\», nunca dentro del nombre de una carpeta"
            : "en Agent homes, cada carpeta o nombre de home que ocupa más de una línea se parte justo después de una «\\», nunca dentro del nombre de una carpeta",
          fault,
        );
        t.check(paths.out.length === 0 && !paths.sideways, `en ${name}, ninguna ruta se sale de su fila ni hace que la página se desplace de lado`, JSON.stringify({ out: paths.out, sideways: paths.sideways }));
        t.shared.viewport = layout.vp;
        await t.shot(name.replace(" ", "-").toLowerCase(), `en ${name}, nada se parte ni se monta a 1100 × 640`);
      }
    },
  },
  {
    id: "10.15",
    async run(t) {
      const { s } = t;
      const hide = `${t.root}\\project-hide`;
      await go(s, "Projects & homes");
      await tab(s, "Projects");
      if (!(await rowButton(s, hide, "Hide"))) {
        // A run of this step alone may find project-hide still hidden.
        await openGroup(s, "Hidden");
        await s.click(await s.until(() => rowButton(s, hide, "Unhide"), { what: "Unhide" }));
        await waitNotice(s, `${hide} is listed again.`);
      }
      await s.click(await s.until(() => rowButton(s, hide, "Hide"), { what: "Hide" }));
      await waitNotice(s, `Hid ${hide}`);
      const search = await s.waitFor("input[aria-label='Search Projects']");
      const view = async () => ({ groups: await projectGroups(s), empty: await s.exec("return document.querySelector('.places .scroll p.empty')?.textContent.trim() ?? null") });
      await typeInto(s, "input[aria-label='Search Projects']", "HIDE");
      await openGroup(s, "Hidden");
      let v = await view();
      t.check(v.groups.some((g) => g.title === "Projects · 0"), "con HIDE, el grupo de Projects dice «Projects · 0»", v.groups.map((g) => g.title).join(", "));
      t.check(!v.groups.flatMap((g) => g.rows).some((r) => r.name === "No Project"), "con HIDE, la fila «No Project» no sale");
      t.check(v.groups.find((g) => g.title.includes("Hidden · 1"))?.rows.some((r) => r.name === hide), `con HIDE, el grupo «Hidden · 1» muestra «${hide}»`);
      await typeInto(s, "input[aria-label='Search Projects']", "gone");
      v = await view();
      t.check(v.groups.some((g) => g.title === "Projects · 0"), "con gone, el grupo de Projects dice «Projects · 0»", v.groups.map((g) => g.title).join(", "));
      t.check(!v.groups.some((g) => g.title.includes("Hidden")), "con gone, el grupo «Hidden» no sale");
      const noLeftovers = noLeftoversGroup(v.groups);
      const goneAt = `${t.root}\\project-gone`;
      t.check(noLeftovers?.title === "Missing, no leftovers · 1" && noLeftovers.open, "con gone, el grupo «Missing, no leftovers · 1» se abre solo", noLeftovers && `${noLeftovers.title} ${noLeftovers.open ? "abierto" : "cerrado"}`);
      t.check(
        noLeftovers?.rows.length === 1 && noLeftovers.rows[0].name === goneAt,
        `con gone, ese grupo muestra solo «${goneAt}», sin leftovers desde el paso 10.7`,
        noLeftovers?.rows.map((r) => r.name).join(", "),
      );
      await typeInto(s, "input[aria-label='Search Projects']", "zz");
      v = await view();
      t.eq(v.empty, "No Project path holds “zz”.", "con zz, la lista dice «No Project path holds “zz”.»");
      await typeInto(s, "input[aria-label='Search Projects']", "");
      v = await view();
      t.check(v.groups.flatMap((g) => g.rows).some((r) => r.name === "No Project") && v.groups.flatMap((g) => g.rows).length >= 4, "al borrar, vuelven todas las filas y «No Project»");
      await openGroup(s, "Hidden");
      await s.click(await s.until(() => rowButton(s, hide, "Unhide"), { what: "Unhide" }));
      await waitNotice(s, `${hide} is listed again.`);
      t.check(inGroup(await projectGroups(s), "Projects", hide), "Unhide devuelve project-hide a la lista principal");
      await tab(s, "Agent homes");
      t.check(!(await s.exec("return !!document.querySelector('input[aria-label=\"Search Projects\"]')")), "la pestaña Agent homes no tiene caja de búsqueda");
    },
  },
  {
    id: "10.18",
    note: "La orden corre con el CLI que va junto a la app, sobre el perfil de prueba.",
    async run(t) {
      const { s } = t;
      const home = `${t.root}\\claude-desktop-stranded-home`;
      const name = "cd-throwaway-stranded";
      await addHome(t, "Claude Desktop", home, { checks: false });
      await inventoryOf(t, "Claude Desktop", "claude-desktop-stranded-home");
      await openItem(t, name);
      await act(t, "Disable");
      await done(s);
      await go(s, "Projects & homes");
      await tab(s, "Agent homes");
      await s.click(await s.until(() => rowButton(s, home, "Remove from list"), { what: "Remove from list" }));
      await waitNotice(s, `Removed ${home}`);
      await tab(s, "Projects");
      const group = (groups) => groups.find((g) => g.title.startsWith("Stranded · "));
      const stranded = await s.until(async () => group(await projectGroups(s)) ?? null, { what: "the Stranded group" }).catch(() => null);
      const row = stranded?.rows.find((r) => r.name === name);
      t.check(stranded?.title.startsWith("Stranded · 1") && row, `tras Remove from list, el grupo «Stranded · 1» muestra «${name}»`, stranded?.title);
      t.check(row?.sub.startsWith("MCP server · Claude Desktop · parked "), "su fila dice «MCP server · Claude Desktop» y la fecha en que se aparcó", row?.sub);
      const text = await s.exec(
        `return [...document.querySelectorAll(".place-row")].find((r) => r.querySelector(".place-name")?.textContent.trim() === arguments[0])?.innerText ?? "";`,
        name,
      );
      const reason = `the Claude Desktop home \`${home}\` is not found`;
      t.check(text.includes(reason), `su fila dice «${reason}»`, text);
      const remove = await rowButton(s, name, "Remove");
      const removeClass = remove ? await s.attr(remove, "class") : "";
      t.check(removeClass.includes("danger") && (await buttonEl(s, "Remove all", ".places")), "su fila tiene un botón «Remove» rojo, y el grupo un botón «Remove all»", removeClass);
      // The shot needs the group's title in view, and the notice of Remove
      // from list gone from over the row.
      await s.until(async () => (await notices(s)).length === 0, { what: "the notices gone", timeout: 8000 }).catch(() => {});
      await scrollTo(s, ".place-group-toggle", "Stranded");
      await t.shot("stranded", "el grupo Stranded con su fila, el Remove rojo y el Remove all");

      const cli = join(dirname(t.app), "config-digger.exe");
      const out = t.ps(`& '${cli}' --color never list --leftovers --wide`);
      const listed = out.slice(out.indexOf("Stranded parked Entries"));
      t.check(
        out.includes("Stranded parked Entries") && listed.split(/\r?\n/).some((l) => l.includes(name) && l.includes("Claude Desktop")),
        `la orden imprime, tras los leftovers, el grupo «Stranded parked Entries» con una fila de «${name}» y «Claude Desktop»`,
        listed.split(/\r?\n/).slice(0, 3).join(" | "),
      );

      await s.click(remove);
      const d = await waitPreview(s);
      t.eq(d.title, "Remove 1 Entry", "el diálogo de Remove se titula «Remove 1 Entry»");
      const removes = `Removes the Stranded Entry \`${name}\` of Claude Desktop`;
      t.check(d.text.includes(removes), `el diálogo dice «${removes}»`, d.text.split("\n").find((l) => l.includes("Stranded")));
      const result = await confirm(s, "Remove");
      const operation = operationId(result.text);
      await done(s);
      const gone = await s.until(async () => (group(await projectGroups(s)) ? null : true), { what: "the Stranded group gone" }).catch(() => false);
      t.check(gone, "tras Remove, el grupo «Stranded» ya no sale");

      await go(s, "History · Undo");
      const op = await s.until(
        () => s.exec(`return [...document.querySelectorAll(".operation-row")].find((r) => r.querySelector(".operation-meta")?.textContent.includes("Operation " + arguments[0])) ?? null;`, operation).then(unwrap),
        { what: `the Operation ${operation}` },
      );
      await s.click(op);
      await s.until(() => s.exec("return !!document.querySelector('.history-detail .history-body') && !/Reading what it changed/.test(document.querySelector('.history-detail').innerText)"), { what: "the detail" });
      await press(s, "Undo", ".history-detail .dialog-foot");
      await waitPreview(s);
      await confirm(s, "Undo");
      await done(s);
      await go(s, "Projects & homes");
      await tab(s, "Projects");
      const back = await s.until(async () => group(await projectGroups(s)) ?? null, { what: "the Stranded group back" }).catch(() => null);
      t.check(back?.title.startsWith("Stranded · 1") && back.rows.some((r) => r.name === name), `tras el Undo, el grupo «Stranded · 1» vuelve con «${name}»`, back?.title);

      await press(s, "Remove all", ".places");
      await waitPreview(s);
      await confirm(s, "Remove");
      await done(s);
      const cleared = await s.until(async () => (group(await projectGroups(s)) ? null : true), { what: "the Stranded group gone" }).catch(() => false);
      t.check(cleared, "tras Remove all, el grupo «Stranded» ya no sale");
    },
  },
];

// The Missing Project's row name, as the screen shows it.
async function goneRow(t) {
  const { s } = t;
  await go(t.s, "Projects & homes");
  await tab(s, "Projects");
  await openGroup(s, "Missing");
  const groups = await projectGroups(s);
  return missingGroup(groups)?.rows.find((r) => /project-gone$/.test(r.name))?.name ?? `${t.root}\\project-gone`;
}

async function openSection(s, title) {
  const header = await s
    .exec(
      `return [...document.querySelectorAll(".dialog button, .dialog summary, .dialog [aria-expanded]")].find((e) => e.textContent.trim().startsWith(arguments[0]) && e.getAttribute("aria-expanded") !== "true") ?? null;`,
      title,
    )
    .then(unwrap);
  if (header) {
    await s.click(header);
    await wait(300);
  }
}

// Adds through the dialog with `command` held, and checks the busy notice
// above the dialog, the disabled Add, and the notice that replaces it.
async function addBusy(t, open, pickButton, field, path, busyText, addedText, agent) {
  const { s } = t;
  await press(s, open, ".places");
  if (agent) await choose(s, "Agent", agent);
  await pickFolder(t, pickButton, field, path);
  await s.instrument();
  await s.delay({ change_list: 1500 });
  await press(s, "Add", ".dialog .dialog-foot");
  const busy = await waitNotice(s, busyText, { timeout: 1500 }).catch(() => null);
  const over = await s.exec(`
    const n = [...document.querySelectorAll(".notice.busy")].at(-1);
    const d = document.querySelector(".dialog-backdrop");
    if (!n || !d) return null;
    const r = n.getBoundingClientRect();
    const top = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
    return n.contains(top);`);
  const addDisabled = await s.exec(`return [...document.querySelectorAll(".dialog .dialog-foot button")].find((b) => b.textContent.trim() === "Add")?.disabled ?? null;`);
  await t.shot("adding", "el aviso de la rueda sale por encima del diálogo");
  t.check(busy?.cls.includes("busy") && over, `Add saca, por encima del diálogo, el aviso de la rueda con «${busyText}»`, `${busy?.text ?? "sin aviso"}; encima: ${over}`);
  t.check(addDisabled === true, "mientras, Add se ve apagado", String(addDisabled));
  await s.delay({});
  const added = await waitNotice(s, addedText);
  const closed = await s.exec("return !document.querySelector('.dialog')");
  t.check(closed && added, `cuando el diálogo se cierra, sale «${addedText}»`);
}
