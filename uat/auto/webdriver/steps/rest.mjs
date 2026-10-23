// Sections 14, 12 and 13 as far as the app's window reaches them: the scan
// log lines, the updater's controls, parking cd-throwaway-desktop, and the
// throwaway data's teardown. Install, update and uninstall run in Windows
// Sandbox (uat/auto/sandbox).
import { spawnSync } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { go, press, wait } from "../lib.mjs";
import { confirm, done, inventoryOf, openItem, project, waitPreview, withClaudeDesktop } from "./common.mjs";
import { unwrap } from "../driver.mjs";

// The scan lines since the last "app started", as the steps' command prints them.
function scanLines(t) {
  const dir = join(t.env.LOCALAPPDATA, "config-digger", "logs");
  const file = readdirSync(dir).filter((f) => f.startsWith("config-digger-app.")).sort().at(-1);
  const lines = readFileSync(join(dir, file), "utf8").split(/\r?\n/);
  const start = lines.findLastIndex((l) => l.includes("app started"));
  return lines.slice(start).filter((l) => /app started|scan started|scan finished|inventory read/.test(l));
}

const kind = (l) => (l.includes("app started") ? "app" : l.includes("scan started") ? "start" : l.includes("scan finished") ? "finish" : "read");

async function codexDisablePreview(t, { apply }) {
  const { s } = t;
  await inventoryOf(t, "Codex", "codex-home");
  await project(t, `${t.root}\\project`);
  await openItem(t, "cd-throwaway-codex");
  await press(s, "Disable", ".panel.detail");
  await waitPreview(s);
  if (apply) {
    await confirm(s, "Disable");
    await done(s);
  } else await press(s, "Cancel", ".dialog");
}

export default [
  {
    id: "14.1",
    note: "La app se cierra y se abre con sesiones nuevas de WebDriver, sin cambiar de ventana entre medias.",
    async run(t) {
      const { s } = t;
      await t.restart();
      await s.waitFor(".panel.items .scroll, .empty-inventory", { timeout: 20000 });
      await wait(1500);
      const first = scanLines(t).map(kind);
      t.eq(first.join(","), "app,start,finish,read", "la primera vez, la orden imprime «app started», «scan started», «scan finished elapsed_ms=<n> paths=<n>» e «inventory read»");
      t.check(scanLines(t).some((l) => /scan finished elapsed_ms=\d+ paths=\d+/.test(l)), "«scan finished» lleva «elapsed_ms=<n> paths=<n>»");
      await t.restart();
      await s.waitFor(".panel.items .scroll", { timeout: 20000 });
      await wait(1000);
      await codexDisablePreview(t, { apply: false });
      await wait(500);
      const second = scanLines(t).map(kind);
      t.eq(second.slice(0, 4).join(","), "app,start,finish,read", "la segunda vez empieza con esas cuatro líneas");
      const rest = second.slice(4);
      // A pair followed by "inventory read" is a re-read on focus; the
      // preview's pairs have none after them.
      const pairs = [];
      for (let i = 0; i < rest.length; i++) if (rest[i] === "start" && rest[i + 1] === "finish") pairs.push(rest[i + 2] === "read" ? "reread" : "preview");
      t.eq(pairs.filter((p) => p === "preview").length, 2, "después siguen exactamente dos parejas «scan started» / «scan finished» sin «inventory read» detrás: los dos escaneos de la preview de Disable", rest.join(","));
    },
  },
  {
    id: "14.2",
    note: "Con el CLI que va junto a la app, sobre tus homes reales y sin escribir: «list -v» hace el escaneo del arranque y «disable --dry-run -v» los dos de la preview, con CODEX_HOME en el home de Codex de prueba solo para esa orden.",
    async run(t) {
      // The step's goal is a full scan of the tester's own homes, which the
      // throwaway profile hides from the app. The CLI reads them as the app
      // does and logs the same lines; neither command writes.
      const cli = join(dirname(t.app), "config-digger.exe");
      const lines = (args, env = {}) =>
        spawnSync(cli, ["--color", "never", "-v", ...args], { encoding: "utf8", env: { ...process.env, ...env } })
          .stderr.split(/\r?\n/)
          .filter((l) => l.includes("scan finished"));
      const startup = lines(["list"]);
      const preview = lines(["--project", `${t.root}\\project`, "disable", "--dry-run", "cd-throwaway-codex"], { CODEX_HOME: `${t.root}\\codex-home` });
      const all = [...startup, ...preview];
      t.check(startup.length >= 1 && preview.length >= 2, "salen al menos tres líneas «scan finished elapsed_ms=<n> paths=<n>»: la del arranque y las dos de la preview del Disable", `${startup.length} + ${preview.length}`);
      const ms = all.map((l) => Number(/elapsed_ms=(\d+)/.exec(l)?.[1]));
      t.check(ms.length > 0 && ms.every((n) => n < 5000), "en cada una, «elapsed_ms» es menor que 5000", ms.join(", "));
    },
  },
  {
    id: "12.1",
    async run(t) {
      const { s } = t;
      await go(s, "Settings");
      const toggle = await s.waitFor("button[role=switch][aria-label='Check for updates on startup']");
      t.eq(await s.attr(toggle, "aria-checked"), "false", "el interruptor «Check for updates on startup» está apagado");
      const note = "Off by default. While it is off, config-digger makes no network call unless you click Check now in About.";
      t.check((await s.exec("return document.querySelector('.settings-updates').innerText")).includes(note), `junto al interruptor sale «${note}»`);
      await press(s, "Version and Check now are in About", ".settings-updates");
      await wait(400);
      const about = await s.exec("return document.querySelector('.about')?.innerText ?? ''");
      t.check(about.includes(`config-digger ${t.version}`), `About muestra la versión «${t.version}»`, about.split("\n").slice(0, 3).join(" | "));
      t.check(about.includes("Last checked: never."), "About dice «Last checked: never.»");
      await press(s, "Check now", ".about");
      const message = await s.until(
        () => s.exec(`const p = [...document.querySelectorAll(".about .side-note, .about .alert, .about [role=alert], .about [role=status]")].map((e) => e.textContent.trim()).find((x) => /update check failed|is the latest version|is available/i.test(x)); return p ?? null;`),
        { what: "the update check's answer", timeout: 30000 },
      );
      if (/-rc\.\d+$/.test(t.version)) {
        t.check(message.startsWith("The update check failed:"), "con una release candidate, Check now muestra un error que empieza por «The update check failed:» y sigue con la causa", message);
        t.check(message.split(/update check failed/i).length === 2, "esas palabras salen una sola vez", message);
      } else t.eq(message, `config-digger ${t.version} is the latest version.`, `con la versión final, Check now dice «config-digger ${t.version} is the latest version.»`);
      await t.shot("updates", "el mensaje del updater en About");
    },
  },
  {
    id: "13.1",
    note: "Con Claude Desktop abierto, el caso que el paso pide que no lo cambie.",
    async run(t) {
      const { s } = t;
      await withClaudeDesktop(t, async () => {
        await inventoryOf(t, "Claude Desktop", "claude-desktop-home");
        const row = await s.until(
          () =>
            s
              .exec(`return [...document.querySelectorAll(".item-row")].find((r) => r.querySelector(".item-name")?.textContent.trim() === "cd-throwaway-desktop" && r.querySelector(".state-pill")?.firstChild?.textContent.trim() === "Effective") ?? null;`)
              .then(unwrap),
          { what: "cd-throwaway-desktop Effective" },
        );
        await s.click(row);
        await wait(400);
        await press(s, "Disable", ".panel.detail");
        const d = await waitPreview(s);
        t.check(!/Claude Desktop is running/.test(d.text), "el diálogo no tiene ninguna frase que empiece por «Claude Desktop is running»");
        const result = await confirm(s, "Disable");
        t.check(/^Disabled 1 Entry\. Operation \S+: verified\.$/m.test(result.text), "el resultado dice «Disabled 1 Entry. Operation <id>: verified.»");
        await done(s);
        await s.until(() => s.exec("return !document.querySelector('.notice.busy, .read-progress')"), { what: "the re-read" });
        const states = await s.exec(`return [...document.querySelectorAll(".item-row")].filter((r) => r.querySelector(".item-name")?.textContent.trim() === "cd-throwaway-desktop").map((r) => r.querySelector(".state-pill").firstChild.textContent.trim());`);
        t.check(states.length > 0 && states.every((x) => x === "Disabled"), "tras Done, cd-throwaway-desktop sale como «Disabled», porque está aparcada", states.join(", "));
      });
    },
  },
  {
    id: "13.5",
    async run(t) {
      await t.s.close().catch(() => {});
      t.ps(t.step.commands[0].text.replace("E:\\cd-throwaway", t.root));
      t.eq(t.ps(t.step.commands[1].text.replace("E:\\cd-throwaway", t.root)), "False", "la segunda orden imprime «False»");
      await t.restart();
    },
  },
];
