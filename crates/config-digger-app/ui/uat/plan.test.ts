import { describe, expect, test } from "vitest";
import { planStep } from "./plan";

describe("a UAT step of the plan", () => {
  test("is found by its id, with its checks", () => {
    const step = planStep("15.1");

    expect(step.expect).toContain("En ese panel, un aviso con una rueda que gira dice «Reading the inventory…».");
  });

  test("is no step at all when uat/plan.json has no step of that id", () => {
    expect(() => planStep("15.99")).toThrow("15.99 is no step of uat/plan.json.");
  });
});
