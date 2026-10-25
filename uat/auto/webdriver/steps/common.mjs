// Moves that several steps share: adding an Agent home, opening a home's
// Inventory, opening an Item, running an action through its dialog.
import { ref, unwrap } from "../driver.mjs";
import { choose, go, press, tab, wait, waitNotice } from "../lib.mjs";

// Add Agent home with `agent` and `folder` typed in, as 1.2, 10.2 and 10.3 do,
// and the checks they share: the notice, the row under its Agent's group, its
// tags, and no row under another group.
export async function addHome(t, agent, folder, { checks = true } = {}) {
  const { s } = t;
  await go(s, "Projects & homes");
  await tab(s, "Agent homes");
  await press(s, "Add Agent home");
  await choose(s, "Agent", agent);
  await pickFolder(t, "Choose", "input[aria-label=Folder]", folder);
  await press(s, "Add", ".dialog");
  const notice = await waitNotice(s, `Added ${folder}`);
  await s.until(() => s.exec("return !document.querySelector('.dialog')"), { what: "the Add dialog closed" });
  if (!checks) return;
  t.eq(notice.text, `Added ${folder} as an Agent home of ${agent}.`, `sale el aviso «Added ${folder} as an Agent home of ${agent}.»`);
  const groups = await homeGroups(s);
  const mine = groups.find((g) => g.agent === agent);
  const row = mine?.rows.find((r) => r.name === folder);
  t.check(row, `la fila de «${folder}» está bajo el grupo «${agent}»`);
  t.check(row && row.tags.join("|") === `${agent}|added by you`, `la fila tiene las etiquetas «${agent}» y «added by you»`, row?.tags.join(", "));
  if (agent !== "Claude Code") {
    const elsewhere = groups.filter((g) => g.agent === "Claude Code").flatMap((g) => g.rows).some((r) => r.name === folder);
    t.check(!elsewhere, `no sale ninguna fila de ${folder.split("\\").at(-1)} bajo «Claude Code»`);
  }
}

// Presses the dialog's picker button and answers the Windows folder picker
// with `folder`, then waits for the field to show it and the core to check it.
export async function pickFolder(t, button, field, folder) {
  const { s } = t;
  await press(s, button, ".dialog");
  t.pick(folder);
  await s.until(async () => (await s.exec("return document.querySelector(arguments[0])?.value", field)) === folder, {
    what: `${folder} in the dialog's field`,
  });
  await wait(700);
}

// The Agent homes tab, as groups of rows: each group's Agent and, for each
// row, its path and tags.
export function homeGroups(s) {
  return s.exec(`
    const groups = [];
    for (const el of document.querySelectorAll(".places .scroll > *")) {
      if (el.matches("h3.agent-group")) groups.push({ agent: el.firstChild.textContent.trim(), rows: [] });
      else if (el.matches(".place-row") && groups.length) groups.at(-1).rows.push({
        name: el.querySelector(".place-name")?.textContent.trim(),
        tags: [...el.querySelectorAll(".tag")].map((t) => t.textContent.trim()),
        busy: el.getAttribute("aria-busy"),
      });
    }
    return groups;`);
}

// Opens the Inventory on `agent` and, when it has more than one, the home
// whose path ends with `home`. Returns the Items list's text.
export async function inventoryOf(t, agent, home) {
  const { s } = t;
  await go(s, "Inventory");
  await s.waitFor(".panel.items .scroll");
  const tile = await s.waitText(".agent-tile .agent-name", agent);
  await s.click(tile);
  await wait(400);
  // With one Agent chosen, its homes are tiles; with All Agents, each Agent's
  // row has a dropdown once it has two homes.
  const homeTile = home
    ? await s
        .exec(`return [...document.querySelectorAll(".home-tile")].find((b) => b.querySelector(".home-name")?.textContent.trim().endsWith(arguments[0])) ?? null;`, home)
        .then(unwrap)
    : null;
  if (homeTile) {
    if ((await s.attr(homeTile, "aria-pressed")) !== "true") {
      await s.click(homeTile);
      await wait(500);
    }
  } else if (home) {
    const picker = await s
      .exec(`return document.querySelector(".home-row [role=combobox]") ?? null;`)
      .then(unwrap);
    if (picker) {
      const value = await s.text(picker);
      if (!value.endsWith(home)) {
        await s.click(picker);
        const opt = await s.until(
          () =>
            s
              .exec(`return [...document.querySelectorAll("[role=option]")].find((o) => o.textContent.trim().endsWith(arguments[0])) ?? null;`, home)
              .then(unwrap),
          { what: `the home ${home}` },
        );
        await s.click(opt);
        await wait(600);
      }
    }
  }
  await s.until(() => s.exec("return !document.querySelector('.loading')"), { what: "the Inventory read" });
  return s.exec("return document.querySelector('.panel.items').innerText");
}

export async function project(t, path) {
  await choose(t.s, "Project", path);
  await t.s.until(() => t.s.exec("return !document.querySelector('.loading')"), { what: "the Project read" });
  await wait(300);
}

// Picks the Item named `name` in the Items list and returns the Details text.
export async function openItem(t, name) {
  const { s } = t;
  const row = await s.until(
    () =>
      s
        .exec(`return [...document.querySelectorAll(".item-row")].find((r) => r.querySelector(".item-name")?.textContent.trim() === arguments[0]) ?? null;`, name)
        .then(unwrap),
    { what: `the Item ${name}` },
  );
  await s.exec("arguments[0].scrollIntoView({block: 'nearest'})", ref(row));
  await s.click(row);
  await wait(400);
  return s.exec("return document.querySelector('.panel.detail').innerText");
}

// The open action dialog: its title, its text, whether it is still
// preparing the preview, and its foot's buttons, all read at once.
export function dialog(s) {
  return s.exec(`
    const d = document.querySelector(".dialog");
    if (!d) return null;
    return {
      title: d.querySelector(".dialog-title")?.textContent.trim(),
      text: d.innerText,
      loading: !!d.querySelector(".loading"),
      foot: [...d.querySelectorAll(".dialog-foot button")].map((b) => b.textContent.trim()),
    };`);
}

export async function waitPreview(s) {
  return s.until(async () => {
    const d = await dialog(s);
    return d && !d.loading && !d.title?.startsWith("Preparing") ? d : null;
  }, { what: "the preview", timeout: 30000 });
}

// The parts of an action dialog that steps check: What changes, Before you
// apply, Files, the backup note of the foot, and every path the checks and
// warnings name.
export function dialogParts(s) {
  return s.exec(`
    const d = document.querySelector(".dialog");
    const cs = (e) => e && { color: getComputedStyle(e).color, bg: getComputedStyle(e).backgroundColor, edge: getComputedStyle(e).borderLeftWidth + " " + getComputedStyle(e).borderLeftStyle + " " + getComputedStyle(e).borderLeftColor, size: getComputedStyle(e).fontSize, weight: getComputedStyle(e).fontWeight, family: getComputedStyle(e).fontFamily };
    const section = (title) => [...d.querySelectorAll(".dialog-section")].find((e) => e.querySelector(".dialog-section-title")?.firstChild?.textContent.trim().startsWith(title));
    const files = section("Files");
    const before = section("Before you apply");
    const changes = section("What changes");
    const foot = d.querySelector(".dialog-backup");
    return {
      title: d.querySelector(".dialog-title")?.textContent.trim(),
      changes: changes && { text: changes.querySelector(".dialog-section-body")?.innerText ?? "" },
      files: files && {
        title: files.querySelector(".dialog-section-title").firstChild.textContent.trim(),
        paths: [...files.querySelectorAll(".file-path")].map((e) => e.textContent.trim()),
        removed: [...files.querySelectorAll(".diff-removed")].map((e) => ({ text: e.textContent, ...cs(e) })),
        added: [...files.querySelectorAll(".diff-added")].map((e) => ({ text: e.textContent, ...cs(e) })),
        text: files.innerText,
      },
      before: [...(before?.querySelectorAll(".check") ?? [])].map((e) => ({
        cls: e.className,
        text: e.innerText,
        ...cs(e),
        icon: !!e.querySelector(":scope > svg.icon, :scope > .icon"),
        iconNextToBox: (() => { const i = e.querySelector(":scope > svg.icon, :scope > .icon"), b = e.querySelector(":scope > input"); return !!(i && b && Math.abs(i.getBoundingClientRect().top - b.getBoundingClientRect().top) < 12); })(),
        box: !!e.querySelector("input[type=checkbox]"),
        todo: e.querySelector(".check-todo") && { text: e.querySelector(".check-todo").innerText, weight: getComputedStyle(e.querySelector(".check-todo")).fontWeight },
        paths: [...e.querySelectorAll(".check-path")].map((p) => ({ text: p.textContent.trim(), display: getComputedStyle(p).display, family: getComputedStyle(p).fontFamily })),
        inlinePaths: [...e.querySelectorAll(".check-why, .check-todo")].some((x) => /[A-Za-z]:\\\\/.test(x.textContent)),
      })),
      foot: foot && { text: foot.textContent.trim(), ...cs(foot) },
      textSize: getComputedStyle(before?.querySelector(".check") ?? d.querySelector(".dialog-subtitle") ?? d).fontSize,
      paths: [...(before?.querySelectorAll(".check-path, .mono") ?? [])].map((e) => e.textContent.trim()).filter((p) => /^[A-Za-z]:[\\\\/]/.test(p)),
    };`);
}

// The looks of Before you apply that a style can show, as 10.7 lists them: a
// warning is its own amber block with an amber edge, its icon by its box, its
// first line bold and each path on a line of its own in monospace; a note is a
// blue block with a light edge and the information icon.
export function checkBeforeYouApply(t, checks) {
  const hue = (c) => {
    const [r, g, b] = (c.match(/\d+/g) ?? []).map(Number);
    return { r, g, b };
  };
  for (const w of checks.filter((c) => c.cls.includes("warning"))) {
    const bg = hue(w.bg);
    const edge = hue(w.edge.split(" ").slice(2).join(" "));
    t.check(bg.r >= bg.b && edge.r > edge.b && edge.g > edge.b && w.edge.startsWith("4px solid"), "el aviso es un bloque propio, en ámbar, con un borde ámbar a la izquierda", `${w.bg}; ${w.edge}`);
    t.check(w.icon && w.iconNextToBox, "el icono del aviso va junto a su casilla");
    t.check(w.todo && Number(w.todo.weight) >= 600, "su primera línea, en negrita, dice qué hacer", w.todo?.text);
    t.check(!w.inlinePaths && w.paths.every((p) => p.display === "block" && /mono|consol|courier/i.test(p.family)), "cada ruta va sola en su línea, en monospace", JSON.stringify(w.paths));
  }
  for (const n of checks.filter((c) => c.cls.includes("info"))) {
    const bg = hue(n.bg);
    t.check(bg.b > bg.r && n.edge.startsWith("4px solid"), "la nota va en un bloque azul con borde a la izquierda", `${n.bg}; ${n.edge}`);
    t.check(n.icon, "la nota lleva el icono de información, no texto gris");
  }
}

// Ticks every warning box of Before you apply, as the steps ask.
export function tickAll(s) {
  return s.exec(`
    for (const box of document.querySelectorAll(".dialog input[type=checkbox]")) if (!box.checked) box.click();`);
}

// Presses the dialog's confirm button, whose text starts with `verb`, and
// waits for the result. Returns the dialog then.
export async function confirm(s, verb) {
  await tickAll(s);
  const button = await s.until(
    () =>
      s
        .exec(
          `return [...document.querySelectorAll(".dialog .dialog-foot button")].find((b) => b.textContent.trim().toLowerCase().startsWith(arguments[0].toLowerCase()) && !b.disabled) ?? null;`,
          verb,
        )
        .then(unwrap),
    { what: `the dialog's ${verb} button` },
  );
  await s.click(button);
  // The result is in when the foot offers Done or Close. Its other buttons
  // may repeat the verb: an Undo's result offers Undo again. The text and the
  // foot come from one read, or a result landing between two reads would
  // return the preview's text.
  return s.until(
    async () => {
      const d = await dialog(s);
      return d?.foot.some((b) => b === "Done" || b === "Close") ? d : null;
    },
    { what: `the result of ${verb}`, timeout: 30000 },
  );
}

// Disable (or another verb) of the Entry card in Details, through its
// dialog, up to the result. Returns the result dialog.
export async function act(t, verb) {
  const { s } = t;
  await press(s, verb, ".panel.detail");
  await waitPreview(s);
  return confirm(s, verb);
}

export async function done(s) {
  await press(s, "Done", ".dialog");
  await s.until(() => s.exec("return !document.querySelector('.dialog')"), { what: "the dialog closed" });
}

// The Operation id in a result's grey line "Operation <id> · …".
export function operationId(text) {
  return /\b(\d{8}-\d{6}-[0-9a-f]{3,})\b/.exec(text ?? "")?.[1];
}

// Runs `body` with Claude Desktop open, as 7.2 and 13.1 ask, opening it when
// it is closed and closing it again after. Claude Desktop keeps the tester's
// own configuration: config-digger runs on the throwaway profile, so nothing
// of it changes. Returns false, with the step Blocked, when it isn't installed.
export async function withClaudeDesktop(t, body) {
  const app = t.ps("@(Get-StartApps | Where-Object { $_.Name -eq 'Claude' } | Select-Object -First 1 -ExpandProperty AppID)");
  if (!app) {
    t.blocked = "Claude Desktop no está instalado en este PC";
    return false;
  }
  // Claude Code's CLI is also claude.exe: only Claude Desktop's own process counts.
  const running = () =>
    Number(t.ps("@(Get-Process claude -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 -and $_.Path -notlike '*\\.local\\bin\\*' }).Count"));
  const before = running();
  if (!before) {
    t.ps(`Start-Process 'shell:AppsFolder\\${app}'`);
    await t.s.until(async () => running() > 0, { what: "Claude Desktop open", timeout: 30000 });
    await wait(3000);
  }
  try {
    t.check(running() > 0, "Claude Desktop está abierto");
    await body();
  } finally {
    if (!before)
      t.ps("Get-Process claude -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 -and $_.Path -notlike '*\\.local\\bin\\*' } | ForEach-Object { [void]$_.CloseMainWindow() }");
  }
  return true;
}

// Runs `body(close)` while a Claude Desktop started with
// `--user-data-dir=<folder>` runs, as the step's command starts it; `close`
// stops that one only, every process of which names `folder` in the switch,
// and the walk stops it after `body` if it is still running. Without Claude
// Desktop installed, the step is Blocked.
export async function withClaudeDesktopIn(t, folder, body) {
  // The walk's own LOCALAPPDATA is the tester's, where the classic app is;
  // its PowerShell gets the throwaway profile's.
  const exe = t.ps(
    `@(Get-ChildItem '${process.env.LOCALAPPDATA}\\AnthropicClaude\\app-*\\claude.exe' -ErrorAction SilentlyContinue | Sort-Object FullName | Select-Object -Last 1 -ExpandProperty FullName) + @(Get-AppxPackage | Where-Object { $_.Name -in 'Claude', 'AnthropicPBC.Claude' } | ForEach-Object { Join-Path $_.InstallLocation 'app\\claude.exe' }) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1`,
  );
  if (!exe) {
    t.blocked = "Claude Desktop no está instalado en este PC";
    return false;
  }
  const naming = `Get-CimInstance Win32_Process -Filter "Name='claude.exe'" | Where-Object { $_.CommandLine -like '*--user-data-dir=*${folder}*' }`;
  const running = () => Number(t.ps(`@(${naming}).Count`));
  const close = async () => {
    t.ps(`${naming} | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }`);
    await t.s.until(async () => running() === 0, { what: "that Claude Desktop closed", timeout: 30000 });
  };
  t.ps(`Start-Process '${exe}' -ArgumentList '--user-data-dir=${folder}'`);
  await t.s.until(async () => running() > 0, { what: "Claude Desktop open on the home", timeout: 30000 });
  await wait(3000);
  try {
    await body(close);
  } finally {
    if (running()) await close();
  }
  return true;
}
