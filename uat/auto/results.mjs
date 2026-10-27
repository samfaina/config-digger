// The one uat-results.json of the local script, run.ps1 (docs/agents/uat.md,
// "The local script"): every step's result from the Sandbox walk and the
// WebDriver walk, in the plan's order, with the runner's fields alone, ready
// for the single ArtifactData batch of "Publishing a run".
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { resultDocument } from "./webdriver/step.mjs";

// The results of each walk as one list of runner documents, in the plan's
// order. The WebDriver walk's `checks`, `looks` and `manual` stay out: they
// stay on the machine. A run takes one result per step of the plan, so a
// step with two, or one the plan doesn't have, is refused.
export function combine(plan, ...walks) {
  const order = plan.sections.flatMap((section) => section.steps.map((step) => step.id));
  const results = walks.flat();
  const seen = new Set();
  for (const { id } of results) {
    if (!order.includes(id)) throw new Error(`${id} is no step of the plan, so its result is refused`);
    if (seen.has(id)) throw new Error(`${id} has two results, so both are refused`);
    seen.add(id);
  }
  return results.sort((a, b) => order.indexOf(a.id) - order.indexOf(b.id)).map(resultDocument);
}

// Writes <work>\uat-results.json from the walks' results in the local
// script's work folder: the WebDriver walk's webdriver.json, with the agent
// judge's verdicts once it merged them, and the Sandbox's
// sandbox\uat-results.json. Returns the file's path.
export function writeResults(work, plan) {
  const file = join(work, "uat-results.json");
  const results = combine(plan, read(join(work, "webdriver.json")), read(join(work, "sandbox", "uat-results.json")));
  writeFileSync(file, JSON.stringify(results, null, 2) + "\n");
  return file;
}

// A JSON file, with or without the byte order mark Windows PowerShell 5.1
// writes in UTF-8.
const read = (file) => JSON.parse(readFileSync(file, "utf8").replace(/^\uFEFF/, ""));

// node uat/auto/results.mjs --work <dir>: what run.ps1 runs once both walks
// wrote their results.
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { values: opt } = parseArgs({ options: { work: { type: "string" } } });
  const plan = JSON.parse(readFileSync(join(dirname(fileURLToPath(import.meta.url)), "..", "plan.json"), "utf8"));
  const file = writeResults(opt.work, plan);
  const results = read(file);
  const count = (status) => results.filter((result) => result.status === status).length;
  console.log(`${file}: ${results.length} results, ${count("ok")} OK, ${count("ko")} KO, ${count("blocked")} Blocked`);
}
