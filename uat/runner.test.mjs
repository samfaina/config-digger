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
  });
});

test("the export is a Markdown table of every step with notes, whatever its verdict, in plan order", () => {
  const done = results(
    Uat.result("2.1", "blocked", "Espera a #175", "0.1.0-rc.2", NOW),
    Uat.result("1.1", "ok", "Funciona, pero el título se corta", "0.1.0-rc.2", NOW),
    Uat.result("1.2", "ko", "Sale vacío | sin servidores\nNi skills", "0.1.0-rc.2", NOW),
  );
  assert.equal(
    Uat.exportMarkdown(PLAN, done),
    [
      "| Paso | Título | Estado | Notas |",
      "| --- | --- | --- | --- |",
      "| 1.1 | Abrir la app | OK | Funciona, pero el título se corta |",
      "| 1.2 | Añadir el home | KO | Sale vacío \\| sin servidores<br>Ni skills |",
      "| 2.1 | Deshacer | Bloqueado | Espera a #175 |",
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
      "| Paso | Título | Estado | Notas |",
      "| --- | --- | --- | --- |",
      "| 2.1 | Deshacer | OK | El atajo no sale en el menú |",
    ].join("\n"),
  );
});

test("the export says so when no step has notes", () => {
  const done = results(Uat.result("1.1", "ok", "", "0.1.0-rc.2", NOW));
  assert.equal(Uat.exportMarkdown(PLAN, done), "Ningún paso con notas.");
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
