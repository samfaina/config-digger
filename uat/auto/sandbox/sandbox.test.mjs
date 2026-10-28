// Tests of the Sandbox walk's command, run.ps1, outside Windows Sandbox: in
// a copy of the checkout whose gh, npm, tauri and Sandbox are stubs.
// Run them with `node --test uat/auto/sandbox/sandbox.test.mjs`.
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const here = fileURLToPath(new URL(".", import.meta.url));
const repoRoot = join(here, "..", "..", "..");
const version = "0.1.0-rc.3";
const walkSteps = ["8.1", "9.5", "9.6", "9.2", "9.3", "9.4", "13.2", "13.3", "13.4", "13.6"];

const git = (cwd, ...args) => execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8", stdio: "pipe" });
// What the run must leave as it was in the repository's .git.
const refsAndWorktrees = (repo) => git(repo, "for-each-ref") + git(repo, "worktree", "list", "--porcelain");
// Every file under `folder`, as paths relative to it.
const files = (folder) => (existsSync(folder) ? readdirSync(folder, { recursive: true, withFileTypes: true }).filter((e) => e.isFile()).map((e) => join(e.parentPath, e.name).slice(folder.length + 1)) : []);
// Each stub call's first two words, such as "build 0.1.0-rc.3" or "sandbox walk".
const heads = (calls) => calls.map((call) => call.split(" ").slice(0, 2).join(" "));
// The files under `folder` that hold the run's private key.
const keysIn = (folder) => files(folder).filter((file) => readFileSync(join(folder, file), "utf8").includes("throwaway-private-generate"));
// The verdicts run.ps1 wrote, which Windows PowerShell 5.1 starts with a byte order mark.
const verdictsIn = (work) => JSON.parse(readFileSync(join(work, "uat-results.json"), "utf8").replace(/^\uFEFF/, ""));

// One check line of in-sandbox.ps1, for the stub Sandbox to write.
const check = (id, what, ok = true) => JSON.stringify({ time: "2026-10-28T10:00:00.000Z", id, check: what, actual: ok ? "1" : "0", expected: "1", ok });

// A tester's PC in a folder: a checkout of the release candidate's commit,
// tagged v<version>, holding the Sandbox walk's scripts and the four files
// that carry the workspace version; a TEMP of its own; and on its PATH, gh,
// npm and the tauri npm ci installs, as stubs. The Sandbox is a stub too:
// open-sandbox.ps1 writes `lines[run]` as the run's results. Each stub notes
// what it was asked in calls.txt.
function testerPc(t, { lines = {}, env: more = {} } = {}) {
  const pc = mkdtempSync(join(tmpdir(), "cd-uat-sandbox-"));
  t.after(() => rmSync(pc, { recursive: true, force: true }));
  const repo = join(pc, "repo");
  const sandbox = join(repo, "uat", "auto", "sandbox");
  mkdirSync(sandbox, { recursive: true });
  for (const name of readdirSync(here).filter((name) => name.endsWith(".ps1"))) cpSync(join(here, name), join(sandbox, name));
  cpSync(join(repoRoot, "uat", "plan.json"), join(repo, "uat", "plan.json"));
  cpSync(join(repoRoot, "uat", "data", "cd-throwaway-setup.ps1"), join(repo, "uat", "data", "cd-throwaway-setup.ps1"));
  const ui = join(repo, "crates", "config-digger-app", "ui");
  mkdirSync(ui, { recursive: true });
  writeFileSync(join(repo, "Cargo.toml"), `[workspace.package]\nversion = "${version}"\n`);
  writeFileSync(join(repo, "Cargo.lock"), ["config-digger-app", "config-digger-cli", "config-digger-core", "xtask"].map((name) => `[[package]]\nname = "${name}"\nversion = "${version}"\n`).join("\n"));
  writeFileSync(join(ui, "package.json"), JSON.stringify({ name: "config-digger-ui", version }, null, 2));
  writeFileSync(join(ui, "package-lock.json"), JSON.stringify({ name: "config-digger-ui", version, packages: { "": { name: "config-digger-ui", version } } }, null, 2));

  const calls = join(pc, "calls.txt");
  const note = (what) => `Add-Content -LiteralPath '${calls}' -Value ${what}`;
  // The Sandbox: the run comes from the LogonCommand, the results folder and
  // what else it sees from the mapped folders.
  writeFileSync(
    join(sandbox, "open-sandbox.ps1"),
    [
      "param([Parameter(Mandatory)] [string]$Wsb, [Parameter(Mandatory)] [string]$Results, [int]$TimeoutMinutes = 40)",
      "$config = [xml](Get-Content -LiteralPath $Wsb -Raw)",
      "$run = [regex]::Match($config.Configuration.LogonCommand.Command, '-Run (\\w+)').Groups[1].Value",
      "$maps = @($config.Configuration.MappedFolders.MappedFolder | ForEach-Object { \"$($_.SandboxFolder)=$($_.HostFolder)\" }) -join ' '",
      note(`"sandbox $run $maps"`),
      `$lines = Join-Path '${pc}' "lines-$run.jsonl"`,
      "if (Test-Path $lines) { Copy-Item $lines (Join-Path $Results results.jsonl) }",
      "Set-Content (Join-Path $Results done.txt) 'done'",
    ].join("\n"),
  );
  for (const [run, runLines] of Object.entries({ walk: walkSteps.map((id) => check(id, "a check")), update: [check("12.2", "a check")], ...lines })) {
    writeFileSync(join(pc, `lines-${run}.jsonl`), runLines.join("\n") + "\n");
  }

  const bin = join(pc, "bin");
  mkdirSync(bin);
  writeFileSync(
    join(bin, "gh.ps1"),
    [
      note(`("gh " + ($args -join ' '))`),
      "$dir = $args[[array]::IndexOf($args, '--dir') + 1]",
      "for ($i = 0; $i -lt $args.Count; $i++) { if ($args[$i] -eq '--pattern') { Set-Content (Join-Path $dir $args[$i + 1]) 'asset' } }",
    ].join("\n"),
  );
  // npm ci installs the tauri CLI, whose signer writes a key pair; npm's
  // tauri script builds the version package.json carries, or fails on
  // FAKE_FAIL_BUILD's.
  writeFileSync(
    join(bin, "npm.ps1"),
    [
      "if ($args[0] -eq 'ci') { New-Item -ItemType Directory -Force node_modules\\.bin | Out-Null; Copy-Item (Join-Path $PSScriptRoot tauri.cmd) node_modules\\.bin; exit 0 }",
      "$v = (Get-Content package.json -Raw | ConvertFrom-Json).version",
      note(`"build $v key=$env:TAURI_SIGNING_PRIVATE_KEY target=$env:CARGO_TARGET_DIR"`),
      "if ($v -eq $env:FAKE_FAIL_BUILD) { exit 1 }",
      "$nsis = Join-Path $env:CARGO_TARGET_DIR release\\bundle\\nsis",
      "New-Item -ItemType Directory -Force $nsis | Out-Null",
      'Set-Content (Join-Path $nsis "config-digger_${v}_x64-setup.exe") "setup $v"',
      'Set-Content (Join-Path $nsis "config-digger_${v}_x64-setup.exe.sig") "signature of $v"',
    ].join("\n"),
  );
  writeFileSync(join(bin, "tauri.cmd"), '@echo off\r\nif not "%1"=="signer" exit /b 9\r\necho throwaway-private-%2>"%~4"\r\necho throwaway-public-key>"%~4.pub"\r\n');

  git(pc, "init", "-q", repo);
  git(repo, "add", ".");
  // A commit older than any build, as a release candidate's often is.
  execFileSync("git", ["-C", repo, "-c", "user.name=uat", "-c", "user.email=uat@example.com", "commit", "-qm", "release candidate"], {
    env: { ...process.env, GIT_AUTHOR_DATE: "2020-01-01T00:00:00Z", GIT_COMMITTER_DATE: "2020-01-01T00:00:00Z" },
    stdio: "pipe",
  });
  git(repo, "tag", `v${version}`);

  const temp = join(pc, "temp");
  mkdirSync(temp);
  const env = { ...process.env, TEMP: temp, TMP: temp, PATH: `${bin};${process.env.PATH}`, TAURI_SIGNING_PRIVATE_KEY: "the-release-key", ...more };
  const run = (args = []) =>
    spawnSync("powershell.exe", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", join(sandbox, "run.ps1"), "-Version", version, ...args], { env, encoding: "utf8" });
  const called = () => (existsSync(calls) ? readFileSync(calls, "utf8").trim().split(/\r?\n/) : []);
  const work = join(temp, "cd-uat-sandbox");
  return { pc, repo, temp, work, run, called };
}

test("given only the version, the command builds A and B, runs the walk and the update Sandboxes, prints the verdicts and writes uat-results.json", (t) => {
  const { work, run, called } = testerPc(t);
  const ran = run();
  assert.equal(ran.status, 0, ran.stderr + ran.stdout);
  const update = join(work, "update");
  const calls = called();
  assert.deepEqual(heads(calls), ["build 0.1.0-rc.3", "build 0.1.0-rc.99", "gh release", "sandbox walk", "sandbox update"]);
  assert.ok(calls[4].includes(`C:\\uat\\update=${join(update, "installers")}`), calls[4]);
  assert.deepEqual(files(join(update, "installers")).sort(), ["config-digger_0.1.0-rc.3_x64-setup.exe", "config-digger_0.1.0-rc.3_x64-setup.exe.sig", "config-digger_0.1.0-rc.99_x64-setup.exe", "config-digger_0.1.0-rc.99_x64-setup.exe.sig", "latest.json"]);
  const verdicts = verdictsIn(work);
  assert.deepEqual(verdicts.map((v) => `${v.id} ${v.status} ${v.by}`), ["9.1", ...walkSteps.slice(0, 6), "12.2", ...walkSteps.slice(6)].map((id) => `${id} ok local`));
  assert.match(ran.stdout, /^12\.2 +ok$/m);
});

test("the command writes on the PC only under its work folder, leaves the repository's .git as it was, and no private key", (t) => {
  const { repo, temp, work, run, called } = testerPc(t);
  const before = refsAndWorktrees(repo);
  const ran = run();
  assert.equal(ran.status, 0, ran.stderr + ran.stdout);
  assert.deepEqual(readdirSync(temp), ["cd-uat-sandbox"]);
  assert.equal(refsAndWorktrees(repo), before);
  assert.equal(git(repo, "status", "--porcelain", "--ignored"), "");
  assert.deepEqual(keysIn(work), []);
  // The run's own key signs both builds, never the release key the PC holds.
  assert.deepEqual(called().filter((call) => call.startsWith("build")).map((call) => call.split(" ")[2]), ["key=throwaway-private-generate", "key=throwaway-private-generate"]);
});

test("when the build of B fails, the command fails before any Sandbox opens, and leaves no private key, no uat-results.json and the .git as it was", (t) => {
  const { repo, work, run, called } = testerPc(t, { env: { FAKE_FAIL_BUILD: "0.1.0-rc.99" } });
  const before = refsAndWorktrees(repo);
  mkdirSync(work);
  writeFileSync(join(work, "uat-results.json"), "[]");
  const ran = run();
  assert.notEqual(ran.status, 0);
  assert.match(ran.stderr, /tauri build of 0\.1\.0-rc\.99 failed/);
  assert.deepEqual(heads(called()), ["build 0.1.0-rc.3", "build 0.1.0-rc.99"]);
  assert.deepEqual(keysIn(work), []);
  assert.equal(existsSync(join(work, "uat-results.json")), false, "an earlier run's file is gone");
  assert.equal(refsAndWorktrees(repo), before);
});

test("a second run starts from a clean state: a fresh copy of the commit, and none of the first run's installers or results", (t) => {
  const { work, run, called } = testerPc(t, { lines: { walk: [check("8.1", "a check", false)] } });
  const tree = join(work, "update", "tree");
  const first = run();
  assert.equal(first.status, 0, first.stderr + first.stdout);
  writeFileSync(join(tree, "left-over.txt"), "from the first run");
  writeFileSync(join(work, "update", "installers", "config-digger_0.1.0-rc.98_x64-setup.exe"), "from the first run");
  const started = Date.now() - 2000;
  const second = run(["-Runs", "update"]);
  assert.equal(second.status, 0, second.stderr + second.stdout);
  assert.deepEqual(called().filter((call) => call.startsWith("build")).map((call) => call.split(" ")[1]), ["0.1.0-rc.3", "0.1.0-rc.99", "0.1.0-rc.3", "0.1.0-rc.99"]);
  assert.equal(existsSync(join(tree, "left-over.txt")), false);
  assert.equal(readFileSync(join(tree, "Cargo.toml"), "utf8").includes('version = "0.1.0-rc.99"'), true, "B's version, raised in a fresh copy");
  // Newer than what the kept target folder built before, so cargo builds
  // the copy's crates again whatever the commit's date.
  assert.ok(statSync(join(tree, "uat", "plan.json")).mtimeMs >= started, "the copy's files take the time they were copied");
  assert.equal(existsSync(join(work, "update", "installers", "config-digger_0.1.0-rc.98_x64-setup.exe")), false);
  assert.equal(existsSync(join(work, "results-walk")), false, "the walk's results of the first run");
  const verdicts = verdictsIn(work);
  assert.deepEqual(verdicts.map((v) => `${v.id} ${v.status}`), ["9.1 ok", "12.2 ok"]);
});

test("-Runs walk builds nothing, and -Update updates between the installers a build before left", (t) => {
  const { pc, run, called } = testerPc(t);
  assert.equal(run(["-Runs", "walk"]).status, 0);
  assert.deepEqual(heads(called()), ["gh release", "sandbox walk"]);
  const installers = join(pc, "installers");
  mkdirSync(installers);
  writeFileSync(join(installers, "latest.json"), "{}");
  const ran = run(["-Runs", "update", "-Update", installers]);
  assert.equal(ran.status, 0, ran.stderr + ran.stdout);
  const calls = called().slice(2);
  assert.deepEqual(heads(calls), ["gh release", "sandbox update"]);
  assert.ok(calls[1].includes(`C:\\uat\\update=${installers}`), calls[1]);
});

test("-From walks the assets of a folder instead of the release's, with no 9.1", (t) => {
  const { pc, work, run, called } = testerPc(t);
  const from = join(pc, "build");
  mkdirSync(from);
  for (const asset of [`config-digger_${version}_x64-setup.exe`, `config-digger_${version}_x64-cli.zip`]) writeFileSync(join(from, asset), "unreleased");
  const ran = run(["-Runs", "walk", "-From", from]);
  assert.equal(ran.status, 0, ran.stderr + ran.stdout);
  assert.deepEqual(heads(called()), ["sandbox walk"]);
  assert.equal(readFileSync(join(work, "release", `config-digger_${version}_x64-setup.exe`), "utf8"), "unreleased");
  const verdicts = verdictsIn(work);
  assert.deepEqual(verdicts.map((v) => v.id), walkSteps);
});

// What `expression` gives in Windows PowerShell, with checks.ps1 loaded as
// in-sandbox.ps1 loads it, and the plan as $plan.
function checks(expression) {
  const script = [
    '$ErrorActionPreference = "Stop"',
    `$plan = [IO.File]::ReadAllText('${join(repoRoot, "uat", "plan.json")}', [Text.Encoding]::UTF8) | ConvertFrom-Json`,
    `. '${join(here, "checks.ps1")}'`,
    `ConvertTo-Json -Compress @(${expression})`,
  ].join("\n");
  const out = execFileSync("powershell.exe", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", "-"], { input: script, encoding: "utf8" });
  return JSON.parse(out);
}

test("9.3's and 9.4's expect quote the Already Installed page as the installer writes it, with the version filled in", () => {
  const nsh = readFileSync(join(repoRoot, "crates", "config-digger-app", "installer", "English.nsh"), "utf8");
  const lang = (name) => nsh.match(new RegExp(`^LangString ${name} \\$\\{LANG_ENGLISH\\} "(.*)"$`, "m"))[1];
  const page = lang("alreadyInstalledLong").replace("${PRODUCTNAME}", "config-digger").replace("${VERSION}", version);
  assert.equal(page, `config-digger ${version} is already installed. Select the operation you want to perform and click Next to continue.`);
  for (const id of ["9.3", "9.4"]) assert.deepEqual(checks(`PlanQuotes $plan "${id}" "Already Installed" "${version}"`), [page]);
});

test("a step's expect line that names nothing of the kind stops the step", () => {
  assert.throws(() => checks(`PlanQuotes $plan "9.3" "Nothing like this" "${version}"`), /step 9\.3 expects nothing about 'Nothing like this'/);
});

test("a folder's state changes with a file's bytes, a file added or one removed, and is the same for the same files", (t) => {
  const folder = mkdtempSync(join(tmpdir(), "cd-uat-roaming-"));
  t.after(() => rmSync(folder, { recursive: true, force: true }));
  const state = () => checks(`FolderState '${folder}'`)[0];
  mkdirSync(join(folder, "lists"));
  writeFileSync(join(folder, "settings.json"), '{"theme":"dark"}');
  writeFileSync(join(folder, "lists", "homes.json"), "[]");
  const before = state();
  assert.match(before, /^lists\\homes\.json [0-9A-F]{64}, settings\.json [0-9A-F]{64}$/);
  assert.equal(state(), before);
  writeFileSync(join(folder, "settings.json"), '{"theme":"light"}');
  assert.notEqual(state(), before);
  writeFileSync(join(folder, "settings.json"), '{"theme":"dark"}');
  writeFileSync(join(folder, "extra.json"), "{}");
  assert.notEqual(state(), before);
  rmSync(join(folder, "extra.json"));
  rmSync(join(folder, "lists", "homes.json"));
  assert.notEqual(state(), before);
  rmSync(folder, { recursive: true });
  assert.equal(state(), "(no folder)");
});
