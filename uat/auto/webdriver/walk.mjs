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
// judge to read. Screenshots stay on this machine.
//
//   node uat/auto/webdriver/walk.mjs --app <config-digger-app.exe> --driver <msedgedriver.exe>
//        --profile <…\cd-throwaway-profile> --out <results.json> --shots <dir> [--only 10.1,10.2] [--attach <session>]
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { profileEnv, Session, startTauriDriver } from "./driver.mjs";
import { refocus, wait } from "./lib.mjs";
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
    // The version under test, as the app's executable reports it.
    version: { type: "string" },
  },
});

const plan = JSON.parse(readFileSync(join(repo, "uat", "plan.json"), "utf8"));
const planSteps = new Map(plan.sections.flatMap((section) => section.steps.map((step) => [step.id, step])));
const version = opt.version ?? JSON.parse(readFileSync(join(repo, "crates", "config-digger-app", "ui", "package.json"), "utf8")).version;
const env = profileEnv(opt.profile);
mkdirSync(opt.shots, { recursive: true });

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
let driver;
let s;
if (opt.attach) s = Session.attach(opt.attach, port);
else {
  driver = await startTauriDriver({ nativeDriver: opt.driver, env, port });
  s = await Session.open({ app: opt.app, port });
}

const results = existsSync(opt.out) ? JSON.parse(readFileSync(opt.out, "utf8")) : [];
const save = (result) => {
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
  setup() {
    return ctx.ps(`& .\\uat\\data\\cd-throwaway-setup.ps1 -Root '${opt.root}' -Reset`);
  },
  // Closes the app's window and opens it again, as the tester does with the X
  // and the Start menu: the session ends and a new one starts.
  async restart() {
    await s.close().catch(() => {});
    const next = await Session.open({ app: opt.app, port });
    s.id = next.id;
  },
};

for (const id of ids) {
  const step = planSteps.get(id);
  const checks = [];
  const looks = [];
  const shots = [];
  const findings = [];
  const t = {
    ...ctx,
    step,
    check(ok, what, actual) {
      checks.push({ ok: Boolean(ok), what, actual });
      return Boolean(ok);
    },
    eq(actual, expected, what) {
      return t.check(actual === expected, what, actual === expected ? undefined : `«${actual}», no «${expected}»`);
    },
    has(text, part, what) {
      return t.check(typeof text === "string" && text.includes(part), what ?? `sale «${part}»`, undefined);
    },
    // A path the plan spells with / where the app shows \, or the other way:
    // the same path, so the check holds, and a note says how it reads.
    path(actual, expected, what) {
      const norm = (p) => (p ?? "").replaceAll("/", "\\");
      const same = norm(actual) === norm(expected);
      const note = `La ruta sale como «${actual}»; el plan la escribe «${expected}».`;
      if (same && actual !== expected && !findings.includes(note)) findings.push(note);
      if (what) t.check(same, what, same ? undefined : `«${actual}», no «${expected}»`);
      return same;
    },
    // Something seen on the way that the plan's checks don't ask about.
    note(text) {
      if (!findings.includes(text)) findings.push(text);
    },
    async shot(name, ...judge) {
      const file = join(opt.shots, `${id}-${shots.length + 1}-${name}.png`);
      await s.screenshot(file);
      shots.push(file);
      for (const what of judge) looks.push({ what, file: basename(file) });
      return file;
    },
    blocked: null,
  };
  const started = Date.now();
  let error;
  // A window left blank, React's root empty, is a finding of the step: its
  // notes keep what the page logged and its last calls. The app opens again,
  // as the tester would, and the step runs once more from there.
  for (let attempt = 1; ; attempt++) {
    error = undefined;
    try {
      if (step.setup) {
        ctx.setup();
        // The tester runs the setup in PowerShell and comes back to the
        // window, whose focus reads the files again.
        await refocus(s).catch(() => {});
        await wait(800);
      }
      await runners.get(id).run(t);
    } catch (e) {
      error = e;
      // What the window showed when the step failed, for whoever reads the
      // KO; then a dialog left open is closed, so it can't block the next step.
      await t.shot("error").catch(() => {});
      await s
        .exec(
          `const foot = document.querySelector(".dialog .dialog-foot");
           const out = foot && [...foot.querySelectorAll("button")].find((b) => ["Cancel", "Close", "Done"].includes(b.textContent.trim()) && !b.disabled);
           out?.click();`,
        )
        .catch(() => {});
    }
    const blank = await s
      .exec(
        `const root = document.getElementById("root");
         if (!root || root.children.length) return null;
         return { errors: JSON.parse(sessionStorage.getItem("__uatErrors") || "[]"),
                  calls: performance.getEntriesByType("resource").map((r) => r.name.replace("http://ipc.localhost/", "")).slice(-8) };`,
      )
      .catch(() => null);
    if (!blank) break;
    findings.push(`La ventana se quedó en blanco (la raíz de React vacía). Errores de la página: ${blank.errors.join(" | ") || "ninguno"}. Últimas llamadas: ${blank.calls.join(", ")}. La app se abrió de nuevo${attempt === 1 ? " y el paso se repitió" : ""}.`);
    await ctx.restart().catch(() => {});
    if (attempt === 2) {
      error ??= new Error("the window went blank");
      break;
    }
    checks.length = 0;
    looks.length = 0;
  }
  const failed = checks.filter((c) => !c.ok);
  // A step that failed before any of its own checks, such as on its setup or
  // on the state an earlier step left, is Blocked, not KO
  // (docs/research/uat-automation.md, section 6).
  if (error && !checks.length && !t.blocked) t.blocked = `antes de sus comprobaciones: ${error.message}`;
  const status = t.blocked ? "blocked" : error || failed.length ? "ko" : "ok";
  const notes = [
    t.blocked && `Bloqueado: ${t.blocked}`,
    ...failed.map((c) => `KO: ${c.what}${c.actual ? `: ${c.actual}` : ""}`),
    error && `Error: ${error.message}`,
    ...findings.map((f) => `Nota: ${f}`),
    `Comprobado (${checks.length - failed.length}/${checks.length}): ${checks.filter((c) => c.ok).map((c) => c.what).join("; ")}`,
    looks.length && `A juzgar en capturas: ${looks.map((l) => `${l.what} [${l.file}]`).join("; ")}`,
    runners.get(id).note,
    runners.get(id).manual && `Queda para una persona: ${runners.get(id).manual}`,
  ]
    .filter(Boolean)
    .join("\n");
  const result = {
    id,
    status,
    notes,
    time: new Date().toISOString(),
    version,
    by: "local",
    evidence: `uat/auto/webdriver/steps · ${id}${shots.length ? ` · ${shots.length} capturas locales` : ""}`,
    checks,
    looks,
    // What a person must still see: the step stays in the tester's walk.
    manual: runners.get(id).manual,
  };
  save(result);
  writeFileSync(sharedFile, JSON.stringify(shared, null, 2) + "\n");
  console.log(`${id} ${status} ${Math.round((Date.now() - started) / 100) / 10}s${failed.length ? ` · ${failed.map((c) => c.what).join(" · ")}` : ""}${error ? ` · ${error.message}` : ""}`);
}

if (!opt.attach) {
  await s.close().catch(() => {});
  driver?.kill();
}

const after = realData();
const changed = [...new Set([...before.keys(), ...after.keys()])].filter((f) => before.get(f) !== after.get(f));
if (changed.length) {
  console.error(`The walk changed the tester's own data:\n${changed.join("\n")}`);
  process.exitCode = 2;
} else console.log(`Nothing outside the throwaway profile and ${opt.root} changed (${before.size} files compared).`);
