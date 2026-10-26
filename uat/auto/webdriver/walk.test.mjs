// Tests of the WebDriver walk's parts that run without the app: the steps
// it knows match the plan, the throwaway profile, opening a group on the
// Projects tab, confirming an action dialog up to its result, reading an
// Operation id, and step 11.11's command that marks an Operation.
// Run them with `node --test uat/auto/webdriver/walk.test.mjs`.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { profileEnv, ref, Session } from "./driver.mjs";
import { setupSwitches } from "./lib.mjs";
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

test("an Operation id is read from the lines that name one", () => {
  assert.equal(operationId("Disabled 1 Entry. Operation 20261023-172530-c63: verified."), "20261023-172530-c63");
  assert.equal(operationId("Operation 20261023-172530-c63 · backup kept"), "20261023-172530-c63");
  assert.equal(operationId("Nothing of this Operation was kept\nOperation 20261023-172614-84d"), "20261023-172614-84d");
  assert.equal(operationId("Nothing of this Operation was kept"), undefined);
});
