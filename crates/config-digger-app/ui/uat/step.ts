/**
 * A test of one UAT step: `uatStep("15.1", async ({ page, open, check, leave }) => …)`.
 * The test is named after the step's id, which must be a step of
 * `uat/plan.json`. Each `check` asserts one check of the step's `expect`,
 * named by its text, and the test fails unless it checks every one of them or
 * says why it `leave`s it to a person.
 */
import { test as base, type Page } from "@playwright/test";
import { launch } from "../scripts/fakeUi.ts";
import { LEFT_TO_A_PERSON, planStep } from "./plan";

/** The page's query: the commands whose answer it holds, in milliseconds, and those that answer an error. */
export type Query = { delay?: Record<string, number>; fail?: string[] };

type StepArgs = {
  page: Page;
  /** Opens the UI on the fake backend, with `query` in the page's address. */
  open: (query?: Query) => Promise<void>;
  /** Asserts the check of the step's `expect` whose text is `text`. */
  check: (text: string, assert: () => Promise<void>) => Promise<void>;
  /** Leaves the check whose text is `text` to a person, for the reason `why`. */
  leave: (text: string, why: string) => void;
};

const test = base.extend<object, object>({
  // Edge, or Chrome, as `npm run screenshots` opens it.
  browser: [
    // Playwright reads the fixtures a fixture needs from its first parameter's pattern, an empty one here.
    async ({}, use) => {
      const browser = await launch();
      await use(browser);
      await browser.close();
    },
    { scope: "worker" },
  ],
});

/** The page's address for `query`. */
export function address(query: Query = {}): string {
  const base = process.env.FAKE_UI_URL;
  if (base === undefined) throw new Error("No fake UI is served: run the tests with `npm run uat:fake`.");
  const params = new URLSearchParams();
  if (query.delay) params.set("delay", Object.entries(query.delay).map(([command, ms]) => `${command}:${ms}`).join(","));
  if (query.fail) params.set("fail", query.fail.join(","));
  const search = params.toString();
  return search ? `${base}?${search}` : base;
}

export function uatStep(id: string, body: (args: StepArgs) => Promise<void>) {
  const step = planStep(id);
  test(id, async ({ page }, testInfo) => {
    const done = new Set<string>();
    const known = (text: string) => {
      if (!step.expect.includes(text)) throw new Error(`«${text}» is no check of step ${id} in uat/plan.json.`);
      if (done.has(text)) throw new Error(`The test of step ${id} checks «${text}» twice.`);
      done.add(text);
    };
    await body({
      page,
      open: async (query) => {
        await page.goto(address(query));
      },
      check: (text, assert) =>
        test.step(text, async () => {
          known(text);
          await assert();
        }),
      leave: (text, why) => {
        known(text);
        testInfo.annotations.push({ type: LEFT_TO_A_PERSON, description: `${text} (${why})` });
      },
    });
    const unchecked = step.expect.filter((text) => !done.has(text));
    if (unchecked.length > 0) throw new Error(`The test of step ${id} neither checks nor leaves to a person: ${unchecked.map((text) => `«${text}»`).join(", ")}`);
  });
}
