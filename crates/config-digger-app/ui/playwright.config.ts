/**
 * `npm run uat:fake`: the UAT steps that the UI on the fake backend can show,
 * one test per step, in `uat/`. Each run writes `uat-results.json`
 * (`uat/reporter.ts`).
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { defineConfig } from "@playwright/test";
import { UI, WINDOW } from "./scripts/fakeUi.ts";

/** The workspace version, as the root `Cargo.toml` sets it. */
const version = /^\[workspace\.package\][^[]*?^version\s*=\s*"([^"]+)"/m.exec(readFileSync(join(UI, "..", "..", "..", "Cargo.toml"), "utf8"))?.[1];
if (version === undefined) throw new Error("The root Cargo.toml sets no workspace version.");

const { GITHUB_SERVER_URL, GITHUB_REPOSITORY, GITHUB_RUN_ID } = process.env;
const runUrl = GITHUB_RUN_ID ? `${GITHUB_SERVER_URL}/${GITHUB_REPOSITORY}/actions/runs/${GITHUB_RUN_ID}` : undefined;

export default defineConfig({
  testDir: "uat",
  testMatch: "*.uat.ts",
  outputDir: "test-results",
  globalSetup: "./uat/serve.ts",
  fullyParallel: true,
  forbidOnly: process.env.CI !== undefined,
  reporter: [["list"], ["./uat/reporter.ts", { file: join(UI, "uat-results.json"), version, runUrl }]],
  // No screenshot, video or trace: the repo is public, and the job uploads only the results.
  use: { viewport: WINDOW, colorScheme: "dark", screenshot: "off", video: "off", trace: "off" },
});
