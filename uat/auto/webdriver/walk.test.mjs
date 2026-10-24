// Tests of the WebDriver walk's parts that run without the app: the steps
// it knows match the plan, the throwaway profile, opening a group on the
// Projects tab, and reading an Operation id.
// Run them with `node --test uat/auto/webdriver/walk.test.mjs`.
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { profileEnv, ref, Session } from "./driver.mjs";
import { operationId } from "./steps/common.mjs";
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
      const value = new Function("document", "args", `return (function () {${body.script}}).apply(null, args);`)(document, body.args);
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

test("an Operation id is read from the lines that name one", () => {
  assert.equal(operationId("Disabled 1 Entry. Operation 20261023-172530-c63: verified."), "20261023-172530-c63");
  assert.equal(operationId("Operation 20261023-172530-c63 · backup kept"), "20261023-172530-c63");
  assert.equal(operationId("Nothing of this Operation was kept\nOperation 20261023-172614-84d"), "20261023-172614-84d");
  assert.equal(operationId("Nothing of this Operation was kept"), undefined);
});
