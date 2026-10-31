// Tests Codex's facts that `codex.toml` marked unverified, on throwaway
// `CODEX_HOME` folders and Projects. A session that needs a model talks to a
// provider at a closed local port, so no request leaves the machine.
//
//   node uat/agents/codex.mjs [--bin <codex.exe or codex.cmd>]
//
// What Codex loads is read from `codex mcp list`, from `codex debug
// prompt-input`, which renders the skills its model would get, and from the
// files it writes.
import fs from "node:fs";
import path from "node:path";
import { fresh, gitInit, probe, run, same, skill, write } from "./lib.mjs";

const bin = process.argv.includes("--bin") ? process.argv[process.argv.indexOf("--bin") + 1] : "codex.cmd";
const codex = (home, cwd, ...args) => run(bin, args, { cwd, env: { CODEX_HOME: home } });
const version = (await codex(fresh("codex-version"), undefined, "--version")).stdout.trim().split(" ").at(-1);
const p = probe("Codex", version);
const base = fresh("codex");
const stdio = (tag) => `command = "cmd"\nargs = ["/c", "${tag}"]\n`;

// `codex mcp list` as `{ name: status }`, or the error Codex stopped with.
async function mcpList(home, cwd) {
  const { stdout, stderr } = await codex(home, cwd, "mcp", "list");
  const status = {};
  for (const line of stdout.split("\n").slice(1)) {
    const cells = line.trim().split(/\s{2,}/);
    if (cells.length > 2) status[cells[0]] = cells.at(-2);
  }
  const error = (stdout + stderr).match(/Caused by:\s*\n\s*(.*)/)?.[1].trim();
  return error ? { error } : status;
}

// `codex mcp list --json` as `{ name: { enabled, transport } }`, the
// transport without its null fields, or the error Codex stopped with.
async function mcpJson(home, cwd) {
  const { stdout, stderr } = await codex(home, cwd, "mcp", "list", "--json");
  const error = (stdout + stderr).match(/Caused by:\s*\n\s*(.*)/)?.[1].trim();
  if (error) return { error };
  const set = (transport) => Object.fromEntries(Object.entries(transport).filter(([, v]) => v !== null && !(Array.isArray(v) && !v.length)));
  return Object.fromEntries(JSON.parse(stdout).map((s) => [s.name, { enabled: s.enabled, transport: set(s.transport) }]));
}

// The skills `codex debug prompt-input` lists, by the description each
// probe skill carries, `d-<name>`.
async function skills(home, cwd) {
  const { stdout } = await codex(home, cwd, "debug", "prompt-input");
  return [...new Set(stdout.match(/d-cd-throwaway-[a-z0-9-]+/g) ?? [])].map((d) => d.slice(2)).sort();
}

const trusted = (folder, level = "trusted") => `[projects.'${folder.toLowerCase()}']\ntrust_level = "${level}"\n`;

// ---------------------------------------------------------------- servers

{
  const dir = fresh("servers", base);
  const home = fresh("home", dir);
  const proj = path.join(dir, "proj");
  await gitInit(proj);
  await p.check("items.mcp.disable[0]", "a higher layer's `enabled = true` wins over a lower layer's `enabled = false`, and the reverse", async () => {
    write(path.join(home, "config.toml"), `${trusted(proj)}[mcp_servers.cd-throwaway-x]\n${stdio("x")}enabled = false\n`);
    write(path.join(proj, ".codex", "config.toml"), "[mcp_servers.cd-throwaway-x]\nenabled = true\n");
    const up = (await mcpList(home, proj))["cd-throwaway-x"];
    write(path.join(home, "config.toml"), `${trusted(proj)}[mcp_servers.cd-throwaway-x]\n${stdio("x")}enabled = true\n`);
    write(path.join(proj, ".codex", "config.toml"), "[mcp_servers.cd-throwaway-x]\nenabled = false\n");
    const down = (await mcpList(home, proj))["cd-throwaway-x"];
    return same({ up, down }, { up: "enabled", down: "disabled" });
  });
  await p.check("items.mcp.disable[0]", "a user table holding `enabled` alone stops Codex in every folder no other layer declares the server in", async () => {
    write(path.join(home, "config.toml"), `${trusted(proj)}[mcp_servers.cd-throwaway-only]\nenabled = false\n`);
    write(path.join(proj, ".codex", "config.toml"), `[mcp_servers.cd-throwaway-only]\n${stdio("only")}`);
    const inProject = (await mcpList(home, proj))["cd-throwaway-only"];
    const elsewhere = (await mcpList(home, fresh("elsewhere", dir))).error;
    return same({ inProject, elsewhere }, { inProject: "disabled", elsewhere: "invalid transport" });
  });
  // A Stand-in: the user table Disable writes for a git-versioned Project
  // layer's server (#297).
  const user = path.join(home, "config.toml");
  const layer = path.join(proj, ".codex", "config.toml");
  await p.check("items.mcp.disable[0]", "a Stand-in with `command` alone turns a stdio layer's server off in the Project, its fields winning, and elsewhere is a Disabled server of its own", async () => {
    write(user, `${trusted(proj)}[mcp_servers.cd-throwaway-sa]\ncommand = "cmd"\nenabled = false\n`);
    write(layer, `[mcp_servers.cd-throwaway-sa]\n${stdio("layer")}env = { CD_THROWAWAY = "not-a-real-secret" }\n`);
    const inProject = (await mcpJson(home, proj))["cd-throwaway-sa"];
    const elsewhere = (await mcpJson(home, fresh("elsewhere", dir)))["cd-throwaway-sa"];
    return same({ inProject, elsewhere }, {
      inProject: { enabled: false, transport: { type: "stdio", command: "cmd", args: ["/c", "layer"], env: { CD_THROWAWAY: "not-a-real-secret" } } },
      elsewhere: { enabled: false, transport: { type: "stdio", command: "cmd" } },
    });
  });
  await p.check("items.mcp.disable[0]", "a Stand-in with `url` alone turns an http layer's server off in the Project, its `url` and `bearer_token_env_var` winning, and elsewhere is a Disabled server of its own", async () => {
    write(user, `${trusted(proj)}[mcp_servers.cd-throwaway-sh]\nurl = "https://cd-throwaway.invalid/mcp"\nenabled = false\n`);
    write(layer, `[mcp_servers.cd-throwaway-sh]\nurl = "https://cd-throwaway.invalid/mcp?key=not-a-real-secret"\nbearer_token_env_var = "CD_THROWAWAY_TOKEN"\n`);
    const inProject = (await mcpJson(home, proj))["cd-throwaway-sh"];
    const elsewhere = (await mcpJson(home, fresh("elsewhere", dir)))["cd-throwaway-sh"];
    return same({ inProject, elsewhere }, {
      inProject: { enabled: false, transport: { type: "streamable_http", url: "https://cd-throwaway.invalid/mcp?key=not-a-real-secret", bearer_token_env_var: "CD_THROWAWAY_TOKEN" } },
      elsewhere: { enabled: false, transport: { type: "streamable_http", url: "https://cd-throwaway.invalid/mcp" } },
    });
  });
  await p.check("items.mcp.disable[0]", "a Stand-in of the other transport than the layer's stops Codex in the Project, and elsewhere Codex starts", async () => {
    write(user, `${trusted(proj)}[mcp_servers.cd-throwaway-sx]\nurl = "https://cd-throwaway.invalid/mcp"\nenabled = false\n`);
    write(layer, `[mcp_servers.cd-throwaway-sx]\n${stdio("layer")}`);
    const inProject = (await mcpJson(home, proj)).error;
    const elsewhere = (await mcpJson(home, fresh("elsewhere", dir)))["cd-throwaway-sx"]?.enabled;
    return same({ inProject, elsewhere }, { inProject: "url is not supported for stdio", elsewhere: false });
  });
  await p.check("items.mcp.disable[0]", "taking only `enabled = false` out of a Stand-in turns the server on in every folder", async () => {
    write(user, `${trusted(proj)}[mcp_servers.cd-throwaway-sa]\ncommand = "cmd"\n`);
    write(layer, `[mcp_servers.cd-throwaway-sa]\n${stdio("layer")}`);
    const inProject = (await mcpJson(home, proj))["cd-throwaway-sa"]?.enabled;
    const elsewhere = (await mcpJson(home, fresh("elsewhere", dir)))["cd-throwaway-sa"]?.enabled;
    return same({ inProject, elsewhere }, { inProject: true, elsewhere: true });
  });
}

// ---------------------------------------------------------------- plugins

{
  const dir = fresh("plugins", base);
  const home = fresh("home", dir);
  const proj = fresh("proj", dir);
  const market = path.join(dir, "market");
  const names = ["cd-throwaway-p1", "cd-throwaway-p2"];
  for (const name of names) {
    write(path.join(market, name, ".codex-plugin", "plugin.json"), { name, version: "1.0.0", description: "probe plugin" });
    write(path.join(market, name, ".mcp.json"), { mcpServers: { [`${name}-srv`]: { command: "cmd", args: ["/c", name] } } });
    skill(path.join(market, name, "skills", `${name}-skill`), [`name: ${name}-skill`, `description: d-${name}-skill`]);
  }
  // p3 holds only a skill 6 and one 7 folders below its skills folder, and
  // is installed by the depth check alone.
  const p3 = "cd-throwaway-p3";
  const deep = path.join("skills", "a", "b", "c", "d", "e");
  const deepSkills = { "cd-throwaway-plugin-depth6": deep, "cd-throwaway-plugin-depth7": path.join(deep, "f") };
  write(path.join(market, p3, ".codex-plugin", "plugin.json"), { name: p3, version: "1.0.0", description: "probe plugin" });
  for (const [name, folder] of Object.entries(deepSkills)) skill(path.join(market, p3, folder, name), [`name: ${name}`, `description: d-${name}`]);
  write(path.join(market, ".agents", "plugins", "marketplace.json"), { name: "cd-throwaway-market", plugins: [...names, p3].map((name) => ({ name, source: { source: "local", path: `./${name}` } })) });
  await codex(home, proj, "plugin", "marketplace", "add", market);
  for (const name of names) await codex(home, proj, "plugin", "add", `${name}@cd-throwaway-market`);
  const config = path.join(home, "config.toml");
  const installed = fs.readFileSync(config, "utf8");

  await p.check("items.mcp.disable[2]", "a plugin missing from `config.toml` is off, with its servers and skills", async () => {
    write(config, installed.replace(/\[plugins\."cd-throwaway-p2@cd-throwaway-market"\]\nenabled = true\n/, ""));
    const servers = Object.keys(await mcpList(home, proj));
    const listed = await skills(home, proj);
    write(config, installed);
    return same({ servers, listed }, { servers: ["cd-throwaway-p1-srv"], listed: ["cd-throwaway-p1-skill"] });
  });
  await p.check("locations[plugin-mcp]", "a plugin's `.mcp.json` that does not parse leaves out its servers, and Codex starts with the plugin's skills", async () => {
    const file = path.join(home, "plugins", "cache", "cd-throwaway-market", "cd-throwaway-p1", "1.0.0", ".mcp.json");
    const kept = fs.readFileSync(file, "utf8");
    write(file, "{ broken");
    const servers = Object.keys(await mcpList(home, proj));
    const listed = await skills(home, proj);
    write(file, kept);
    return same({ servers, listed }, { servers: ["cd-throwaway-p2-srv"], listed: ["cd-throwaway-p1-skill", "cd-throwaway-p2-skill"] });
  });
  await p.check("plugins[installed]", "`codex plugin remove <id>` takes out the plugin's folder in the cache and its table in `config.toml`", async () => {
    await codex(home, proj, "plugin", "remove", "cd-throwaway-p2@cd-throwaway-market");
    const folder = fs.existsSync(path.join(home, "plugins", "cache", "cd-throwaway-market", "cd-throwaway-p2"));
    const table = fs.readFileSync(config, "utf8").includes('"cd-throwaway-p2@cd-throwaway-market"');
    return same({ folder, table }, { folder: false, table: false });
  });
  await p.check("locations[plugin-skills]", "an installed plugin's skill folders are found up to 6 folders below its skills folder, not 7, though its copy in the cache holds both", async () => {
    await codex(home, proj, "plugin", "add", `${p3}@cd-throwaway-market`);
    const copy = path.join(home, "plugins", "cache", "cd-throwaway-market", p3, "1.0.0");
    const copied = Object.entries(deepSkills).every(([name, folder]) => fs.existsSync(path.join(copy, folder, name, "SKILL.md")));
    const listed = (await skills(home, proj)).filter((n) => n.includes("depth"));
    return same({ copied, listed }, { copied: true, listed: ["cd-throwaway-plugin-depth6"] });
  });
}

// ---------------------------------------------------------------- skills

{
  const dir = fresh("skills", base);
  const home = fresh("home", dir);
  const proj = path.join(dir, "proj");
  await gitInit(proj);
  const at = (folder, name) => skill(path.join(folder, name), [`name: ${name}`, `description: d-${name}`]);
  at(path.join(proj, ".agents", "skills"), "cd-throwaway-agents");
  at(path.join(proj, ".codex", "skills"), "cd-throwaway-codex");
  const user = path.join(home, "skills");
  at(path.join(user, "a"), "cd-throwaway-depth2");
  at(path.join(user, "a", "b", "c", "d", "e"), "cd-throwaway-depth6");
  at(path.join(user, "a", "b", "c", "d", "e", "f"), "cd-throwaway-depth7");
  at(path.join(proj, ".agents", "skills", "a", "b", "c", "d", "e"), "cd-throwaway-project-depth6");
  at(path.join(proj, ".agents", "skills", "a", "b", "c", "d", "e", "f"), "cd-throwaway-project-depth7");
  write(path.join(user, "cd-throwaway-badyaml", "SKILL.md"), "---\nname: [unclosed\ndescription: d-cd-throwaway-badyaml\n---\n");
  write(path.join(user, "cd-throwaway-badutf8", "SKILL.md"), Buffer.from("---\nname: cd-throwaway-badutf8\ndescription: d-cd-throwaway-badutf8 \xff\xfe\n---\n", "latin1"));
  at(user, "cd-throwaway-denied");
  // Frontmatters that are not YAML, each in the folder `cd-throwaway-fm-<case>`.
  const frontmatters = {
    colon: "name: cd-throwaway-fm-colon\ndescription: d-fm Use when: x\n---",
    quoted: "name: \"cd-throwaway-fm-quoted\"\ndescription: 'd-fm: x'\n---",
    stray: "name: cd-throwaway-fm-stray\njust text\ndescription: d-fm: x\n---",
    indented: "  name: cd-throwaway-fm-indented\ndescription: d-fm\n---",
    twonames: "name: cd-throwaway-fm-first\nname: cd-throwaway-fm-second\ndescription: d-fm\n---",
    continued: "name: cd-throwaway-fm-continued\ndescription: d-fm one\n  two\n---",
    continuedcolon: "name: cd-throwaway-fm-continuedcolon\ndescription: d-fm: one\n  two\n---",
    unclosedquote: "name: \"cd-throwaway-fm-unclosedquote\ndescription: d-fm: x\n---",
    trailing: "name: cd-throwaway-fm-trailing\ndescription: d-fm for the following:\n---",
    trailingcolon: "name: cd-throwaway-fm-trailingcolon\ndescription: d-fm: for the following:\n---",
    unclosed: "name: cd-throwaway-fm-unclosed\ndescription: d-fm: x",
    number: "name: 123\ndescription: d-fm-number: x\n---",
    noname: "description: d-fm: x\n---",
  };
  for (const [name, frontmatter] of Object.entries(frontmatters)) {
    write(path.join(user, `cd-throwaway-fm-${name}`, "SKILL.md"), `---\n${frontmatter}\nbody\n`);
  }
  const denied = path.join(user, "cd-throwaway-denied", "SKILL.md");
  await run("icacls", [denied, "/deny", `${process.env.USERNAME}:(R)`]);
  const listed = await skills(home, proj);
  await run("icacls", [denied, "/remove:d", process.env.USERNAME]);
  // Each `cd-throwaway-fm-<case>` skill Codex lists, as `<name>: <description>`.
  const { stdout: prompt } = await codex(home, proj, "debug", "prompt-input");
  const read = Object.fromEntries(
    [...prompt.replaceAll("\\n", "\n").matchAll(/^- (.*?): (d-fm.*?) \(file: r\d+\/cd-throwaway-fm-([a-z]+)\/SKILL\.md\)$/gm)].map(([, name, description, folder]) => [folder, `${name}: ${description}`]),
  );
  const readOf = (...folders) => Object.fromEntries(folders.map((folder) => [folder, read[folder] ?? null]));

  await p.check("locations[project-agents-skills]", "`.agents\\skills` is read in a Project Codex does not trust", async () => {
    return same(listed.includes("cd-throwaway-agents"), true);
  });
  await p.check("items.skill", "skill folders are found up to 6 folders below a root, not 7", async () => {
    return same(listed.filter((n) => n.includes("depth")), ["cd-throwaway-depth2", "cd-throwaway-depth6", "cd-throwaway-project-depth6"]);
  });
  await p.check("items.skill", "a `SKILL.md` that is not UTF-8, or that cannot be opened, is skipped", async () => {
    return same(listed.filter((n) => /badutf8|denied/.test(n)), []);
  });
  await p.check("items.skill", "an unparsable `[` value is read again quoted: `name: [unclosed` names the skill `[unclosed`", async () => {
    const { stdout } = await codex(home, proj, "debug", "prompt-input");
    return same(stdout.includes("- [unclosed: d-cd-throwaway-badyaml"), true);
  });
  await p.check("items.skill", "a frontmatter that is not YAML is read again with each plain value holding `: ` quoted, and quotes are YAML's", async () => {
    return same(readOf("colon", "quoted"), { colon: "cd-throwaway-fm-colon: d-fm Use when: x", quoted: "cd-throwaway-fm-quoted: d-fm: x" });
  });
  await p.check("items.skill", "a stray line, an indented `name`, two `name` lines, a quote that never closes and a `: ` value going on over indented lines skip the skill", async () => {
    return same(readOf("stray", "indented", "twonames", "continuedcolon", "unclosedquote"), { stray: null, indented: null, twonames: null, continuedcolon: null, unclosedquote: null });
  });
  await p.check("items.skill", "a value ending in `:` skips the skill, unless it holds `: ` too, which Codex quotes", async () => {
    return same(readOf("trailing", "trailingcolon"), { trailing: null, trailingcolon: "cd-throwaway-fm-trailingcolon: d-fm: for the following:" });
  });
  await p.check("items.skill", "a plain `description` goes on over indented lines", async () => {
    return same(readOf("continued"), { continued: "cd-throwaway-fm-continued: d-fm one two" });
  });
  await p.check("items.skill", "a frontmatter that never closes skips the skill", async () => {
    return same(readOf("unclosed"), { unclosed: null });
  });
  await p.check("items.skill", "`name: 123` names the skill `123`", async () => {
    return same(readOf("number"), { number: "123: d-fm-number: x" });
  });
  // Found beside #299, and `no_name = "folder-name"` since #466.
  await p.check("items.skill", "a frontmatter without a `name` names the skill after its folder, with its description", async () => {
    return same(readOf("noname"), { noname: "cd-throwaway-fm-noname: d-fm: x" });
  });
  await p.check("items.skill.disable[0]", "a rule's path matches the `SKILL.md` in any case, with either slash, with or without `\\\\?\\`", async () => {
    const file = path.join(user, "a", "cd-throwaway-depth2", "SKILL.md");
    const spellings = { plain: file, "lower-case drive": file[0].toLowerCase() + file.slice(1), "forward slashes": file.replaceAll("\\", "/"), "upper case": file.toUpperCase(), "long-path prefix": `\\\\?\\${file}` };
    const matched = {};
    for (const [label, spelling] of Object.entries(spellings)) {
      write(path.join(home, "config.toml"), `[[skills.config]]\npath = '${spelling}'\nenabled = false\n`);
      matched[label] = !(await skills(home, proj)).includes("cd-throwaway-depth2");
    }
    fs.rmSync(path.join(home, "config.toml"));
    return same(matched, Object.fromEntries(Object.keys(spellings).map((label) => [label, true])));
  });
}

// ---------------------------------------------------------------- sessions

{
  const dir = fresh("sessions", base);
  const home = fresh("home", dir);
  const proj = fresh("proj", dir);
  write(path.join(home, "config.toml"), 'model = "cd-throwaway-model"\nmodel_provider = "cdthrowaway"\n[model_providers.cdthrowaway]\nname = "cd-throwaway"\nbase_url = "http://127.0.0.1:9/v1"\nwire_api = "responses"\nrequest_max_retries = 0\nstream_max_retries = 0\n');
  await codex(home, proj, "exec", "--skip-git-repo-check", "hi");
  const day = (folder) => fs.readdirSync(folder, { recursive: true }).filter((f) => f.endsWith(".jsonl"));
  await p.check("projects", "an archived session lies directly in `archived_sessions`, not by date, and keeps its `cwd`", async () => {
    const [started] = day(path.join(home, "sessions"));
    const id = started.match(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/)[0];
    await codex(home, proj, "archive", id);
    const archived = day(path.join(home, "archived_sessions"));
    const head = JSON.parse(fs.readFileSync(path.join(home, "archived_sessions", archived[0]), "utf8").split("\n")[0]);
    return same({ flat: archived.length === 1 && !archived[0].includes(path.sep), cwd: head.payload.cwd }, { flat: true, cwd: proj });
  });
}

p.finish();
