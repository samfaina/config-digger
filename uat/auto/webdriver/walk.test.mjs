// Tests of the WebDriver walk's parts that run without the app: the steps
// it knows match the plan, the throwaway profile, and reading an Operation id.
// Run them with `node --test uat/auto/webdriver/walk.test.mjs`.
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { profileEnv } from "./driver.mjs";
import { operationId } from "./steps/common.mjs";
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

test("an Operation id is read from the lines that name one", () => {
  assert.equal(operationId("Disabled 1 Entry. Operation 20261023-172530-c63: verified."), "20261023-172530-c63");
  assert.equal(operationId("Operation 20261023-172530-c63 · backup kept"), "20261023-172530-c63");
  assert.equal(operationId("Nothing of this Operation was kept\nOperation 20261023-172614-84d"), "20261023-172614-84d");
  assert.equal(operationId("Nothing of this Operation was kept"), undefined);
});
