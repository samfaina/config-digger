// Walks one step of the plan through its module in steps/ and gives its
// result in the shape of the runner's `results` documents, with `by`,
// `evidence` and the local-only `checks`, `looks` and `manual`, which the
// plan's step holds.
//
// On CI (`ci: { evidence }`) the walk takes no screenshot, not even of a
// failed step: a step's looks are left to the tester, its verdict comes from
// its checks alone, and a step whose module names what a runner lacks
// (`ciLacks`) is Blocked without running. A step that also checks things
// which don't need it (`ciRuns`) runs, reads `t.ciLacks` to leave out what
// needs it, and is Blocked when the rest passes.
import { basename, join } from "node:path";
import { refocus, wait } from "./lib.mjs";

export async function walkStep(ctx, step, runner, { shots, ci }) {
  const { s } = ctx;
  const { id } = runner;
  const checks = [];
  const looks = [];
  const files = [];
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
      if (ci) {
        for (const what of judge) looks.push({ what });
        return null;
      }
      const file = join(shots, `${id}-${files.length + 1}-${name}.png`);
      await s.screenshot(file);
      files.push(file);
      for (const what of judge) looks.push({ what, file: basename(file) });
      return file;
    },
    blocked: null,
    ciLacks: (ci && runner.ciLacks) || null,
  };
  let error;
  const lacks = t.ciLacks && `el runner de CI no tiene ${t.ciLacks}`;
  if (lacks && !runner.ciRuns) t.blocked = lacks;
  // A window left blank, React's root empty, is a finding of the step: its
  // notes keep what the page logged and its last calls. The app opens again,
  // as the tester would, and the step runs once more from there.
  for (let attempt = 1; !t.blocked; attempt++) {
    error = undefined;
    try {
      if (step.setup) {
        ctx.setup(step);
        // The tester runs the setup in PowerShell and comes back to the
        // window, whose focus reads the files again.
        await refocus(s).catch(() => {});
        await wait(800);
      }
      await runner.run(t);
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
  const skipped = lacks && runner.ciRuns && (error || failed.length || t.blocked) && `Sin comprobar: ${lacks}`;
  if (lacks && !skipped) t.blocked ??= lacks;
  const status = t.blocked ? "blocked" : error || failed.length ? "ko" : "ok";
  const notes = [
    t.blocked && `Bloqueado: ${t.blocked}`,
    ...failed.map((c) => `KO: ${c.what}${c.actual ? `: ${c.actual}` : ""}`),
    skipped,
    error && `Error: ${error.message}`,
    ...findings.map((f) => `Nota: ${f}`),
    `Comprobado (${checks.length - failed.length}/${checks.length}): ${checks.filter((c) => c.ok).map((c) => c.what).join("; ")}`,
    looks.length &&
      (ci
        ? `Sin juzgar en CI: ${looks.map((l) => l.what).join("; ")}. Lo juzga el tester en su walk.`
        : `A juzgar en capturas: ${looks.map((l) => `${l.what} [${l.file}]`).join("; ")}`),
    runner.note,
    step.manual && `Queda para una persona: ${step.manual}`,
  ]
    .filter(Boolean)
    .join("\n");
  return {
    id,
    status,
    notes,
    time: new Date().toISOString(),
    version: ctx.version,
    by: ci ? "ci" : "local",
    evidence: ci ? ci.evidence : `uat/auto/webdriver/steps · ${id}${files.length ? ` · ${files.length} capturas locales` : ""}`,
    checks,
    looks,
    // What a person must still see, from the plan: the step stays in the
    // tester's walk.
    manual: step.manual,
  };
}

// The result of the plan's `step`, which the walk never reached because the
// app's session did not open: Blocked, with why, in the shape walkStep gives.
export function unwalkedStep(ctx, step, why, { ci }) {
  return {
    id: step.id,
    status: "blocked",
    notes: `Bloqueado: la sesión de WebDriver no se abrió: ${why}`,
    time: new Date().toISOString(),
    version: ctx.version,
    by: ci ? "ci" : "local",
    evidence: ci ? ci.evidence : `uat/auto/webdriver/steps · ${step.id}`,
    checks: [],
    looks: [],
    manual: step.manual,
  };
}

// A result as it reaches a run, here uat-results.json on CI: the runner's
// fields, `by` and `evidence`, without the local-only `checks`, `looks` and
// `manual` (docs/agents/uat.md).
export const resultDocument = ({ id, status, notes, time, version, by, evidence }) => ({ id, status, notes, time, version, by, evidence });

// Why a KO or a Blocked step is so: the first line of its notes.
export const reason = (result) => result.notes.split("\n")[0];

// The CI job's summary, in Markdown: a row per step with its verdict and,
// for a KO or a Blocked, its reason.
export function jobSummary(results) {
  const cell = (r) => (r.status === "ok" ? "" : ` ${reason(r).replaceAll("|", "\\|")}`);
  return ["| Paso | Veredicto | Motivo |", "| --- | --- | --- |", ...results.map((r) => `| ${r.id} | ${r.status} |${cell(r)} |`), ""].join("\n");
}
