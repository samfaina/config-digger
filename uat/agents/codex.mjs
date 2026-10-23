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
  write(path.join(market, ".agents", "plugins", "marketplace.json"), { name: "cd-throwaway-market", plugins: names.map((name) => ({ name, source: { source: "local", path: `./${name}` } })) });
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
  const denied = path.join(user, "cd-throwaway-denied", "SKILL.md");
  await run("icacls", [denied, "/deny", `${process.env.USERNAME}:(R)`]);
  const listed = await skills(home, proj);
  await run("icacls", [denied, "/remove:d", process.env.USERNAME]);

  await p.check("locations[project-agents-skills]", "`.agents\\skills` is read in a Project Codex does not trust", async () => {
    return same(listed.includes("cd-throwaway-agents"), true);
  });
  await p.check("items.skill", "skill folders are found up to 6 folders below a root, not 7", async () => {
    return same(listed.filter((n) => n.includes("depth")), ["cd-throwaway-depth2", "cd-throwaway-depth6", "cd-throwaway-project-depth6"]);
  });
  await p.check("items.skill", "a `SKILL.md` that is not UTF-8, or that cannot be opened, is skipped", async () => {
    return same(listed.filter((n) => /badutf8|denied/.test(n)), []);
  });
  await p.check("items.skill", "a frontmatter that is not YAML is read line by line: `name: [unclosed` names the skill `[unclosed`", async () => {
    const { stdout } = await codex(home, proj, "debug", "prompt-input");
    return same(stdout.includes("- [unclosed: d-cd-throwaway-badyaml"), true);
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
