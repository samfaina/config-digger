// Tests Claude Code's facts that need a claude.ai account: the skills it
// syncs from claude.ai (`synced-skills`) and its sync gate. It runs on one
// throwaway home that the owner signs in to once, outside their own home
// folder:
//
//   CLAUDE_CONFIG_DIR='E:\cd-throwaway-agents\claude-account-home' claude auth login --claudeai
//   node uat/agents/claude-account.mjs [--bin <claude.exe>]
//
// The model is the local stand-in, as in `claude-code.mjs`, so no prompt
// reaches it; Claude Code still signs in, fetches its gates and syncs the
// account's skills. The probe plants skills next to the synced ones and
// changes the gate's cached value, and puts both back at the end. Sign the
// home out with `claude auth logout` on the same `CLAUDE_CONFIG_DIR` when
// done. `CD_PROBE_ACCOUNT_HOME` names another signed-in home, a folder
// named `claude-account-home*` in the probe root.
import fs from "node:fs";
import path from "node:path";
import { fresh, json, modelServer, probe, root, run, same, skill, write } from "./lib.mjs";

const bin = process.argv.includes("--bin") ? process.argv[process.argv.indexOf("--bin") + 1] : "claude.exe";
// The signed-in home sits in the probe root, and `fresh("claude-account")`
// never takes it.
const home = path.resolve(process.env.CD_PROBE_ACCOUNT_HOME ?? path.join(root, "claude-account-home"));
if (path.dirname(home) !== root || !path.basename(home).startsWith("claude-account-home")) {
  throw new Error(`CD_PROBE_ACCOUNT_HOME must be a folder named claude-account-home* in ${root}, not ${home}`);
}
// Any of these would take Claude Code off the account or off its gates.
for (const name of ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "CLAUDE_CODE_OAUTH_TOKEN", "DISABLE_GROWTHBOOK", "DISABLE_TELEMETRY", "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "DO_NOT_TRACK", "CLAUDE_CODE_USE_BEDROCK", "CLAUDE_CODE_USE_VERTEX"]) {
  if (process.env[name]) throw new Error(`unset ${name} first`);
}
const status = JSON.parse((await run(bin, ["auth", "status", "--json"], { env: { CLAUDE_CONFIG_DIR: home } })).stdout || "{}");
if (status.authMethod !== "claude.ai") {
  console.error(`Sign the throwaway home in to claude.ai first:\n  CLAUDE_CONFIG_DIR='${home}' claude auth login --claudeai`);
  process.exit(2);
}
const version = (await run(bin, ["--version"])).stdout.trim().split(" ")[0];
console.log(`Claude Code ${version}, signed in to claude.ai, plan ${status.subscriptionType}\n`);
const p = probe("Claude Code", version);

const proj = fresh("claude-account");
const state = path.join(home, ".claude.json");
const synced = path.join(home, "skills", "synced");
const { organizationUuid, accountUuid } = json(state).oauthAccount;
const bucket = path.join(synced, `${organizationUuid}_${accountUuid}`);
const reply = { id: "msg_x", type: "message", role: "assistant", model: "claude-x", content: [{ type: "text", text: "ok" }], stop_reason: "end_turn", usage: { input_tokens: 1, output_tokens: 1 } };
// Claude Code skips its gate fetch with this set, and holds the gate off.
const noFetch = { DISABLE_GROWTHBOOK: "1" };

// One `claude -p` run: the skills its init event lists, the skill lines of
// the request its model gets, and that request.
async function session({ prompt = "hi", env = {} } = {}) {
  const model = await modelServer(reply);
  const { stdout } = await run(bin, ["-p", prompt, "--output-format", "stream-json", "--verbose"], { cwd: proj, env: { CLAUDE_CONFIG_DIR: home, ANTHROPIC_BASE_URL: model.url, ...env } });
  model.close();
  const init = JSON.parse(stdout.split("\n").find((l) => l.includes('"subtype":"init"')));
  const listing = model.bodies.map((b) => b.match(/The following skills are available(?:[^"\\]|\\.)*/)).find(Boolean);
  const listed = listing ? JSON.parse(`"${listing[0]}"`).split("\n").filter((l) => l.startsWith("- ")) : [];
  return { skills: init.skills, listed, sent: model.bodies.join("\n") };
}

// Sets the sync gate's value in Claude Code's cache of its gates.
function cacheGate(value) {
  const s = json(state);
  s.cachedGrowthBookFeatures.tengu_claudeai_skills_sync_enabled = value;
  write(state, s);
}
const cachedGate = () => json(state).cachedGrowthBookFeatures.tengu_claudeai_skills_sync_enabled;

const planted = (name) => /throwaway|^fold-/.test(name);
// The skill folders Claude Code downloaded into the account's folder.
const downloaded = () => (fs.existsSync(bucket) ? fs.readdirSync(bucket).filter((x) => !x.startsWith(".") && !planted(x) && fs.statSync(path.join(bucket, x)).isDirectory()).sort() : []);
const syncedListed = (s) => s.skills.filter((n) => n.startsWith("anthropic-skills:"));
// Clears the copy as a new sign-in finds it. While `.last-complete-round`
// stands, Claude Code does not download again a skill deleted by hand.
function clearDownloaded() {
  for (const x of [...downloaded(), "manifest.json", ".last-complete-round"]) fs.rmSync(path.join(bucket, x), { recursive: true, force: true });
}
function unplant() {
  for (const dir of [path.join(bucket, "fold-s"), path.join(bucket, "cd-throwaway-group"), path.join(synced, "cd-throwaway-direct"), path.join(synced, "cd-throwaway-org_cd-throwaway-acct"), path.join(home, "skills", "fold-s")]) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}
// Sessions with the gate on until the account's skills are back on disk.
async function syncAgain(atLeast = 1) {
  cacheGate(true);
  for (let i = 0; i < 6 && downloaded().length < atLeast; i++) await session();
  await session();
  return downloaded().length;
}

// ---------------------------------------------------------------- the sync

unplant();
try {
  const accountSkills = await syncAgain();
  await p.check("locations[synced-skills]", "with the gate on, Claude Code downloads the account's skills to `synced/<org>_<account>` and lists each as `anthropic-skills:<folder>`", async () => {
    const s = await session();
    const expected = downloaded().map((f) => `anthropic-skills:${f}`);
    return { pass: accountSkills > 0 && same(syncedListed(s).sort(), expected).pass, observed: JSON.stringify({ plan: status.subscriptionType, downloaded: accountSkills, listed: syncedListed(s).sort() }) };
  });

  await p.check("locations[synced-skills]", "while `.last-complete-round` stands, a synced skill deleted by hand is not downloaded again", async () => {
    const gone = downloaded()[0];
    fs.rmSync(path.join(bucket, gone), { recursive: true });
    await session();
    const back = downloaded().includes(gone);
    clearDownloaded();
    await syncAgain(accountSkills);
    return same({ back, restored: downloaded().length }, { back: false, restored: accountSkills });
  });

  skill(path.join(bucket, "fold-s"), ["name: cd-throwaway-sname", "description: planted in the bucket"], "body-of-synced-fold-s");
  skill(path.join(synced, "cd-throwaway-direct"), ["name: cd-throwaway-direct", "description: directly under synced"]);
  skill(path.join(bucket, "cd-throwaway-group", "cd-throwaway-deep"), ["name: cd-throwaway-deep", "description: deeper"]);
  skill(path.join(synced, "cd-throwaway-org_cd-throwaway-acct", "cd-throwaway-other"), ["name: cd-throwaway-other", "description: another account"]);
  cacheGate(false);

  await p.check("locations[synced-skills]", "a synced skill is named `anthropic-skills:<folder>`, its frontmatter `name` an alias with that prefix, and one `manifest.json` does not list is read too", async () => {
    const s = await session({ env: noFetch });
    return same(s.listed.filter((l) => l.includes("fold-s")), ["- anthropic-skills:fold-s (anthropic-skills:cd-throwaway-sname): planted in the bucket"]);
  });

  await p.check("locations[synced-skills]", "a skill directly under `synced/`, one deeper than a skill folder and one in another account's folder are not read", async () => {
    const s = await session({ env: noFetch });
    const read = s.skills.filter((n) => /cd-throwaway-(direct|deep|other)/.test(n));
    const kept = ["cd-throwaway-direct", "cd-throwaway-org_cd-throwaway-acct"].every((f) => fs.existsSync(path.join(synced, f)));
    return same({ read, kept }, { read: [], kept: true });
  });

  // ---------------------------------------------------------------- names

  await p.check("items.skill", "a synced skill never competes with a local one: both are listed, and only the bare `/<folder>` goes to the local skill", async () => {
    const local = path.join(home, "skills", "fold-s");
    skill(local, ["name: cd-throwaway-local", "description: user fold-s"], "body-of-user-fold-s");
    const listed = (await session({ env: noFetch })).listed.filter((l) => l.includes("fold-s"));
    const runs = async (prompt) => {
      const { sent } = await session({ prompt, env: noFetch });
      return sent.includes("body-of-user-fold-s") ? "user" : sent.includes("body-of-synced-fold-s") ? "synced" : "none";
    };
    const withLocal = { "/fold-s": await runs("/fold-s"), "/anthropic-skills:fold-s": await runs("/anthropic-skills:fold-s") };
    fs.rmSync(local, { recursive: true });
    const alone = { "/fold-s": await runs("/fold-s"), "/cd-throwaway-sname": await runs("/cd-throwaway-sname"), "/anthropic-skills:cd-throwaway-sname": await runs("/anthropic-skills:cd-throwaway-sname") };
    return same(
      { listed, withLocal, alone },
      {
        listed: ["- fold-s (cd-throwaway-local): user fold-s", "- anthropic-skills:fold-s (anthropic-skills:cd-throwaway-sname): planted in the bucket"],
        withLocal: { "/fold-s": "user", "/anthropic-skills:fold-s": "synced" },
        alone: { "/fold-s": "synced", "/cd-throwaway-sname": "none", "/anthropic-skills:cd-throwaway-sname": "synced" },
      },
    );
  });

  await p.check("items.skill.disable[0]", "`skillOverrides` turns a synced skill off by `anthropic-skills:<folder>` or by `<folder>`, not by its frontmatter `name`", async () => {
    const file = path.join(proj, ".claude", "settings.local.json");
    const off = {};
    for (const key of ["anthropic-skills:fold-s", "fold-s", "anthropic-skills:cd-throwaway-sname", "cd-throwaway-sname"]) {
      write(file, { skillOverrides: { [key]: "off" } });
      off[key] = !(await session({ env: noFetch })).skills.includes("anthropic-skills:fold-s");
    }
    fs.rmSync(path.join(proj, ".claude"), { recursive: true });
    return same(off, { "anthropic-skills:fold-s": true, "fold-s": true, "anthropic-skills:cd-throwaway-sname": false, "cd-throwaway-sname": false });
  });

  // ---------------------------------------------------------------- the gate

  await p.check("locations[synced-skills]", "with the gate cached off, Claude Code still reads the synced folder, and downloads nothing", async () => {
    clearDownloaded();
    cacheGate(false);
    const runs = [];
    for (let i = 0; i < 2; i++) runs.push({ listed: (await session({ env: noFetch })).skills.includes("anthropic-skills:fold-s"), downloaded: downloaded().length });
    return same(runs, [{ listed: true, downloaded: 0 }, { listed: true, downloaded: 0 }]);
  });

  await p.check("feature_gates[tengu_claudeai_skills_sync_enabled]", "with `DISABLE_GROWTHBOOK` set, the gate is off whatever the cache holds: nothing is downloaded", async () => {
    cacheGate(true);
    const runs = [];
    for (let i = 0; i < 2; i++) {
      await session({ env: noFetch });
      runs.push(downloaded().length);
    }
    return same(runs, [0, 0]);
  });

  await p.check("feature_gates[tengu_claudeai_skills_sync_enabled]", "the gate turns the download on, by the value cached at start: a run starting from a cached off reads the folder, downloads nothing and caches on, and later runs download", async () => {
    cacheGate(false);
    const s = await session();
    const first = { listed: s.skills.includes("anthropic-skills:fold-s"), downloaded: downloaded().length, cached: cachedGate() };
    const after = await syncAgain();
    return { pass: first.listed && first.downloaded === 0 && first.cached === true && after > 0, observed: JSON.stringify({ first, after }) };
  });

  await p.check("locations[synced-skills]", "a sync round that downloads deletes what sits directly under `synced/` and another account's folder", async () => {
    const left = ["cd-throwaway-direct", "cd-throwaway-org_cd-throwaway-acct"].filter((f) => fs.existsSync(path.join(synced, f)));
    return same(left, []);
  });

  // ---------------------------------------------------------------- the setting

  await p.check("items.skill.rules[0]", "`syncClaudeAiSkills: false` in the user's settings: no synced skill is read, and the downloaded ones are deleted", async () => {
    const settings = path.join(home, "settings.json");
    write(settings, { syncClaudeAiSkills: false });
    const s = await session();
    fs.rmSync(settings);
    return same({ listed: syncedListed(s), downloaded: downloaded().length, planted: fs.existsSync(path.join(bucket, "fold-s")) }, { listed: [], downloaded: 0, planted: true });
  });
} finally {
  // Put back: the account's skills downloaded again, the gate cached on.
  unplant();
  fs.rmSync(path.join(home, "settings.json"), { force: true });
  console.log(`\nput back: ${await syncAgain()} of the account's skills downloaded again, gate cached ${cachedGate()}`);
}

p.finish();
