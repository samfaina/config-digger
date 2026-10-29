// Section 11: History · Undo, with real Operations on the throwaway Codex
// home: Disable, Undo, a refused Undo, a failed Operation, Needs attention,
// Delete backup, the busy states and the History filters.
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { KEY, ref, unwrap } from "../driver.mjs";
import { buttonEl, choose, focused, go, hoverFresh, press, refocus, tabTo, tooltip, typeInto, wait, waitNotice } from "../lib.mjs";
import { confirm, dialog, dialogParts, done, inventoryOf, openItem, operationId, project, waitPreview } from "./common.mjs";
import { loadingState } from "./loading.mjs";

const rgb = (c) => (c?.match(/\d+(\.\d+)?/g) ?? []).map(Number);
const sum = (c) => rgb(c).slice(0, 3).reduce((a, b) => a + b, 0);

// The Inventory on cd-throwaway-codex, as 11.1, 11.4, 11.5, 11.8 and 11.9
// reach it: Codex, codex-home, the Project, the Item. Returns the Details text.
async function codex(t) {
  await inventoryOf(t, "Codex", "codex-home");
  await project(t, `${t.root}\\project`);
  return openItem(t, "cd-throwaway-codex");
}

// Whether the dialog's line `said` gives the refused write of `file` by its
// path and its code, whatever Windows' words between them: on CI they are in
// the runner's language. The line goes on with the rollback.
export const causeOnCi = (said, file) => / \(os error 5\)/.test(said?.split(`cannot write ${file}: `)[1] ?? "");

function card(s) {
  return s.exec(`
    const c = document.querySelector(".panel.detail .entry-card");
    if (!c) return null;
    return { state: c.querySelector(".entry-head .state-pill")?.firstChild?.textContent.trim(), reading: c.querySelector(".entry-reading")?.textContent.trim() ?? null,
      spinner: !!c.querySelector(".entry-reading .spinner"), busy: c.getAttribute("aria-busy"),
      buttons: [...c.querySelectorAll(".entry-actions button")].map((b) => ({ text: b.textContent.trim(), disabled: b.disabled })) };`);
}

// The History list: each row's title, meta, chip and whether it is chosen.
function rows(s) {
  return s.exec(`return [...document.querySelectorAll(".operation-row")].map((r) => ({
    title: r.querySelector(".operation-title")?.textContent.trim(), meta: r.querySelector(".operation-meta")?.textContent.trim(),
    chip: r.querySelector(".status-chip")?.textContent.trim(), selected: r.classList.contains("selected") }));`);
}

async function pickOperation(s, id) {
  const row = await s.until(
    () => s.exec(`return [...document.querySelectorAll(".operation-row")].find((r) => r.querySelector(".operation-meta")?.textContent.includes("Operation " + arguments[0])) ?? null;`, id).then(unwrap),
    { what: `the Operation ${id}` },
  );
  await s.exec("arguments[0].scrollIntoView({ block: 'nearest' });", ref(row));
  await s.click(row);
  await s.until(() => s.exec("return !!document.querySelector('.history-detail .history-body') && !/Reading what it changed/.test(document.querySelector('.history-detail').innerText)"), { what: "the detail" });
  await wait(200);
}

function detail(s) {
  return s.exec(`
    const d = document.querySelector(".history-detail");
    const sec = (title) => [...d.querySelectorAll(".dialog-section")].find((e) => e.querySelector(".dialog-section-title")?.firstChild?.textContent.trim().startsWith(title));
    return { title: d.querySelector(".panel-title")?.firstChild?.textContent.trim(), chip: d.querySelector(".panel-title .status-chip")?.textContent.trim(), caption: d.querySelector(".caption")?.textContent.trim(),
      text: d.innerText, sections: [...d.querySelectorAll(".dialog-section-title")].map((e) => e.firstChild.textContent.trim()),
      states: sec("States changed")?.innerText ?? null, files: sec("Files")?.innerText ?? null, backup: sec("Backup")?.querySelector(".mono")?.textContent.trim() ?? null,
      callouts: [...d.querySelectorAll(".dialog-callout, .alert")].map((e) => ({ cls: e.className, text: e.innerText, top: e.getBoundingClientRect().top, visible: e.getBoundingClientRect().top >= d.getBoundingClientRect().top && e.getBoundingClientRect().top < innerHeight })),
      buttons: [...d.querySelectorAll("button")].map((b) => ({ text: b.textContent.trim(), disabled: b.disabled })),
      notUndoable: d.querySelector(".not-undoable")?.getAttribute("aria-label") ?? null };`);
}

const resultLine = (text, re) => text.split("\n").find((l) => re.test(l));

// Opens the one folder of the dialog's Files section and returns what it says.
async function folderNote(s) {
  const head = await s.waitFor(".dialog .file-block .file-head");
  if ((await s.attr(head, "aria-expanded")) !== "true") await s.click(head);
  return s.until(() => s.exec("return document.querySelector('.dialog .file-block .file-note')?.textContent.trim() ?? null"), { what: "the folder's sentence" });
}

export default [
  {
    id: "11.1",
    async run(t) {
      const { s } = t;
      const text = await codex(t);
      t.check(/^MCP server · Codex · /m.test(text), "sobre la tarjeta, la línea dice «MCP server · Codex · …»", text.split("\n")[2]);
      t.eq((await card(s))?.state, "Effective", "la Entry sale «Effective»");
      await press(s, "Disable", ".panel.detail");
      const d = await waitPreview(s);
      t.eq(d.title, "Disable 1 Entry", "el diálogo se titula «Disable 1 Entry»");
      const parts = await dialogParts(s);
      t.check(parts.files?.added.some((l) => l.text.includes("enabled = false")), "el diff añade «enabled = false»", parts.files?.added.map((l) => l.text).join(" | "));
      const masked = /--api-key"?,\s*"••••"/.test(parts.files?.text ?? "") || /"--api-key",\s*"••••"/.test(parts.files?.text ?? "");
      t.check(masked || (parts.files?.text.includes("--api-key") && parts.files.text.includes("••••")), "en args, el valor tras «--api-key» sale como «••••»", parts.files?.text.split("\n").find((l) => l.includes("--api-key")));
      t.check(!(await s.bodyText()).includes("not-a-real-secret"), "«not-a-real-secret» no sale en ninguna parte");
      const result = await confirm(s, "Disable");
      const steps = await s.exec(`return [...document.querySelectorAll(".dialog .steps .step")].map((e) => ({ text: e.innerText.replace(/\\s+/g, " ").trim(), cls: e.className }));`);
      for (const name of ["Back up", "Write", "Verify"]) t.check(steps.some((x) => x.text.includes(name) && /\bdone\b|\bok\b|\bpassed\b/.test(x.cls)), `pasa «${name}»`, JSON.stringify(steps));
      const line = resultLine(result.text, /^Disabled 1 Entry\. Operation \S+: verified\.$/);
      t.check(line, "el resultado dice «Disabled 1 Entry. Operation <id>: verified.»", result.text.split("\n").slice(0, 6).join(" | "));
      t.shared.disable111 = operationId(line ?? "");
      const foot = await s.exec(`return [...document.querySelectorAll(".dialog .dialog-foot button")].map((b) => b.textContent.trim());`);
      t.check(["Undo", "Open History", "Done"].every((b) => foot.includes(b)), "el resultado tiene los botones «Undo», «Open History» y «Done»", foot.join(", "));
    },
  },
  {
    id: "11.2",
    async run(t) {
      const { s } = t;
      await press(s, "Open History", ".dialog");
      await s.until(() => s.exec("return !!document.querySelector('.history .operation-row.selected')"), { what: "History" });
      await wait(400);
      const title = await s.exec("return document.querySelector('.history .banner-title').textContent.trim()");
      t.check(/^\d+ Operations?$/.test(title), "History se titula «N Operations»", title);
      const top = (await rows(s))[0];
      t.check(top.title === "Disable 1 Entry" && top.chip === "Applied" && top.selected, "la fila de arriba, «Disable 1 Entry», tiene la etiqueta «Applied» y está elegida", JSON.stringify(top));
      const d = await detail(s);
      t.check(d.states?.includes("Effective → Disabled") || /Effective\s*→\s*Disabled/.test(d.states ?? ""), "el detalle tiene «States changed», con «Effective → Disabled»", d.states);
      t.check(d.sections.includes("Files · 1") && d.files.includes("enabled = false") && d.files.includes("••••"), "el detalle tiene «Files · 1», con el mismo diff y «••••»", d.sections.join(", "));
      t.check(d.backup && /backups[\\/]/.test(d.backup), "el detalle tiene «Backup», con la ruta de la copia", d.backup);
      const windows = () => Number(t.ps("@((New-Object -ComObject Shell.Application).Windows()).Count"));
      const before = windows();
      await press(s, "Open backup folder", ".history-detail");
      const opened = await s.until(async () => windows() > before, { what: "an Explorer window", timeout: 8000 }).catch(() => false);
      const shown = t.ps(`@((New-Object -ComObject Shell.Application).Windows() | ForEach-Object { $_.Document.Folder.Self.Path }) -join '|'`);
      t.check(opened && shown.toLowerCase().includes(d.backup.toLowerCase()), "el Explorador abre esa carpeta", shown);
      const files = existsSync(d.backup) ? readdirSync(d.backup) : [];
      t.check(files.includes("manifest.json") && files.some((f) => f.startsWith("1-") && f.endsWith("config.toml")), "la carpeta tiene «manifest.json» y una copia de «config.toml» cuyo nombre empieza por «1-»", files.join(", "));
      t.ps(`(New-Object -ComObject Shell.Application).Windows() | Where-Object { $_.Document.Folder.Self.Path -eq '${d.backup.replaceAll("'", "''")}' } | ForEach-Object { $_.Quit() }`);
    },
  },
  {
    id: "11.3",
    async run(t) {
      const { s } = t;
      const id = t.shared.disable111;
      await pickOperation(s, id);
      await press(s, "Undo", ".history-detail .dialog-foot");
      // A refused Undo opens no preview: it shows Can't undo in the detail.
      const opened = await s.until(
        async () => (await dialog(s)) ?? ((await detail(s)).callouts.find((c) => c.text.startsWith("Can't undo")) ? "refused" : null),
        { what: "the preview or a refusal", timeout: 30000 },
      );
      if (opened === "refused") {
        const box = (await detail(s)).callouts.find((c) => c.text.startsWith("Can't undo"));
        t.check(false, `la vista previa se titula «Undo ${id} "Disable 1 Entry"»`, `el Undo se rechaza: ${box.text.replace(/\s+/g, " ")}`);
        return;
      }
      const d = await waitPreview(s);
      t.eq(d.title, `Undo ${id} "Disable 1 Entry"`, `la vista previa se titula «Undo ${id} "Disable 1 Entry"»`);
      const result = await confirm(s, "Undo");
      const line = resultLine(result.text, new RegExp(`^Undid ${id}\\. Operation \\S+: verified\\.$`));
      t.check(line, "el resultado dice «Undid <id>. Operation <id nuevo>: verified.»", result.text.split("\n").slice(0, 6).join(" | "));
      const undo = operationId(line?.replace(/^Undid \S+ /, "") ?? "");
      t.shared.undo113 = undo;
      await done(s);
      await wait(600);
      const list = await rows(s);
      t.check(list.some((r) => r.chip === `Undo of ${id}`), `la lista tiene una fila nueva con la etiqueta «Undo of ${id}»`, list.slice(0, 3).map((r) => `${r.title} [${r.chip}]`).join(" | "));
      const original = list.find((r) => r.meta?.includes(id));
      t.check(original?.chip === `Undone by ${undo}`, `la fila original dice «Undone by ${undo}»`, original?.chip);
      await pickOperation(s, id);
      const det = await detail(s);
      t.check(det.notUndoable, "la fila original muestra «Can't undo»");
      const cant = await s.find(".history-detail .not-undoable");
      await hoverFresh(s, cant);
      await wait(900);
      t.eq((await tooltip(s))?.text, `It was undone by ${undo}; undo that Operation to apply it again.`, `el tooltip de «Can't undo» dice «It was undone by ${undo}; undo that Operation to apply it again.»`);
    },
  },
  {
    id: "11.4",
    note: "config.toml se edita desde la prueba, no en el Bloc de notas: el mismo cambio de «enabled = false» a «enabled = true».",
    async run(t) {
      const { s } = t;
      const file = `${t.root}\\codex-home\\config.toml`;
      await codex(t);
      if ((await card(s))?.state === "Disabled") {
        // The Undo of 11.3 was refused, so cd-throwaway-codex is still
        // Disabled, and the app offers no Enable for it ("change it by
        // hand"). Do that by hand, and end config.toml with a newline, which
        // the refusal of 11.3 turns on, so the rest of the section checks its
        // own flows instead of meeting that refusal again.
        const enable = await s.exec(`return [...document.querySelectorAll(".panel.detail .action-note")].map((e) => e.textContent.trim()).find((x) => x.startsWith("Enable")) ?? null;`);
        const toml = readFileSync(file, "utf8").replace(/\r?\nenabled = false/, "");
        writeFileSync(file, toml.endsWith("\n") ? toml : toml + "\n");
        t.note(`cd-throwaway-codex seguía Disabled porque el Undo del paso 11.3 se rechazó, y la tarjeta no ofrece Enable${enable ? ` («${enable}»)` : ""}. Se quitó «enabled = false» a mano y config.toml recibió el salto de línea final que le falta, para que los pasos siguientes no choquen con el mismo rechazo.`);
        await refocus(s);
        await wait(800);
        await codex(t);
      }
      await press(s, "Disable", ".panel.detail");
      await waitPreview(s);
      const result = await confirm(s, "Disable");
      const id = operationId(resultLine(result.text, /^Disabled 1 Entry\. Operation \S+: verified\.$/) ?? "");
      t.shared.refused114 = id;
      await done(s);
      const toml = readFileSync(file, "utf8");
      t.check(toml.includes("enabled = false"), "config.toml tiene «enabled = false» tras el Disable");
      writeFileSync(file, toml.replace("enabled = false", "enabled = true"));
      await go(s, "History · Undo");
      await pickOperation(s, id);
      const d = await detail(s);
      const refusal = `\`mcp_servers.cd-throwaway-codex.enabled\` in ${file} changed since the Operation, so the Undo is refused;`;
      t.check(d.callouts.some((c) => c.text.includes(refusal)), "antes de pulsar Undo, el detalle ya muestra un aviso con el mismo texto del rechazo", d.callouts.map((c) => c.text).join(" | "));
      const changes = (d.states ?? "").split("\n").filter((l) => /→/.test(l));
      t.check(changes.length > 0 && new Set(changes).size === changes.length, "en «States changed», cada cambio sale una sola vez", changes.join(" | "));
      const contexts = /\b\d+ contexts\b/.test(d.states ?? "");
      t.check(contexts, "un cambio igual en varios contextos sale bajo un título como «3 contexts»", d.states);
      await press(s, "Undo", ".history-detail .dialog-foot");
      await wait(400);
      t.check(!(await dialog(s)), "al pulsar Undo no se abre ningún diálogo");
      const after = await detail(s);
      const box = after.callouts.find((c) => c.cls.includes("danger") && c.text.startsWith("Can't undo"));
      t.check(box && box.visible, "en el acto, arriba del detalle y a la vista, sale un recuadro rojo «Can't undo»", JSON.stringify(box));
      t.check(box?.text.includes(refusal), `el recuadro dice «${refusal} …»`, box?.text);
      t.check(after.sections.includes("Backup → now"), "debajo sale la sección «Backup → now», con la diferencia", after.sections.join(", "));
      const names = after.buttons.map((b) => b.text);
      t.check(names.includes("Open backup folder") && names.includes("Copy path"), "debajo salen los botones «Open backup folder» y «Copy path»", names.join(", "));
      t.check(!names.some((n) => /force|restore|overwrite/i.test(n)), "no hay ningún botón para restaurar a la fuerza", names.join(", "));
      await t.shot("rechazo", "el recuadro rojo «Can't undo» arriba del detalle, a la vista");
    },
  },
  {
    id: "11.5",
    // The cause is Windows' own text, in its display language. The rest of
    // the step needs none, and 11.11 needs its Operation.
    ciLacks: "Windows en español, que escribe «Acceso denegado. (os error 5)» en la causa: el del runner la escribe en inglés",
    ciRuns: true,
    async run(t) {
      const { s } = t;
      const file = `${t.root}\\codex-home\\config.toml`;
      t.ps(t.step.commands[0].text);
      try {
        await codex(t);
        t.eq((await card(s))?.state, "Effective", "«cd-throwaway-codex» sale «Effective», por el cambio a mano del paso 11.4");
        await press(s, "Disable", ".panel.detail");
        await waitPreview(s);
        const result = await confirm(s, "Disable");
        const steps = await s.exec(`return [...document.querySelectorAll(".dialog .steps .step")].map((e) => e.innerText.replace(/\\s+/g, " ").trim());`);
        t.check(steps.some((x) => /Back up/.test(x) && x.includes("✓")) && steps.some((x) => /Write failed/.test(x) && x.includes("✗")) && steps.some((x) => /Rollback/.test(x) && x.includes("✓")), "el diálogo muestra «Back up ✓», «Write failed ✗» y «Rollback ✓»", steps.join(" | "));
        t.check(result.text.includes("Nothing of this Operation was kept"), "el diálogo muestra un recuadro rojo «Nothing of this Operation was kept»");
        const cause = `cannot write ${file}: Acceso denegado. (os error 5)`;
        const said = result.text.split("\n").find((l) => l.includes("cannot write"));
        // On CI the path and the code are checked, not Windows' words between them.
        if (t.ciLacks) t.check(causeOnCi(said, file), `la causa dice «cannot write ${file}: … (os error 5)»`, said);
        else t.check(result.text.includes(cause), `la causa dice «${cause}»`, said);
        const id = operationId(result.text);
        t.shared.failed115 = id;
        await press(s, "Close", ".dialog");
        await go(s, "History · Undo");
        await pickOperation(s, id);
        const d = await detail(s);
        t.eq(d.chip, "Failed, rolled back", "en History, la Operation tiene la etiqueta «Failed, rolled back»");
        t.check(d.sections.includes("Rollback") && /never written/i.test(d.text), "la Operation tiene una sección «Rollback» con el archivo como «never written»", d.sections.join(", "));
        t.check(d.notUndoable, "la Operation muestra «Can't undo»");
      } finally {
        t.ps(t.step.commands[1].text);
      }
    },
  },
  {
    id: "11.6",
    note: "La app se cierra y se abre con una sesión nueva de WebDriver; manifest.json se edita desde la prueba.",
    async run(t) {
      const { s } = t;
      const id = t.shared.removeLeftoversOperation;
      if (!id) {
        t.blocked = "falta el id de la Operation del paso 10.7";
        return;
      }
      await s.close();
      const manifest = join(t.env.LOCALAPPDATA, "config-digger", "backups", id, "manifest.json");
      const text = readFileSync(manifest, "utf8");
      t.check(text.includes('"status": "applied",'), "el registro de la Operation tiene «\"status\": \"applied\",»");
      writeFileSync(manifest, text.replace('"status": "applied",', '"status": "needs_attention",'));
      await t.restart();
      await s.waitFor(".rail button[aria-label='Inventory']");
      const banner = await s.until(() => s.exec("return document.querySelector('.recovery-banner')?.innerText ?? null"), { what: "the recovery banner" }).catch(() => null);
      t.check(banner?.includes("An Operation needs manual recovery") && /Open History/i.test(banner), "el Inventario muestra una barra roja «An Operation needs manual recovery» con el botón «Open History»", banner);
      await press(s, "Open History", ".recovery-banner");
      await s.until(() => s.exec("return !!document.querySelector('.history-detail .panel-title .status-chip')"), { what: "the Operation" });
      await pickOperation(s, id);
      let d = await detail(s);
      t.eq(d.chip, "Needs attention", "en History, la Operation tiene la etiqueta «Needs attention»");
      t.check(d.callouts.some((c) => c.text.startsWith("This Operation needs manual recovery")), "la Operation tiene un recuadro rojo «This Operation needs manual recovery»");
      await s.instrument();
      await s.delay({ dismiss_operation: 1500 });
      await press(s, "Dismiss", ".history-detail");
      const busy = await waitNotice(s, `Dismissing Operation ${id}…`, { timeout: 1500 }).catch(() => null);
      d = await detail(s);
      const off = d.buttons.filter((b) => ["Dismiss", "Delete backup"].includes(b.text));
      t.check(busy?.cls.includes("busy"), `nada más pulsar Dismiss sale abajo un aviso con rueda «Dismissing Operation ${id}…»`, busy?.text);
      t.check(off.length === 2 && off.every((b) => b.disabled), "mientras, los botones «Dismiss» y «Delete backup» del detalle están apagados", JSON.stringify(off));
      await s.delay({});
      const dismissed = await waitNotice(s, `Dismissed Operation ${id}.`);
      t.check(dismissed, `luego sale el aviso «Dismissed Operation ${id}.»`);
      await wait(400);
      t.eq((await detail(s)).chip, "Rollback incomplete, dismissed", "la etiqueta pasa a «Rollback incomplete, dismissed»");
      await go(s, "Inventory");
      await wait(500);
      t.check(!(await s.exec("return !!document.querySelector('.recovery-banner')")), "la barra roja ya no está en el Inventario");
    },
  },
  {
    id: "11.7",
    async run(t) {
      const { s } = t;
      const id = t.shared.refused114;
      await go(s, "History · Undo");
      await pickOperation(s, id);
      await press(s, "Delete backup", ".history-detail");
      const d = await s.until(() => dialog(s), { what: "the dialog" });
      t.eq(d.title, `Delete the backup of ${id} "Disable 1 Entry"`, `el diálogo se titula «Delete the backup of ${id} "Disable 1 Entry"»`);
      t.check(/forever|permanently|for good/i.test(d.text) && /no longer be undone|can't be undone|cannot be undone/i.test(d.text), "el diálogo avisa de que el backup se borra para siempre y de que la Operation ya no se podrá deshacer", d.text.split("\n").slice(1, 4).join(" | "));
      const out = await confirm(s, "Delete backup");
      t.check(out.text.includes(`Deleted the backup of ${id}. It can no longer be undone.`), `el resultado dice «Deleted the backup of ${id}. It can no longer be undone.»`);
      t.check(out.text.includes(`Operation ${id} · backup deleted · can no longer be undone`), `debajo sale la línea «Operation ${id} · backup deleted · can no longer be undone»`);
      await press(s, "Done", ".dialog").catch(() => press(s, "Close", ".dialog"));
      await wait(500);
      const det = await detail(s);
      t.eq(det.chip, "Applied, backup deleted, not undoable", "la etiqueta pasa a «Applied, backup deleted, not undoable»");
      t.check(det.text.includes(`Can't show a diff: the backup of Operation ${id} was deleted, with the values of its edits.`), `Files dice «Can't show a diff: the backup of Operation ${id} was deleted, with the values of its edits.»`, det.files);
      t.check(!det.buttons.some((b) => b.text === "Delete backup"), "el botón «Delete backup» ya no está");
    },
  },
  {
    id: "11.8",
    async run(t) {
      const { s } = t;
      await codex(t);
      await s.instrument();
      await s.delay({ plan_action: 1500 });
      await press(s, "Disable", ".panel.detail");
      const preparing = await s.until(() => dialog(s), { what: "the dialog", timeout: 1500 });
      const state = await loadingState(s, ".dialog");
      t.eq(preparing.title, "Preparing the preview…", "al pulsar Disable, el diálogo se titula «Preparing the preview…»");
      t.check(state.loading === "Reading the files this action touches…" && state.rows > 0 && state.spinner !== "none", "el diálogo muestra un aviso con rueda «Reading the files this action touches…», sobre filas grises que laten", JSON.stringify(state));
      await s.delay({});
      await waitPreview(s);
      await confirm(s, "Disable");
      await s.delay({ read_inventory: 1500 });
      await done(s);
      const busy = await waitNotice(s, "Reading the inventory again…", { timeout: 1500 }).catch(() => null);
      const c = await card(s);
      t.check(busy?.cls.includes("busy"), "tras Done sale abajo un aviso con rueda «Reading the inventory again…»", busy?.text);
      t.check(c?.reading === "Reading…" && c.spinner, "junto al State de la tarjeta sale una rueda pequeña con «Reading…»", JSON.stringify(c));
      const off = c?.buttons.filter((b) => ["Disable", "Remove"].includes(b.text)) ?? [];
      t.check(off.length === 2 && off.every((b) => b.disabled), "mientras, los botones «Disable» y «Remove» de la tarjeta se ven apagados", JSON.stringify(off));
      await s.delay({});
      const after = await s.until(async () => {
        const x = await card(s);
        return x && !x.reading && x.state === "Disabled" ? x : null;
      }, { what: "the card Disabled" });
      t.check(after.buttons.some((b) => b.text === "Enable"), "luego la tarjeta dice «Disabled» y ofrece «Enable»", JSON.stringify(after));
      await go(s, "History · Undo");
      const top = await s.until(() => s.exec("return document.querySelector('.operation-row')?.querySelector('.operation-meta')?.textContent ?? null"), { what: "the rows" });
      const id = operationId(top + " ");
      await pickOperation(s, id);
      await press(s, "Undo", ".history-detail .dialog-foot");
      await waitPreview(s);
      const result = await confirm(s, "Undo");
      const undo = operationId(resultLine(result.text, /^Undid /)?.replace(/^Undid \S+ /, "") ?? "");
      await s.delay({ read_history: 1500 });
      await done(s);
      const hbusy = await waitNotice(s, "Reading History again…", { timeout: 1500 }).catch(() => null);
      const d = await detail(s);
      const hoff = d.buttons.filter((b) => ["Delete backup", "Undo"].includes(b.text));
      t.check(hbusy?.cls.includes("busy"), "en History, tras Done sale abajo el aviso con rueda «Reading History again…»", hbusy?.text);
      t.check(hoff.length === 2 && hoff.every((b) => b.disabled), "mientras, los botones «Delete backup» y «Undo» del detalle se ven apagados", JSON.stringify(hoff));
      await s.delay({});
      await wait(1800);
      await pickOperation(s, id);
      const fin = await detail(s);
      t.check(fin.chip === `Undone by ${undo}` && fin.notUndoable, `luego la Operation dice «Undone by ${undo}» y muestra «Can't undo»`, `${fin.chip}; ${fin.notUndoable}`);
    },
  },
  {
    id: "11.9",
    async run(t) {
      const { s } = t;
      await codex(t);
      await press(s, "Disable", ".panel.detail");
      await waitPreview(s);
      await confirm(s, "Disable");
      await done(s);
      await s.until(() => s.exec("return !document.querySelector('.notice.busy, .read-progress')"), { what: "the re-read" });
      const snack = await s.until(() => s.find(".snackbar .snackbar-action").catch(() => null), { what: "the Undo snackbar" });
      t.check((await s.exec("return document.querySelector('.snackbar').textContent")).startsWith("Disabled 1 Entry"), "el aviso de abajo empieza por «Disabled 1 Entry»");
      await s.click(snack);
      await waitPreview(s);
      await confirm(s, "Undo");
      await s.instrument();
      await s.delay({ read_inventory: 1500 });
      await done(s);
      const busy = await waitNotice(s, "Reading the inventory again…", { timeout: 1500 }).catch(() => null);
      const c = await card(s);
      t.check(busy?.cls.includes("busy"), "tras el último Done sale abajo un aviso con rueda «Reading the inventory again…»", busy?.text);
      t.check(c?.reading === "Reading…" && c.spinner, "la tarjeta tiene junto a su State una rueda pequeña con «Reading…»", JSON.stringify(c));
      const off = c?.buttons.filter((b) => ["Enable", "Remove"].includes(b.text)) ?? [];
      t.check(off.length === 2 && off.every((b) => b.disabled), "mientras, sus botones «Enable» y «Remove» se ven apagados", JSON.stringify(c?.buttons));
      await s.delay({});
      const after = await s.until(async () => {
        const x = await card(s);
        return x && !x.reading && x.state === "Effective" ? x : null;
      }, { what: "the card Effective" });
      t.check(after.buttons.some((b) => b.text === "Disable"), "luego la tarjeta dice «Effective» y ofrece «Disable»", JSON.stringify(after));
    },
  },
  {
    id: "11.10",
    async run(t) {
      const { s } = t;
      await codex(t);
      await press(s, "Disable", ".panel.detail");
      await waitPreview(s);
      await confirm(s, "Disable");
      // The snackbar enters in a fraction of a second: slowed down twenty
      // times, its keyframes can be read while it plays.
      await s.cdp("Animation.enable");
      await s.cdp("Animation.setPlaybackRate", { playbackRate: 0.05 });
      await done(s);
      await s.hover({ x: 600, y: 4 });
      const look = await s.until(
        () =>
          s.exec(`
            const n = document.querySelector(".snackbar"); if (!n) return null;
            const c = getComputedStyle(n), r = n.getBoundingClientRect();
            const undo = n.querySelector(".snackbar-action"), x = n.querySelector("button[aria-label=Dismiss]");
            const cs = (e) => ({ bg: getComputedStyle(e).backgroundColor, color: getComputedStyle(e).color, border: getComputedStyle(e).borderTopWidth + " " + getComputedStyle(e).borderTopStyle + " " + getComputedStyle(e).borderTopColor });
            return { text: n.textContent, radius: parseFloat(c.borderTopLeftRadius), height: r.height, bg: c.backgroundColor, edge: c.borderLeftWidth + " " + c.borderLeftColor, color: c.color,
              panel: getComputedStyle(document.querySelector(".panel")).backgroundColor, cardBg: getComputedStyle(document.querySelector(".entry-card") ?? document.querySelector(".panel")).backgroundColor,
              noticeRadius: parseFloat(getComputedStyle(document.querySelector(".entry-card")).borderTopLeftRadius),
              undo: cs(undo), x: cs(x), frames: n.getAnimations().map((a) => ({ name: a.animationName, frames: a.effect.getKeyframes().map((k) => ({ translate: k.translate, transform: k.transform, opacity: k.opacity })) })) };`),
        { what: "the snackbar" },
      );
      await s.cdp("Animation.setPlaybackRate", { playbackRate: 1 });
      const from = Date.now();
      const [r, g, b] = rgb(look.bg);
      t.check(look.radius < look.height / 2 - 2, "el aviso tiene las esquinas redondeadas de los avisos del paso 5.1, no de píldora", `radio ${look.radius}, alto ${Math.round(look.height)}`);
      t.check(b > g + 30 && r > g && sum(look.bg) > sum(look.panel) && sum(look.bg) > sum(look.cardBg), "su fondo morado destaca, más claro que el panel y las tarjetas", `${look.bg} frente a ${look.panel}`);
      const [er, eg, eb] = rgb(look.edge.split(" ").slice(1).join(" "));
      t.check(!look.edge.startsWith("0px") && eb > eg && er > 150, "tiene un borde lila claro a la izquierda", look.edge);
      t.check(rgb(look.color).every((v) => v > 230), "su texto es blanco", look.color);
      const [ur, ug, ub] = rgb(look.undo.color);
      t.check(!look.undo.border.startsWith("0px") && ub > ug && ur > 150, "«Undo» es un botón con borde y letras lila claro", JSON.stringify(look.undo));
      t.check(rgb(look.x.bg)[3] === 0 || look.x.bg === "rgba(0, 0, 0, 0)", "«✕» es lila claro, sin fondo", JSON.stringify(look.x));
      t.check(look.frames.some((k) => k.frames.length >= 2 && k.frames[0].translate && k.frames[0].translate !== "none"), "entra subiendo un poco mientras aparece", JSON.stringify(look.frames));
      // The shot shows the snackbar at rest, once it has floated in.
      await s
        .until(() => s.exec("return document.querySelector('.snackbar')?.getAnimations().every((a) => a.playState === 'finished') ?? true;"), { what: "the snackbar at rest", timeout: 3000 })
        .catch(() => {});
      await t.shot("snackbar", "fondo morado que destaca, borde lila claro a la izquierda, texto blanco, Undo con borde y letras lila claro, ✕ lila claro sin fondo");
      const undo = await s.find(".snackbar .snackbar-action");
      await s.hover(undo);
      await wait(300);
      const hovered = await s.exec("const u = document.querySelector('.snackbar .snackbar-action'); return { bg: getComputedStyle(u).backgroundColor, color: getComputedStyle(u).color }");
      t.check(sum(hovered.bg) > 300 && rgb(hovered.color)[2] > rgb(hovered.color)[1], "con el ratón encima, «Undo» se rellena de lila con letras moradas", JSON.stringify(hovered));
      await t.shot("hover", "Undo relleno de lila con letras moradas");
      const xButton = await s.find(".snackbar button[aria-label=Dismiss]");
      await s.hover(xButton);
      await wait(300);
      const xh = await s.exec("const u = document.querySelector('.snackbar button[aria-label=Dismiss]'); return { bg: getComputedStyle(u).backgroundColor, color: getComputedStyle(u).color }");
      t.check(sum(xh.bg) > 300, "con el ratón encima, «✕» se rellena de lila", JSON.stringify(xh));
      await wait(9000);
      t.check(await s.exec("return !!document.querySelector('.snackbar')"), "con el ratón encima no se va", `${Math.round((Date.now() - from) / 1000)} s`);
      await s.exec("document.activeElement?.blur()");
      const f = await tabTo(s, (e) => e.text === "Undo" && e.tag === "BUTTON", 60);
      await wait(200);
      const ring1 = await s.exec("const c = getComputedStyle(document.activeElement); return c.outlineStyle + ' ' + c.outlineColor + ' | ' + c.boxShadow");
      await s.keys(KEY.Tab);
      await wait(200);
      const f2 = await focused(s);
      const ring2 = await s.exec("const c = getComputedStyle(document.activeElement); return c.outlineStyle + ' ' + c.outlineColor + ' | ' + c.boxShadow");
      const white = (x) => /rgb\(25[0-5], 25[0-5], 25[0-5]\)/.test(x) && !/^none/.test(x);
      t.check(f && f2?.label === "Dismiss" && white(ring1) && white(ring2), "con el teclado, el foco en «Undo» y en «✕» se ve con un anillo blanco", `${ring1} / ${ring2}`);
      await s.exec("document.activeElement?.blur()");
      await s.hover({ x: 600, y: 4 });
      const left = Date.now();
      const out = await s.until(
        () => s.exec(`const n = document.querySelector(".snackbar.leaving"); return n ? n.getAnimations().map((a) => a.effect.getKeyframes().map((k) => ({ translate: k.translate, opacity: k.opacity }))) : null;`),
        { what: "the snackbar leaving", timeout: 20000 },
      );
      const gone = Date.now() - left;
      t.check(gone > 9000 && gone < 12000, "unos 10 segundos después de apartar el ratón, se va", `${gone} ms`);
      t.check(out.some((k) => k.at(-1)?.translate && k.at(-1).translate !== "none" && Number(k.at(-1).opacity) === 0), "se va bajando un poco mientras se desvanece", JSON.stringify(out));
      await s.until(() => s.exec("return !document.querySelector('.snackbar')"), { what: "the snackbar gone" });
      const enable = await buttonEl(s, "Enable", ".panel.detail");
      if (!enable || (await s.attr(enable, "disabled")) !== null) {
        const why = await s.exec(`return [...document.querySelectorAll(".panel.detail .action-note")].map((e) => e.textContent.trim()).find((x) => x.startsWith("Enable")) ?? null;`);
        t.check(false, "en la tarjeta de cd-throwaway-codex, Enable se puede pulsar", why);
        return;
      }
      await s.click(enable);
      await waitPreview(s);
      await confirm(s, "Enable");
      await done(s);
      const x = await s.until(() => s.find(".snackbar button[aria-label=Dismiss]").catch(() => null), { what: "the Enable snackbar" });
      t.check((await s.exec("return document.querySelector('.snackbar').textContent")).startsWith("Enabled 1 Entry"), "sale el aviso «Enabled 1 Entry»");
      await s.click(x);
      const leaving = await s.until(() => s.exec("return document.querySelector('.snackbar')?.classList.contains('leaving') ? document.querySelector('.snackbar').getAnimations().length : null"), { what: "the snackbar leaving", timeout: 2000 }).catch(() => null);
      t.check(leaving > 0, "al pulsar ✕, el aviso de Enable se va igual, sin desaparecer de golpe", String(leaving));
      t.shared.enable1110 = true;
    },
  },
  {
    id: "11.11",
    note: "La app se cierra y se abre con una sesión nueva de WebDriver; la orden del paso marca el manifest.json desde la prueba.",
    async run(t) {
      const { s } = t;
      await s.close();
      let id;
      try {
        id = t.ps(t.step.commands[0].text);
      } catch (error) {
        await t.restart();
        t.blocked = `la orden no marcó la Operation: ${String(error.stderr ?? error.message).trim().split("\n")[0]}`;
        return;
      }
      await t.restart();
      await s.waitFor(".rail button[aria-label='Inventory']");
      await go(s, "History · Undo");
      await s.until(() => s.exec("return document.querySelectorAll('.operation-row').length > 0"), { what: "the rows" });
      const title = () => s.exec("return document.querySelector('.history .banner-title').textContent.trim()");
      const n = Number((await title()).match(/^(\d+) Operations?$/)?.[1]);
      t.check(n > 0, "el título dice «N Operations»", await title());
      await choose(s, "Verb", "Enable");
      let list = await rows(s);
      const m = (await title()).match(/^(\d+) of (\d+) Operations$/);
      t.check(m && Number(m[1]) < n && Number(m[2]) === n, "con Enable, el título dice «M of N Operations», con M menor que N", await title());
      t.check(list.length > 0 && list.every((r) => r.title === "Enable 1 Entry") && list.some((r) => r.meta.includes(id)), "con Enable, solo salen filas cuyo título es «Enable 1 Entry», sea cual sea su etiqueta, entre ellas la del id que apuntaste", list.map((r) => `${r.title} [${r.chip}]`).join(", "));
      await choose(s, "Verb", "Undo");
      list = await rows(s);
      t.check(list.length > 0 && list.every((r) => r.title.startsWith("Undo") && /^Undo of \S+/.test(r.chip)), "con Undo, solo salen filas que empiezan por «Undo», con la etiqueta «Undo of <id>»", list.map((r) => `${r.title} [${r.chip}]`).join(", "));
      await choose(s, "Verb", "Any verb");
      await choose(s, "Status", "Undone");
      list = await rows(s);
      t.check(list.length > 0 && list.every((r) => /undone by \S+/i.test(r.chip)), "con Undone, solo salen filas cuya etiqueta incluye «undone by <id>»", list.map((r) => r.chip).join(", "));
      t.check(!list.some((r) => r.meta.includes(id)), "con Undone, no sale la Operation del id que apuntaste", list.map((r) => r.meta).join(" | "));
      await choose(s, "Status", "Failed");
      list = await rows(s);
      t.check(list.length > 0 && list.every((r) => ["Failed, rolled back", "Rollback incomplete, dismissed"].includes(r.chip)) && list.some((r) => r.meta.includes(t.shared.failed115)), "con Failed, solo salen filas «Failed, rolled back» o «Rollback incomplete, dismissed», entre ellas la del paso 11.5", list.map((r) => r.chip).join(", "));
      t.check(!list.some((r) => r.meta.includes(id)), "con Failed, no sale la Operation del id que apuntaste, que necesita atención pero no ha fallado", list.map((r) => r.meta).join(" | "));
      await choose(s, "Status", "Needs attention");
      list = await rows(s);
      const marked = list.find((r) => r.meta.includes(id));
      t.check(marked?.title === "Enable 1 Entry" && marked.chip === "Needs attention", "con Needs attention, sale la Operation del id que apuntaste, «Enable 1 Entry», con la etiqueta «Needs attention»", JSON.stringify(marked));
      t.check(list.length > 0 && list.every((r) => r.chip === "Needs attention"), "con Needs attention, todas las filas tienen la etiqueta «Needs attention»", list.map((r) => r.chip).join(", "));
      const dismissed116 = t.shared.removeLeftoversOperation;
      t.check(dismissed116 && !list.some((r) => r.meta.includes(dismissed116)), "con Needs attention, no sale la Operation del paso 11.6, que ya dice «Rollback incomplete, dismissed»", list.map((r) => r.meta).join(" | "));
      await choose(s, "Status", "Any status");
      await typeInto(s, "input[aria-label='Search Entry names']", "CODEX");
      await wait(300);
      const codexRows = await s.exec("return [...document.querySelectorAll('.operation-row')].map((r) => r.querySelector('.operation-meta').textContent)");
      let allCodex = codexRows.length > 0;
      for (const meta of codexRows.slice(0, 4)) {
        await pickOperation(s, operationId(meta + " "));
        const d = await detail(s);
        allCodex &&= /ENTRY\s+cd-throwaway-codex/i.test(d.states ?? "");
      }
      t.check(allCodex, "con CODEX, cada fila elegida tiene «ENTRY cd-throwaway-codex» en «States changed»", `${codexRows.length} filas`);
      const userDisable = (await s.exec("return [...document.querySelectorAll('.operation-row')].map((r) => r.textContent)")).some((x) => x.includes("cd-throwaway-user"));
      t.check(!userDisable, "con CODEX, no sale el Disable de cd-throwaway-user del paso 7.3");
      await typeInto(s, "input[aria-label='Search Entry names']", "cd-throwaway-nada");
      await wait(300);
      const empty = await s.exec("const e = document.querySelector('.history-list .empty'); return e ? { text: e.innerText, button: !!e.querySelector('button') } : null");
      t.check(empty?.text.startsWith("No Operation matches the filter.") && empty.button && /Clear filter/i.test(empty.text), "con cd-throwaway-nada, la lista dice «No Operation matches the filter.», con el botón «Clear filter»", JSON.stringify(empty));
      t.eq(await title(), `0 of ${n} Operations`, `con cd-throwaway-nada, el título dice «0 of ${n} Operations»`);
      await press(s, "Clear filter", ".history-list");
      await wait(300);
      const state = await s.exec("return { box: document.querySelector('input[aria-label=\"Search Entry names\"]').value, verb: document.querySelector('[role=combobox][aria-label=Verb]').textContent.trim(), status: document.querySelector('[role=combobox][aria-label=Status]').textContent.trim() }");
      t.check(state.box === "" && state.verb === "Any verb" && state.status === "Any status", "tras Clear filter, la caja está vacía y los desplegables dicen «Any verb» y «Any status»", JSON.stringify(state));
      t.eq(await title(), `${n} Operations`, `tras Clear filter, el título vuelve a «${n} Operations»`);
      await pickOperation(s, id);
      await press(s, "Dismiss", ".history-detail");
      await waitNotice(s, `Dismissed Operation ${id}.`);
      await wait(400);
      t.eq((await detail(s)).chip, "Rollback incomplete, dismissed", "tras Dismiss, la etiqueta de esa Operation pasa a «Rollback incomplete, dismissed»");
      await go(s, "Inventory");
      await wait(500);
      t.check(!(await s.exec("return !!document.querySelector('.recovery-banner')")), "tras Dismiss, la barra roja ya no está en el Inventario");
    },
  },
  {
    id: "11.12",
    async run(t) {
      const { s } = t;
      const link = `${t.root}\\claude-code-home\\.claude\\skills\\cd-throwaway-linked-skill`;
      const target = `${t.root}\\linked\\cd-throwaway-linked-skill`;
      const written = String.raw`..\..\..\linked\cd-throwaway-linked-skill`;
      await inventoryOf(t, "Claude Code", "claude-code-home");
      await openItem(t, "cd-throwaway-linked-skill");
      const entry = await s.exec(`
        const c = document.querySelector(".panel.detail .entry-card");
        return { tag: c?.querySelector(".tag.linked")?.textContent.trim(), note: [...(c?.querySelectorAll(".action-note") ?? [])].map((e) => e.textContent.trim()) };`);
      t.eq(entry.tag, `symlink → ${target}`, `la tarjeta tiene la etiqueta «symlink → ${target}»`);
      t.check(entry.note.some((n) => n.endsWith("It removes the symlink only, never its target.")), "la nota bajo Remove dice «It removes the symlink only, never its target.»", entry.note.join(" | "));
      await press(s, "Remove", ".panel.detail .entry-card");
      await waitPreview(s);
      const removes = await folderNote(s);
      const says = removes.startsWith(`Removes the folder symlink ${link} only; its target ${target} (1 file, `) && removes.endsWith(") stays as it is.");
      t.check(says,`la carpeta del Remove dice «Removes the folder symlink ${link} only; its target ${target} (1 file, …) stays as it is.»`, removes);
      const result = await confirm(s, "Remove");
      const line = resultLine(result.text, /^Removed 1 Entry\. Operation \S+: verified\.$/);
      t.check(line, "el resultado del Remove dice «Removed 1 Entry. Operation <id>: verified.»", result.text.split("\n").slice(0, 6).join(" | "));
      const id = operationId(line ?? "");
      await done(s);
      await go(s, "History · Undo");
      await pickOperation(s, id);
      await press(s, "Undo", ".history-detail .dialog-foot");
      await waitPreview(s);
      const creates = await folderNote(s);
      t.eq(creates, `Creates the folder symlink ${link} to ${written} again.`, `la carpeta del Undo dice «Creates the folder symlink ${link} to ${written} again.»`);
      const undone = await confirm(s, "Undo");
      const undid = resultLine(undone.text, new RegExp(`^Undid ${id}\\. Operation \\S+: verified\\.$`));
      t.check(undid, "el resultado del Undo dice «Undid <id>. Operation <id nuevo>: verified.»", undone.text.split("\n").slice(0, 6).join(" | "));
      await done(s);
      const [kind, holds] = t.ps(`$link = Get-Item '${link}'; $link.LinkType; $link.Target`).split(/\r?\n/);
      t.eq(kind, "SymbolicLink", "la orden muestra primero «SymbolicLink»");
      t.eq(holds, written, `la orden muestra después «${written}», sin «E:\\»`);
    },
  },
];
