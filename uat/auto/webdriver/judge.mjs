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
// it Blocked, took no shot of it, or a shot is missing on disk. A walk KO
// stays KO, so it is never Blocked here.
function unjudgeable(result, looks) {
  if (result.status === "ko") return undefined;
  if (result.status === "blocked") return "el walk dejó el paso bloqueado";
  if (!looks.length) return "el walk no tomó ninguna captura que juzgar";
  const missing = looks.filter((look) => look.missing).map((look) => look.file);
  if (missing.length) return `falta la captura ${missing.join(", ")}`;
  return undefined;
}

// The steps to judge: each result of the walk with looks, and each one whose
// step lists agent-judge in its automation, but those the judge already
// judged. Each comes with its step's plan fields, the walk's own verdict and
// the absolute path of each shot, a shot missing on disk marked, and with
// `blocked` when the judge can't judge it.
export function toJudge(plan, results, shots) {
  const planSteps = new Map(plan.sections.flatMap((section) => section.steps.map((step) => [step.id, step])));
  const judges = (result) => planSteps.get(result.id)?.automation?.some((a) => a.tool === "agent-judge");
  return results
    .filter((result) => result.by !== "agent-judge" && (result.looks?.length || judges(result)))
    .map((result) => {
      const { id, title, goal, instructions, expect } = planSteps.get(result.id);
      const looks = (result.looks ?? []).map(({ what, file }) => {
        const path = join(shots, file);
        return existsSync(path) ? { what, file: path } : { what, file: path, missing: true };
      });
      const blocked = unjudgeable(result, looks);
      return {
        id,
        title,
        goal,
        instructions,
        expect,
        walk: { status: result.status, checks: result.checks, notes: result.notes },
        looks,
        ...(blocked && { blocked }),
      };
    });
}

// The walk's results with the judge's verdicts merged in, by the rules of
// docs/agents/uat.md. Each verdict, `{ id, status, notes, evidence }`, sets
// the step's status and evidence and `by: "agent-judge"`, and adds its notes
// to the walk's under "Juez:". The other fields stay as the walk wrote them.
// A verdict for a step the listing didn't list is refused, and so is a
// listing with a step left without a verdict.
export function merge(results, listing, verdicts) {
  const listed = new Map(listing.map((step) => [step.id, step]));
  const byId = new Map(verdicts.map((verdict) => [verdict.id, verdict]));
  for (const verdict of verdicts) {
    if (!listed.has(verdict.id)) throw new Error(`${verdict.id} is no step the listing listed, so its verdict is refused`);
    const fault = verdictFault(verdict, listed.get(verdict.id));
    if (fault) throw new Error(`the verdict for ${verdict.id} is refused: ${fault}`);
  }
  for (const id of listed.keys()) {
    if (!byId.has(id)) throw new Error(`no verdict for ${id}, a step the listing listed`);
  }
  return results.map((result) => {
    const verdict = byId.get(result.id);
    if (!verdict) return result;
    const { blocked, looks } = listed.get(result.id);
    const judge = [blocked && `Bloqueado: ${blocked}`, verdict.notes].filter(Boolean).join("\n");
    return {
      ...result,
      status: status(result, blocked, verdict),
      notes: `${result.notes}\nJuez:\n${judge}`,
      by: "agent-judge",
      evidence: evidence(result, blocked, looks, verdict),
    };
  });
}

// What is wrong with a judge's verdict on a listed step, or undefined. Its
// evidence is the local paths of the step's shots it looked at, joined by
// "; ", never an image: none for a step the judge can't judge.
function verdictFault({ status, notes, evidence }, { blocked, looks }) {
  if (!["ok", "ko", "blocked"].includes(status)) return `its status is ${JSON.stringify(status)}, not ok, ko or blocked`;
  if (typeof notes !== "string") return "it has no notes";
  if (status !== "ok" && !blocked && !notes.trim()) return `a ${status} needs notes saying why`;
  const files = typeof evidence === "string" && evidence ? evidence.split("; ") : [];
  if (!blocked && !files.length) return "its evidence names no shot";
  const strange = files.filter((file) => !looks.some((look) => look.file === file));
  if (strange.length) return `its evidence names what is no shot of the step: ${strange.join("; ")}`;
  return undefined;
}

// A walk KO stays KO, a step the judge can't judge is Blocked, and any other
// takes the judge's status.
function status(result, blocked, verdict) {
  if (result.status === "ko") return "ko";
  if (blocked) return "blocked";
  return verdict.status;
}

// The shot that decided the verdict: a missing one, the judge's for a step
// it judged, and the walk's own evidence for a step it couldn't judge.
function evidence(result, blocked, looks, verdict) {
  const missing = looks.filter((look) => look.missing).map((look) => look.file);
  if (missing.length) return missing.join("; ");
  if (blocked) return result.evidence;
  return verdict.evidence;
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
  const out = join(opt.work, "webdriver.json");
  const results = JSON.parse(readFileSync(out, "utf8"));
  const listing = toJudge(plan, results, join(opt.work, "shots"));
  const [mode] = positionals;
  if (mode === "list") console.log(JSON.stringify(listing, null, 2));
  else if (mode === "merge" && opt.verdicts) {
    const merged = merge(results, listing, JSON.parse(readFileSync(opt.verdicts, "utf8")));
    writeFileSync(out, JSON.stringify(merged, null, 2) + "\n");
    const judged = merged.filter((result) => listing.some((step) => step.id === result.id));
    const count = (status) => judged.filter((result) => result.status === status).length;
    console.log(`Merged ${judged.length} verdicts into ${out}: ${count("ok")} OK, ${count("ko")} KO, ${count("blocked")} Blocked`);
  } else throw new Error("usage: judge.mjs list [--work <dir>] | judge.mjs merge --verdicts <file> [--work <dir>]");
}
