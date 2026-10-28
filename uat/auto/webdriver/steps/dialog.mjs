// Section 7: the action dialog and the buttons that delete.
import { KEY, ref, unwrap } from "../driver.mjs";
import { buttonEl, fieldValue, go, hoverFresh, press, rowButton, tab, tabTo, tooltip, wait, waitNotice } from "../lib.mjs";
import { act, addHome, confirm, DESKTOP_HOME, dialog, done, inventoryOf, openItem, operationId, project, rootSlash, tickAll, waitPreview, withClaudeDesktop, withClaudeDesktopIn } from "./common.mjs";

// Claude Code Disables a user Entry per Project, so Disable is offered only
// with a Project chosen. The steps that Disable cd-throwaway-user name none:
// the manual walk still has the one the dropdown steps left. With none, the
// step picks the root's project and says so.
async function withProject(t) {
  if ((await fieldValue(t.s, "Project")) === "No Project") {
    await project(t, `${t.root}\\project`);
    t.note(`El paso no dice qué Project elegir, y con «No Project» Claude Code no ofrece Disable: se elige «${t.root}\\project», como deja la sección 2.`);
  }
}

const rgb = (c) => (c?.match(/\d+(\.\d+)?/g) ?? []).map(Number);
const red = (c) => {
  const [r, g, b] = rgb(c);
  return r > 150 && r > g + 60 && r > b + 40;
};
const violet = (c) => {
  const [r, g, b] = rgb(c);
  return b > g + 40 && r > g;
};
const grey = (c) => {
  const [r, g, b, a = 1] = rgb(c);
  return a === 0 || (Math.abs(r - g) < 20 && Math.abs(g - b) < 25);
};

async function openRemoveLeftovers(t, project) {
  const { s } = t;
  await go(s, "Projects & homes");
  await tab(s, "Projects");
  const name = await s.exec(
    `return [...document.querySelectorAll(".place-row .place-name")].map((e) => e.textContent.trim()).find((n) => n.replaceAll("/", "\\\\") === arguments[0].replaceAll("/", "\\\\")) ?? null;`,
    project,
  );
  t.path(name, project);
  await s.click(await s.until(() => rowButton(s, name, "Remove leftovers"), { what: `Remove leftovers of ${project}` }));
  return waitPreview(s);
}

// The look of each button whose text starts with one of `texts`.
function buttons(s, texts, scope = "body") {
  return s.exec(
    `return [...document.querySelectorAll(arguments[1] + " button")].filter((b) => b.offsetParent && arguments[0].some((x) => b.textContent.trim().startsWith(x)))
       .map((b) => ({ text: b.textContent.trim(), bg: getComputedStyle(b).backgroundColor, color: getComputedStyle(b).color, cls: b.className }));`,
    texts,
    scope,
  );
}

async function hoverBg(s, text, scope) {
  const el = await buttonEl(s, text, scope);
  await s.hover({ x: 600, y: 4 });
  await wait(200);
  const before = await s.exec("return getComputedStyle(arguments[0]).backgroundColor", ref(el));
  await s.hover(el);
  await wait(300);
  const after = await s.exec("return getComputedStyle(arguments[0]).backgroundColor", ref(el));
  return { before, after, lighter: rgb(after).slice(0, 3).reduce((a, b) => a + b, 0) > rgb(before).slice(0, 3).reduce((a, b) => a + b, 0) };
}

export default [
  {
    id: "7.1",
    async run(t) {
      const { s } = t;
      const gone = rootSlash(t, "project-gone");
      await openRemoveLeftovers(t, gone);
      const block = await s.exec(`
        const body = [...document.querySelectorAll(".dialog .dialog-section")].find((e) => e.querySelector(".dialog-section-title")?.firstChild?.textContent.trim() === "What changes");
        const p = body.querySelector(".change-project-name"), f = body.querySelector(".change-file-name");
        const parts = [...body.querySelectorAll(".change-part")].map((x) => ({ label: x.querySelector(".change-label").innerText.trim(), text: x.querySelector("dd").innerText.trim(), left: x.querySelector(".change-label").getBoundingClientRect().left, weight: getComputedStyle(x.querySelector("dd b") ?? x.querySelector("dd")).fontWeight, top: x.querySelector("dd").getBoundingClientRect().top, bottom: x.querySelector("dd").getBoundingClientRect().bottom }));
        const key = body.querySelector(".change-key"), kc = getComputedStyle(key);
        const label = getComputedStyle(body.querySelector(".change-part .change-label"));
        const row = body.querySelector(".change-row"), rc = getComputedStyle(row);
        return { project: p.innerText.trim(), file: f.innerText.trim(), parts, keyColor: kc.color, keyFont: kc.fontFamily, labelColor: label.color, labelCase: label.textTransform,
          box: rc.borderTopStyle !== "none" || rc.backgroundColor !== "rgba(0, 0, 0, 0)", gaps: parts.slice(1).map((x, i) => Math.round(x.top - parts[i].bottom)) };`);
      t.path(block.project.replace(/^PROJECT\s+/i, ""), gone, `arriba sale «PROJECT ${gone}» en su línea`);
      t.check(/^PROJECT\s/i.test(block.project), "con la etiqueta «PROJECT»", block.project);
      t.check(/^FILE\s+\.claude\.json$/i.test(block.file), "debajo sale «FILE .claude.json»", block.file);
      const labels = block.parts.map((p) => p.label.toUpperCase());
      t.check(block.box && block.labelCase === "uppercase" && grey(block.labelColor) && new Set(block.parts.map((p) => Math.round(p.left))).size === 1, "el cambio sale en su recuadro, con una etiqueta gris en mayúsculas a la izquierda de cada línea, en una columna", `${labels.join(", ")}; ${block.labelColor}`);
      const part = (l) => block.parts.find((p) => p.label.toUpperCase() === l);
      t.check(labels[0] === "ENTRY" && part("ENTRY")?.text === "cd-throwaway-leftover" && Number(part("ENTRY").weight) >= 600, "la primera línea es «ENTRY cd-throwaway-leftover», en negrita", JSON.stringify(part("ENTRY")));
      t.check(labels[1] === "AGENT" && part("AGENT")?.text.startsWith("Claude Code"), "luego sale «AGENT Claude Code»", part("AGENT")?.text);
      if (part("AGENT")?.text !== "Claude Code") t.note(`La línea AGENT dice «${part("AGENT")?.text}»; el plan espera «AGENT Claude Code».`);
      t.check(labels[2] === "KEY" && part("KEY")?.text === `projects.${gone}.mcpServers.cd-throwaway-leftover` && grey(block.keyColor) && /mono|consol/i.test(block.keyFont), `luego sale «KEY projects.${gone}.mcpServers.cd-throwaway-leftover», en gris y monoespaciada`, `${part("KEY")?.text}; ${block.keyColor}`);
      t.check(labels.at(-1) === "STATE" && /^Unread\s*→\s*Removed$/.test(part("STATE")?.text ?? ""), "al final sale «STATE Unread → Removed»", part("STATE")?.text);
      t.check(block.gaps.length > 0 && block.gaps.every((g) => g >= 3), "hay espacio entre las líneas", block.gaps.join(", "));
      const key = await s.find(".dialog .change-key");
      await hoverFresh(s, key);
      await wait(900);
      const tip = await tooltip(s);
      t.check(tip?.text.startsWith(`${t.root}\\claude-code-home\\.claude.json#`), `sobre la clave sale un tooltip con el locator entero, que empieza por «${t.root}\\claude-code-home\\.claude.json#»`, tip?.text);
      await s.click(await s.find(".dialog button[aria-label='Copy the locator']"));
      const copied = await waitNotice(s, "Copied the locator.", { timeout: 3000 }).catch(() => null);
      t.check(copied, "al copiarlo sale el aviso «Copied the locator.»");
      await s.setWindow(1100, 640);
      await wait(400);
      const wrap = await s.exec(`
        const key = document.querySelector(".dialog .change-key");
        const pieces = [...key.querySelectorAll(".change-key-piece")];
        const lines = new Set(pieces.map((p) => Math.round(p.getBoundingClientRect().top))).size;
        return { lines, split: pieces.filter((p) => p.getClientRects().length > 1).map((p) => p.textContent) };`);
      if (wrap.lines > 1) t.check(wrap.split.length === 0, "con la ventana estrecha, la clave pasa a la línea siguiente tras un punto o una barra, nunca en medio de una palabra", JSON.stringify(wrap));
      else {
        // The smallest window holds the key on one line: force the narrower
        // dialog the step asks for, the way a narrower window would lay it out.
        await s.exec("document.querySelector('.dialog').style.maxWidth = '420px'");
        await wait(200);
        const narrow = await s.exec(`
          const pieces = [...document.querySelectorAll(".dialog .change-key-piece")];
          return { lines: new Set(pieces.map((p) => Math.round(p.getBoundingClientRect().top))).size, split: pieces.filter((p) => p.getClientRects().length > 1).map((p) => p.textContent) };`);
        t.check(narrow.lines > 1 && narrow.split.length === 0, "con la ventana estrecha, la clave pasa a la línea siguiente tras un punto o una barra, nunca en medio de una palabra", JSON.stringify(narrow));
        t.note("La ventana no se estrecha por debajo de 1100 px y ahí la clave cabe en una línea: el corte se comprueba estrechando el diálogo a 420 px.");
        // What changes scrolls inside the dialog: the shot needs the key in view.
        await s.exec("document.querySelector('.dialog .change-key').scrollIntoView({ block: 'center' });");
        await wait(200);
        await t.shot("clave", "la clave partida tras un punto o una barra");
        await s.exec("document.querySelector('.dialog').style.maxWidth = ''");
      }
      await press(s, "Cancel", ".dialog");
    },
  },
  {
    id: "7.2",
    note: "Claude Desktop de verdad, abierto en este PC. config-digger corre con el perfil de prueba, así que solo cambia el home de prueba.",
    async run(t) {
      const { s } = t;
      await withClaudeDesktop(t, async () => {
        await inventoryOf(t, "Claude Desktop", "claude-desktop-home");
        await openItem(t, "cd-throwaway-desktop");
        await press(s, "Disable", ".panel.detail");
        const d = await waitPreview(s);
        t.check(!/Claude Desktop is running/.test(d.text), "el diálogo no tiene ninguna frase que empiece por «Claude Desktop is running»", d.text.match(/Claude Desktop is running[^\n]*/)?.[0]);
        // The step ticks the warning boxes first, as it asks, then presses Disable.
        await tickAll(s);
        const disable = await buttonEl(s, "Disable", ".dialog .dialog-foot");
        t.check(disable && (await s.attr(disable, "disabled")) === null, "su botón «Disable» se puede pulsar");
        await confirm(s, "Disable");
        await done(s);
        await s.until(() => s.exec("return !document.querySelector('.notice.busy, .read-progress')"), { what: "the re-read" });
        const states = await s.exec(`return [...document.querySelectorAll(".item-row")].filter((r) => r.querySelector(".item-name")?.textContent.trim() === "cd-throwaway-desktop").map((r) => r.querySelector(".state-pill").firstChild.textContent.trim());`);
        t.check(states.includes("Disabled"), "tras Done, «cd-throwaway-desktop» sale como «Disabled» en ese home", states.join(", "));
      });
    },
  },
  {
    id: "7.6",
    note: "Claude Desktop de verdad, abierto con --user-data-dir en el home de prueba, y cerrado solo ese. Otro Claude Desktop abierto en este PC no lee ese home.",
    async run(t) {
      const { s } = t;
      const home = `${t.root}\\${DESKTOP_HOME}`;
      await withClaudeDesktopIn(t, home, async (close) => {
        await inventoryOf(t, "Claude Desktop", "claude-desktop-home");
        await openItem(t, "cd-throwaway-desktop");
        await press(s, "Disable", ".panel.detail");
        const blocked = await waitPreview(s);
        t.check(/Claude Desktop is running and rewrites/.test(blocked.text), "con ese Claude Desktop abierto, el diálogo tiene un bloqueo que empieza por «Claude Desktop is running and rewrites»", blocked.text.slice(0, 400));
        t.check(blocked.text.includes("Close Claude Desktop and retry."), "ese bloqueo dice «Close Claude Desktop and retry.»");
        const file = `${home}\\claude_desktop_config.json`;
        t.check(blocked.text.includes(file), `ese bloqueo nombra «${file}»`);
        const off = await buttonEl(s, "Disable", ".dialog .dialog-foot");
        t.check(off && (await s.attr(off, "disabled")) !== null, "su botón «Disable» no se puede pulsar");
        await press(s, "Cancel", ".dialog");
        await close();
        await press(s, "Disable", ".panel.detail");
        const d = await waitPreview(s);
        t.check(!/Claude Desktop is running/.test(d.text), "cerrado ese Claude Desktop, el diálogo no tiene ninguna frase que empiece por «Claude Desktop is running»", d.text.match(/Claude Desktop is running[^\n]*/)?.[0]);
        await tickAll(s);
        const disable = await buttonEl(s, "Disable", ".dialog .dialog-foot");
        t.check(disable && (await s.attr(disable, "disabled")) === null, "entonces su botón «Disable» se puede pulsar");
        await confirm(s, "Disable");
        await done(s);
        await s.until(() => s.exec("return !document.querySelector('.notice.busy, .read-progress')"), { what: "the re-read" });
        const states = await s.exec(`return [...document.querySelectorAll(".item-row")].filter((r) => r.querySelector(".item-name")?.textContent.trim() === "cd-throwaway-desktop").map((r) => r.querySelector(".state-pill").firstChild.textContent.trim());`);
        t.check(states.includes("Disabled"), "tras Done, «cd-throwaway-desktop» sale como «Disabled» en ese home", states.join(", "));
      });
    },
  },
  {
    id: "7.3",
    async run(t) {
      const { s } = t;
      await inventoryOf(t, "Claude Code", "claude-code-home");
      await withProject(t);
      await openItem(t, "cd-throwaway-user");
      const result = await act(t, "Disable");
      const grey1 = await s.exec(`return [...document.querySelectorAll(".dialog *")].filter((e) => e.children.length === 0 && /^Operation /.test(e.textContent.trim())).map((e) => e.textContent.trim());`);
      const id = operationId(grey1[0] ?? result.text);
      t.check(grey1.some((l) => l === `Operation ${id} · backup kept`), "tras Disable, la línea gris dice «Operation <id> · backup kept»", grey1.join(" | "));
      await done(s);
      await go(s, "History · Undo");
      await s.click(await s.until(() => s.exec(`return [...document.querySelectorAll(".operation-row")].find((r) => r.textContent.includes(arguments[0])) ?? null;`, id).then(unwrap), { what: `Operation ${id}` }));
      await wait(400);
      await press(s, "Delete backup", ".history-detail");
      await s.until(() => dialog(s), { what: "the Delete backup dialog" });
      const out = await confirm(s, "Delete backup");
      t.check(out.text.includes(`Deleted the backup of ${id}. It can no longer be undone.`), "tras Delete backup, el resultado dice «Deleted the backup of <id>. It can no longer be undone.»", out.text.split("\n").slice(0, 4).join(" | "));
      const lines = await s.exec(`return [...document.querySelectorAll(".dialog *")].filter((e) => e.children.length === 0 && /^Operation/.test(e.textContent.trim())).map((e) => e.textContent.trim());`);
      t.check(lines.includes(`Operation ${id} · backup deleted · can no longer be undone`), "debajo, la línea gris dice «Operation <id> · backup deleted · can no longer be undone», con el mismo id", lines.join(" | "));
      t.check(!out.text.includes("backup kept"), "no sale «backup kept»");
      t.check(!/Operation\s+·/.test(out.text), "no sale «Operation  ·» sin id");
      t.check(!(await buttonEl(s, "Undo", ".dialog")), "no sale el botón «Undo»");
      await press(s, "Done", ".dialog").catch(() => press(s, "Close", ".dialog"));
    },
  },
  {
    id: "7.4",
    async run(t) {
      const { s } = t;
      await s.setWindow(1100, 640);
      await go(s, "Projects & homes");
      await tab(s, "Agent homes");
      const listed = await s.exec(`return [...document.querySelectorAll(".place-name")].some((e) => e.textContent.trim() === arguments[0]);`, `${t.root}\\claude-code-many-home`);
      if (!listed) await addHome(t, "Claude Code", `${t.root}\\claude-code-many-home`, { checks: false });
      else t.note("claude-code-many-home ya estaba en la lista desde el paso 4.1, así que no se añade otra vez.");
      await openRemoveLeftovers(t, rootSlash(t, "many/project-gone"));
      const layout = await s.exec(`
        const d = document.querySelector(".dialog"), r = d.getBoundingClientRect();
        const sec = (title) => [...d.querySelectorAll(".dialog-section")].find((e) => e.querySelector(".dialog-section-title")?.firstChild?.textContent.trim().startsWith(title));
        const info = (e) => { if (!e) return null; const b = e.querySelector(".dialog-section-body") ?? e; const br = b.getBoundingClientRect(); return { title: e.querySelector(".dialog-section-title").firstChild.textContent.trim(), titleTop: e.querySelector(".dialog-section-title").getBoundingClientRect().top, scrolls: b.scrollHeight > b.clientHeight + 1, shown: br.height > 30, top: br.top, bottom: br.bottom }; };
        const foot = [...d.querySelectorAll(".dialog-foot button")].map((b) => b.getBoundingClientRect());
        return { fits: r.top >= 0 && r.bottom <= innerHeight && r.right <= innerWidth && d.scrollHeight <= d.clientHeight + 1, scroll: [d.scrollHeight, d.clientHeight], footInView: foot.length > 0 && foot.every((b) => b.top >= 0 && b.bottom <= Math.min(innerHeight, r.bottom) + 1), subtitle: d.querySelector(".dialog-subtitle")?.getBoundingClientRect().height, title: d.querySelector(".dialog-title").textContent.trim(), changes: info(sec("What changes")), files: info(sec("Files")), before: info(sec("Before you apply")) };`);
      t.check(layout.fits, "el diálogo cabe en la ventana, sin desplazarse entero", `contenido ${layout.scroll[0]} px en ${layout.scroll[1]} px; subtítulo de ${Math.round(layout.subtitle)} px`);
      t.check(layout.footInView, "los botones «Cancel» y «Remove 12 leftovers» están a la vista");
      t.check(layout.changes?.shown && layout.files?.shown, "«What changes» y «Files» enseñan cada uno su título y parte de lo que tienen", JSON.stringify([layout.changes, layout.files]));
      t.eq(layout.title, "Remove 12 Entries", "el título es «Remove 12 Entries»");
      const fixed = async () =>
        s.exec(`
          const d = document.querySelector(".dialog");
          const box = (e) => e && Math.round(e.getBoundingClientRect().top);
          const before = [...d.querySelectorAll(".dialog-section")].find((e) => e.querySelector(".dialog-section-title")?.firstChild?.textContent.trim() === "Before you apply");
          return [box(d.querySelector(".dialog-title")), box(d.querySelector(".dialog-backup")), ...[...d.querySelectorAll(".dialog-foot button")].map(box), box(before)];`);
      const still = await fixed();
      const changes = await s.find(".dialog .dialog-section:first-of-type .dialog-section-body");
      const scrolled = await s.exec(`
        const sections = [...document.querySelectorAll(".dialog .dialog-section-body")];
        const [c, f] = [sections[0], sections.at(-1)];
        const fTop = f.scrollTop;
        c.scrollTop = c.scrollHeight;
        // A change is several lines, Entry, Agent, Key and State, taller than
        // the block at the smallest window: at the bottom, the last one's end shows.
        const row = [...c.querySelectorAll(".change-row")].at(-1);
        return { cTop: c.scrollTop, fTop: f.scrollTop, fBefore: fTop, last: row?.querySelector(".change-item")?.textContent.trim(), lastVisible: (() => { const l = row.getBoundingClientRect(), b = c.getBoundingClientRect(); return l.bottom <= b.bottom + 1 && l.bottom > b.top; })() };`);
      t.check(scrolled.cTop > 0 && scrolled.fTop === scrolled.fBefore, "al desplazarte dentro de uno, solo se mueve su contenido, bajo su título", JSON.stringify(scrolled));
      t.check(scrolled.last === "cd-throwaway-gone-12" && scrolled.lastVisible, "«What changes» llega abajo, al cambio de «cd-throwaway-gone-12»", scrolled.last);
      await s.exec("const f = [...document.querySelectorAll('.dialog .dialog-section-body')].at(-1); f.scrollTop = f.scrollHeight;");
      t.eq(JSON.stringify(await fixed()), JSON.stringify(still), "el título «Remove 12 Entries», la línea gris del backup y los botones «Cancel» y «Remove 12 leftovers» se quedan quietos y a la vista");
      const foot = await s.exec(`return [...document.querySelectorAll(".dialog .dialog-foot button")].map((b) => b.textContent.trim());`);
      t.eq(foot.join(", "), "Cancel, Remove 12 leftovers", "los botones son «Cancel» y «Remove 12 leftovers»");
      if (layout.before) t.check(layout.before.top >= layout.changes.bottom - 1 && layout.before.bottom <= layout.files.titleTop + 1, "el bloque «Before you apply» se queda quieto y a la vista, entre «What changes» y «Files»", JSON.stringify(layout.before));
      await s.exec("document.querySelector('.dialog .dialog-section-body').scrollTop = 0; document.activeElement?.blur();");
      const f = await tabTo(s, (e) => e.role === "region");
      await wait(250);
      const ring = await s.exec("const c = getComputedStyle(document.activeElement); return { outline: c.outlineStyle + ' ' + c.outlineColor + ' ' + c.outlineWidth, shadow: c.boxShadow, text: document.activeElement.textContent.slice(0, 40) }");
      t.check(f && (violet(ring.outline.split(" ").slice(1).join(" ")) || violet(ring.shadow)), "con el foco del teclado, el bloque «What changes» tiene un anillo violeta", JSON.stringify(ring));
      const top0 = await s.exec("return document.activeElement.scrollTop");
      await s.keys(KEY.Down, KEY.Down, KEY.Down);
      await wait(300);
      t.check((await s.exec("return document.activeElement.scrollTop")) > top0, "con el foco, «What changes» se desplaza con la flecha abajo");
      await t.shot("dialogo", "el diálogo cabe en la ventana; What changes con anillo violeta");
      await press(s, "Cancel", ".dialog");
      void changes;
    },
  },
  {
    id: "7.5",
    async run(t) {
      const { s } = t;
      await s.setWindow(1440, 900);
      await inventoryOf(t, "Claude Code", "claude-code-home");
      await withProject(t);
      await openItem(t, "cd-throwaway-user");
      const reds = [];
      const card = await buttons(s, ["Disable", "Remove"], ".panel.detail .entry-card");
      const remove = card.find((b) => b.text === "Remove");
      reds.push(remove);
      t.check(violet(card.find((b) => b.text === "Disable")?.bg), "«Disable» sigue violeta", card.find((b) => b.text === "Disable")?.bg);
      const hoverRemove = await hoverBg(s, "Remove", ".panel.detail .entry-card");
      await press(s, "Remove", ".panel.detail .entry-card");
      await waitPreview(s);
      const removeFoot = await buttons(s, ["Cancel", "Remove"], ".dialog .dialog-foot");
      await press(s, "Cancel", ".dialog");
      await s.until(() => s.exec("return !document.querySelector('.dialog')"), { what: "the dialog closed" });
      await openItem(t, "cd-throwaway-user");
      await act(t, "Disable");
      await done(s);
      await go(s, "History · Undo");
      await s.click(await s.waitFor(".operation-row"));
      await wait(400);
      const detail = await buttons(s, ["Delete backup", "Undo"], ".history-detail");
      reds.push(detail.find((b) => b.text === "Delete backup"));
      t.check(violet(detail.find((b) => b.text === "Undo")?.bg), "«Undo» sigue violeta", detail.find((b) => b.text === "Undo")?.bg);
      const hoverDelete = await hoverBg(s, "Delete backup", ".history-detail");
      await press(s, "Delete backup", ".history-detail");
      await s.until(() => dialog(s), { what: "the dialog" });
      const deleteFoot = await buttons(s, ["Cancel", "Delete backup"], ".dialog .dialog-foot");
      await press(s, "Cancel", ".dialog");
      await go(s, "Projects & homes");
      await tab(s, "Projects");
      const goneRow = await s.exec(`return [...document.querySelectorAll(".place-row")].find((r) => /project-gone$/.test(r.querySelector(".place-name")?.textContent.trim()) && r.textContent.includes("Remove leftovers"))?.querySelector(".place-name").textContent.trim() ?? null;`);
      const rowButtons = await s.exec(
        `const row = [...document.querySelectorAll(".place-row")].find((r) => r.querySelector(".place-name")?.textContent.trim() === arguments[0]);
         return [...row.querySelectorAll("button")].map((b) => ({ text: b.textContent.trim(), bg: getComputedStyle(b).backgroundColor, color: getComputedStyle(b).color }));`,
        goneRow,
      );
      reds.push(rowButtons.find((b) => b.text === "Remove leftovers"));
      const show = rowButtons.find((b) => b.text === "Show leftovers");
      await s.click(await rowButton(s, goneRow, "Remove leftovers"));
      await waitPreview(s);
      const leftoversFoot = await buttons(s, ["Cancel", "Remove"], ".dialog .dialog-foot");
      await press(s, "Cancel", ".dialog");
      await tab(s, "Agent homes");
      const homeButtons = await s.exec(
        `const row = [...document.querySelectorAll(".place-row")].find((r) => r.querySelector(".place-name")?.textContent.trim() === arguments[0]);
         return [...row.querySelectorAll("button")].map((b) => ({ text: b.textContent.trim(), bg: getComputedStyle(b).backgroundColor, color: getComputedStyle(b).color }));`,
        `${t.root}\\claude-code-home`,
      );
      reds.push(homeButtons.find((b) => b.text === "Remove from list"));
      await t.shot("botones", "«Remove from list» rojo con texto blanco; los demás botones de la fila, grises");
      t.check(reds.every((b) => b && red(b.bg)) && reds.every((b) => rgb(b.color).every((x) => x > 220)), "«Remove», «Delete backup», «Remove leftovers» y «Remove from list» son rojos, con el texto blanco", JSON.stringify(reds.map((b) => b && `${b.text}: ${b.bg} / ${b.color}`)));
      t.check(new Set(reds.map((b) => b?.bg)).size === 1, "los cuatro tienen el mismo rojo", reds.map((b) => b?.bg).join(", "));
      t.check(hoverRemove.lighter && hoverDelete.lighter, "con el ratón encima, se aclaran", JSON.stringify([hoverRemove, hoverDelete]));
      const confirms = [removeFoot.find((b) => b.text === "Remove"), deleteFoot.find((b) => b.text === "Delete backup"), leftoversFoot.find((b) => b.text.startsWith("Remove"))];
      t.check(confirms.every((b) => b && b.bg === reds[0].bg), "en cada diálogo, el botón que confirma («Remove», «Delete backup», «Remove 1 leftover») es de ese mismo rojo", JSON.stringify(confirms.map((b) => b && `${b.text}: ${b.bg}`)));
      const cancels = [removeFoot, deleteFoot, leftoversFoot].map((f) => f.find((b) => b.text === "Cancel"));
      t.check(cancels.every((b) => b && grey(b.bg)), "en cada diálogo, «Cancel» es gris", cancels.map((b) => b?.bg).join(", "));
      const others = homeButtons.filter((b) => b.text !== "Remove from list");
      t.check(show && grey(show.bg) && others.every((b) => grey(b.bg)), "«Show leftovers» y los demás botones de la fila de claude-code-home son grises", JSON.stringify([show, ...others].map((b) => b && `${b.text}: ${b.bg}`)));
    },
  },
  {
    id: "7.7",
    async run(t) {
      const { s } = t;
      const file = `${t.root}\\codex-home\\config.toml`;
      t.ps(t.step.commands[0].text);
      try {
        await s.setWindow(1100, 640);
        await inventoryOf(t, "Codex", "codex-home");
        await project(t, `${t.root}\\project`);
        await openItem(t, "cd-throwaway-codex");
        await act(t, "Disable");
        const layout = await s.exec(`
          const d = document.querySelector(".dialog"), r = d.getBoundingClientRect();
          const inView = (e) => { if (!e) return false; const b = e.getBoundingClientRect(); return b.top >= 0 && b.bottom <= Math.min(innerHeight, r.bottom) + 1; };
          const callout = d.querySelector(".dialog-callout.danger");
          const region = [...d.querySelectorAll("[role=region]")].find((e) => document.getElementById(e.getAttribute("aria-labelledby"))?.textContent.trim() === "Untouched");
          const title = region && document.getElementById(region.getAttribute("aria-labelledby"));
          return {
            fits: r.top >= 0 && r.bottom <= innerHeight && r.right <= innerWidth && d.scrollHeight <= d.clientHeight + 1, scroll: [d.scrollHeight, d.clientHeight],
            callout: callout?.querySelector(".callout-title")?.textContent.trim(), calloutBg: callout && getComputedStyle(callout).backgroundColor, calloutBottom: callout?.getBoundingClientRect().bottom,
            untouched: region ? [...region.querySelectorAll(".mono")].map((e) => e.textContent.trim()) : null, untouchedTop: title?.getBoundingClientRect().top, untouchedInView: inView(title),
            title: d.querySelector(".dialog-title").textContent.trim(), titleInView: inView(d.querySelector(".dialog-title")),
            foot: [...d.querySelectorAll(".dialog-foot button")].map((b) => ({ text: b.textContent.trim(), inView: inView(b) })) };`);
        // The tint of a danger callout is a red so faint that only its hue says red.
        const [cr, cg, cb] = rgb(layout.calloutBg);
        t.check(layout.callout === "Nothing of this Operation was kept" && cr > cg && cr > cb, "el diálogo muestra un recuadro rojo «Nothing of this Operation was kept»", `${layout.callout} · ${layout.calloutBg}`);
        t.check(layout.untouched?.[0] === file && layout.untouchedInView && layout.untouchedTop >= layout.calloutBottom - 1, `debajo sale el bloque «Untouched», que empieza por «${file}»`, JSON.stringify(layout.untouched));
        t.check(layout.fits, "el diálogo cabe en la ventana", `contenido ${layout.scroll[0]} px en ${layout.scroll[1]} px`);
        const foot = layout.foot.map((b) => b.text).join(", ");
        t.check(layout.title === "Disable 1 Entry" && layout.titleInView && foot === "Close, Retry" && layout.foot.every((b) => b.inView), "el título «Disable 1 Entry» y los botones «Close» y «Retry» están a la vista", `${layout.title} · ${foot}`);
        await s.exec("document.activeElement?.blur();");
        const f = await tabTo(s, (e) => e.role === "region" && e.text.includes(file));
        await wait(250);
        const ring = await s.exec("const c = getComputedStyle(document.activeElement); return { outline: c.outlineStyle + ' ' + c.outlineColor + ' ' + c.outlineWidth, shadow: c.boxShadow }");
        t.check(f && (violet(ring.outline.split(" ").slice(1).join(" ")) || violet(ring.shadow)), "con el foco del teclado, el bloque «Untouched» tiene un anillo violeta", JSON.stringify(ring));
        await t.shot("resultado", "el resultado cabe en la ventana; Untouched con anillo violeta");
        await press(s, "Close", ".dialog");
      } finally {
        t.ps(t.step.commands[1].text);
      }
    },
  },
];
