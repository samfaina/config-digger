// Tests of the agent judge's mechanical half, judge.mjs: which results of
// the WebDriver walk it lists, with their plan fields and screenshots, and
// how the judge's verdicts merge into the walk's results.
// Run them with `node --test uat/auto/webdriver/judge.test.mjs`.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { merge, toJudge } from "./judge.mjs";

const step = (id, more = {}) => ({
  id,
  title: `Paso ${id}`,
  goal: `Prueba ${id}.`,
  instructions: [`Abre ${id}.`],
  commands: [],
  expect: [`Se ve ${id}.`],
  covers: ["#268"],
  ...more,
});
const plan = (...steps) => ({ title: "UAT", language: "es", sections: [{ id: "7", title: "History", steps }] });
const result = (id, more = {}) => ({
  id,
  status: "ok",
  notes: `Comprobado (1/1): sale «${id}»`,
  time: "2026-10-27T10:00:00.000Z",
  version: "0.1.0",
  by: "local",
  evidence: `uat/auto/webdriver/steps · ${id}`,
  checks: [{ ok: true, what: `sale «${id}»` }],
  looks: [],
  ...more,
});
// A plan step's automation entry that names the agent judge.
const judged = (id) => ({ tool: "agent-judge", where: "local", test: `uat/auto/webdriver/steps/history.mjs > ${id}` });

// A shots folder holding the files named, each a stand-in for a PNG, gone
// when test `t` ends.
function shotsWith(t, ...names) {
  const dir = mkdtempSync(join(tmpdir(), "uat-judge-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  for (const name of names) writeFileSync(join(dir, name), "png");
  return dir;
}

// The walk's results, a shots folder holding `names`, and their listing.
function walked(t, results, names, steps = results.map((r) => step(r.id))) {
  const shots = shotsWith(t, ...names);
  return { shots, results, listing: toJudge(plan(...steps), results, shots) };
}

test("the listing takes each result with looks, with its plan fields, the walk's verdict and each shot's absolute path", (t) => {
  const shots = shotsWith(t, "7.5-1-botones.png");
  const results = [
    result("7.4"),
    result("7.5", { looks: [{ what: "los cuatro botones comparten un rojo", file: "7.5-1-botones.png" }] }),
  ];
  assert.deepEqual(toJudge(plan(step("7.4"), step("7.5")), results, shots), [
    {
      id: "7.5",
      title: "Paso 7.5",
      goal: "Prueba 7.5.",
      instructions: ["Abre 7.5."],
      expect: ["Se ve 7.5."],
      walk: { status: "ok", checks: [{ ok: true, what: "sale «7.5»" }], notes: "Comprobado (1/1): sale «7.5»" },
      looks: [{ what: "los cuatro botones comparten un rojo", file: join(shots, "7.5-1-botones.png") }],
    },
  ]);
});

test("the listing marks a shot missing on disk, and the step as one the judge can't judge", (t) => {
  const shots = shotsWith(t, "4.1-1-lista.png");
  const results = [
    result("4.1", {
      looks: [
        { what: "la barra de scroll fina", file: "4.1-1-lista.png" },
        { what: "la barra de scroll al pasar el ratón", file: "4.1-2-hover.png" },
      ],
    }),
  ];
  const [listed] = toJudge(plan(step("4.1")), results, shots);
  assert.deepEqual(listed.looks, [
    { what: "la barra de scroll fina", file: join(shots, "4.1-1-lista.png") },
    { what: "la barra de scroll al pasar el ratón", file: join(shots, "4.1-2-hover.png"), missing: true },
  ]);
  assert.equal(listed.unjudged, `falta la captura ${join(shots, "4.1-2-hover.png")}`);
});

test("the listing marks a step the walk Blocked as one the judge can't judge", (t) => {
  const shots = shotsWith(t, "2.1-1-error.png");
  const results = [result("2.1", { status: "blocked", looks: [{ what: "la ventana al fallar", file: "2.1-1-error.png" }] })];
  assert.equal(toJudge(plan(step("2.1")), results, shots)[0].unjudged, "el walk dejó el paso bloqueado");
});

test("the listing takes a step whose automation lists agent-judge, unjudged when the walk took no shot of it", (t) => {
  const shots = shotsWith(t);
  const listed = toJudge(plan(step("7.4"), step("7.6", { automation: [judged("7.6")] })), [result("7.4"), result("7.6")], shots);
  assert.deepEqual(
    listed.map(({ id, looks, unjudged }) => ({ id, looks, unjudged })),
    [{ id: "7.6", looks: [], unjudged: "el walk no tomó ninguna captura que juzgar" }],
  );
});

test("the listing leaves out a result the judge already judged", (t) => {
  const shots = shotsWith(t, "7.5-1-botones.png");
  const results = [result("7.5", { by: "agent-judge", looks: [{ what: "un rojo", file: "7.5-1-botones.png" }] })];
  assert.deepEqual(toJudge(plan(step("7.5", { automation: [judged("7.5")] })), results, shots), []);
});

test("a verdict merges with by agent-judge, its status and evidence, the walk's notes and the judge's under Juez", (t) => {
  const looks = [{ what: "los cuatro botones comparten un rojo", file: "7.5-1-botones.png" }];
  const { shots, results, listing } = walked(t, [result("7.4"), result("7.5", { looks })], ["7.5-1-botones.png"]);
  const shot = join(shots, "7.5-1-botones.png");
  const notes = "Delete backup en History es violeta, no rojo: expect dice que los cuatro botones comparten un rojo. Captura 7.5-1-botones.png.";
  const merged = merge(results, listing, [{ id: "7.5", status: "ko", notes, evidence: shot }]);
  assert.deepEqual(merged, [
    result("7.4"),
    result("7.5", {
      looks,
      status: "ko",
      by: "agent-judge",
      evidence: shot,
      notes: `Comprobado (1/1): sale «7.5»\nJuez:\n${notes}`,
    }),
  ]);
  assert.equal(results[1].by, "local", "the walk's results are left as they were");
});

test("a verdict for a step the listing didn't list is refused, and nothing merges", (t) => {
  const looks = [{ what: "un rojo", file: "7.5-1-botones.png" }];
  const { shots, results, listing } = walked(t, [result("7.4"), result("7.5", { looks })], ["7.5-1-botones.png"]);
  const ok = { id: "7.5", status: "ok", notes: "Los botones son rojos.", evidence: join(shots, "7.5-1-botones.png") };
  assert.throws(
    () => merge(results, listing, [ok, { id: "7.4", status: "ok", notes: "Bien.", evidence: "" }]),
    /7\.4 is no step the listing listed/,
  );
});

test("a listed step with no verdict, or with two, is refused, so no step keeps an unjudged OK", (t) => {
  const { shots, results, listing } = walked(t, [result("7.5", { looks: [{ what: "un rojo", file: "7.5-1-botones.png" }] })], ["7.5-1-botones.png"]);
  assert.throws(() => merge(results, listing, []), /no verdict for 7\.5/);
  const ok = { id: "7.5", status: "ok", notes: "", evidence: join(shots, "7.5-1-botones.png") };
  assert.throws(() => merge(results, listing, [ok, { ...ok, status: "ko", notes: "No es rojo." }]), /7\.5 has two verdicts/);
});

test("a step whose shot is missing merges Blocked, whatever the judge said, with the missing shot as evidence", (t) => {
  const looks = [
    { what: "la barra fina", file: "4.1-1-lista.png" },
    { what: "la barra al pasar el ratón", file: "4.1-2-hover.png" },
  ];
  const { shots, results, listing } = walked(t, [result("4.1", { looks })], ["4.1-1-lista.png"]);
  const missing = join(shots, "4.1-2-hover.png");
  const [merged] = merge(results, listing, [{ id: "4.1", status: "ok", notes: "La barra es fina.", evidence: join(shots, "4.1-1-lista.png") }]);
  assert.equal(merged.status, "blocked");
  assert.equal(merged.by, "agent-judge");
  assert.equal(merged.evidence, missing);
  assert.equal(merged.notes, `Comprobado (1/1): sale «4.1»\nJuez:\nBloqueado: falta la captura ${missing}\nLa barra es fina.`);
});

test("a step the walk Blocked, or took no shot of, stays Blocked with the walk's evidence", (t) => {
  const steps = [step("2.1"), step("7.6", { automation: [judged("7.6")] })];
  const blocked = result("2.1", { status: "blocked", notes: "Bloqueado: antes de sus comprobaciones", looks: [{ what: "la ventana", file: "2.1-1-error.png" }] });
  const { shots, results, listing } = walked(t, [blocked, result("7.6")], ["2.1-1-error.png"], steps);
  const merged = merge(results, listing, [
    { id: "2.1", status: "ok", notes: "", evidence: join(shots, "2.1-1-error.png") },
    { id: "7.6", status: "ok", notes: "", evidence: "" },
  ]);
  assert.deepEqual(
    merged.map(({ status, notes, evidence }) => ({ status, notes, evidence })),
    [
      { status: "blocked", notes: "Bloqueado: antes de sus comprobaciones\nJuez:\nBloqueado: el walk dejó el paso bloqueado", evidence: "uat/auto/webdriver/steps · 2.1" },
      { status: "blocked", notes: "Comprobado (1/1): sale «7.6»\nJuez:\nBloqueado: el walk no tomó ninguna captura que juzgar", evidence: "uat/auto/webdriver/steps · 7.6" },
    ],
  );
});

test("a walk KO stays KO when the judge finds the look OK, with the judge's findings added", (t) => {
  const looks = [{ what: "un rojo", file: "7.5-1-botones.png" }];
  const ko = result("7.5", { status: "ko", notes: "KO: sale «Delete backup»", looks });
  const { shots, results, listing } = walked(t, [ko], ["7.5-1-botones.png"]);
  const shot = join(shots, "7.5-1-botones.png");
  const [merged] = merge(results, listing, [{ id: "7.5", status: "ok", notes: "Los cuatro botones son rojos.", evidence: shot }]);
  assert.deepEqual(
    { status: merged.status, by: merged.by, evidence: merged.evidence, notes: merged.notes },
    { status: "ko", by: "agent-judge", evidence: shot, notes: "KO: sale «Delete backup»\nJuez:\nLos cuatro botones son rojos." },
  );
});

test("a walk KO the judge can't judge, with no shot or a missing one, stays KO and says so", (t) => {
  const steps = [step("7.5"), step("7.6", { automation: [judged("7.6")] })];
  const missing = result("7.5", { status: "ko", notes: "KO: sale «Delete backup»", looks: [{ what: "un rojo", file: "7.5-1-botones.png" }] });
  const { shots, results, listing } = walked(t, [missing, result("7.6", { status: "ko", notes: "KO: sale «7.6»" })], [], steps);
  const merged = merge(results, listing, [
    { id: "7.5", status: "ok", notes: "", evidence: "" },
    { id: "7.6", status: "ok", notes: "", evidence: "" },
  ]);
  assert.deepEqual(
    merged.map(({ status, notes, evidence }) => ({ status, notes, evidence })),
    [
      { status: "ko", notes: `KO: sale «Delete backup»\nJuez:\nSin juzgar: falta la captura ${join(shots, "7.5-1-botones.png")}`, evidence: join(shots, "7.5-1-botones.png") },
      { status: "ko", notes: "KO: sale «7.6»\nJuez:\nSin juzgar: el walk no tomó ninguna captura que juzgar", evidence: "uat/auto/webdriver/steps · 7.6" },
    ],
  );
});

test("a verdict is refused when its status is unknown, a KO has no notes, or its evidence names no shot of the step", (t) => {
  const looks = [{ what: "un rojo", file: "7.5-1-botones.png" }];
  const { shots, results, listing } = walked(t, [result("7.5", { looks })], ["7.5-1-botones.png"]);
  const shot = join(shots, "7.5-1-botones.png");
  const refused = (verdict) => assert.throws(() => merge(results, listing, [{ id: "7.5", ...verdict }]), /7\.5/);
  refused({ status: "bien", notes: "", evidence: shot });
  refused({ status: "ko", notes: "", evidence: shot });
  refused({ status: "ok", notes: "", evidence: "" });
  refused({ status: "ok", notes: "", evidence: "data:image/png;base64,iVBORw0KGgo=" });
  refused({ status: "ok", notes: "", evidence: `${shot}; ${join(shots, "7.4-1-otro.png")}` });
  assert.equal(merge(results, listing, [{ id: "7.5", status: "ok", notes: "", evidence: shot }])[0].status, "ok");
});

test("evidence that spells a shot's path with / or another case names that shot, as the listing spells it", (t) => {
  const looks = [{ what: "un rojo", file: "7.5-1-botones.png" }];
  const { shots, results, listing } = walked(t, [result("7.5", { looks })], ["7.5-1-botones.png"]);
  const shot = join(shots, "7.5-1-botones.png");
  const [merged] = merge(results, listing, [{ id: "7.5", status: "ok", notes: "", evidence: shot.replaceAll("\\", "/").toUpperCase() }]);
  assert.equal(merged.evidence, shot);
});

test("the command lists the work folder's steps to judge, then merges a verdicts file into its webdriver.json", (t) => {
  const real = JSON.parse(readFileSync(new URL("../../plan.json", import.meta.url), "utf8")).sections[0].steps[0];
  const work = shotsWith(t);
  mkdirSync(join(work, "shots"));
  writeFileSync(join(work, "shots", `${real.id}-1-vista.png`), "png");
  const looks = [{ what: "la vista", file: `${real.id}-1-vista.png` }];
  writeFileSync(join(work, "webdriver.json"), JSON.stringify([result(real.id, { looks })]));
  const judge = (...args) => execFileSync(process.execPath, [fileURLToPath(new URL("./judge.mjs", import.meta.url)), ...args, "--work", work], { encoding: "utf8", stdio: "pipe" });

  const [listed] = JSON.parse(judge("list"));
  assert.deepEqual({ id: listed.id, title: listed.title, looks: listed.looks }, { id: real.id, title: real.title, looks: [{ what: "la vista", file: join(work, "shots", `${real.id}-1-vista.png`) }] });

  const verdicts = join(work, "verdicts.json");
  writeFileSync(verdicts, JSON.stringify([{ id: "99.9", status: "ok", notes: "", evidence: "" }]));
  assert.throws(() => judge("merge", "--verdicts", verdicts), /99\.9 is no step the listing listed/);
  assert.equal(JSON.parse(readFileSync(join(work, "webdriver.json"), "utf8"))[0].by, "local", "a refused merge writes nothing");

  writeFileSync(verdicts, JSON.stringify([{ id: real.id, status: "ok", notes: "Se ve bien.", evidence: listed.looks[0].file }]));
  assert.match(judge("merge", "--verdicts", verdicts), /: 1 OK, 0 KO, 0 Blocked/);
  const [merged] = JSON.parse(readFileSync(join(work, "webdriver.json"), "utf8"));
  assert.deepEqual({ by: merged.by, status: merged.status, evidence: merged.evidence }, { by: "agent-judge", status: "ok", evidence: listed.looks[0].file });
  assert.deepEqual(JSON.parse(judge("list")), [], "a judged step is listed no more");
});

test("in the local script's work folder, the merge writes the judged results into its uat-results.json too", (t) => {
  const real = JSON.parse(readFileSync(new URL("../../plan.json", import.meta.url), "utf8")).sections[0].steps[0];
  const work = shotsWith(t);
  mkdirSync(join(work, "shots"));
  mkdirSync(join(work, "sandbox"));
  writeFileSync(join(work, "shots", `${real.id}-1-vista.png`), "png");
  writeFileSync(join(work, "webdriver.json"), JSON.stringify([result(real.id, { looks: [{ what: "la vista", file: `${real.id}-1-vista.png` }] })]));
  writeFileSync(join(work, "sandbox", "uat-results.json"), "[]");
  writeFileSync(join(work, "uat-results.json"), JSON.stringify([result(real.id)]));
  const verdicts = join(work, "verdicts.json");
  const shot = join(work, "shots", `${real.id}-1-vista.png`);
  writeFileSync(verdicts, JSON.stringify([{ id: real.id, status: "ok", notes: "Se ve bien.", evidence: shot }]));
  execFileSync(process.execPath, [fileURLToPath(new URL("./judge.mjs", import.meta.url)), "merge", "--verdicts", verdicts, "--work", work], { stdio: "pipe" });
  const [published] = JSON.parse(readFileSync(join(work, "uat-results.json"), "utf8"));
  assert.deepEqual({ by: published.by, evidence: published.evidence, looks: published.looks }, { by: "agent-judge", evidence: shot, looks: undefined });
});

test("a merge whose uat-results.json can't be written again writes nothing, not even webdriver.json", (t) => {
  const real = JSON.parse(readFileSync(new URL("../../plan.json", import.meta.url), "utf8")).sections[0].steps[0];
  const work = shotsWith(t);
  mkdirSync(join(work, "shots"));
  writeFileSync(join(work, "shots", `${real.id}-1-vista.png`), "png");
  writeFileSync(join(work, "webdriver.json"), JSON.stringify([result(real.id, { looks: [{ what: "la vista", file: `${real.id}-1-vista.png` }] })]));
  // The local script's file, with no Sandbox results beside it.
  writeFileSync(join(work, "uat-results.json"), JSON.stringify([result(real.id)]));
  const verdicts = join(work, "verdicts.json");
  writeFileSync(verdicts, JSON.stringify([{ id: real.id, status: "ok", notes: "Se ve bien.", evidence: join(work, "shots", `${real.id}-1-vista.png`) }]));
  assert.throws(() => execFileSync(process.execPath, [fileURLToPath(new URL("./judge.mjs", import.meta.url)), "merge", "--verdicts", verdicts, "--work", work], { stdio: "pipe" }));
  assert.equal(JSON.parse(readFileSync(join(work, "webdriver.json"), "utf8"))[0].by, "local");
  assert.equal(JSON.parse(readFileSync(join(work, "uat-results.json"), "utf8"))[0].by, "local");
});
