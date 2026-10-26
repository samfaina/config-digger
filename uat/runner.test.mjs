// The runner's logic, the `uat-logic` script inside runner.html, and the
// words of its page, run by `node --test uat/runner.test.mjs`. The page
// itself stays one self-contained file.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const html = readFileSync(new URL("./runner.html", import.meta.url), "utf8");
const logic = html.match(/<script id="uat-logic">([\s\S]*?)<\/script>/)[1];
const Uat = new Function(`${logic}; return Uat;`)();

const NOW = new Date("2026-10-18T09:30:00Z");

test("a KO needs notes", () => {
  assert.throws(() => Uat.result("1.2", "ko", "  ", "0.1.0-rc.2", NOW), {
    message: "Escribe en las notas qué falló.",
  });
});

test("a Blocked step needs notes", () => {
  assert.throws(() => Uat.result("1.2", "blocked", "", "0.1.0-rc.2", NOW), {
    message: "Escribe en las notas qué lo bloquea.",
  });
});

test("an OK is stored with its id, notes, time and app version", () => {
  assert.deepEqual(Uat.result("1.1", "ok", "", "0.1.0-rc.2", NOW), {
    id: "1.1",
    status: "ok",
    notes: "",
    time: "2026-10-18T09:30:00.000Z",
    version: "0.1.0-rc.2",
  });
});

test("an OK keeps its notes", () => {
  assert.equal(Uat.result("1.1", "ok", " El título se corta\n", "0.1.0-rc.2", NOW).notes, "El título se corta");
});

test("KO or Blocked without notes waits as the step's pending verdict", () => {
  assert.deepEqual(Uat.choose("ko", " ", "ok"), { save: null, pending: "ko" });
  assert.deepEqual(Uat.choose("blocked", "", undefined), { save: null, pending: "blocked" });
});

test("KO or Blocked with notes saves on the first click, and OK always does", () => {
  assert.deepEqual(Uat.choose("ko", "Sale vacío", "ok"), { save: "ko", pending: null });
  assert.deepEqual(Uat.choose("blocked", "Espera a #175", undefined), { save: "blocked", pending: null });
  assert.deepEqual(Uat.choose("ok", "", "ko"), { save: "ok", pending: null });
});

test("the step's saved verdict without notes drops the pending one and saves nothing", () => {
  assert.deepEqual(Uat.choose("ko", "", "ko"), { save: null, pending: null });
});

test("a pending verdict shows as chosen, unlike the saved one it would replace", () => {
  assert.equal(Uat.look("ko", "ko", "ok"), "pending");
  assert.equal(Uat.look("ok", "ko", "ok"), null);
  assert.equal(Uat.look("blocked", "ko", "ok"), null);
});

test("with nothing pending the saved verdict shows as chosen", () => {
  assert.equal(Uat.look("ok", null, "ok"), "saved");
  assert.equal(Uat.look("ko", null, "ok"), null);
  assert.equal(Uat.look("ok", null, undefined), null);
});

/** A plan of two sections, three steps. */
const PLAN = {
  title: "Prueba",
  language: "es",
  sections: [
    {
      id: "1",
      title: "Inventario",
      steps: [
        { id: "1.1", title: "Abrir la app" },
        { id: "1.2", title: "Añadir el home" },
      ],
    },
    { id: "2", title: "History", steps: [{ id: "2.1", title: "Deshacer" }] },
  ],
};

/** Results keyed by step id, as the runner keeps them. */
function results(...docs) {
  return Object.fromEntries(docs.map((doc) => [doc.id, doc]));
}

test("progress counts the plan's steps that have a result, by status", () => {
  const done = results(
    Uat.result("1.2", "ok", "", "0.1.0-rc.2", NOW),
    Uat.result("2.1", "blocked", "Espera a #175", "0.1.0-rc.2", NOW),
    Uat.result("9.9", "ok", "", "0.1.0-rc.2", NOW),
  );
  assert.deepEqual(Uat.progress(PLAN, done), {
    done: 2,
    total: 3,
    counts: { ok: 1, ko: 0, blocked: 1 },
    toWalk: 1,
    unrun: 0,
  });
});

test("progress tells the automated steps no tool has a result for apart from the steps left to walk", () => {
  assert.deepEqual(Uat.progress(AUTOMATED, {}), { done: 0, total: 3, counts: { ok: 0, ko: 0, blocked: 0 }, toWalk: 2, unrun: 1 });
  const done = results(Uat.result("1.2", "ok", "", "0.1.0-rc.2", NOW));
  assert.deepEqual(Uat.progress(AUTOMATED, done), { done: 1, total: 3, counts: { ok: 1, ko: 0, blocked: 0 }, toWalk: 2, unrun: 0 });
});

test("the export is a Markdown table of every step with notes, whatever its verdict, in plan order, naming what wrote a tool's verdict", () => {
  const done = results(
    { ...Uat.result("2.1", "blocked", "Espera a #175", "0.1.0-rc.2", NOW), by: "ci", evidence: "https://github.com/o/r/actions/runs/1" },
    Uat.result("1.1", "ok", "Funciona, pero el título se corta", "0.1.0-rc.2", NOW),
    Uat.result("1.2", "ko", "Sale vacío | sin servidores\nNi skills", "0.1.0-rc.2", NOW),
  );
  assert.equal(
    Uat.exportMarkdown(PLAN, done),
    [
      "| Paso | Título | Estado | Por | Notas |",
      "| --- | --- | --- | --- | --- |",
      "| 1.1 | Abrir la app | OK |  | Funciona, pero el título se corta |",
      "| 1.2 | Añadir el home | KO |  | Sale vacío \\| sin servidores<br>Ni skills |",
      "| 2.1 | Deshacer | Bloqueado | CI | Espera a #175 |",
    ].join("\n"),
  );
});

test("the export leaves out an OK without notes", () => {
  const done = results(
    Uat.result("1.1", "ok", "", "0.1.0-rc.2", NOW),
    Uat.result("2.1", "ok", "El atajo no sale en el menú", "0.1.0-rc.2", NOW),
  );
  assert.equal(
    Uat.exportMarkdown(PLAN, done),
    [
      "| Paso | Título | Estado | Por | Notas |",
      "| --- | --- | --- | --- | --- |",
      "| 2.1 | Deshacer | OK |  | El atajo no sale en el menú |",
    ].join("\n"),
  );
});

test("the export says so when no step has notes", () => {
  const done = results(Uat.result("1.1", "ok", "", "0.1.0-rc.2", NOW));
  assert.equal(Uat.exportMarkdown(PLAN, done), "Ningún paso con notas.");
});

test("a check sets its exact UI text apart from the words around it", () => {
  assert.deepEqual(Uat.checkParts("Sale el aviso «Copied the path.» y luego «Done»."), [
    { text: "Sale el aviso ", ui: false },
    { text: "Copied the path.", ui: true },
    { text: " y luego ", ui: false },
    { text: "Done", ui: true },
    { text: ".", ui: false },
  ]);
});

test("a check without exact UI text is one plain part", () => {
  assert.deepEqual(Uat.checkParts("El diálogo cabe en la ventana."), [
    { text: "El diálogo cabe en la ventana.", ui: false },
  ]);
});

/** The words of the page's element with this id, as its markup or the
 * render script's `el(tag, { …, id }, words)` writes them. */
function pageText(id) {
  const found =
    html.match(new RegExp(`id="${id}"[^>]*>([^<]*)<`)) ??
    html.match(new RegExp(`id: "${id}" }, "([^"]*)"`));
  assert.ok(found, `runner.html has no element with id "${id}" and words of its own`);
  return found[1];
}

test("the notes hint says notes on an OK step are read too", () => {
  assert.equal(
    pageText("notes-hint"),
    "Obligatorias en KO y Bloqueado: qué viste y qué esperabas. En OK son opcionales y también se leen: escríbelas antes de pulsar OK.",
  );
});

test("the checklist says its ticks are only a guide", () => {
  assert.equal(pageText("checks-hint"), "Marca cada punto en cuanto lo veas. Las marcas solo te guían: no se guardan.");
});

test("the Export button is named for the steps with notes it copies", () => {
  assert.equal(pageText("export"), "Exportar pasos con notas");
});

test("after a result the runner moves on to the next step without one", () => {
  const done = results(
    Uat.result("1.1", "ok", "", "0.1.0-rc.2", NOW),
    Uat.result("2.1", "ok", "", "0.1.0-rc.2", NOW),
  );
  assert.equal(Uat.nextStep(PLAN, done, "2.1"), "1.2");
  assert.equal(Uat.nextStep(PLAN, done, "1.1"), "1.2");
});

test("once every step has a result the runner stays on the step", () => {
  const done = results(
    Uat.result("1.1", "ok", "", "0.1.0-rc.2", NOW),
    Uat.result("1.2", "ok", "", "0.1.0-rc.2", NOW),
    Uat.result("2.1", "ok", "", "0.1.0-rc.2", NOW),
  );
  assert.equal(Uat.nextStep(PLAN, done, "1.2"), "1.2");
});

/** A plan whose 1.2 a tool walks, and whose 2.1 a tool walks but a person
 * still checks. */
const AUTOMATED = {
  ...PLAN,
  sections: [
    {
      ...PLAN.sections[0],
      steps: [
        PLAN.sections[0].steps[0],
        { id: "1.2", title: "Añadir el home", automation: [{ tool: "webdriver", where: "local", test: "uat/auto/webdriver/steps/data.mjs > 1.2" }] },
      ],
    },
    {
      ...PLAN.sections[1],
      steps: [
        {
          id: "2.1",
          title: "Deshacer",
          automation: [{ tool: "webdriver", where: "local", test: "uat/auto/webdriver/steps/history.mjs > 2.1" }],
          manual: "Que el aviso entre sin saltos.",
        },
      ],
    },
  ],
};

test("the runner moves on past a step a tool walks, with or without its result, but not past one a person still checks", () => {
  assert.equal(Uat.nextStep(AUTOMATED, {}, "1.1"), "2.1");
  assert.equal(Uat.nextStep(AUTOMATED, results(Uat.result("1.2", "ok", "", "0.1.0-rc.2", NOW)), "1.1"), "2.1");
  assert.equal(Uat.nextStep(AUTOMATED, results(Uat.result("2.1", "ok", "", "0.1.0-rc.2", NOW)), "1.1"), "1.1");
});

test("the first step shown is the first in the queue, past the steps a tool walks", () => {
  const first = { ...AUTOMATED, sections: [AUTOMATED.sections[0], AUTOMATED.sections[1]] };
  first.sections[0] = { ...first.sections[0], steps: [...first.sections[0].steps].reverse() };
  assert.equal(Uat.firstStep(first, {}), "1.1");
  assert.equal(Uat.firstStep(first, results(Uat.result("1.1", "ok", "", "0.1.0-rc.2", NOW))), "2.1");
});

test("with the queue empty the first step shown is the plan's first", () => {
  const done = results(Uat.result("1.1", "ok", "", "0.1.0-rc.2", NOW), Uat.result("2.1", "ko", "Salta", "0.1.0-rc.2", NOW));
  assert.equal(Uat.firstStep(AUTOMATED, done), "1.1");
});

test("a result a tool wrote names it, and a tester's names nobody", () => {
  const by = (who) => ({ ...Uat.result("1.2", "ok", "", "0.1.0-rc.2", NOW), by: who });
  assert.equal(Uat.byLabel(by("ci")), "CI");
  assert.equal(Uat.byLabel(by("local")), "Local");
  assert.equal(Uat.byLabel(by("agent-judge")), "Agente juez");
  assert.equal(Uat.byLabel(Uat.result("1.2", "ok", "", "0.1.0-rc.2", NOW)), null);
  assert.equal(Uat.byLabel(undefined), null);
});

/** A result the agent judge wrote from the local walk's screenshots. */
function judged(id, status, notes) {
  return { ...Uat.result(id, status, notes, "0.1.0-rc.2", NOW), by: "agent-judge", evidence: `uat/auto/webdriver/shots/${id}-1-dialog.png` };
}

test("Revisar lists the agent judge's KOs in plan order, and no other result", () => {
  const done = results(
    judged("2.1", "ko", "El diálogo tapa el botón"),
    judged("1.1", "ko", "La sombra se corta"),
    judged("1.2", "ok", ""),
  );
  assert.deepEqual(
    Uat.toReview(PLAN, done).map(({ step, result }) => [step.id, step.title, result.notes, result.evidence]),
    [
      ["1.1", "Abrir la app", "La sombra se corta", "uat/auto/webdriver/shots/1.1-1-dialog.png"],
      ["2.1", "Deshacer", "El diálogo tapa el botón", "uat/auto/webdriver/shots/2.1-1-dialog.png"],
    ],
  );
  const others = results(
    { ...Uat.result("1.1", "ko", "Sale vacío", "0.1.0-rc.2", NOW), by: "local" },
    { ...Uat.result("1.2", "ko", "Sale vacío", "0.1.0-rc.2", NOW), by: "ci" },
    Uat.result("2.1", "ko", "Sale vacío", "0.1.0-rc.2", NOW),
  );
  assert.deepEqual(Uat.toReview(PLAN, others), []);
});

test("Revisar leaves out a judged KO that has a tester's verdict", () => {
  const done = results(Uat.result("1.1", "ok", "Se ve bien", "0.1.0-rc.2", NOW, judged("1.1", "ko", "La sombra se corta")));
  assert.deepEqual(Uat.toReview(PLAN, done), []);
});

test("a tester's verdict on a judged step stores no by and keeps the judge's verdict as judged", () => {
  const judge = judged("1.1", "ko", "La sombra se corta");
  assert.deepEqual(Uat.result("1.1", "ok", " Se ve bien ", "0.1.0-rc.2", NOW, judge), {
    id: "1.1",
    status: "ok",
    notes: "Se ve bien",
    time: "2026-10-18T09:30:00.000Z",
    version: "0.1.0-rc.2",
    judged: { status: "ko", notes: "La sombra se corta" },
  });
});

test("a tester's KO on a judged step still needs notes", () => {
  assert.throws(() => Uat.result("1.1", "ko", "", "0.1.0-rc.2", NOW, judged("1.1", "ko", "La sombra se corta")), {
    message: "Escribe en las notas qué falló.",
  });
});

test("the judge's verdict stays when the tester saves the step again, and no other tool's verdict is kept", () => {
  const first = Uat.result("1.1", "ok", "", "0.1.0-rc.2", NOW, judged("1.1", "ko", "La sombra se corta"));
  assert.deepEqual(Uat.result("1.1", "ko", "Ahora falla", "0.1.0-rc.2", NOW, first).judged, { status: "ko", notes: "La sombra se corta" });
  const local = { ...Uat.result("1.1", "ko", "Sale vacío", "0.1.0-rc.2", NOW), by: "local" };
  assert.equal("judged" in Uat.result("1.1", "ok", "", "0.1.0-rc.2", NOW, local), false);
});

test("a step a tool walks says it is out of the tester's walk, and may still be walked", () => {
  assert.equal(
    pageText("automated-hint"),
    "Este paso lo recorre una herramienta, así que no está en tu recorrido. Puedes recorrerlo igualmente: tu veredicto sustituye al suyo.",
  );
});

test("Revisar says the tester's verdict replaces the judge's, and what it needs", () => {
  assert.equal(
    pageText("review-hint"),
    "El agente juez los marcó KO al mirar las capturas del walk local. Abre la captura en ese PC y guarda tu veredicto: sustituye al del juez, que queda guardado aparte. KO y Bloqueado necesitan notas.",
  );
});
