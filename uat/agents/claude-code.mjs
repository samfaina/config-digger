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
// (`- <name> (<alias>): <description>`), before and after the stand-in
// answers with a Read tool call, and the body of a skill a prompt runs, and
// what Claude Code extracts into a plugin's `.mcpb-cache`.
import fs from "node:fs";
import path from "node:path";
import { fresh, gitInit, json, modelServer, pluginStatus, probe, run, same, sameIn, skill, subst, unsubst, write } from "./lib.mjs";

const bin = process.argv.includes("--bin") ? process.argv[process.argv.indexOf("--bin") + 1] : "claude.exe";
const version = (await run(bin, ["--version"])).stdout.trim().split(" ")[0];
const p = probe("Claude Code", version);
const base = fresh("claude-code");

const stdio = (tag) => ({ command: "cmd", args: ["/c", tag] });
const reply = { id: "msg_x", type: "message", role: "assistant", model: "claude-x", content: [{ type: "text", text: "ok" }], stop_reason: "end_turn", usage: { input_tokens: 1, output_tokens: 1 } };

// One `claude -p` run: what its init event lists, and the skill lines of
// the first request its model gets. With `read`, the model first answers
// with a Read tool call on that file, and `listedAfter` holds the skill
// lines of the last request, which follows the call: those listed at the
// start, and those listed since.
async function session(home, cwd, { prompt = "hi", args = [], extraEnv = {}, read } = {}) {
  const readCall = { ...reply, content: [{ type: "tool_use", id: "toolu_cd", name: "Read", input: { file_path: read } }], stop_reason: "tool_use" };
  const answer = (body) => (body.includes('"tool_result"') || !body.includes('"tools"') ? reply : readCall);
  const model = await modelServer(read === undefined ? reply : answer);
  const env = { CLAUDE_CONFIG_DIR: home, ANTHROPIC_API_KEY: "not-a-real-secret", ANTHROPIC_BASE_URL: model.url, ...extraEnv };
  const { stdout } = await run(bin, ["-p", prompt, "--output-format", "stream-json", "--verbose", ...args], { cwd, env });
  model.close();
  const init = JSON.parse(stdout.split("\n").find((l) => l.includes('"subtype":"init"')));
  // A later request repeats the earlier listings before its own.
  const listings = model.bodies.map((b) => [...b.matchAll(/The following skills are available(?:[^"\\]|\\.)*/g)]).filter((l) => l.length > 0);
  const lines = (listing) => (listing ? JSON.parse(`"${listing[0]}"`).split("\n").filter((l) => l.startsWith("- ")) : []);
  return {
    servers: init.mcp_servers.map((s) => s.name),
    plugins: init.plugins.filter((x) => x.path !== "builtin").map((x) => x.name),
    skills: init.skills,
    listed: lines(listings[0]?.[0]),
    listedAfter: (listings.at(-1) ?? []).flatMap(lines),
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

// A skill folder named `folder` in `dir`, whose `SKILL.md` has no `name`.
// `\\?\` keeps the spaces Windows would otherwise trim from a folder name.
function verbatimSkill(dir, folder) {
  const target = `\\\\?\\${path.join(dir, folder)}`;
  fs.mkdirSync(target, { recursive: true });
  fs.writeFileSync(`${target}\\SKILL.md`, "---\ndescription: folder rule\n---\nbody\n");
}

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
  // A parent folder's settings are not the Project's: `sub`'s list leaves
  // its own server read, and only the Project's list skips one.
  write(path.join(repo, "sub", ".claude", "settings.local.json"), { disabledMcpjsonServers: ["cd-throwaway-sub"] });
  write(path.join(deep, ".claude", "settings.local.json"), { disabledMcpjsonServers: ["cd-throwaway-outer"] });
  await p.check("items.mcp.disable[1]", "a parent folder's server is skipped by the Project's own `disabledMcpjsonServers`, not by the parent folder's", async () => {
    const s = await session(home, deep);
    return same(throwaway(s.servers).sort(), ["cd-throwaway-repo", "cd-throwaway-same", "cd-throwaway-sub"]);
  });

  // The user's home folder, moved to a throwaway one, with a Project below it.
  const userHome = path.join(dir, "user-home");
  write(path.join(userHome, ".mcp.json"), { mcpServers: { "cd-throwaway-home": stdio("home") } });
  const belowHome = path.join(userHome, "proj");
  fs.mkdirSync(belowHome, { recursive: true });
  await p.check("locations[project-mcp]", "the home folder's `.mcp.json` is read as a Project file when the Project is below it", async () => {
    const s = await session(home, belowHome, { extraEnv: { USERPROFILE: userHome, HOME: userHome } });
    return same(throwaway(s.servers), ["cd-throwaway-home"]);
  });

  // The root of a drive `subst` maps to a throwaway folder, never a real one.
  const drive = path.join(dir, "drive");
  write(path.join(drive, ".mcp.json"), { mcpServers: { "cd-throwaway-root": stdio("root") } });
  write(path.join(drive, "top", ".mcp.json"), { mcpServers: { "cd-throwaway-top": stdio("top") } });
  fs.mkdirSync(path.join(drive, "top", "proj"), { recursive: true });
  const letter = await subst(drive);
  try {
    await p.check("locations[project-mcp]", "the `.mcp.json` at the root of the drive is not read", async () => {
      const s = await session(home, `${letter}:\\top\\proj`);
      return same(throwaway(s.servers), ["cd-throwaway-top"]);
    });
  } finally {
    await unsubst(letter);
  }

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
  const approved = async (cwd = proj) => {
    const { status } = await mcpList(home, cwd);
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

  // The forward-slash key spelled in another case: its drive letter, its
  // last folder's name, and the whole key.
  const swapCase = (s) => (s === s.toLowerCase() ? s.toUpperCase() : s.toLowerCase());
  const spellings = {
    drive: swapCase(key[0]) + key.slice(1),
    folder: key.slice(0, key.lastIndexOf("/") + 1) + swapCase(path.basename(proj)),
    whole: key.toUpperCase(),
  };

  await p.check("items.mcp.gates[0]", "the Project's state under its forward-slash key in another case is not found: neither its trust nor its approvals count, and the Project's settings do not approve", async () => {
    const seen = {};
    for (const [label, spelled] of Object.entries(spellings)) {
      reset({ projects: { [spelled]: { hasTrustDialogAccepted: true, enableAllProjectMcpServers: true } } });
      seen[`${label} state`] = await approved();
      reset({ projects: { [spelled]: { hasTrustDialogAccepted: true } } });
      write(path.join(proj, ".claude", "settings.json"), { enableAllProjectMcpServers: true });
      seen[`${label} settings`] = await approved();
    }
    const none = ["pending", "pending"];
    return same(seen, { "drive state": none, "drive settings": none, "folder state": none, "folder settings": none, "whole state": none, "whole settings": none });
  });

  await p.check("items.mcp.gates[0]", "started in the Project spelled in another case, Claude Code finds the state under that spelling, and not under the Project's own", async () => {
    const seen = {};
    for (const [label, spelled] of Object.entries({ drive: spellings.drive, folder: spellings.folder })) {
      for (const [under, stateKey] of [["started", spelled], ["own", key]]) {
        reset({ projects: { [stateKey]: { hasTrustDialogAccepted: true, enableAllProjectMcpServers: true } } });
        seen[`${label}, under ${under}`] = await approved(spelled.replaceAll("/", "\\"));
      }
    }
    const all = ["approved", "approved"];
    const none = ["pending", "pending"];
    return same(seen, { "drive, under started": all, "drive, under own": none, "folder, under started": all, "folder, under own": none });
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
  // Each plugin's folder, nested below the marketplace's, as its entry's `source` names it.
  const folder = (name) => path.join(market, "plugins", name);
  fs.mkdirSync(proj, { recursive: true });
  const plugins = {
    "cd-throwaway-p1": { "cd-throwaway-ep1": { type: "http", url: "http://127.0.0.1:9/mcp" } },
    "cd-throwaway-p2": { "cd-throwaway-ep2": { type: "http", url: "http://127.0.0.1:9/mcp" } },
    "cd-throwaway-p3": { "cd-throwaway-p3srv": stdio("p3") },
  };
  for (const [name, servers] of Object.entries(plugins)) {
    write(path.join(folder(name), ".claude-plugin", "plugin.json"), { name, version: "1.0.0" });
    write(path.join(folder(name), ".mcp.json"), { mcpServers: servers });
  }
  skill(path.join(folder("cd-throwaway-p3"), "skills", "cd-throwaway-p3skill"), ["name: cd-throwaway-p3skill", "description: p3's skill"]);
  write(path.join(market, ".claude-plugin", "marketplace.json"), { name: "cd-throwaway-market", owner: { name: "cd-throwaway" }, plugins: Object.keys(plugins).map((name) => ({ name, source: `./plugins/${name}` })) });
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

  await p.check("locations[plugin-mcp]", "a plugin of a `directory` marketplace is read from the folder its entry's `source` names in the marketplace's folder, not from the cache", async () => {
    const { stdout } = await claude(home, proj, "plugin", "list");
    return same(stdout.includes(`Read from: ${folder("cd-throwaway-p3")}`), true);
  });

  await p.check("locations[plugin-mcp]", "a plugin's `.mcp.json` that does not parse leaves the plugin loaded without its servers", async () => {
    const file = path.join(folder("cd-throwaway-p3"), ".mcp.json");
    const kept = fs.readFileSync(file, "utf8");
    write(file, '{ "mcpServers": { broken');
    const s = await session(home, proj);
    write(file, kept);
    return same({ loaded: s.plugins.includes("cd-throwaway-p3"), servers: s.servers.filter((n) => n.includes("p3")) }, { loaded: true, servers: [] });
  });

  await p.check("locations[plugin-manifest-mcp]", "a plugin whose `plugin.json` does not parse fails to load, servers of its `.mcp.json` included", async () => {
    const file = path.join(folder("cd-throwaway-p3"), ".claude-plugin", "plugin.json");
    const kept = fs.readFileSync(file, "utf8");
    write(file, '{ "name": broken');
    const s = await session(home, proj);
    const { stdout } = await claude(home, proj, "plugin", "list");
    write(file, kept);
    return same({ loaded: s.plugins.includes("cd-throwaway-p3"), servers: s.servers.filter((n) => n.includes("p3")), error: stdout.includes("Manifest parse error") }, { loaded: false, servers: [], error: true });
  });

  // The same plugin, with a manifest that loads or one that fails either
  // way, on, off by `false` and off by default: whether it loads, its
  // server, its skill, and what `claude plugin list` says of it.
  const offSeen = (async () => {
    const file = path.join(folder("cd-throwaway-p3"), ".claude-plugin", "plugin.json");
    const kept = fs.readFileSync(file, "utf8");
    const { enabledPlugins, ...offByDefault } = installed;
    const seen = {};
    try {
      for (const [manifest, text] of [["loads", kept], ["does not parse", '{ "name": broken'], ["mcpServers 42", JSON.stringify({ ...JSON.parse(kept), mcpServers: 42 })]]) {
        write(file, text);
        for (const [switched, settings] of [["on", { ...installed, enabledPlugins: { [id(3)]: true } }], ["false", { ...installed, enabledPlugins: { [id(3)]: false } }], ["by default", offByDefault]]) {
          write(settingsFile, settings);
          const s = await session(home, proj);
          const { stdout } = await claude(home, proj, "plugin", "list");
          seen[`${manifest}, ${switched}`] = {
            loaded: s.plugins.includes("cd-throwaway-p3"),
            servers: s.servers.filter((n) => n.includes("cd-throwaway-p3")),
            skills: s.skills.filter((n) => n.includes("cd-throwaway-p3")),
            ...pluginStatus(stdout, id(3)),
          };
        }
      }
    } finally {
      write(file, kept);
      write(settingsFile, installed);
    }
    return seen;
  })();
  const fails = (error) => ({ loaded: false, servers: [], skills: [], status: "failed to load", error });
  const off = { loaded: false, servers: [], skills: [], status: "disabled", error: null };
  const offExpected = {
    "loads, on": { loaded: true, servers: ["plugin:cd-throwaway-p3:cd-throwaway-p3srv"], skills: ["cd-throwaway-p3:cd-throwaway-p3skill"], status: "enabled", error: null },
    "loads, false": off, "loads, by default": off,
    "does not parse, on": fails("Manifest parse error"), "does not parse, false": off, "does not parse, by default": off,
    "mcpServers 42, on": fails("Manifest validation failed"), "mcpServers 42, false": off, "mcpServers 42, by default": off,
  };

  await p.check("items.mcp", "an installed plugin whose manifest does not parse, or whose `mcpServers` is `42`, off by `false` or by default, is not loaded and has no server, as with a manifest that loads, and `claude plugin list` says `disabled` with no error; on, it says `failed to load` and the manifest's error", async () => {
    return sameIn(["loaded", "servers", "status", "error"], await offSeen, offExpected);
  });

  await p.check("items.skill", "an installed plugin whose manifest fails, off by `false` or by default, has no skill, as with a manifest that loads, and `claude plugin list` reports no error", async () => {
    return sameIn(["skills", "status", "error"], await offSeen, offExpected);
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
  for (const folder of Object.values(folders)) verbatimSkill(path.join(rule, ".claude", "skills"), folder);
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

// ---------------------------------------------------------------- nested skills

{
  const dir = fresh("nested-skills", base);
  const home = path.join(dir, "home");
  const repo = path.join(dir, "repo");
  await gitInit(repo);
  const nested = (folder, name, description) => skill(path.join(repo, folder, ".claude", "skills", name), [`description: ${description}`]);
  nested(".", "cd-throwaway-dupe", "project dupe");
  skill(path.join(repo, ".claude", "skills", "fold-alias"), ["name: cd-throwaway-alias", "description: project alias"]);
  skill(path.join(home, "skills", "cd-throwaway-mine"), ["description: user mine"]);
  for (const name of ["cd-throwaway-web", "cd-throwaway-dupe", "cd-throwaway-mine", "cd-throwaway-alias", "cd-throwaway-twin"]) nested("web", name, `web ${name}`);
  for (const name of ["cd-throwaway-deep", "cd-throwaway-twin"]) nested("web/src/deep", name, `deep ${name}`);
  nested("sibling", "cd-throwaway-sibling", "sibling");
  write(path.join(repo, "sibling", "file.txt"), "sibling\n");
  // Git leaves out the folder holding `gen`'s `.claude`, and only the
  // `.claude` folder of `deep`.
  write(path.join(repo, ".gitignore"), "web/src/gen/\n");
  write(path.join(repo, "web", "src", "deep", ".gitignore"), ".claude/\n");
  nested("web/src/gen", "cd-throwaway-ignored", "ignored");
  const deepFile = path.join(repo, "web", "src", "deep", "file.txt");
  const genFile = path.join(repo, "web", "src", "gen", "x", "file.txt");
  write(deepFile, "deep\n");
  write(genFile, "gen\n");
  // The name of each line, such as `web:cd-throwaway-dupe` for
  // `- web:cd-throwaway-dupe: …`.
  const names = (lines) => throwaway(lines.map((l) => l.slice(2, l.indexOf(": "))));
  const deep = await session(home, repo, { read: deepFile });

  await p.check("locations[nested-skills]", "a nested skill is not listed before a file under its folder is read, and is listed after", async () => {
    const listed = (lines) => names(lines).includes("cd-throwaway-web");
    return same({ before: listed(deep.listed), after: listed(deep.listedAfter) }, { before: false, after: true });
  });
  await p.check("locations[nested-skills]", "one read loads the skills of every folder above the file and below the Project, at depth 1 and 3", async () => {
    return same(["cd-throwaway-web", "cd-throwaway-deep"].map((n) => names(deep.listedAfter).includes(n)), [true, true]);
  });
  await p.check("locations[nested-skills]", "a sibling folder no file was read under stays unlisted", async () => {
    return same(names(deep.listedAfter).filter((n) => n.includes("sibling")), []);
  });
  await p.check("locations[nested-skills]", "a nested skills folder whose holding folder git leaves out is not listed, and one whose `.claude` folder alone it leaves out is", async () => {
    const gen = await session(home, repo, { read: genFile });
    const listed = names(gen.listedAfter);
    return same({ ignored: listed.includes("cd-throwaway-ignored"), web: listed.includes("cd-throwaway-web"), deep: names(deep.listedAfter).includes("cd-throwaway-deep") }, { ignored: false, web: true, deep: true });
  });
  await p.check("items.skill", "a nested skill keeps its folder's name unless a Project, user or other nested skill has it: it is then `<folder>:<name>`, the other keeping its own, and a frontmatter alias does not count", async () => {
    const listed = names(deep.listedAfter).filter((n) => /dupe|mine|alias|twin/.test(n)).sort();
    return same(listed, [
      "cd-throwaway-alias",
      "cd-throwaway-dupe",
      "cd-throwaway-mine",
      "fold-alias (cd-throwaway-alias)",
      "web/src/deep:cd-throwaway-twin",
      "web:cd-throwaway-dupe",
      "web:cd-throwaway-mine",
      "web:cd-throwaway-twin",
    ]);
  });
  await p.check("items.skill.disable[0]", "`skillOverrides` turns a nested skill off by the name it is listed under, and one listed as `<folder>:<name>` by its folder's name too", async () => {
    const local = path.join(repo, ".claude", "settings.local.json");
    const off = async (key) => {
      write(local, { skillOverrides: { [key]: "off" } });
      return names((await session(home, repo, { read: deepFile })).listedAfter);
    };
    const byName = await off("cd-throwaway-web");
    const byPrefixed = await off("web:cd-throwaway-dupe");
    const byFolder = await off("cd-throwaway-twin");
    fs.rmSync(local);
    return same(
      { byName: byName.includes("cd-throwaway-web"), byPrefixed: ["web:cd-throwaway-dupe", "cd-throwaway-dupe"].map((n) => byPrefixed.includes(n)), byFolder: ["web:cd-throwaway-twin", "web/src/deep:cd-throwaway-twin"].map((n) => byFolder.includes(n)) },
      { byName: false, byPrefixed: [false, true], byFolder: [false, false] },
    );
  });

  const plain = path.join(dir, "plain");
  skill(path.join(plain, "web", ".claude", "skills", "cd-throwaway-plain"), ["description: plain"]);
  write(path.join(plain, "web", "file.txt"), "plain\n");
  await p.check("locations[nested-skills]", "outside a git repo, a nested skill loads once a file under its folder is read", async () => {
    const s = await session(home, plain, { read: path.join(plain, "web", "file.txt") });
    return same(names(s.listedAfter).includes("cd-throwaway-plain"), true);
  });
}

// ---------------------------------------------------------------- bundles and declared skills

{
  const dir = fresh("bundles", base);
  const home = path.join(dir, "home");
  const proj = path.join(dir, "proj");
  const market = path.join(dir, "market");
  fs.mkdirSync(proj, { recursive: true });

  // An MCP bundle is a zip of a `manifest.json` and the server it runs.
  const source = path.join(dir, "bundle-source");
  write(path.join(source, "manifest.json"), {
    manifest_version: "0.2",
    name: "cd-throwaway-bundled",
    version: "1.0.0",
    description: "throwaway",
    author: { name: "cd-throwaway" },
    server: { type: "node", entry_point: "server.js", mcp_config: { command: "node", args: ["${__dirname}/server.js"] } },
  });
  write(path.join(source, "server.js"), "process.exit(0)\n");
  const pb = path.join(market, "cd-throwaway-pb");
  const tar = path.join(process.env.SystemRoot, "System32", "tar.exe");
  fs.mkdirSync(pb, { recursive: true });
  await run(tar, ["-a", "-c", "--format", "zip", "-f", path.join(pb, "cd-throwaway.mcpb"), "-C", source, "manifest.json", "server.js"]);
  write(path.join(pb, ".claude-plugin", "plugin.json"), { name: "cd-throwaway-pb", version: "1.0.0", mcpServers: "./cd-throwaway.mcpb", skills: ["./one", "./unnamed"] });
  skill(path.join(pb, "one"), ["name: cd-throwaway-onename", "description: one"]);
  skill(path.join(pb, "unnamed"), ["description: unnamed"]);

  // A plugin whose manifest the probe rewrites, with a server in its
  // `.mcp.json` and a skill.
  const pw = path.join(market, "cd-throwaway-pw");
  const manifest = path.join(pw, ".claude-plugin", "plugin.json");
  write(manifest, { name: "cd-throwaway-pw", version: "1.0.0" });
  write(path.join(pw, ".mcp.json"), { mcpServers: { "cd-throwaway-pwsrv": stdio("pw") } });
  skill(path.join(pw, "skills", "fold-w"), ["name: cd-throwaway-wname", "description: wrong"]);

  write(path.join(market, ".claude-plugin", "marketplace.json"), { name: "cd-throwaway-market", owner: { name: "cd-throwaway" }, plugins: ["cd-throwaway-pb", "cd-throwaway-pw"].map((name) => ({ name, source: `./${name}` })) });
  await claude(home, proj, "plugin", "marketplace", "add", market);
  for (const name of ["cd-throwaway-pb", "cd-throwaway-pw"]) await claude(home, proj, "plugin", "install", `${name}@cd-throwaway-market`);
  const s = await session(home, proj);

  await p.check("locations[plugin-manifest-mcp]", "a bundle is recorded in `.mcpb-cache/<hash>.metadata.json` with its `source` and the absolute `extractedPath` it went to, and its server is named by its manifest's `name`, the folder in place of `${__dirname}`", async () => {
    const cache = path.join(pb, ".mcpb-cache");
    const metadata = fs.readdirSync(cache).filter((f) => /^[0-9a-f]+\.metadata\.json$/.test(f)).map((f) => json(path.join(cache, f)));
    const extracted = metadata.map((m) => ({
      source: m.source,
      // `.mcpb-cache/<content hash>`, the hash the metadata records.
      inCache: m.extractedPath === path.join(cache, m.contentHash),
      manifest: fs.existsSync(path.join(m.extractedPath, "manifest.json")),
    }));
    // `claude mcp get` shows no command for a bundle's server; `mcp list` does.
    const { text } = await mcpList(home, proj);
    const command = text.match(/^plugin:cd-throwaway-pb:cd-throwaway-bundled: (.*) - /m)?.[1];
    return same(
      { extracted, servers: s.servers.filter((n) => n.includes("cd-throwaway-pb")), command },
      { extracted: [{ source: "./cd-throwaway.mcpb", inCache: true, manifest: true }], servers: ["plugin:cd-throwaway-pb:cd-throwaway-bundled"], command: `node ${metadata[0]?.extractedPath}/server.js` },
    );
  });

  await p.check("locations[plugin-skills]", "a folder the manifest's `skills` declares that holds a `SKILL.md` is one skill named by its frontmatter `name`, or by its folder without one", async () => {
    return same(s.listed.filter((l) => l.includes("cd-throwaway-pb")).sort(), ["- cd-throwaway-pb:cd-throwaway-onename: one", "- cd-throwaway-pb:unnamed: unnamed"]);
  });

  await p.check("locations[plugin-manifest-mcp]", "a manifest `mcpServers` of the wrong shape fails the plugin whole, the servers of its `.mcp.json` and its skills with it", async () => {
    const loaded = (t) => [...t.plugins, ...t.servers, ...t.skills].filter((n) => n.includes("cd-throwaway-pw"));
    const kept = fs.readFileSync(manifest, "utf8");
    // With a manifest of the right shape, the plugin loads all it carries.
    const seen = { valid: loaded(s) };
    try {
      for (const wrong of [42, true, [42], null]) {
        write(manifest, { ...JSON.parse(kept), mcpServers: wrong });
        const { stdout } = await claude(home, proj, "plugin", "list");
        seen[JSON.stringify(wrong)] = { loaded: loaded(await session(home, proj)), error: stdout.includes("Manifest validation failed: mcpServers: Invalid input") };
      }
    } finally {
      write(manifest, kept);
    }
    const failed = { loaded: [], error: true };
    return same(seen, { valid: ["cd-throwaway-pw", "plugin:cd-throwaway-pw:cd-throwaway-pwsrv", "cd-throwaway-pw:fold-w"], 42: failed, true: failed, "[42]": failed, null: failed });
  });
}

// A plugin's skill names: no name rule skips its folder or refuses its
// `name`; Claude Code rewrites them, then holds the whole
// `<plugin>:<name>` to the folder name rule.
{
  const dir = fresh("plugin-names", base);
  const home = path.join(dir, "home");
  const plugin = path.join(dir, "cd-throwaway-pn");
  const prefix = "cd-throwaway-pn:";
  const stem = "cd-throwaway-";
  // Each frontmatter `name` of a declared folder that is one skill, and the
  // name Claude Code gives that skill.
  const names = {
    "cd throwaway spaced": "cd-throwaway-spaced",
    '"cd-throwaway\\ttab"': "cd-throwaway-tab",
    "cd-throwaway<lt": "cd-throwaway-lt",
    "cd-throwaway​zwsp": "cd-throwaway-zwsp",
    "cd-throwaway.dot": "cd-throwaway-dot",
    "cd-throwaway-ñandú": "cd-throwaway--and-",
    "cd-throwaway-😀": "cd-throwaway---",
    "CD_Throwaway": "CD_Throwaway",
    '"  cd-throwaway-padded  "': "cd-throwaway-padded",
    "cd-throwaway-pn:cd-throwaway-pref": "cd-throwaway-pref",
  };
  // Two more, whose `<plugin>:<name>` is 256 characters once rewritten,
  // padded and prefixed before, and 257.
  const x256 = `${stem}${"x".repeat(256 - prefix.length - stem.length - 1)}.`;
  const x257 = `${stem}${"x".repeat(257 - prefix.length - stem.length)}`;
  const long = { kept: `"  ${prefix}${x256}  "`, skipped: x257 };
  const declared = [...Object.keys(names), long.kept, long.skipped];
  write(path.join(plugin, ".claude-plugin", "plugin.json"), { name: "cd-throwaway-pn", version: "1.0.0", skills: declared.map((_, i) => `./one/n${i}`) });
  declared.forEach((name, i) => skill(path.join(plugin, "one", `n${i}`), [`name: ${name}`, `description: one n${i}`]));
  // Each folder name in the plugin's `skills` folder, and its skill's name.
  const folders = {
    " cd-throwaway-f-lead": "-cd-throwaway-f-lead",
    "cd-throwaway-f​zw": "cd-throwaway-f-zw",
    "cd-throwaway-f　ideo": "cd-throwaway-f-ideo",
    "cd-throwaway-f.dot": "cd-throwaway-f-dot",
    "cd-throwaway-fÑ": "cd-throwaway-f-",
    "cd-throwaway-f😀": "cd-throwaway-f--",
  };
  for (const folder of Object.keys(folders)) verbatimSkill(path.join(plugin, "skills"), folder);
  skill(path.join(plugin, "skills", "fold-a"), ["name: cd throwaway alias", "description: alias"]);
  const s = await session(home, fresh("empty", dir), { args: ["--plugin-dir", plugin] });

  await p.check("locations[plugin-skills]", "no name rule skips a plugin's skill folder: each UTF-16 unit of its name but `A`-`Z`, `a`-`z`, `0`-`9`, `_` and `-` becomes `-`", async () => {
    return same(Object.keys(folders).map((f) => s.skills.includes(`${prefix}${folders[f]}`)), Object.keys(folders).map(() => true));
  });
  await p.check("locations[plugin-skills]", "a declared skill's frontmatter `name` the rule refuses is not counted as none: it is trimmed, rid of a leading `<plugin>:` and rewritten the same way", async () => {
    // A skill's line is `- <name>: <description>`, or `- <name> (<alias>):
    // <description>` when the `name` as written, such as
    // `cd-throwaway.dot`, differs from the rewritten one.
    const line = (name, i) => new RegExp(`^- ${prefix}${names[name]}( \\(.*\\))?: one n${i}$`);
    const named = Object.keys(names).map((name, i) => s.listed.some((l) => line(name, i).test(l)));
    return same(named, Object.keys(names).map(() => true));
  });
  await p.check("locations[plugin-skills]", "the folder name rule takes the rewritten `<plugin>:<name>`: one of 257 characters is skipped, one of 256 kept", async () => {
    const listed = (name) => s.skills.includes(`${prefix}${name}`);
    const skipped = declared.length - 1;
    return same({ kept: listed(x256.replace(".", "-")), skipped: listed(x257) || listed(`n${skipped}`) }, { kept: true, skipped: false });
  });
  await p.check("locations[plugin-skills]", "in a folder of skill folders, a frontmatter `name` the rule refuses is no alias, the skill named by its folder", async () => {
    return same(s.listed.filter((l) => l.includes("fold-a")), [`- ${prefix}fold-a: alias`]);
  });
}

// ---------------------------------------------------------------- wrong shapes

{
  const dir = fresh("wrong-shapes", base);
  const home = path.join(dir, "home");
  const proj = path.join(dir, "proj");
  fs.mkdirSync(proj, { recursive: true });
  const key = proj.replaceAll("\\", "/");
  const user = { "cd-throwaway-user": stdio("user") };
  const local = { "cd-throwaway-local": stdio("local") };

  await p.check("locations[user-mcp]", "a `mcpServers` of `~/.claude.json` that is not an object, the user's or a Project's, is skipped with a diagnostic, the file's other servers read and the value left as written", async () => {
    const seen = {};
    for (const [label, state] of [
      ["user list", { mcpServers: [stdio("x")], projects: { [key]: { mcpServers: local } } }],
      ["user text", { mcpServers: "x", projects: { [key]: { mcpServers: local } } }],
      ["project list", { mcpServers: user, projects: { [key]: { mcpServers: [stdio("x")] } } }],
      ["project number", { mcpServers: user, projects: { [key]: { mcpServers: 3 } } }],
    ]) {
      fs.rmSync(home, { recursive: true, force: true });
      write(path.join(home, ".claude.json"), state);
      const servers = throwaway((await session(home, proj)).servers);
      const list = await mcpList(home, proj);
      const after = json(path.join(home, ".claude.json"));
      seen[label] = {
        servers,
        diagnostic: list.text.match(/\[Failed to parse\] (User|Local) config/)?.[1] ?? null,
        kept: same([after.mcpServers, after.projects[key].mcpServers], [state.mcpServers, state.projects[key].mcpServers]).pass,
      };
    }
    const userBroken = { servers: ["cd-throwaway-local"], diagnostic: "User", kept: true };
    const projectBroken = { servers: ["cd-throwaway-user"], diagnostic: "Local", kept: true };
    return same(seen, { "user list": userBroken, "user text": userBroken, "project list": projectBroken, "project number": projectBroken });
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
