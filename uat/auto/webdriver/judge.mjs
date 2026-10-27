// The agent judge's mechanical half (docs/agents/uat.md, "The agent judge"):
// it lists the WebDriver walk's results whose screenshots the judge must
// read, and merges the judge's verdicts back into the walk's results. The
// model only does the looking.
//
//   node uat/auto/webdriver/judge.mjs list [--work <dir>]
//   node uat/auto/webdriver/judge.mjs merge --verdicts <file> [--work <dir>]
//
// `--work` is run.ps1's -Work, $env:TEMP\config-digger-uat by default: the
// walk's results are its webdriver.json and its screenshots its shots folder.
// `list` prints the steps to judge as JSON. `merge` writes the verdicts into
// webdriver.json, or nothing when it refuses one.
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

// Why the judge can't judge a step, or undefined when it can: the walk left
// it Blocked, took no shot of it, or a shot is missing on disk.
function unjudgeable(result, looks, missing) {
  if (result.status === "blocked") return "el walk dejó el paso bloqueado";
  if (!looks.length) return "el walk no tomó ninguna captura que juzgar";
  if (missing.length) return `falta la captura ${missing.join(", ")}`;
  return undefined;
}

// The steps to judge: each result of the walk with looks, and each one whose
// step lists agent-judge in its automation, but those the judge already
// judged. Each comes with its step's plan fields, the walk's own verdict and
// the absolute path of each shot, a shot missing on disk marked, and with
// `unjudged`, saying why, when the judge can't judge it.
export function toJudge(plan, results, shots) {
  const planSteps = new Map(plan.sections.flatMap((section) => section.steps.map((step) => [step.id, step])));
  const listsAgentJudge = (result) => planSteps.get(result.id)?.automation?.some((a) => a.tool === "agent-judge");
  return results
    .filter((result) => result.by !== "agent-judge" && (result.looks?.length || listsAgentJudge(result)))
    .map((result) => {
      const { id, title, goal, instructions, expect } = planSteps.get(result.id);
      const looks = (result.looks ?? []).map(({ what, file }) => {
        const path = join(shots, file);
        return existsSync(path) ? { what, file: path } : { what, file: path, missing: true };
      });
      const unjudged = unjudgeable(result, looks, missingShots({ looks }));
      return {
        id,
        title,
        goal,
        instructions,
        expect,
        walk: { status: result.status, checks: result.checks, notes: result.notes },
        looks,
        ...(unjudged && { unjudged }),
      };
    });
}

const missingShots = (listed) => listed.looks.filter((look) => look.missing).map((look) => look.file);

// The walk's results with the judge's verdicts merged in, by the rules of
// docs/agents/uat.md. Each verdict, `{ id, status, notes, evidence }`, sets
// the step's status and evidence and `by: "agent-judge"`, and adds its notes
// to the walk's under "Juez:". The other fields stay as the walk wrote them.
// A verdict for a step the listing didn't list is refused, and so is a
// listing with a step left without a verdict, or with two.
export function merge(results, listing, verdicts) {
  const listed = new Map(listing.map((step) => [step.id, step]));
  const byId = new Map();
  for (const verdict of verdicts) {
    if (!listed.has(verdict.id)) throw new Error(`${verdict.id} is no step the listing listed, so its verdict is refused`);
    if (byId.has(verdict.id)) throw new Error(`${verdict.id} has two verdicts, so both are refused`);
    const fault = verdictFault(verdict, listed.get(verdict.id));
    if (fault) throw new Error(`the verdict for ${verdict.id} is refused: ${fault}`);
    byId.set(verdict.id, verdict);
  }
  for (const id of listed.keys()) {
    if (!byId.has(id)) throw new Error(`no verdict for ${id}, a step the listing listed`);
  }
  return results.map((result) => {
    const verdict = byId.get(result.id);
    return verdict ? judgedResult(result, listed.get(result.id), verdict) : result;
  });
}

// What is wrong with a judge's verdict on a listed step, or undefined. Its
// evidence is the local paths of the step's shots it looked at, joined by
// "; ", never an image: none for a step the judge can't judge.
function verdictFault({ status, notes, evidence }, listed) {
  if (!["ok", "ko", "blocked"].includes(status)) return `its status is ${JSON.stringify(status)}, not ok, ko or blocked`;
  if (typeof notes !== "string") return "it has no notes";
  if (status !== "ok" && !listed.unjudged && !notes.trim()) return `a ${status} needs notes saying why`;
  const files = typeof evidence === "string" && evidence ? evidence.split("; ") : [];
  if (!listed.unjudged && !files.length) return "its evidence names no shot";
  const notShots = files.filter((file) => !shotNamed(listed, file));
  if (notShots.length) return `its evidence names what is no shot of the step: ${notShots.join("; ")}`;
  return undefined;
}

// The step's shot that `file` names, as the listing spells it, whatever the
// slashes or the case `file` is written with.
function shotNamed(listed, file) {
  const same = (path) => path.replaceAll("\\", "/").toLowerCase();
  return listed.looks.find((look) => same(look.file) === same(file))?.file;
}

// The walk's result once judged. A walk KO stays KO, a step the judge can't
// judge is Blocked, and any other takes the judge's status. Its evidence is
// the shot that decided the verdict: a missing one, those the judge named,
// or the walk's own evidence when the judge looked at no shot.
function judgedResult(result, listed, verdict) {
  const { unjudged } = listed;
  let status = verdict.status;
  if (unjudged) status = "blocked";
  if (result.status === "ko") status = "ko";
  const why = unjudged && `${status === "ko" ? "Sin juzgar" : "Bloqueado"}: ${unjudged}`;
  const judgeNotes = [why, verdict.notes].filter(Boolean).join("\n");
  const missing = missingShots(listed);
  const named = verdict.evidence ? verdict.evidence.split("; ").map((file) => shotNamed(listed, file)) : [];
  let evidence = result.evidence;
  if (missing.length) evidence = missing.join("; ");
  else if (!unjudged && named.length) evidence = named.join("; ");
  return { ...result, status, notes: `${result.notes}\nJuez:\n${judgeNotes}`, by: "agent-judge", evidence };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { positionals, values: opt } = parseArgs({
    allowPositionals: true,
    options: {
      work: { type: "string", default: join(tmpdir(), "config-digger-uat") },
      verdicts: { type: "string" },
    },
  });
  const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");
  const plan = JSON.parse(readFileSync(join(repo, "uat", "plan.json"), "utf8"));
  const resultsFile = join(opt.work, "webdriver.json");
  const results = JSON.parse(readFileSync(resultsFile, "utf8"));
  const listing = toJudge(plan, results, join(opt.work, "shots"));
  const [mode] = positionals;
  if (mode === "list") console.log(JSON.stringify(listing, null, 2));
  else if (mode === "merge" && opt.verdicts) {
    const merged = merge(results, listing, JSON.parse(readFileSync(opt.verdicts, "utf8")));
    writeFileSync(resultsFile, JSON.stringify(merged, null, 2) + "\n");
    const judged = merged.filter((result) => listing.some((step) => step.id === result.id));
    const count = (status) => judged.filter((result) => result.status === status).length;
    console.log(`Merged ${judged.length} verdicts into ${resultsFile}: ${count("ok")} OK, ${count("ko")} KO, ${count("blocked")} Blocked`);
  } else throw new Error("usage: judge.mjs list [--work <dir>] | judge.mjs merge --verdicts <file> [--work <dir>]");
}
