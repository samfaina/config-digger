/**
 * Writes `uat-results.json`: one document per UAT step a test walked, in the
 * shape of the runner's `results` (`docs/agents/uat.md`), with `by: "ci"`.
 * A step is OK when its test passed, KO when one of its checks failed, with
 * the check and the failed assertion's message as notes, and Blocked when its
 * test failed before its first check, such as on loading the page.
 */
import { writeFileSync } from "node:fs";
import type { FullResult, Reporter, TestCase, TestResult } from "@playwright/test/reporter";
import { LEFT_TO_A_PERSON } from "./plan";

type Options = {
  /** Where the documents go. */
  file: string;
  /** The workspace version. */
  version: string;
  /** The CI run's URL, the evidence of each step; the test's name is, outside CI. */
  runUrl?: string;
};

type Document = {
  id: string;
  status: "ok" | "ko" | "blocked";
  notes: string;
  time: string;
  version: string;
  by: "ci";
  evidence: string;
};

export default class UatResults implements Reporter {
  /** Each step's last document, as `--repeat-each` runs its test again. */
  private readonly documents = new Map<string, Document>();
  /** The tests of each step, by their title path: two of them walk the step twice. */
  private readonly tests = new Map<string, Set<string>>();

  constructor(private readonly options: Options) {}

  onTestEnd(test: TestCase, result: TestResult) {
    if (result.status === "skipped") return;
    const checks = result.steps.filter((step) => step.category === "test.step");
    const failed = checks.find((check) => check.error !== undefined);
    const [status, notes]: [Document["status"], string] =
      result.status === "passed"
        ? ["ok", leftToAPerson(test)]
        : checks.length === 0
          ? ["blocked", plain(result.error?.message)]
          : ["ko", [knownBug(test), failed?.title, plain(failed?.error?.message ?? result.error?.message)].filter(Boolean).join("\n")];
    const tests = this.tests.get(test.title) ?? new Set();
    this.tests.set(test.title, tests.add(test.titlePath().join(" › ")));
    this.documents.set(test.title, {
      id: test.title,
      status,
      notes,
      time: new Date(result.startTime.getTime() + result.duration).toISOString(),
      version: this.options.version,
      by: "ci",
      evidence: this.options.runUrl ?? test.titlePath().filter(Boolean).join(" › "),
    });
  }

  async onEnd(): Promise<{ status?: FullResult["status"] } | undefined> {
    writeFileSync(this.options.file, JSON.stringify([...this.documents.values()], null, 2) + "\n");
    const twice = [...this.tests].filter(([, tests]) => tests.size > 1).map(([id]) => id);
    if (twice.length > 0) {
      console.error(`More than one test walks step ${twice.join(", ")}.`);
      return { status: "failed" };
    }
    return undefined;
  }

  printsToStdio() {
    return false;
  }
}

/** The notes of a passed step: the checks its test leaves to a person, if any. */
function leftToAPerson(test: TestCase): string {
  const left = test.annotations.filter((annotation) => annotation.type === LEFT_TO_A_PERSON).map((annotation) => `- ${annotation.description}`);
  return left.length > 0 ? ["Queda para una persona:", ...left].join("\n") : "";
}

/** The bug a test is marked to fail by, `test.fail()`'s description, as the notes' first line. */
function knownBug(test: TestCase): string | undefined {
  const bug = test.annotations.find((annotation) => annotation.type === "fail")?.description;
  return bug === undefined ? undefined : `Fallo conocido: ${bug}`;
}

/** A message without the terminal's colour codes. */
function plain(message: string | undefined): string {
  return (message ?? "").replace(/\u001b\[[0-9;]*m/g, "");
}
