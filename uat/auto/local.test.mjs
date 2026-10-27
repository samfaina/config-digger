// Tests of the local script, run.ps1: the one uat-results.json it writes from
// the Sandbox walk and the WebDriver walk, and the check that it wrote
// nothing of the tester's own.
// Run them with `node --test uat/auto/local.test.mjs`.
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { combine, writeResults } from "./results.mjs";
import { changed, snapshot } from "./untouched.mjs";

const here = fileURLToPath(new URL(".", import.meta.url));

const step = (id) => ({ id, title: `Paso ${id}`, goal: `Prueba ${id}.`, instructions: [], commands: [], expect: [], covers: [] });
const plan = {
  title: "UAT",
  language: "es",
  sections: [
    { id: "9", title: "Instalar", steps: [step("9.1"), step("9.2")] },
    { id: "7", title: "History", steps: [step("7.1"), step("7.2")] },
  ],
};
const result = (id, more = {}) => ({
  id,
  status: "ok",
  notes: `Comprobado: ${id}`,
  time: "2026-10-27T10:00:00.000Z",
  version: "0.1.0-rc.4",
  by: "local",
  evidence: `uat/auto/${id}`,
  ...more,
});

test("the results of both walks are one list in the plan's order, with the runner's fields alone", () => {
  const webdriver = [
    result("7.2", { status: "blocked", notes: "Claude Desktop no está instalado en este PC", checks: [], looks: [], manual: "" }),
    result("7.1", { checks: [{ what: "sale «Undo»", ok: true }], looks: [{ what: "rojo", file: "7.1-1.png" }] }),
  ];
  const sandbox = [result("9.2"), result("9.1")];
  assert.deepEqual(combine(plan, webdriver, sandbox), [result("9.1"), result("9.2"), result("7.1"), result("7.2", { status: "blocked", notes: "Claude Desktop no está instalado en este PC" })]);
});

test("a step with a result from each walk, or with two from one, is refused, as a run takes one per step", () => {
  assert.throws(() => combine(plan, [result("9.1")], [result("9.1")]), /9\.1 has two results/);
  assert.throws(() => combine(plan, [result("7.1"), result("7.1")], []), /7\.1 has two results/);
});

test("a result for a step the plan doesn't have is refused, as a run writes only the plan's steps", () => {
  assert.throws(() => combine(plan, [result("7.9")], []), /7\.9 is no step of the plan/);
});

// A work folder of the local script, removed when the test ends.
function workFolder(t) {
  const work = mkdtempSync(join(tmpdir(), "cd-uat-local-"));
  t.after(() => rmSync(work, { recursive: true, force: true }));
  mkdirSync(join(work, "sandbox"));
  return work;
}

test("the work folder's uat-results.json holds the WebDriver walk's and the Sandbox's results, a byte order mark and all", (t) => {
  const work = workFolder(t);
  writeFileSync(join(work, "webdriver.json"), JSON.stringify([result("7.1", { looks: [] })]));
  // Windows PowerShell 5.1 writes UTF-8 with a byte order mark.
  writeFileSync(join(work, "sandbox", "uat-results.json"), "\uFEFF" + JSON.stringify([result("9.1")]));
  const file = writeResults(work, plan);
  assert.equal(file, join(work, "uat-results.json"));
  assert.deepEqual(JSON.parse(readFileSync(file, "utf8")), [result("9.1"), result("7.1")]);
});

// A tester's PC in a folder: its APPDATA, LOCALAPPDATA and USERPROFILE, and
// a git checkout.
function testerPc(t) {
  const pc = mkdtempSync(join(tmpdir(), "cd-uat-pc-"));
  t.after(() => rmSync(pc, { recursive: true, force: true }));
  const env = { APPDATA: join(pc, "Roaming"), LOCALAPPDATA: join(pc, "Local"), USERPROFILE: join(pc, "user") };
  mkdirSync(join(env.APPDATA, "config-digger"), { recursive: true });
  mkdirSync(join(env.USERPROFILE, ".codex"), { recursive: true });
  writeFileSync(join(env.APPDATA, "config-digger", "settings.json"), "{}");
  writeFileSync(join(env.USERPROFILE, ".codex", "config.toml"), "model = 'o3'\n");
  const checkout = join(pc, "checkout");
  mkdirSync(checkout);
  execFileSync("git", ["init", "-q", checkout]);
  return { pc, env, checkout };
}

test("the check names each file of config-digger's real data or an Agent's configuration that changed, and nothing else", (t) => {
  const { pc, env, checkout } = testerPc(t);
  const before = snapshot({ env, checkout });
  writeFileSync(join(env.APPDATA, "config-digger", "settings.json"), '{"theme":"dark"}');
  mkdirSync(join(env.LOCALAPPDATA, "config-digger", "logs"), { recursive: true });
  writeFileSync(join(env.LOCALAPPDATA, "config-digger", "logs", "app.log"), "hola");
  rmSync(join(env.USERPROFILE, ".codex", "config.toml"));
  writeFileSync(join(pc, "cd-throwaway-profile.txt"), "lo de usar y tirar");
  assert.deepEqual(changed(before, snapshot({ env, checkout })).sort(), [
    join(env.APPDATA, "config-digger", "settings.json"),
    join(env.LOCALAPPDATA, "config-digger", "logs", "app.log"),
    join(env.USERPROFILE, ".codex", "config.toml"),
  ].sort());
});

test("the check names the checkout when its working tree changed", (t) => {
  const { env, checkout } = testerPc(t);
  const before = snapshot({ env, checkout });
  assert.deepEqual(changed(before, snapshot({ env, checkout })), []);
  writeFileSync(join(checkout, "uat-results.json"), "[]");
  assert.deepEqual(changed(before, snapshot({ env, checkout })), [`the working tree of ${checkout}: ?? uat-results.json`]);
});

test("the check names a file of the checkout that was changed before the run and changed again", (t) => {
  const { env, checkout } = testerPc(t);
  writeFileSync(join(checkout, "notes.md"), "uno");
  const before = snapshot({ env, checkout });
  writeFileSync(join(checkout, "notes.md"), "dos");
  assert.deepEqual(changed(before, snapshot({ env, checkout })), [`the working tree of ${checkout}: ?? notes.md`]);
});

// The local script in a copy of the checkout whose three tools are stubs:
// each keeps the real one's param block, notes the parameters it was given
// in calls.txt, by name, runs `runsFirst`, and writes the results the real one
// writes, or runs `fail` instead. It runs in Windows PowerShell on the
// tester's PC of `testerPc`, with `app` as its -App.
function localScript(t, { fail = {}, runsFirst = {} } = {}) {
  const { pc, env, checkout } = testerPc(t);
  cpSync(here, join(checkout, "uat", "auto"), { recursive: true });
  writeFileSync(join(checkout, "uat", "plan.json"), JSON.stringify(plan));
  const stub = (tool, writes) => {
    const file = join(checkout, "uat", "auto", ...tool.split("/"));
    const real = readFileSync(file, "utf8").replace(/\r\n/g, "\n");
    const params = real.slice(real.indexOf("\nparam(") + 1, real.indexOf("\n)\n") + 2);
    const given = "($PSBoundParameters.Keys | Sort-Object | ForEach-Object { \"-$_ $($PSBoundParameters[$_])\" }) -join ' '";
    const call = `New-Item -ItemType Directory -Force $Work | Out-Null; Add-Content -LiteralPath (Join-Path '${pc}' calls.txt) -Value ('${tool} ' + (${given}))`;
    writeFileSync(file, [params, call, runsFirst[tool] ?? "", fail[tool] ?? writes].join("\n"));
  };
  stub("sandbox/build-update.ps1", "");
  stub("sandbox/run.ps1", `Set-Content -LiteralPath (Join-Path $Work uat-results.json) -Value '${JSON.stringify([result("9.1"), result("9.2")])}' -Encoding utf8`);
  stub("webdriver/run.ps1", `Set-Content -LiteralPath (Join-Path $Work webdriver.json) -Value '${JSON.stringify([result("7.1", { looks: [] }), result("7.2")])}'`);
  execFileSync("git", ["-C", checkout, "add", "."], { stdio: "pipe" });
  execFileSync("git", ["-C", checkout, "-c", "user.name=uat", "-c", "user.email=uat@example.com", "commit", "-qm", "checkout"], { stdio: "pipe" });
  const work = join(pc, "work");
  const run = (args) =>
    spawnSync("powershell.exe", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", join(checkout, "uat", "auto", "run.ps1"), ...args], {
      env: { ...process.env, ...env },
      encoding: "utf8",
    });
  const calls = () => (existsSync(join(pc, "calls.txt")) ? readFileSync(join(pc, "calls.txt"), "utf8").trim().split(/\r?\n/) : []);
  return { pc, env, work, run, calls };
}
// An executable whose version is known, for -App.
const app = join(process.env.SystemRoot, "System32", "WindowsPowerShell", "v1.0", "powershell.exe");
const appVersion = execFileSync("powershell.exe", ["-NoProfile", "-Command", `(Get-Item '${app}').VersionInfo.ProductVersion`], { encoding: "utf8" }).trim();

test("the command runs build-update.ps1, the Sandbox walk and the WebDriver walk in order, and writes one uat-results.json of both", (t) => {
  const { work, run, calls } = localScript(t);
  const root = "E:\\cd-throwaway";
  const ran = run(["-Version", appVersion, "-App", app, "-Work", work, "-Root", root]);
  assert.equal(ran.status, 0, ran.stderr + ran.stdout);
  assert.deepEqual(calls(), [
    `sandbox/build-update.ps1 -Commit v${appVersion} -Work ${join(work, "update")}`,
    `sandbox/run.ps1 -Update ${join(work, "update", "installers")} -Version ${appVersion} -Work ${join(work, "sandbox")}`,
    `webdriver/run.ps1 -App ${app} -Root ${root} -Work ${work}`,
  ]);
  const results = JSON.parse(readFileSync(join(work, "uat-results.json"), "utf8"));
  assert.deepEqual(results, [result("9.1"), result("9.2"), result("7.1"), result("7.2")]);
  assert.match(ran.stdout, /uat-results\.json: 4 results, 4 OK, 0 KO, 0 Blocked/);
  assert.ok(ran.stdout.includes("The tester's own data is as before the run: the 2 files of config-digger's real data"), ran.stdout);
});

test("the command stops at the tool that fails, saying which and what did not run, and writes no uat-results.json", (t) => {
  const { work, run, calls } = localScript(t, { fail: { "sandbox/run.ps1": 'throw "gh release download v9.9.9 failed"' } });
  mkdirSync(work);
  writeFileSync(join(work, "uat-results.json"), "[]");
  const ran = run(["-Version", appVersion, "-App", app, "-Work", work]);
  assert.notEqual(ran.status, 0);
  assert.match(ran.stderr, /The Sandbox walk failed: gh release download v9\.9\.9 failed\. The WebDriver walk did not run\./);
  assert.deepEqual(calls().map((call) => call.split(" ")[0]), ["sandbox/build-update.ps1", "sandbox/run.ps1"]);
  assert.equal(existsSync(join(work, "uat-results.json")), false, "an earlier run's file is gone");
});

test("a tool that exits with a code fails the command too", (t) => {
  const { work, run, calls } = localScript(t, { fail: { "sandbox/build-update.ps1": "exit 3" } });
  const ran = run(["-Version", appVersion, "-App", app, "-Work", work]);
  assert.notEqual(ran.status, 0);
  assert.match(ran.stderr, /build-update\.ps1 failed with exit code 3: see its output above\. The Sandbox walk and the WebDriver walk did not run\./);
  assert.deepEqual(calls().map((call) => call.split(" ")[0]), ["sandbox/build-update.ps1"]);
});

test("the command refuses an -App that reports another version than -Version, before running anything", (t) => {
  const { work, run, calls } = localScript(t);
  const ran = run(["-Version", "0.1.0-rc.4", "-App", app, "-Work", work]);
  assert.notEqual(ran.status, 0);
  assert.ok(ran.stderr.replace(/\s+/g, " ").includes(`reports version ${appVersion}, not 0.1.0-rc.4`), ran.stderr);
  assert.deepEqual(calls(), []);
});

test("the command fails when a tool changed the tester's own data, naming the file", (t) => {
  const { env, work, run } = localScript(t, { runsFirst: { "webdriver/run.ps1": "Set-Content -LiteralPath (Join-Path $env:APPDATA config-digger/settings.json) -Value '{}x'" } });
  const ran = run(["-Version", appVersion, "-App", app, "-Work", work]);
  assert.notEqual(ran.status, 0);
  assert.ok(ran.stderr.includes(`The run changed the tester's own data:\n${join(env.APPDATA, "config-digger", "settings.json")}`), ran.stderr);
  assert.match(ran.stderr, /the run changed the tester's own data: see the list above/);
});

test("a check that breaks fails the command as broken, not as a change to the tester's data", (t) => {
  const { work, run } = localScript(t, { runsFirst: { "webdriver/run.ps1": "Remove-Item -LiteralPath (Join-Path $Work untouched.json)" } });
  const ran = run(["-Version", appVersion, "-App", app, "-Work", work]);
  assert.notEqual(ran.status, 0);
  assert.match(ran.stderr, /the check of the tester's own data failed: see its output above/);
  assert.doesNotMatch(ran.stderr, /the run changed the tester's own data/);
});
