// Tests Codex's facts about `%ProgramData%\OpenAI\Codex\requirements.toml`,
// which only Windows Sandbox may write: `uat/agents/sandbox.ps1` runs it
// there. Each run gets its own `CODEX_HOME`; no session starts.
//
//   node uat/agents/codex-machine.mjs [--bin <codex.exe>]
//
// What Codex allows is read from `codex mcp list`.
import fs from "node:fs";
import path from "node:path";
import { fresh, probe, run, same, sandboxOnly, write } from "./lib.mjs";

sandboxOnly();
const bin = process.argv.includes("--bin") ? process.argv[process.argv.indexOf("--bin") + 1] : "codex.exe";
const base = fresh("codex-machine");
const home = fresh("home", base);
const proj = fresh("proj", base);
const codex = (...args) => run(bin, args, { cwd: proj, env: { CODEX_HOME: home } });
const version = (await codex("--version")).stdout.trim().split(" ").at(-1);
const p = probe("Codex", version);
const requirements = path.join(process.env.ProgramData, "OpenAI", "Codex", "requirements.toml");

const stdio = (name, ...args) => `[mcp_servers.${name}]\ncommand = "cmd"\nargs = ${JSON.stringify(args)}\n`;
const http = (name, url) => `[mcp_servers.${name}]\nurl = "${url}"\n`;
const exact = (value) => `{ match = "exact", value = '${value}' }`;
const prefix = (value) => `{ match = "prefix", value = '${value}' }`;
const regex = (expression) => `{ match = "regex", expression = '${expression}' }`;

// `codex mcp list` as `{ name: status }` for the probe's servers, or the
// error Codex stopped with. The status of a server a requirement turns off
// is `disabled: requirements (<file>)`.
async function mcpList() {
  const { stdout, stderr } = await codex("mcp", "list");
  const status = {};
  for (const line of stdout.split("\n").slice(1)) {
    const cells = line.trim().split(/\s{2,}/);
    if (cells.length > 2 && cells[0].startsWith("cd-throwaway-")) status[cells[0]] = cells.at(-2).replace(`: requirements (${requirements})`, " by requirements");
  }
  const error = (stdout + stderr).match(/Caused by:\s*\n\s*(.*)/)?.[1].trim() ?? (stdout + stderr).match(/^Error: (.*)/m)?.[1].trim();
  return error ? { error } : Object.fromEntries(Object.entries(status).sort());
}

// The user's servers in `config.toml` and the machine's requirements.
function deploy(servers, required) {
  write(path.join(home, "config.toml"), servers.join("\n"));
  write(requirements, required.map(([name, identity]) => `[mcp_servers.${name}.identity]\n${identity}\n`).join("\n"));
}

await p.check("items.mcp.rules[0]", "a requirement allows a server by its exact command, whatever its args, or its exact URL, and turns off every server it does not name", async () => {
  deploy(
    [stdio("cd-throwaway-command", "/c", "any"), http("cd-throwaway-url", "http://127.0.0.1:9/mcp"), http("cd-throwaway-other-url", "http://127.0.0.1:9/other"), stdio("cd-throwaway-unnamed", "/c", "unnamed")],
    [["cd-throwaway-command", 'command = "cmd"'], ["cd-throwaway-url", 'url = "http://127.0.0.1:9/mcp"'], ["cd-throwaway-other-url", 'url = "http://127.0.0.1:9/mcp"']],
  );
  return same(await mcpList(), { "cd-throwaway-command": "enabled", "cd-throwaway-other-url": "disabled by requirements", "cd-throwaway-unnamed": "disabled by requirements", "cd-throwaway-url": "enabled" });
});

await p.check("items.mcp.rules[0]", "a requirement on `command = { executable, args }` allows a server whose command is the executable and whose args match one by one, exactly, by prefix or by a regex of the whole arg", async () => {
  const args = `command = { executable = "cmd", args = [${exact("/c")}, ${prefix("pre-")}, ${regex("n[0-9]+")}] }`;
  deploy(
    [
      stdio("cd-throwaway-match", "/c", "pre-fix", "n42"),
      stdio("cd-throwaway-prefix-no", "/c", "fix", "n42"),
      stdio("cd-throwaway-regex-part", "/c", "pre-fix", "xn42"),
      stdio("cd-throwaway-count", "/c", "pre-fix"),
    ],
    [["cd-throwaway-match", args], ["cd-throwaway-prefix-no", args], ["cd-throwaway-regex-part", args], ["cd-throwaway-count", args]],
  );
  return same(await mcpList(), { "cd-throwaway-count": "disabled by requirements", "cd-throwaway-match": "enabled", "cd-throwaway-prefix-no": "disabled by requirements", "cd-throwaway-regex-part": "disabled by requirements" });
});

await p.check("items.mcp.rules[0]", "a requirement on `url = { match }` allows a server whose URL matches exactly, by prefix or by a regex of the whole URL", async () => {
  deploy(
    [http("cd-throwaway-prefix", "http://127.0.0.1:9/mcp/a"), http("cd-throwaway-prefix-no", "http://127.0.0.1:9/other"), http("cd-throwaway-regex", "http://127.0.0.1:9/r/7"), http("cd-throwaway-regex-part", "http://127.0.0.1:9/r/7/x")],
    [
      ["cd-throwaway-prefix", `url = ${prefix("http://127.0.0.1:9/mcp/")}`],
      ["cd-throwaway-prefix-no", `url = ${prefix("http://127.0.0.1:9/mcp/")}`],
      ["cd-throwaway-regex", `url = ${regex("http://127\\.0\\.0\\.1:9/r/[0-9]+")}`],
      ["cd-throwaway-regex-part", `url = ${regex("http://127\\.0\\.0\\.1:9/r/[0-9]+")}`],
    ],
  );
  return same(await mcpList(), { "cd-throwaway-prefix": "enabled", "cd-throwaway-prefix-no": "disabled by requirements", "cd-throwaway-regex": "enabled", "cd-throwaway-regex-part": "disabled by requirements" });
});

await p.check("items.mcp.rules[0]", "a requirement whose regex does not compile stops Codex", async () => {
  deploy([stdio("cd-throwaway-x", "/c", "x")], [["cd-throwaway-x", `url = ${regex("[")}`]]);
  const seen = await mcpList();
  return { pass: typeof seen.error === "string", observed: JSON.stringify(seen) };
});

fs.rmSync(path.dirname(requirements), { recursive: true, force: true });
p.finish();
