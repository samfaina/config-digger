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
  private readonly documents: Document[] = [];

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
          : ["ko", [failed?.title, plain(failed?.error?.message ?? result.error?.message)].filter(Boolean).join("\n")];
    this.documents.push({
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
    writeFileSync(this.options.file, JSON.stringify(this.documents, null, 2) + "\n");
    const ids = this.documents.map((document) => document.id);
    const twice = ids.filter((id, at) => ids.indexOf(id) !== at);
    if (twice.length > 0) {
      console.error(`More than one test walks step ${[...new Set(twice)].join(", ")}.`);
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

/** A message without the terminal's colour codes. */
function plain(message: string | undefined): string {
  return (message ?? "").replace(/\u001b\[[0-9;]*m/g, "");
}
