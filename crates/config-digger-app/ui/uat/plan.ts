/** The UAT plan, `uat/plan.json`, as the tests of its steps read it. */
import { readFileSync } from "node:fs";

export type PlanStep = { id: string; title: string; expect: string[] };

type Plan = { sections: { steps: PlanStep[] }[] };

/** The annotation of a check that a step's test leaves to a person. */
export const LEFT_TO_A_PERSON = "left to a person";

const plan: Plan = JSON.parse(readFileSync(new URL("../../../../uat/plan.json", import.meta.url), "utf8"));

/** The step of the plan whose id is `id`. */
export function planStep(id: string): PlanStep {
  const step = plan.sections.flatMap((section) => section.steps).find((each) => each.id === id);
  if (step === undefined) throw new Error(`${id} is no step of uat/plan.json.`);
  return step;
}
