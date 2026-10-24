// Tests Claude Code's facts that `claude-code.toml` marked unverified, on
// throwaway homes and Projects. Every launch gets its own
// `CLAUDE_CONFIG_DIR`, the fake key `not-a-real-secret`, and a local
// stand-in for the Messages API, so no request leaves the machine but
// Claude Code's own fetch of its feature gates.
//
//   node uat/agents/claude-code.mjs [--bin <claude.exe>]
//
// What Claude Code loads is read from `claude mcp list`, `claude mcp get`,
// `claude plugin list`, the init event of `claude -p --output-format
// stream-json`, and the request it sends its model: the skill listing
// (`- <name> (<alias>): <description>`) and the body of a skill a prompt
// runs.
import fs from "node:fs";
import path from "node:path";
import { fresh, gitInit, json, modelServer, probe, run, same, skill, write } from "./lib.mjs";

const bin = process.argv.includes("--bin") ? process.argv[process.argv.indexOf("--bin") + 1] : "claude.exe";
const version = (await run(bin, ["--version"])).stdout.trim().split(" ")[0];
const p = probe("Claude Code", version);
const base = fresh("claude-code");

const stdio = (tag) => ({ command: "cmd", args: ["/c", tag] });
const reply = { id: "msg_x", type: "message", role: "assistant", model: "claude-x", content: [{ type: "text", text: "ok" }], stop_reason: "end_turn", usage: { input_tokens: 1, output_tokens: 1 } };

// One `claude -p` run: what its init event lists, and the skill lines of
// the request its model gets.
async function session(home, cwd, { prompt = "hi", args = [] } = {}) {
  const model = await modelServer(reply);
  const env = { CLAUDE_CONFIG_DIR: home, ANTHROPIC_API_KEY: "not-a-real-secret", ANTHROPIC_BASE_URL: model.url };
  const { stdout } = await run(bin, ["-p", prompt, "--output-format", "stream-json", "--verbose", ...args], { cwd, env });
  model.close();
  const init = JSON.parse(stdout.split("\n").find((l) => l.includes('"subtype":"init"')));
  const listing = model.bodies.map((b) => b.match(/The following skills are available(?:[^"\\]|\\.)*/)).find(Boolean);
  const listed = listing ? JSON.parse(`"${listing[0]}"`).split("\n").filter((l) => l.startsWith("- ")) : [];
  return {
    servers: init.mcp_servers.map((s) => s.name),
    plugins: init.plugins.filter((x) => x.path !== "builtin").map((x) => x.name),
    skills: init.skills,
    listed,
    sent: model.bodies.join("\n"),
  };
}

const claude = (home, cwd, ...args) => run(bin, args, { cwd, env: { CLAUDE_CONFIG_DIR: home } });

// `claude mcp list` as `{ name: status }`, the status cut to its first words.
async function mcpList(home, cwd) {
  const { stdout } = await claude(home, cwd, "mcp", "list");
  const status = {};
  for (const line of stdout.split("\n")) {
    const m = line.match(/^(\S+): .* - (✘ Failed|⏸ Pending approval|✔ Connected)/);
    if (m) status[m[1]] = m[2].replace(/^\S+ /, "");
  }
  return { status, text: stdout };
}

const throwaway = (names) => names.filter((n) => /throwaway|fold/i.test(n));

// ---------------------------------------------------------------- .mcp.json

{
  const dir = fresh("parents", base);
  const home = path.join(dir, "home");
  write(path.join(home, "settings.json"), { enableAllProjectMcpServers: true });
  const outer = path.join(dir, "outer");
  const repo = path.join(outer, "repo");
  await gitInit(repo);
  write(path.join(outer, ".mcp.json"), { mcpServers: { "cd-throwaway-outer": stdio("outer"), "cd-throwaway-same": stdio("outer") } });
  write(path.join(repo, ".mcp.json"), { mcpServers: { "cd-throwaway-repo": stdio("repo"), "cd-throwaway-same": stdio("repo") } });
  write(path.join(repo, "sub", ".mcp.json"), { mcpServers: { "cd-throwaway-sub": stdio("sub"), "cd-throwaway-same": stdio("sub") } });
  const deep = path.join(repo, "sub", "deep");
  fs.mkdirSync(deep, { recursive: true });

  await p.check("locations[project-mcp]", "`.mcp.json` is read in every folder above the Project, past the repo root", async () => {
    const s = await session(home, deep);
    return same(throwaway(s.servers).sort(), ["cd-throwaway-outer", "cd-throwaway-repo", "cd-throwaway-same", "cd-throwaway-sub"]);
  });
  await p.check("locations[project-mcp]", "of same-named servers in those files, the nearest folder's wins", async () => {
    const { stdout } = await claude(home, deep, "mcp", "get", "cd-throwaway-same");
    return same(stdout.match(/Args: (.*)/)?.[1].trim(), "/c sub");
  });

  const plain = path.join(dir, "plain");
  write(path.join(plain, ".mcp.json"), '{ "mcpServers": { broken');
  write(path.join(home, ".claude.json"), { mcpServers: { "cd-throwaway-user": stdio("user") } });
  await p.check("locations[project-mcp]", "a `.mcp.json` that does not parse is skipped with a diagnostic, and the other servers load", async () => {
    const list = await mcpList(home, plain);
    const diagnostic = list.text.includes("[Failed to parse] Project config (shared via .mcp.json)");
    return same({ diagnostic, servers: Object.keys(list.status) }, { diagnostic: true, servers: ["cd-throwaway-user"] });
  });
}

// ---------------------------------------------------------------- approvals

{
  const dir = fresh("approvals", base);
  const home = path.join(dir, "home");
  const proj = path.join(dir, "proj");
  write(path.join(proj, ".mcp.json"), { mcpServers: { "cd-throwaway-a": stdio("a"), "cd-throwaway-b": stdio("b") } });
  // Claude Code keys a Project by its path with forward slashes.
  const key = proj.replaceAll("\\", "/");
  const reset = (state = {}) => {
    for (const f of [path.join(home, "settings.json"), path.join(proj, ".claude", "settings.json"), path.join(proj, ".claude", "settings.local.json")]) fs.rmSync(f, { force: true });
    write(path.join(home, ".claude.json"), state);
  };
  const approved = async () => {
    const { status } = await mcpList(home, proj);
    return ["cd-throwaway-a", "cd-throwaway-b"].map((n) => (status[n] === undefined ? "absent" : status[n] === "Pending approval" ? "pending" : "approved"));
  };

  await p.check("items.mcp.gates[0]", "the Project's state in `~/.claude.json` approves by `enableAllProjectMcpServers`, once the Project is trusted", async () => {
    reset({ projects: { [key]: { hasTrustDialogAccepted: true, enableAllProjectMcpServers: true } } });
    const trusted = await approved();
    reset({ projects: { [key]: { enableAllProjectMcpServers: true } } });
    const untrusted = await approved();
    reset({ projects: { [proj]: { hasTrustDialogAccepted: true, enableAllProjectMcpServers: true } } });
    const backslashes = await approved();
    return same({ trusted, untrusted, backslashes }, { trusted: ["approved", "approved"], untrusted: ["pending", "pending"], backslashes: ["pending", "pending"] });
  });

  await p.check("items.mcp.gates[0]", "the Project's settings files approve only in a trusted Project, the user's settings in any", async () => {
    const seen = {};
    for (const [label, file] of [["project", path.join(proj, ".claude", "settings.json")], ["local", path.join(proj, ".claude", "settings.local.json")], ["user", path.join(home, "settings.json")]]) {
      reset();
      write(file, { enableAllProjectMcpServers: true });
      seen[`${label} untrusted`] = (await approved())[0];
      reset({ projects: { [key]: { hasTrustDialogAccepted: true } } });
      write(file, { enableAllProjectMcpServers: true });
      seen[`${label} trusted`] = (await approved())[0];
    }
    return same(seen, { "project untrusted": "pending", "project trusted": "approved", "local untrusted": "pending", "local trusted": "approved", "user untrusted": "approved", "user trusted": "approved" });
  });

  await p.check("items.mcp.gates[0]", "`enabledMcpjsonServers` written as one plain-string name approves that server in a settings file, and nothing in the Project's state", async () => {
    const seen = {};
    for (const [label, file] of [["project", path.join(proj, ".claude", "settings.json")], ["local", path.join(proj, ".claude", "settings.local.json")], ["user", path.join(home, "settings.json")]]) {
      reset({ projects: { [key]: { hasTrustDialogAccepted: true } } });
      write(file, { enabledMcpjsonServers: "cd-throwaway-a" });
      seen[label] = await approved();
    }
    reset({ projects: { [key]: { hasTrustDialogAccepted: true, enabledMcpjsonServers: "cd-throwaway-a" } } });
    seen.state = await approved();
    const one = ["approved", "pending"];
    return same(seen, { project: one, local: one, user: one, state: ["pending", "pending"] });
  });

  await p.check("items.mcp.disable[1]", "`disabledMcpjsonServers` written as one plain-string name skips that server, in every settings file, trusted or not", async () => {
    const seen = {};
    for (const [label, file] of [["project", path.join(proj, ".claude", "settings.json")], ["local", path.join(proj, ".claude", "settings.local.json")], ["user", path.join(home, "settings.json")]]) {
      reset();
      write(path.join(home, "settings.json"), { enableAllProjectMcpServers: true });
      const settings = fs.existsSync(file) ? json(file) : {};
      write(file, { ...settings, disabledMcpjsonServers: "cd-throwaway-a" });
      seen[label] = await approved();
    }
    const one = ["absent", "approved"];
    return same(seen, { project: one, local: one, user: one });
  });
}

// ---------------------------------------------------------------- plugins

{
  const dir = fresh("plugins", base);
  const home = path.join(dir, "home");
  const proj = path.join(dir, "proj");
  const market = path.join(dir, "market");
  fs.mkdirSync(proj, { recursive: true });
  const plugins = {
    "cd-throwaway-p1": { "cd-throwaway-ep1": { type: "http", url: "http://127.0.0.1:9/mcp" } },
    "cd-throwaway-p2": { "cd-throwaway-ep2": { type: "http", url: "http://127.0.0.1:9/mcp" } },
    "cd-throwaway-p3": { "cd-throwaway-p3srv": stdio("p3") },
  };
  for (const [name, servers] of Object.entries(plugins)) {
    write(path.join(market, name, ".claude-plugin", "plugin.json"), { name, version: "1.0.0" });
    write(path.join(market, name, ".mcp.json"), { mcpServers: servers });
  }
  write(path.join(market, ".claude-plugin", "marketplace.json"), { name: "cd-throwaway-market", owner: { name: "cd-throwaway" }, plugins: Object.keys(plugins).map((name) => ({ name, source: `./${name}` })) });
  await claude(home, proj, "plugin", "marketplace", "add", market);
  for (const name of Object.keys(plugins)) await claude(home, proj, "plugin", "install", `${name}@cd-throwaway-market`);
  const settingsFile = path.join(home, "settings.json");
  const installed = json(settingsFile);
  const id = (n) => `cd-throwaway-p${n}@cd-throwaway-market`;
  const withEnabled = (enabledPlugins) => write(settingsFile, { ...installed, enabledPlugins });

  await p.check("items.mcp.disable[6]", "a plugin missing from every settings file is off", async () => {
    const { enabledPlugins, ...rest } = installed;
    write(settingsFile, rest);
    const s = await session(home, proj);
    write(settingsFile, installed);
    return same({ plugins: s.plugins, servers: throwaway(s.servers) }, { plugins: [], servers: [] });
  });

  await p.check("items.mcp", "of two plugins' servers with one endpoint, the one whose plugin comes first in `enabledPlugins` wins", async () => {
    withEnabled({ [id(1)]: true, [id(2)]: true, [id(3)]: true });
    const first = throwaway((await session(home, proj)).servers);
    withEnabled({ [id(3)]: true, [id(2)]: true, [id(1)]: true });
    const reversed = throwaway((await session(home, proj)).servers);
    withEnabled(installed.enabledPlugins);
    return same({ first, reversed }, { first: ["plugin:cd-throwaway-p1:cd-throwaway-ep1", "plugin:cd-throwaway-p3:cd-throwaway-p3srv"], reversed: ["plugin:cd-throwaway-p3:cd-throwaway-p3srv", "plugin:cd-throwaway-p2:cd-throwaway-ep2"] });
  });

  await p.check("items.mcp", "`enabledPlugins` merges user, then Project, then local settings, a key keeping its first place", async () => {
    const key = proj.replaceAll("\\", "/");
    const state = json(path.join(home, ".claude.json"));
    write(path.join(home, ".claude.json"), { ...state, projects: { [key]: { hasTrustDialogAccepted: true } } });
    withEnabled({ [id(3)]: true });
    write(path.join(proj, ".claude", "settings.json"), { enabledPlugins: { [id(1)]: true } });
    write(path.join(proj, ".claude", "settings.local.json"), { enabledPlugins: { [id(2)]: true } });
    const servers = throwaway((await session(home, proj)).servers);
    fs.rmSync(path.join(proj, ".claude"), { recursive: true });
    withEnabled(installed.enabledPlugins);
    return same(servers, ["plugin:cd-throwaway-p3:cd-throwaway-p3srv", "plugin:cd-throwaway-p1:cd-throwaway-ep1"]);
  });

  await p.check("locations[plugin-mcp]", "a plugin of a `directory` marketplace is read from the marketplace's folder, not from the cache", async () => {
    const { stdout } = await claude(home, proj, "plugin", "list");
    return same(stdout.includes(`Read from: ${path.join(market, "cd-throwaway-p3")}`), true);
  });

  await p.check("locations[plugin-mcp]", "a plugin's `.mcp.json` that does not parse leaves the plugin loaded without its servers", async () => {
    const file = path.join(market, "cd-throwaway-p3", ".mcp.json");
    const kept = fs.readFileSync(file, "utf8");
    write(file, '{ "mcpServers": { broken');
    const s = await session(home, proj);
    write(file, kept);
    return same({ loaded: s.plugins.includes("cd-throwaway-p3"), servers: s.servers.filter((n) => n.includes("p3")) }, { loaded: true, servers: [] });
  });

  await p.check("locations[plugin-manifest-mcp]", "a plugin whose `plugin.json` does not parse fails to load, servers of its `.mcp.json` included", async () => {
    const file = path.join(market, "cd-throwaway-p3", ".claude-plugin", "plugin.json");
    const kept = fs.readFileSync(file, "utf8");
    write(file, '{ "name": broken');
    const s = await session(home, proj);
    const { stdout } = await claude(home, proj, "plugin", "list");
    write(file, kept);
    return same({ loaded: s.plugins.includes("cd-throwaway-p3"), servers: s.servers.filter((n) => n.includes("p3")), error: stdout.includes("Manifest parse error") }, { loaded: false, servers: [], error: true });
  });
}

// ---------------------------------------------------------------- skills

{
  const dir = fresh("skills", base);
  const home = path.join(dir, "home");
  const repo = path.join(dir, "repo");
  const sub = path.join(repo, "sub");
  await gitInit(repo);
  skill(path.join(repo, ".claude", "skills", "cd-throwaway-dupe"), ["name: cd-throwaway-dupe", "description: from the repo root"]);
  skill(path.join(sub, ".claude", "skills", "cd-throwaway-dupe"), ["name: cd-throwaway-dupe", "description: from sub"]);
  await p.check("locations[project-skills]", "of same-named skills in the Project's folder and a folder above it, the nearest wins", async () => {
    const s = await session(home, sub);
    return same(s.listed.filter((l) => l.includes("cd-throwaway-dupe")), ["- cd-throwaway-dupe: from sub"]);
  });

  const proj = path.join(dir, "proj");
  const skills = path.join(proj, ".claude", "skills");
  skill(path.join(skills, "fold-a"), ["name: cd-throwaway-named", "description: project fold-a"], "body-of-fold-a");
  skill(path.join(skills, "fold-b"), ["name: cd-throwaway-project-b", "description: project fold-b"]);
  skill(path.join(home, "skills", "fold-b"), ["name: cd-throwaway-user-b", "description: user fold-b"]);
  skill(path.join(skills, "cd-throwaway-Upper_Case"), ["name: cd-throwaway-Upper_Case", "description: upper"]);

  await p.check("items.skill", "a skill is named by its folder, and competes by it: a user skill shadows a Project skill of the same folder name", async () => {
    const s = await session(home, proj);
    return same(throwaway(s.listed).sort(), ["- cd-throwaway-Upper_Case: upper", "- fold-a (cd-throwaway-named): project fold-a", "- fold-b (cd-throwaway-user-b): user fold-b"]);
  });
  await p.check("items.skill", "the frontmatter `name` is an alias: `/<folder>` and `/<name>` both run the skill", async () => {
    const byFolder = (await session(home, proj, { prompt: "/fold-a" })).sent.includes("body-of-fold-a");
    const byName = (await session(home, proj, { prompt: "/cd-throwaway-named" })).sent.includes("body-of-fold-a");
    return same({ byFolder, byName }, { byFolder: true, byName: true });
  });
  await p.check("items.skill", "names are used as written, upper case and `_` included", async () => {
    const s = await session(home, proj);
    return same(s.skills.includes("cd-throwaway-Upper_Case"), true);
  });
  await p.check("items.skill.disable[0]", "`skillOverrides` turns a skill off by its folder name, not its frontmatter `name`", async () => {
    const local = path.join(proj, ".claude", "settings.local.json");
    write(local, { skillOverrides: { "fold-a": "off" } });
    const byFolder = (await session(home, proj)).skills.includes("fold-a");
    write(local, { skillOverrides: { "cd-throwaway-named": "off" } });
    const byName = (await session(home, proj)).skills.includes("fold-a");
    fs.rmSync(local);
    return same({ listedWhenOffByFolder: byFolder, listedWhenOffByName: byName }, { listedWhenOffByFolder: false, listedWhenOffByName: true });
  });

  const plugin = path.join(dir, "cd-throwaway-pk");
  write(path.join(plugin, ".claude-plugin", "plugin.json"), { name: "cd-throwaway-pk", version: "1.0.0" });
  skill(path.join(plugin, "skills", "fold-p"), ["name: cd-throwaway-pname", "description: plugin skill"]);
  await p.check("locations[plugin-skills]", "a plugin's skill is named `<plugin>:<folder>`, its frontmatter `name` an alias", async () => {
    const s = await session(home, fresh("empty", dir), { args: ["--plugin-dir", plugin] });
    return same(s.listed.filter((l) => l.includes("cd-throwaway-pk")), ["- cd-throwaway-pk:fold-p (cd-throwaway-pk:cd-throwaway-pname): plugin skill"]);
  });

  // The name rule: a frontmatter `name` it refuses is no alias, and a folder
  // it refuses is no skill.
  const rule = path.join(dir, "rule");
  const names = {
    "inner space": "cd throwaway spaced",
    "trailing space": '"cd-throwaway-trailing "',
    "space after colon": '"cd-throwaway: spaced"',
    "space before colon": '"cd-throwaway :spaced"',
    tab: '"cd-throwaway\\ttab"',
    slash: "cd-throwaway/slash",
    "less than": "cd-throwaway<lt",
    "greater than": "cd-throwaway>gt",
    "zero-width space": "cd-throwaway​zwsp",
    "right-to-left override": "cd-throwaway‮rlo",
    "no-break space": "cd-throwaway nbsp",
    "257 characters": `cd-throwaway-${"x".repeat(244)}`,
    "256 characters": `cd-throwaway-${"x".repeat(243)}`,
    "upper case": "CD-THROWAWAY-UPPER",
    underscore: "cd_throwaway_under",
    dot: "cd-throwaway.dot",
    "non-ASCII letters": "cd-throwaway-ñandú",
    "inner colon": "cd-throwaway:colon",
    "leading colon": '":cd-throwaway-lead"',
    "trailing colon": '"cd-throwaway-trail:"',
  };
  const keys = Object.keys(names);
  keys.forEach((k, i) => skill(path.join(rule, ".claude", "skills", `fold-n${i}`), [`name: ${names[k]}`, `description: d${i}`]));
  const folders = {
    "leading space": " cd-throwaway-f-lead",
    "trailing space": "cd-throwaway-f-trail ",
    "zero-width space": "cd-throwaway-f​zw",
    "right-to-left override": "cd-throwaway-f‮rlo",
    "no-break space": "cd-throwaway-f nbsp",
    "ideographic space": "cd-throwaway-f　ideo",
    "inner space": "cd-throwaway-f inner",
    dot: "cd-throwaway-f.dot",
    "non-ASCII letters": "cd-throwaway-fÑ",
  };
  for (const folder of Object.values(folders)) {
    // `\\?\` keeps the spaces Windows would otherwise trim from a folder name.
    const target = `\\\\?\\${path.join(rule, ".claude", "skills", folder)}`;
    fs.mkdirSync(target, { recursive: true });
    fs.writeFileSync(`${target}\\SKILL.md`, "---\ndescription: folder rule\n---\nbody\n");
  }
  const s = await session(home, rule);
  await p.check("items.skill", "the name rule refuses a frontmatter `name` with any blank, `/`, `<`, `>`, a hidden or direction character, or over 256 characters", async () => {
    const refused = keys.filter((k, i) => !s.listed.some((l) => l.startsWith(`- fold-n${i} (`)));
    return same(refused, keys.slice(0, 12));
  });
  await p.check("items.skill", "the name rule skips a skill folder whose name starts or ends with a space, or holds a hidden, direction or blank-like character", async () => {
    const skipped = Object.entries(folders).filter(([, f]) => !s.skills.includes(f)).map(([k]) => k);
    return same(skipped, Object.keys(folders).slice(0, 6));
  });

  // Facts that need a claude.ai account: with an API key, neither is read.
  const synced = path.join(dir, "synced-home");
  skill(path.join(synced, "skills", "synced", "cd-throwaway-org_cd-throwaway-acct", "cd-throwaway-sync"), ["name: cd-throwaway-sync", "description: synced"]);
  await p.check("locations[synced-skills]", "with an API key, the synced skills folder is not read, whatever the gate", async () => {
    const t = await session(synced, fresh("empty", dir));
    return same(t.skills.filter((n) => n.includes("cd-throwaway-sync")), []);
  });
}

{
  const dir = fresh("remote-settings", base);
  const home = path.join(dir, "home");
  write(path.join(home, ".claude.json"), { mcpServers: { "cd-throwaway-a": stdio("a"), "cd-throwaway-b": stdio("b") } });
  write(path.join(home, "remote-settings.json"), { deniedMcpServers: [{ serverName: "cd-throwaway-a" }] });
  await p.check("settings[managed]", "with an API key, `remote-settings.json` is not read", async () => {
    const s = await session(home, fresh("empty", dir));
    return same(throwaway(s.servers), ["cd-throwaway-a", "cd-throwaway-b"]);
  });
}

p.finish();
