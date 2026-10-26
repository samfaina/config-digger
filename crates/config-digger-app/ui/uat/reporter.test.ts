import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { TestCase, TestResult, TestStep } from "@playwright/test/reporter";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import UatResults from "./reporter";

const RUN = "https://github.com/samfaina/config-digger/actions/runs/42";
const START = new Date("2026-10-26T09:30:00.000Z");
const CHECK = "En ese panel, un aviso con una rueda que gira dice «Reading the inventory…».";

let dir: string;
let file: string;
beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), "uat-results-"));
  file = join(dir, "uat-results.json");
});
afterEach(() => rmSync(dir, { recursive: true, force: true }));

function testCase(id: string, annotations: TestCase["annotations"] = []): TestCase {
  return {
    title: id,
    titlePath: () => ["", "", "loading.uat.ts", id],
    annotations,
  } as unknown as TestCase;
}

function step(title: string, error?: string): TestStep {
  return { title, category: "test.step", error: error && { message: error }, steps: [] } as unknown as TestStep;
}

function result(status: TestResult["status"], { steps = [], error }: { steps?: TestStep[]; error?: string } = {}): TestResult {
  return { status, startTime: START, duration: 1500, steps, error: error && { message: error } } as unknown as TestResult;
}

/** The documents a run of `tests` writes. */
async function run(tests: [TestCase, TestResult][], runUrl?: string) {
  const reporter = new UatResults({ file, version: "0.1.0-rc.3", runUrl });
  for (const [each, outcome] of tests) reporter.onTestEnd(each, outcome);
  const end = await reporter.onEnd();
  return { end, documents: JSON.parse(readFileSync(file, "utf8")) };
}

describe("the UAT results of a run", () => {
  test("say a step whose test passed is OK, by CI, with the run as evidence", async () => {
    const { documents } = await run([[testCase("15.1"), result("passed", { steps: [step(CHECK)] })]], RUN);

    expect(documents).toEqual([
      { id: "15.1", status: "ok", notes: "", time: "2026-10-26T09:30:01.500Z", version: "0.1.0-rc.3", by: "ci", evidence: RUN },
    ]);
  });

  test("give the test's name as evidence outside CI", async () => {
    const { documents } = await run([[testCase("15.1"), result("passed", { steps: [step(CHECK)] })]]);

    expect(documents[0].evidence).toBe("loading.uat.ts › 15.1");
  });

  test("say a step whose check failed is KO, with the check and the failed assertion's message", async () => {
    const message = "\u001b[31mexpect(locator).toHaveText(expected)\u001b[39m\n\nExpected: \"Reading the inventory…\"";
    const failed = result("failed", { steps: [step("Antes de que salgan los paneles, el cuerpo es un único panel."), step(CHECK, message)], error: message });

    const { documents } = await run([[testCase("15.1"), failed]]);

    expect(documents[0]).toMatchObject({
      status: "ko",
      notes: `${CHECK}\nexpect(locator).toHaveText(expected)\n\nExpected: "Reading the inventory…"`,
    });
  });

  test("say a step whose test failed before its first check is Blocked, with the error", async () => {
    const { documents } = await run([[testCase("15.1"), result("timedOut", { error: "Test timeout of 30000ms exceeded." })]]);

    expect(documents[0]).toMatchObject({ status: "blocked", notes: "Test timeout of 30000ms exceeded." });
  });

  test("name the checks a passed step leaves to a person", async () => {
    const left = [
      { type: "left to a person", description: "Entra subiendo un poco mientras aparece, en un instante y sin saltos." },
      { type: "left to a person", description: "Se queda unos 4 segundos y se va bajando un poco mientras se desvanece." },
    ];

    const { documents } = await run([[testCase("5.1", left), result("passed", { steps: [step(CHECK)] })]]);

    expect(documents[0]).toMatchObject({
      status: "ok",
      notes:
        "Queda para una persona:\n- Entra subiendo un poco mientras aparece, en un instante y sin saltos.\n- Se queda unos 4 segundos y se va bajando un poco mientras se desvanece.",
    });
  });

  test("write nothing of a skipped test", async () => {
    const { documents } = await run([
      [testCase("15.1"), result("skipped")],
      [testCase("15.2"), result("passed", { steps: [step(CHECK)] })],
    ]);

    expect(documents.map((document: { id: string }) => document.id)).toEqual(["15.2"]);
  });

  test("fail the run when two tests walk the same step", async () => {
    const { end } = await run([
      [testCase("15.1"), result("passed", { steps: [step(CHECK)] })],
      [testCase("15.1"), result("passed", { steps: [step(CHECK)] })],
    ]);

    expect(end).toEqual({ status: "failed" });
  });
});
