// Walks the UAT steps of uat/plan.json that the real app can show, through
// WebDriver, on the throwaway data and a throwaway Windows profile. Each step
// writes a result in the shape of the runner's `results` documents
// (docs/agents/uat.md), with `by` and `evidence` as
// docs/research/uat-automation.md proposes:
//
//   { id, status: "ok" | "ko" | "blocked", notes, time, version, by: "local", evidence }
//
// Checks that need eyes, such as a colour or a shadow, are not guessed: the
// step takes a screenshot and lists the check under "a juzgar", for the agent
// judge to read. Screenshots stay on this machine. With --ci the walk takes
// none and writes only the runner's fields, `by: "ci"` and the run's URL
// (step.mjs).
//
//   node uat/auto/webdriver/walk.mjs --app <config-digger-app.exe> --driver <msedgedriver.exe>
//        --profile <…\cd-throwaway-profile> --out <results.json> --shots <dir> [--only 10.1,10.2] [--attach <session>]
//        [--ci --evidence <run URL> --summary <file>] [--debug-port 9222]
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { appendFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { profileEnv, Session, startTauriDriver } from "./driver.mjs";
import { setupSwitches } from "./lib.mjs";
import { jobSummary, reason, resultDocument, unwalkedStep, walkStep } from "./step.mjs";
import sections from "./steps/index.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "..", "..", "..");
const { values: opt } = parseArgs({
  options: {
    app: { type: "string" },
    driver: { type: "string" },
    profile: { type: "string" },
    out: { type: "string" },
    shots: { type: "string" },
    only: { type: "string" },
    from: { type: "string" },
    attach: { type: "string" },
    root: { type: "string", default: "E:\\cd-throwaway" },
    port: { type: "string", default: "4444" },
    // With a port, the walk starts the app itself and msedgedriver attaches
    // to its WebView2 on that port, as an elevated app needs (driver.mjs).
    "debug-port": { type: "string" },
    // The version under test, as the app's executable reports it.
    version: { type: "string" },
    // CI mode: no screenshot at all, results with `by: "ci"` and the run's
    // URL as `evidence`, and the job summary appended to `summary`.
    ci: { type: "boolean", default: false },
    evidence: { type: "string" },
    summary: { type: "string" },
  },
});
if (opt.ci && !opt.evidence) throw new Error("--ci needs --evidence, the URL of the CI run");
const ci = opt.ci ? { evidence: opt.evidence } : null;

const plan = JSON.parse(readFileSync(join(repo, "uat", "plan.json"), "utf8"));
const planSteps = new Map(plan.sections.flatMap((section) => section.steps.map((step) => [step.id, step])));
const version = opt.version ?? JSON.parse(readFileSync(join(repo, "crates", "config-digger-app", "ui", "package.json"), "utf8")).version;
const env = profileEnv(opt.profile);
if (!ci) mkdirSync(opt.shots, { recursive: true });

// The steps in the plan's order, so the state each one leaves is the state the
// next one starts from, as in the manual walk.
const order = plan.sections.flatMap((section) => section.steps.map((step) => step.id));
const runners = new Map(sections.flat().map((step) => [step.id, step]));
let ids = order.filter((id) => runners.has(id));
if (opt.only) ids = ids.filter((id) => opt.only.split(",").includes(id));
if (opt.from) {
  if (!ids.includes(opt.from)) throw new Error(`--from ${opt.from} is no step the walk knows`);
  ids = ids.slice(ids.indexOf(opt.from));
}

// The tester's own data the walk must never touch: config-digger's real data
// folders and the configuration files of the Agents the steps open. A hash of
// each file before the walk is compared with one after. Claude Code's files
// are left out: an agent running the walk writes them itself.
function realData() {
  const files = [];
  const walkDir = (dir) => {
    if (!existsSync(dir)) return;
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) walkDir(path);
      else files.push(path);
    }
  };
  walkDir(join(process.env.APPDATA, "config-digger"));
  walkDir(join(process.env.LOCALAPPDATA, "config-digger"));
  files.push(join(process.env.USERPROFILE, ".codex", "config.toml"), join(process.env.APPDATA, "Claude", "claude_desktop_config.json"));
  const packages = join(process.env.LOCALAPPDATA, "Packages");
  if (existsSync(packages)) {
    for (const name of readdirSync(packages).filter((n) => n.startsWith("Claude_"))) {
      files.push(join(packages, name, "LocalCache", "Roaming", "Claude", "claude_desktop_config.json"));
    }
  }
  return new Map(files.filter((f) => existsSync(f)).map((f) => [f, createHash("sha256").update(readFileSync(f)).digest("hex")]));
}
const before = realData();

const port = Number(opt.port);
const debugPort = opt["debug-port"] ? Number(opt["debug-port"]) : undefined;
let driver;
let s;
// A session that does not open leaves every step Blocked, in the results all
// the same, and the walk breaking.
let unopened;
if (opt.attach) s = Session.attach(opt.attach, port);
else {
  try {
    driver = await startTauriDriver({ nativeDriver: opt.driver, env, port });
    s = await Session.open({ app: opt.app, port, env, debugPort });
  } catch (e) {
    unopened = e;
  }
}

const results = existsSync(opt.out) ? JSON.parse(readFileSync(opt.out, "utf8")) : [];
const save = (walked) => {
  // On CI the results are uat-results.json, which reaches a run as it is.
  const result = ci ? resultDocument(walked) : walked;
  const at = results.findIndex((r) => r.id === result.id);
  if (at >= 0) results[at] = result;
  else results.push(result);
  writeFileSync(opt.out, JSON.stringify(results, null, 2) + "\n");
};

// What a step leaves for a later one, such as 10.7's Operation id for 11.6.
// Kept beside the results, so a run of a later step alone still finds it.
const sharedFile = opt.out.replace(/\.json$/, "") + ".shared.json";
const shared = existsSync(sharedFile) ? JSON.parse(readFileSync(sharedFile, "utf8")) : {};

const ctx = {
  s,
  shared,
  version,
  env,
  root: opt.root,
  repo,
  app: opt.app,
  // Runs a PowerShell command with the throwaway profile's variables, as the
  // tester runs a step's command, and returns what it prints.
  ps(command) {
    return execFileSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", command], {
      cwd: repo,
      env: { ...process.env, ...env },
      encoding: "utf8",
    }).trim();
  },
  // Answers the Windows folder picker the app has just opened.
  pick(folder) {
    return execFileSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-File", join(here, "picker.ps1"), "-Folder", folder], {
      encoding: "utf8",
    });
  },
  // With a step, its plan's setup may add switches, such as -Symlink.
  setup(step) {
    const more = step?.setup ? setupSwitches(step.setup.text) : "";
    return ctx.ps(`& .\\uat\\data\\cd-throwaway-setup.ps1 -Root '${opt.root}' -Reset${more}`);
  },
  // Closes the app's window and opens it again, as the tester does with the X
  // and the Start menu: the session ends and a new one starts.
  async restart() {
    await s.close().catch(() => {});
    const next = await Session.open({ app: opt.app, port, env, debugPort });
    s.id = next.id;
    s.app = next.app;
  },
};

if (unopened) {
  for (const id of ids) save(unwalkedStep(ctx, runners.get(id), unopened.message, { ci }));
} else {
  for (const id of ids) {
    const started = Date.now();
    const result = await walkStep(ctx, planSteps.get(id), runners.get(id), { shots: opt.shots, ci });
    save(result);
    writeFileSync(sharedFile, JSON.stringify(shared, null, 2) + "\n");
    console.log(`${id} ${result.status} ${Math.round((Date.now() - started) / 100) / 10}s${result.status === "ok" ? "" : ` · ${reason(result)}`}`);
  }
}
if (opt.summary) appendFileSync(opt.summary, jobSummary(results));

// A driver that stopped on the way leaves every later step Blocked: that is
// the walk breaking, not a verdict.
const driverStopped = !unopened && driver && (driver.exitCode !== null || driver.signalCode !== null);
if (!opt.attach) {
  await s?.close().catch(() => {});
  driver?.kill();
}
if (unopened) {
  console.error(`The app's session did not open, so every step is Blocked: ${unopened.stack}`);
  // The app's own log says how far it got.
  const logs = join(env.LOCALAPPDATA, "config-digger", "logs");
  for (const name of existsSync(logs) ? readdirSync(logs) : []) {
    console.error(`${name}, its last lines:\n${readFileSync(join(logs, name), "utf8").trimEnd().split("\n").slice(-30).join("\n")}`);
  }
  process.exitCode = 4;
}
if (driverStopped) {
  console.error(`tauri-driver stopped during the walk, with ${driver.signalCode ?? `exit code ${driver.exitCode}`}`);
  process.exitCode = 3;
}

const after = realData();
const changed = [...new Set([...before.keys(), ...after.keys()])].filter((f) => before.get(f) !== after.get(f));
if (changed.length) {
  console.error(`The walk changed the tester's own data:\n${changed.join("\n")}`);
  process.exitCode = 2;
} else console.log(`Nothing outside the throwaway profile and ${opt.root} changed (${before.size} files compared).`);
