// Tests of the WebDriver walk's parts that run without the app: the steps
// it knows match the plan, the throwaway profile, opening a group on the
// Projects tab, confirming an action dialog up to its result, reading an
// Operation id, step 11.11's command that marks an Operation, and the walk's
// CI mode: no screenshot, the steps a runner can't do, uat-results.json and
// the job summary.
// Run them with `node --test uat/auto/webdriver/walk.test.mjs`.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { profileEnv, ref, Session } from "./driver.mjs";
import { setupSwitches } from "./lib.mjs";
import { jobSummary, resultDocument, walkStep } from "./step.mjs";
import { confirm, operationId } from "./steps/common.mjs";
import { openGroup } from "./steps/places.mjs";
import sections from "./steps/index.mjs";

const plan = JSON.parse(readFileSync(new URL("../../plan.json", import.meta.url), "utf8"));
const planIds = new Set(plan.sections.flatMap((section) => section.steps.map((step) => step.id)));
const steps = sections.flat();

test("every step the walk knows is a step of the plan, once", () => {
  const ids = steps.map((step) => step.id);
  assert.deepEqual(
    ids.filter((id) => !planIds.has(id)),
    [],
  );
  assert.equal(new Set(ids).size, ids.length);
});

test("every step of a section the walk covers is walked, but those the Sandbox walks", () => {
  const sandbox = new Set(["12.2", "13.2", "13.3", "13.4"]);
  const walked = new Set(steps.map((step) => step.id));
  const covered = plan.sections.filter((section) => section.steps.some((step) => walked.has(step.id)));
  assert.deepEqual(
    covered.flatMap((section) => section.steps.map((step) => step.id)).filter((id) => !walked.has(id) && !sandbox.has(id)),
    [],
  );
});

test("step 1.1 fails when the setup script does not list the home whose Project servers are a list", async () => {
  const root = "E:\\cd-throwaway";
  const listed = ["claude-code-home\\.claude.json", "codex-home\\config.toml", "claude-desktop-home\\claude_desktop_config.json"];
  const walk = async (files) => {
    const failed = [];
    const has = (text, part, what) => (text.includes(part) ? true : (failed.push(what), false));
    const out = [...files.map((file) => `${root}\\${file}`), `Created the throwaway homes and Projects in ${root}`].join("\n");
    await steps.find((step) => step.id === "1.1").run({ root, setup: () => out, has, check: (ok, what) => ok || failed.push(what) });
    return failed;
  };
  assert.deepEqual(await walk([...listed, "claude-code-shape-home\\.claude.json"]), []);
  assert.deepEqual(await walk(listed), ["lista «claude-code-shape-home\\.claude.json»"]);
});

test("a step's setup runs the data script with the switches the plan adds to -Reset", () => {
  assert.equal(setupSwitches(String.raw`.\uat\data\cd-throwaway-setup.ps1 -Reset`), "");
  assert.equal(setupSwitches(String.raw`.\uat\data\cd-throwaway-setup.ps1 -Reset -Symlink`), " -Symlink");
  assert.equal(setupSwitches(String.raw`.\uat\data\cd-throwaway-setup.ps1 -Root D:\x\cd-throwaway -Reset -Symlink`), " -Symlink");
});

test("a step that leaves something to a person says what", () => {
  for (const step of steps.filter((s) => "manual" in s)) {
    assert.ok(typeof step.manual === "string" && step.manual.length > 20, step.id);
  }
});

test("the profile must be a folder named cd-throwaway-profile", () => {
  assert.throws(() => profileEnv(join(tmpdir(), "my-profile")), /cd-throwaway-profile/);
});

test("the profile holds both AppData folders, the known folders and a Claude Code home", () => {
  const root = mkdtempSync(join(tmpdir(), "uat-walk-"));
  try {
    const profile = join(root, "cd-throwaway-profile");
    const env = profileEnv(profile);
    assert.equal(env.USERPROFILE, profile);
    assert.equal(env.APPDATA, join(profile, "AppData", "Roaming"));
    assert.equal(env.LOCALAPPDATA, join(profile, "AppData", "Local"));
    for (const folder of [env.APPDATA, env.LOCALAPPDATA, join(profile, "Desktop"), join(profile, "Documents"), join(profile, "Downloads")]) {
      assert.ok(existsSync(folder), folder);
    }
    assert.equal(readFileSync(join(profile, ".claude.json"), "utf8"), "{}\n");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

// Runs a script of the walk on a fake page's `document`, with its `args` as
// `arguments`.
function runScript(document, { script, args }) {
  return new Function("document", "args", `return (function () {${script}}).apply(null, args);`)(document, args);
}

// A session whose page is only the group toggles of a Projects tab, each
// `{ text, open }`: a script runs on them, and a click opens or closes one.
class GroupsPage extends Session {
  constructor(groups) {
    super("http://fake", "fake");
    this.toggles = groups.map(({ text, open }, i) => ({ id: `toggle-${i}`, textContent: text, open }));
  }

  async cmd(method, path, body) {
    if (path === "/execute/sync") {
      const document = { querySelectorAll: (css) => (css === ".place-group-toggle" ? this.toggles : []) };
      const value = runScript(document, body);
      return this.toggles.includes(value) ? ref(value.id) : value;
    }
    const [, id, action] = /^\/element\/([^/]+)\/(.+)$/.exec(path);
    const toggle = this.toggles.find((t) => t.id === id);
    if (action === "click") toggle.open = !toggle.open;
    else if (action === "attribute/aria-expanded") return String(toggle.open);
    else throw new Error(`${method} ${path}`);
    return null;
  }
}

test("a closed group on the Projects tab opens, and an open one stays open", async () => {
  const page = new GroupsPage([
    { text: "▾ Missing · 1", open: true },
    { text: "▸ Missing, no leftovers · 1", open: false },
    { text: "▸ Hidden · 1", open: false },
  ]);
  await openGroup(page, "Hidden");
  await openGroup(page, "Missing");
  assert.deepEqual(
    page.toggles.map((t) => t.open),
    [true, false, true],
  );
});

test("opening a group waits for the tab to read its list", async () => {
  const page = new GroupsPage([]);
  setTimeout(() => page.toggles.push({ id: "toggle-0", textContent: "▸ Hidden · 1", open: false }), 300);
  await openGroup(page, "Hidden");
  assert.equal(page.toggles[0].open, true);
});

test("opening a group fails when no group has that title", async () => {
  const page = new GroupsPage([{ text: "▸ Missing, no leftovers · 1", open: false }]);
  await assert.rejects(openGroup(page, "Hidden", { timeout: 0 }), /group "Hidden"/);
  await assert.rejects(openGroup(page, "Missing", { timeout: 0 }), /group "Missing"/);
  assert.equal(page.toggles[0].open, false);
});

// A session whose page is only an action dialog, each of its states
// `{ title, text, foot }` with the foot's button texts. Clicking a foot
// button whose text starts with `verb` lands `result` right after the page's
// next script: a read of the dialog that runs as two scripts sees the preview
// in the first and the result's foot in the second.
class DialogPage extends Session {
  constructor(preview, verb, result) {
    super("http://fake", "fake");
    this.state = preview;
    this.verb = verb;
    this.result = result;
    this.landing = false;
  }

  buttons() {
    return this.state.foot.map((text, i) => ({ id: `button-${i}`, textContent: text, disabled: false }));
  }

  async cmd(method, path, body) {
    if (path === "/execute/sync") {
      const page = this;
      const buttons = this.buttons();
      const dialog = {
        get innerText() {
          return page.state.text;
        },
        querySelector: (css) => (css === ".dialog-title" ? { textContent: page.state.title } : null),
        querySelectorAll: (css) => (css === ".dialog-foot button" ? buttons : []),
      };
      const document = {
        querySelector: (css) => (css === ".dialog" ? dialog : null),
        querySelectorAll: (css) => (css === ".dialog .dialog-foot button" ? buttons : []),
      };
      const value = runScript(document, body);
      if (this.landing) {
        this.state = this.result;
        this.landing = false;
      }
      return buttons.includes(value) ? ref(value.id) : value;
    }
    const [, id, action] = /^\/element\/([^/]+)\/(.+)$/.exec(path);
    if (action !== "click") throw new Error(`${method} ${path}`);
    const button = this.buttons().find((b) => b.id === id);
    if (button.textContent.startsWith(this.verb)) this.landing = true;
    return null;
  }
}

test("confirming an action dialog returns its result, though the result lands while the walk reads the dialog", async () => {
  const page = new DialogPage(
    { title: "Delete backup", text: 'Delete the backup of 20261024-161841-38c "Disable 1 Entry"', foot: ["Cancel", "Delete backup"] },
    "Delete backup",
    { title: "Delete backup", text: "Operation 20261024-161841-38c · backup deleted · can no longer be undone", foot: ["Done"] },
  );
  const d = await confirm(page, "Delete backup");
  assert.equal(d.text, "Operation 20261024-161841-38c · backup deleted · can no longer be undone");
});

// A LOCALAPPDATA whose config-digger backups hold one Operation folder per
// `{ id, title, status }`, each with its manifest as the core writes it.
function localAppDataWith(operations) {
  const local = mkdtempSync(join(tmpdir(), "uat-backups-"));
  for (const { id, title, status } of operations) {
    const folder = join(local, "config-digger", "backups", id);
    mkdirSync(folder, { recursive: true });
    writeFileSync(join(folder, "manifest.json"), JSON.stringify({ manifest_version: 1, id, sequence: 1, title, undo_of: null, status, cause: null }, null, 2));
  }
  return local;
}

const manifestOf = (local, id) => readFileSync(join(local, "config-digger", "backups", id, "manifest.json"), "utf8");

// Runs step 11.11's command as the tester pastes it, on that LOCALAPPDATA.
function markNewest(local) {
  const step = plan.sections.flatMap((section) => section.steps).find((s) => s.id === "11.11");
  return execFileSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", step.commands[0].text], {
    env: { ...process.env, LOCALAPPDATA: local },
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
}

test("step 11.11's command marks the newest Operation by its id, the Enable of step 11.10, as needing attention", () => {
  const local = localAppDataWith([
    { id: "20261026-102000-c3d", title: "Enable 1 Entry", status: "applied" },
    { id: "20261026-101500-a1b", title: "Disable 1 Entry", status: "applied" },
    { id: "20261026-091500-fff", title: "Enable 1 Entry", status: "applied" },
  ]);
  try {
    assert.equal(markNewest(local), "20261026-102000-c3d");
    const marked = manifestOf(local, "20261026-102000-c3d");
    assert.equal(JSON.parse(marked).status, "needs_attention");
    assert.ok(marked.startsWith("{"), "no byte order mark");
    assert.equal(JSON.parse(manifestOf(local, "20261026-101500-a1b")).status, "applied");
    assert.equal(JSON.parse(manifestOf(local, "20261026-091500-fff")).status, "applied");
  } finally {
    rmSync(local, { recursive: true, force: true });
  }
});

test("step 11.11's command changes nothing when the newest Operation is not an applied Enable", () => {
  for (const newest of [
    { id: "20261026-102000-c3d", title: "Disable 1 Entry", status: "applied" },
    { id: "20261026-102000-c3d", title: "Enable 1 Entry", status: "needs_attention" },
  ]) {
    const local = localAppDataWith([newest, { id: "20261026-101500-a1b", title: "Enable 1 Entry", status: "applied" }]);
    try {
      const before = manifestOf(local, newest.id);
      assert.throws(() => markNewest(local), /is not the applied Enable 1 Entry of step 11\.10/);
      assert.equal(manifestOf(local, newest.id), before);
      assert.equal(JSON.parse(manifestOf(local, "20261026-101500-a1b")).status, "applied");
    } finally {
      rmSync(local, { recursive: true, force: true });
    }
  }
});

// A session whose page is an app window that is not blank: every script
// answers null. It keeps each command the walk sends, and writes a picture
// for a screenshot, as the driver does.
class QuietPage extends Session {
  constructor() {
    super("http://fake", "fake");
    this.calls = [];
  }

  async cmd(method, path) {
    this.calls.push(`${method} ${path}`);
    return path === "/screenshot" ? Buffer.from("png").toString("base64") : null;
  }
}

const screenshots = (calls) => calls.filter((c) => c.endsWith("/screenshot"));

// Walks the step module `runner` in CI mode, with `ctx` added to the
// walk's, and a folder for screenshots that the walk must leave empty.
async function walkOnCi(runner, ctx = {}) {
  const shots = mkdtempSync(join(tmpdir(), "uat-shots-"));
  const s = new QuietPage();
  try {
    const result = await walkStep({ s, version: "0.1.0", ...ctx }, { id: runner.id }, runner, { shots, ci: { evidence: "https://github.com/o/r/actions/runs/1" } });
    return { result, calls: s.calls, files: readdirSync(shots) };
  } finally {
    rmSync(shots, { recursive: true, force: true });
  }
}

test("on CI, a step with looks takes no screenshot and is OK on its checks, its looks left to the tester", async () => {
  const { result, calls, files } = await walkOnCi({
    id: "4.1",
    async run(t) {
      t.check(true, "sale el tooltip");
      await t.shot("tooltip", "su sombra", "su color");
    },
  });
  assert.deepEqual(screenshots(calls), []);
  assert.deepEqual(files, []);
  assert.equal(result.status, "ok");
  assert.match(result.notes, /Sin juzgar en CI: su sombra; su color/);
  assert.doesNotMatch(result.notes, /\.png/);
});

test("on CI, a step that throws before its checks takes no screenshot and is Blocked with the error", async () => {
  const { result, calls, files } = await walkOnCi({
    id: "3.1",
    async run() {
      throw new Error("timed out waiting for the Projects tab");
    },
  });
  assert.deepEqual(screenshots(calls), []);
  assert.deepEqual(files, []);
  assert.equal(result.status, "blocked");
  assert.equal(result.notes.split("\n")[0], "Bloqueado: antes de sus comprobaciones: timed out waiting for the Projects tab");
});

test("on CI, a failed check makes the step KO with its message, and a throw after it takes no screenshot", async () => {
  const { result, calls } = await walkOnCi({
    id: "3.2",
    async run(t) {
      t.eq("2 Projects", "3 Projects", "sale «3 Projects»");
      throw new Error("no button Remove");
    },
  });
  assert.deepEqual(screenshots(calls), []);
  assert.equal(result.status, "ko");
  assert.equal(result.notes.split("\n")[0], "KO: sale «3 Projects»: «2 Projects», no «3 Projects»");
});

test("on CI, a step that needs what a runner lacks is Blocked, naming it, without running", async () => {
  let ran = false;
  const { result } = await walkOnCi({
    id: "14.2",
    ciLacks: "los homes de un tester",
    async run() {
      ran = true;
    },
  });
  assert.equal(ran, false);
  assert.equal(result.status, "blocked");
  assert.equal(result.notes.split("\n")[0], "Bloqueado: el runner de CI no tiene los homes de un tester");
});

test("on a runner without Claude Desktop, 7.2, 7.6 and 13.1 are Blocked naming it, and 14.2 naming the tester's homes", async () => {
  const reasons = {};
  for (const id of ["7.2", "7.6", "13.1", "14.2"]) {
    // A runner's PowerShell finds no Claude Desktop: no Start menu app, no claude.exe.
    const { result } = await walkOnCi(steps.find((step) => step.id === id), { root: "E:\\cd-throwaway", ps: () => "" });
    assert.equal(result.status, "blocked", id);
    reasons[id] = result.notes.split("\n")[0];
  }
  assert.deepEqual(reasons, {
    "7.2": "Bloqueado: Claude Desktop no está instalado en este PC",
    "7.6": "Bloqueado: Claude Desktop no está instalado en este PC",
    "13.1": "Bloqueado: Claude Desktop no está instalado en este PC",
    "14.2": "Bloqueado: el runner de CI no tiene los homes reales de un tester, que el paso mide",
  });
});

test("a step's CI result carries by ci and the run's URL", async () => {
  const { result } = await walkOnCi({
    id: "4.1",
    async run(t) {
      t.check(true, "sale");
    },
  });
  assert.equal(result.by, "ci");
  assert.equal(result.evidence, "https://github.com/o/r/actions/runs/1");
});

test("a document of uat-results.json has the runner's fields, by and evidence, and nothing else", async () => {
  const { result } = await walkOnCi({
    id: "5.1",
    manual: "Que el aviso entre y se vaya sin saltos.",
    async run(t) {
      t.check(true, "sale el aviso");
      await t.shot("notice", "su color");
    },
  });
  const doc = resultDocument(result);
  assert.deepEqual(Object.keys(doc).sort(), ["by", "evidence", "id", "notes", "status", "time", "version"]);
  assert.equal(doc.version, "0.1.0");
  assert.match(doc.notes, /Queda para una persona: Que el aviso entre/);
});

test("the job summary has a line per step, with the first line of the reason of a KO or a Blocked", () => {
  const summary = jobSummary([
    { id: "1.1", status: "ok", notes: "Comprobado (2/2): a; b" },
    { id: "3.2", status: "ko", notes: "KO: sale «3 Projects»: «2 Projects», no «3 Projects»\nComprobado (0/1): " },
    { id: "7.2", status: "blocked", notes: "Bloqueado: Claude Desktop no está instalado en este PC\nComprobado (0/0): " },
  ]);
  assert.equal(
    summary,
    [
      "| Paso | Veredicto | Motivo |",
      "| --- | --- | --- |",
      "| 1.1 | ok | |",
      "| 3.2 | ko | KO: sale «3 Projects»: «2 Projects», no «3 Projects» |",
      "| 7.2 | blocked | Bloqueado: Claude Desktop no está instalado en este PC |",
      "",
    ].join("\n"),
  );
});

test("a reason with a pipe keeps the summary's table whole", () => {
  assert.match(jobSummary([{ id: "6.1", status: "ko", notes: "KO: sale «a | b»" }]), /\| 6\.1 \| ko \| KO: sale «a \\\| b» \|/);
});

test("an Operation id is read from the lines that name one", () => {
  assert.equal(operationId("Disabled 1 Entry. Operation 20261023-172530-c63: verified."), "20261023-172530-c63");
  assert.equal(operationId("Operation 20261023-172530-c63 · backup kept"), "20261023-172530-c63");
  assert.equal(operationId("Nothing of this Operation was kept\nOperation 20261023-172614-84d"), "20261023-172614-84d");
  assert.equal(operationId("Nothing of this Operation was kept"), undefined);
});
