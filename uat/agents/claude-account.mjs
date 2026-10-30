// Tests Claude Code's facts that need a claude.ai account: the skills and
// plugins it syncs from claude.ai (`synced-skills`, `plugins[synced]`), its
// sync gate, and what takes it off the account (`account`, `account.off`).
// It runs on one throwaway home that the owner signs in to once, outside
// their own home folder, and on copies of it:
//
//   CLAUDE_CONFIG_DIR='E:\cd-throwaway-agents\claude-account-home' claude auth login --claudeai
//   node uat/agents/claude-account.mjs [--bin <claude.exe>]
//
// The model is the local stand-in, as in `claude-code.mjs`, so no prompt
// reaches it; Claude Code still signs in, fetches its gates and syncs the
// account's skills. The probe plants skills and plugins next to the synced
// ones, changes the gates' cached values, the user settings and two keys
// of `.claude.json`, and puts them back at the end. Sign the home out
// with `claude auth logout` on the same `CLAUDE_CONFIG_DIR` when done,
// never on a copy. `CD_PROBE_ACCOUNT_HOME` names another signed-in home, a folder
// named `claude-account-home*` in the probe root.
import fs from "node:fs";
import path from "node:path";
import { fresh, json, modelServer, probe, root, run, same, skill, syncedPlugin, write } from "./lib.mjs";

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
// What the account checks change, put back at the end: the user settings
// and two keys of `.claude.json`.
const userSettings = path.join(home, "settings.json");
const keptSettings = fs.existsSync(userSettings) ? fs.readFileSync(userSettings) : null;
const keptKeys = (({ primaryApiKey, customApiKeyResponses }) => ({ primaryApiKey, customApiKeyResponses }))(json(state));
function putBackSettings() {
  if (keptSettings) write(userSettings, keptSettings);
  else fs.rmSync(userSettings, { force: true });
}
function putBackState() {
  const s = json(state);
  for (const [key, value] of Object.entries(keptKeys)) {
    if (value === undefined) delete s[key];
    else s[key] = value;
  }
  write(state, s);
}
const reply = { id: "msg_x", type: "message", role: "assistant", model: "claude-x", content: [{ type: "text", text: "ok" }], stop_reason: "end_turn", usage: { input_tokens: 1, output_tokens: 1 } };
// Claude Code skips its gate fetch with this set, and holds the gate off.
const noFetch = { DISABLE_GROWTHBOOK: "1" };

// One `claude -p` run, on `on`, the signed-in home or a copy of it: the
// skills and plugins its init event lists, the skill lines of the request
// its model gets, and that request.
async function session({ prompt = "hi", env = {}, on = home, timeout } = {}) {
  const model = await modelServer(reply);
  const { stdout } = await run(bin, ["-p", prompt, "--output-format", "stream-json", "--verbose"], { cwd: proj, env: { CLAUDE_CONFIG_DIR: on, ANTHROPIC_BASE_URL: model.url, ...env }, timeout });
  model.close();
  const init = JSON.parse(stdout.split("\n").find((l) => l.includes('"subtype":"init"')));
  const listing = model.bodies.map((b) => b.match(/The following skills are available(?:[^"\\]|\\.)*/)).find(Boolean);
  const listed = listing ? JSON.parse(`"${listing[0]}"`).split("\n").filter((l) => l.startsWith("- ")) : [];
  return { skills: init.skills, plugins: init.plugins.map((x) => x.name), listed, sent: model.bodies.join("\n") };
}

// The auth Claude Code picks in the Project, as `claude auth status` says.
async function auth({ env = {}, on = home } = {}) {
  const { authMethod, apiProvider } = JSON.parse((await run(bin, ["auth", "status", "--json"], { cwd: proj, env: { CLAUDE_CONFIG_DIR: on, ...env } })).stdout);
  return `${authMethod}/${apiProvider}`;
}

// Sets a sync gate's value, the skills' unless named, in Claude Code's
// cache of its gates.
const pluginsGate = "tengu_claudeai_plugins_sync_enabled";
function cacheGate(value, gate = "tengu_claudeai_skills_sync_enabled") {
  const s = json(state);
  s.cachedGrowthBookFeatures = { ...s.cachedGrowthBookFeatures, [gate]: value };
  write(state, s);
}
const cachedGate = () => json(state).cachedGrowthBookFeatures?.tengu_claudeai_skills_sync_enabled;

const planted = (name) => /throwaway|^fold-/.test(name);
// The skill folders Claude Code downloaded into the account's folder.
const downloaded = () => (fs.existsSync(bucket) ? fs.readdirSync(bucket).filter((x) => !x.startsWith(".") && !planted(x) && fs.statSync(path.join(bucket, x)).isDirectory()).sort() : []);
const syncedListed = (s) => s.skills.filter((n) => n.startsWith("anthropic-skills:"));
// Clears the copy as a new sign-in finds it. While `.last-complete-round`
// stands, Claude Code does not download again a skill deleted by hand.
function clearDownloaded() {
  for (const x of [...downloaded(), "manifest.json", ".last-complete-round"]) fs.rmSync(path.join(bucket, x), { recursive: true, force: true });
}
// The synced plugins: the account's folder, which the probe makes when the
// account has none, and another account's.
const syncedPlugins = path.join(home, "plugins", "synced");
const pluginBucket = path.join(syncedPlugins, `${organizationUuid}_${accountUuid}`);
const otherPlugins = path.join(syncedPlugins, "cd-throwaway-org_cd-throwaway-acct");
const hadPluginBucket = fs.existsSync(pluginBucket);
function unplantPlugins() {
  fs.rmSync(otherPlugins, { recursive: true, force: true });
  if (!fs.existsSync(pluginBucket)) return;
  if (!hadPluginBucket) return fs.rmSync(pluginBucket, { recursive: true });
  for (const x of fs.readdirSync(pluginBucket).filter((x) => x.startsWith("cd-throwaway"))) fs.rmSync(path.join(pluginBucket, x), { recursive: true });
  const list = path.join(pluginBucket, "manifest.json");
  if (fs.existsSync(list)) {
    const listed = json(list);
    write(list, { ...listed, plugins: listed.plugins.filter((x) => !x.name.startsWith("cd-throwaway")) });
  }
}
function unplant() {
  for (const dir of [path.join(bucket, "fold-s"), path.join(bucket, "cd-throwaway-group"), path.join(synced, "cd-throwaway-direct"), path.join(synced, "cd-throwaway-org_cd-throwaway-acct"), path.join(home, "skills", "fold-s")]) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
  unplantPlugins();
}

// Copies of the signed-in home, deleted at the end.
const copies = [];
function copyHome(name, change) {
  const dir = fresh(`claude-account-copy-${name}`);
  fs.cpSync(home, dir, { recursive: true });
  change(dir);
  copies.push(dir);
  return dir;
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

  // ---------------------------------------------------------------- the account

  // Claude Code reads a synced plugin only while the gate
  // `tengu_claudeai_plugins_sync_enabled` is on, and only one the account's
  // `manifest.json` lists; a sync round deletes one claude.ai does not
  // list. These sessions run with no way out of the machine but to the
  // model stand-in: the gates hold their cached values and no round runs.
  const offline = { HTTPS_PROXY: "http://127.0.0.1:9", HTTP_PROXY: "http://127.0.0.1:9", NO_PROXY: "127.0.0.1" };
  cacheGate(true, pluginsGate);
  // The folder `<name>~g<n>` from 2.1.310, `<name>` before, as the
  // account's own synced plugins have it.
  const listedPlugins = fs.existsSync(path.join(pluginBucket, "manifest.json")) ? json(path.join(pluginBucket, "manifest.json")).plugins : [];
  const generation = listedPlugins.some((x) => "generation" in x) ? 1 : undefined;
  syncedPlugin(pluginBucket, "cd-throwaway-synced", { generation, skills: ["cd-throwaway-pskill"] });
  syncedPlugin(otherPlugins, "cd-throwaway-elsewhere", { generation });
  // What a session read of the account's: its synced skills, and the
  // synced plugin planted in its folder.
  const reads = (s) => ({ skills: syncedListed(s).length > 0, plugin: s.plugins.includes("cd-throwaway-synced") });
  const both = { skills: true, plugin: true };
  const none = { skills: false, plugin: false };
  const onAccount = "claude.ai/firstParty";

  await p.check("plugins[synced]", "a synced plugin is read, with its skills, only in the signed-in account's folder, `plugins/synced/<org>_<account>`, and not in another account's folder", async () => {
    const s = await session({ env: offline });
    return same({ plugins: s.plugins.filter((n) => n.startsWith("cd-throwaway")), skill: s.skills.includes("cd-throwaway-synced:cd-throwaway-pskill"), auth: await auth() }, { plugins: ["cd-throwaway-synced"], skill: true, auth: onAccount });
  });

  await p.check("plugins[synced]", "a synced plugin is read only while the gate `tengu_claudeai_plugins_sync_enabled` is on, and only one the account's `manifest.json` lists", async () => {
    const unlisted = path.join(pluginBucket, generation === undefined ? "cd-throwaway-unlisted" : "cd-throwaway-unlisted~g1");
    write(path.join(unlisted, ".claude-plugin", "plugin.json"), { name: "cd-throwaway-unlisted", version: "1.0.0" });
    const loaded = async (env) => (await session({ env })).plugins.filter((n) => n.startsWith("cd-throwaway"));
    const gateOff = await loaded({ ...offline, ...noFetch });
    const gateOn = await loaded(offline);
    fs.rmSync(unlisted, { recursive: true });
    return same({ gateOff, gateOn }, { gateOff: [], gateOn: ["cd-throwaway-synced"] });
  });

  // Signed out, on copies of the signed-in home. Never `claude auth logout`
  // on a copy: it shares the home's tokens, which a logout may revoke.
  await p.check("account", "on a copy of the home whose `.claude.json` holds no `oauthAccount`, Claude Code reads no synced skill or plugin offline; online it gets `oauthAccount` back from claude.ai at start and reads the synced skills", async () => {
    const copy = copyHome("no-account", (dir) => {
      const s = json(path.join(dir, ".claude.json"));
      delete s.oauthAccount;
      write(path.join(dir, ".claude.json"), s);
    });
    const offlineReads = reads(await session({ env: offline, on: copy }));
    const online = await session({ env: noFetch, on: copy });
    const back = json(path.join(copy, ".claude.json")).oauthAccount;
    return same(
      { auth: await auth({ on: copy }), offline: offlineReads, online: syncedListed(online).length > 0, back: `${back?.organizationUuid}_${back?.accountUuid}` },
      { auth: onAccount, offline: none, online: true, back: `${organizationUuid}_${accountUuid}` },
    );
  });

  await p.check("account", "on a copy of the home with `oauthAccount` kept and no credentials file, Claude Code is signed out and reads no synced skill or plugin", async () => {
    const copy = copyHome("no-credentials", (dir) => fs.rmSync(path.join(dir, ".credentials.json")));
    return same({ auth: await auth({ on: copy }), reads: reads(await session({ env: offline, on: copy })) }, { auth: "none/firstParty", reads: none });
  });

  // One settings file at a time, the user's or the Project's, holding what
  // takes Claude Code off the account or leaves it on. `claude auth status`
  // leaves out a Project's token variables. Bedrock points at a closed port,
  // and the session stops before it gives up: the init event comes before
  // any request.
  const projectSettings = path.join(proj, ".claude", "settings.json");
  const bedrock = (value) => ({ env: { CLAUDE_CODE_USE_BEDROCK: value, ANTHROPIC_BEDROCK_BASE_URL: "http://127.0.0.1:9", CLAUDE_CODE_SKIP_BEDROCK_AUTH: "1", AWS_REGION: "us-east-1" } });
  // Each case: what the file holds, what `claude auth status` says, which
  // of the account's synced skills and plugins Claude Code reads, and what
  // that shows.
  const off = "takes Claude Code off the account: it reads no synced skill or plugin";
  const on = "leaves Claude Code on the account: it reads the synced skills and plugins";
  const gateOff = "leaves Claude Code on the account but holds the plugin sync gate off: it reads the synced skills, and no synced plugin";
  const settingsCases = [
    ["`apiKeyHelper` in the user settings", userSettings, { apiKeyHelper: "echo not-a-real-secret" }, "api_key_helper/firstParty", none, off],
    ["`apiKeyHelper` in the Project's `.claude/settings.json`", projectSettings, { apiKeyHelper: "echo not-a-real-secret" }, "api_key_helper/firstParty", none, off],
    ["`env.ANTHROPIC_AUTH_TOKEN` in the user settings", userSettings, { env: { ANTHROPIC_AUTH_TOKEN: "not-a-real-secret" } }, "oauth_token/firstParty", none, off],
    ["`env.ANTHROPIC_AUTH_TOKEN` in the Project's settings", projectSettings, { env: { ANTHROPIC_AUTH_TOKEN: "not-a-real-secret" } }, onAccount, none, off],
    ["`env.ANTHROPIC_AUTH_TOKEN` set to `\"\"` in the Project's settings", projectSettings, { env: { ANTHROPIC_AUTH_TOKEN: "" } }, onAccount, both, on],
    ["`env.CLAUDE_CODE_OAUTH_TOKEN` in the user settings", userSettings, { env: { CLAUDE_CODE_OAUTH_TOKEN: "not-a-real-secret" } }, "oauth_token/firstParty", none, off],
    ["`env.CLAUDE_CODE_OAUTH_TOKEN` in the Project's settings", projectSettings, { env: { CLAUDE_CODE_OAUTH_TOKEN: "not-a-real-secret" } }, onAccount, { skills: false, plugin: true }, "takes Claude Code's synced skills off the account, not its synced plugins"],
    ["`env.CLAUDE_CODE_USE_BEDROCK` set to `\"1\"` in the Project's settings", projectSettings, bedrock("1"), "third_party/bedrock", { skills: true, plugin: false }, gateOff],
    ["`env.CLAUDE_CODE_USE_BEDROCK` set to `\"1\"` in the user settings", userSettings, bedrock("1"), "third_party/bedrock", { skills: true, plugin: false }, gateOff],
    ["`env.CLAUDE_CODE_USE_BEDROCK` set to `\"0\"` in the user settings", userSettings, bedrock("0"), onAccount, both, on],
  ];
  for (const [what, file, settings, expected, expectedReads, shows] of settingsCases) {
    await p.check("account.off", `${what} ${shows}`, async () => {
      write(file, { ...(file === userSettings && keptSettings ? JSON.parse(keptSettings) : {}), ...settings });
      try {
        return same({ auth: await auth(), reads: reads(await session({ env: offline, timeout: 60_000 })) }, { auth: expected, reads: expectedReads });
      } finally {
        if (file === userSettings) putBackSettings();
        else fs.rmSync(file);
      }
    });
  }

  await p.check("account.off", "`primaryApiKey` in `.claude.json` leaves Claude Code on the account", async () => {
    write(state, { ...json(state), primaryApiKey: "not-a-real-secret" });
    try {
      return same({ auth: await auth(), reads: reads(await session({ env: offline })) }, { auth: onAccount, reads: both });
    } finally {
      putBackState();
    }
  });

  await p.check("account.off", "`CLAUDE_CODE_OAUTH_TOKEN` in the environment takes Claude Code off the account: it reads no synced skill or plugin", async () => {
    const env = { ...offline, CLAUDE_CODE_OAUTH_TOKEN: "not-a-real-secret" };
    return same({ auth: await auth({ env }), reads: reads(await session({ env })) }, { auth: "oauth_token/firstParty", reads: none });
  });

  await p.check("account.off", "`ANTHROPIC_API_KEY` in the environment leaves Claude Code on the account under `-p`: it reads the synced skills and plugins", async () => {
    const env = { ...offline, ANTHROPIC_API_KEY: "not-a-real-secret" };
    return same({ auth: await auth({ env }), reads: reads(await session({ env })) }, { auth: "api_key/firstParty", reads: both });
  });

  await p.check("account.off", "`claude auth status` does not tell whether an interactive session uses `ANTHROPIC_API_KEY`: it says the same whether `customApiKeyResponses` approves it, rejects it or holds nothing", async () => {
    const env = { ANTHROPIC_API_KEY: "not-a-real-secret" };
    const states = {};
    try {
      for (const [name, responses] of [["none", undefined], ["approved", { approved: ["not-a-real-secret"], rejected: [] }], ["rejected", { approved: [], rejected: ["not-a-real-secret"] }]]) {
        const s = json(state);
        if (responses) s.customApiKeyResponses = responses;
        else delete s.customApiKeyResponses;
        write(state, s);
        states[name] = await auth({ env });
      }
    } finally {
      putBackState();
    }
    return same(states, { none: "api_key/firstParty", approved: "api_key/firstParty", rejected: "api_key/firstParty" });
  });

  unplantPlugins();

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
    write(userSettings, { syncClaudeAiSkills: false });
    const s = await session();
    putBackSettings();
    return same({ listed: syncedListed(s), downloaded: downloaded().length, planted: fs.existsSync(path.join(bucket, "fold-s")) }, { listed: [], downloaded: 0, planted: true });
  });
} finally {
  // Put back: the user settings, `.claude.json`'s keys, the account's
  // skills downloaded again, the gate cached on.
  unplant();
  putBackSettings();
  putBackState();
  for (const dir of copies) fs.rmSync(dir, { recursive: true, force: true });
  console.log(`\nput back: ${await syncAgain()} of the account's skills downloaded again, gate cached ${cachedGate()}`);
}

p.finish();
