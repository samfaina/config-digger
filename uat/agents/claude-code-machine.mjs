// Tests Claude Code's facts about the files and registry values an
// administrator deploys, `%ProgramFiles%\ClaudeCode` and
// `HKLM\SOFTWARE\Policies\ClaudeCode`, which only Windows Sandbox may write:
// `uat/agents/sandbox.ps1` runs it there. Each launch gets its own
// `CLAUDE_CONFIG_DIR`, the fake key `not-a-real-secret`, and a local
// stand-in for the Messages API.
//
//   node uat/agents/claude-code-machine.mjs [--bin <claude.exe>]
//
// What Claude Code loads is read from `claude mcp list`; whether it starts,
// from `claude -p`.
import fs from "node:fs";
import path from "node:path";
import { fresh, modelServer, probe, registry, run, same, sandboxOnly, write } from "./lib.mjs";

sandboxOnly();
const bin = process.argv.includes("--bin") ? process.argv[process.argv.indexOf("--bin") + 1] : "claude.exe";
const version = (await run(bin, ["--version"])).stdout.trim().split(" ")[0];
const p = probe("Claude Code", version);
const base = fresh("claude-code-machine");

const stdio = (tag) => ({ command: "cmd", args: ["/c", tag] });
const managed = path.join(process.env.ProgramFiles, "ClaudeCode");
const hklm = "HKLM\\SOFTWARE\\Policies\\ClaudeCode";
const hkcu = "HKCU\\SOFTWARE\\Policies\\ClaudeCode";
const broken = '{ "managedMcpServers": { broken';
// `managedMcpServers` takes remote servers only, at an `https://` URL.
const holding = (tag) => JSON.stringify({ managedMcpServers: { [`cd-throwaway-${tag}`]: { type: "http", url: `https://cd-throwaway-${tag}.invalid/mcp` } } });
const reply = { id: "msg_x", type: "message", role: "assistant", model: "claude-x", content: [{ type: "text", text: "ok" }], stop_reason: "end_turn", usage: { input_tokens: 1, output_tokens: 1 } };

const home = path.join(base, "home");
write(path.join(home, ".claude.json"), { mcpServers: { "cd-throwaway-user": stdio("user") } });
const proj = fresh("proj", base);
const env = { CLAUDE_CONFIG_DIR: home, ANTHROPIC_API_KEY: "not-a-real-secret" };

async function clear() {
  fs.rmSync(managed, { recursive: true, force: true });
  await registry(hklm, "Settings", null);
  await registry(hkcu, "Settings", null);
}

// The managed sources, each set to its text or left out.
async function deploy({ hklmText, file, dropIn, hkcuText, mcp } = {}) {
  await clear();
  if (hklmText) await registry(hklm, "Settings", hklmText);
  if (hkcuText) await registry(hkcu, "Settings", hkcuText);
  if (file) write(path.join(managed, "managed-settings.json"), file);
  if (dropIn) write(path.join(managed, "managed-settings.d", "10-probe.json"), dropIn);
  if (mcp) write(path.join(managed, "managed-mcp.json"), mcp);
}

// The probe's servers `claude mcp list` lists, and its other lines.
async function servers() {
  const { stdout, stderr } = await run(bin, ["mcp", "list"], { cwd: proj, env });
  const lines = (stdout + stderr).split("\n").map((l) => l.trim()).filter(Boolean);
  const names = lines.map((l) => l.match(/^(cd-throwaway-[\w-]+):/)?.[1]).filter(Boolean).sort();
  return { names, other: lines.filter((l) => !/^cd-throwaway-/.test(l) && !/^Checking MCP server health/.test(l)) };
}

// Whether `claude -p` answers, and the start of what it printed otherwise.
async function starts() {
  const model = await modelServer(reply);
  const { code, stdout, stderr } = await run(bin, ["-p", "hi"], { cwd: proj, env: { ...env, ANTHROPIC_BASE_URL: model.url } });
  model.close();
  return code === 0 && stdout.trim() === "ok" ? "starts" : `stops: ${code} ${(stdout + stderr).trim().split("\n")[0]}`;
}

await p.check("settings[managed]", "with no managed source, `claude -p` answers", async () => {
  await clear();
  return same(await starts(), "starts");
});

// ---------------------------------------------------------------- managed-mcp.json

await p.check("locations[managed-mcp]", "a `managed-mcp.json` that parses loads its servers and those of `managedMcpServers`, and no user server", async () => {
  await deploy({ mcp: JSON.stringify({ mcpServers: { "cd-throwaway-managed": stdio("managed") } }), file: holding("file") });
  return same((await servers()).names, ["cd-throwaway-file", "cd-throwaway-managed"]);
});

await p.check("locations[managed-mcp]", "a `managed-mcp.json` that does not parse is skipped with a diagnostic and keeps its exclusive control: only `managedMcpServers` load", async () => {
  await deploy({ mcp: '{ "mcpServers": { broken', file: holding("file") });
  const seen = await servers();
  const diagnostic = seen.other.includes("[Failed to parse] Enterprise config (managed by your organization)");
  return same({ names: seen.names, diagnostic, starts: await starts() }, { names: ["cd-throwaway-file"], diagnostic: true, starts: "starts" });
});

// ---------------------------------------------------------------- managed settings

await p.check("settings[managed]", "each admin source alone delivers its `managedMcpServers`; the HKCU value's are never read", async () => {
  const seen = {};
  for (const [label, sources] of [["hklm", { hklmText: holding("hklm") }], ["file", { file: holding("file") }], ["drop-in", { dropIn: holding("drop-in") }], ["hkcu", { hkcuText: holding("hkcu") }]]) {
    await deploy(sources);
    seen[label] = (await servers()).names;
  }
  return same(seen, {
    hklm: ["cd-throwaway-hklm", "cd-throwaway-user"],
    file: ["cd-throwaway-file", "cd-throwaway-user"],
    "drop-in": ["cd-throwaway-drop-in", "cd-throwaway-user"],
    hkcu: ["cd-throwaway-user"],
  });
});

await p.check("locations[managed-settings-mcp]", "a `managedMcpServers` entry with a command, or at an `http://` URL, is dropped and the others load", async () => {
  const declared = {
    "cd-throwaway-stdio": stdio("stdio"),
    "cd-throwaway-plain": { type: "http", url: "http://cd-throwaway-plain.invalid/mcp" },
    "cd-throwaway-https": { type: "http", url: "https://cd-throwaway-https.invalid/mcp" },
  };
  await deploy({ file: JSON.stringify({ managedMcpServers: declared }) });
  return same((await servers()).names, ["cd-throwaway-https", "cd-throwaway-user"]);
});

await p.check("settings[managed]", "of two admin sources that hold settings, only the higher one is read", async () => {
  await deploy({ hklmText: holding("hklm"), file: holding("file") });
  return same((await servers()).names, ["cd-throwaway-hklm", "cd-throwaway-user"]);
});

await p.check("locations[managed-settings-mcp]", "an HKLM value, `managed-settings.json` or drop-in that does not parse stops Claude Code, even beside a source that parses", async () => {
  const seen = {};
  for (const [label, sources] of [
    ["hklm", { hklmText: broken, file: holding("file") }],
    ["file", { file: broken, hklmText: holding("hklm") }],
    ["drop-in", { file: holding("file"), dropIn: broken }],
  ]) {
    await deploy(sources);
    seen[label] = await starts();
  }
  const pass = Object.values(seen).every((s) => s.startsWith("stops:") && s.includes("Managed settings document could not be parsed as a JSON object"));
  return { pass, observed: JSON.stringify(seen) };
});

await p.check("locations[managed-settings-mcp]", "an HKCU value that does not parse is skipped, and Claude Code starts with the user's servers", async () => {
  await deploy({ hkcuText: broken });
  const seen = { names: (await servers()).names, starts: await starts() };
  return same(seen, { names: ["cd-throwaway-user"], starts: "starts" });
});

// ---------------------------------------------------------------- managed approvals

// Three Projects with the same two `.mcp.json` servers, in a home of their
// own: one with no state in `.claude.json`, one whose state does not hold
// `hasTrustDialogAccepted`, and one trusted, the control.
{
  const approvalsHome = path.join(base, "approvals-home");
  const projects = { "no state": path.join(base, "no-state"), "state untrusted": path.join(base, "state-untrusted"), trusted: path.join(base, "trusted") };
  for (const dir of Object.values(projects)) write(path.join(dir, ".mcp.json"), { mcpServers: { "cd-throwaway-a": stdio("a"), "cd-throwaway-b": stdio("b") } });
  // Claude Code keys a Project by its path with forward slashes.
  const key = (dir) => dir.replaceAll("\\", "/");
  write(path.join(approvalsHome, ".claude.json"), {
    projects: { [key(projects["state untrusted"])]: { allowedTools: [] }, [key(projects.trusted)]: { hasTrustDialogAccepted: true } },
  });
  // Each server's approval in `claude mcp list` run in `dir`: a server it
  // lists is approved unless its line says it waits for approval. In the
  // Sandbox the status marks before the words are not those
  // `claude-code.mjs` matches.
  const approved = async (dir) => {
    const { stdout } = await run(bin, ["mcp", "list"], { cwd: dir, env: { ...env, CLAUDE_CONFIG_DIR: approvalsHome } });
    const lines = stdout.split("\n").map((l) => l.trim());
    return ["cd-throwaway-a", "cd-throwaway-b"].map((n) => {
      const line = lines.find((l) => l.startsWith(`${n}:`));
      return line === undefined ? "absent" : line.includes("Pending approval") ? "pending" : "approved";
    });
  };

  await p.check("items.mcp.gates[0]", "`enabledMcpjsonServers` and `enableAllProjectMcpServers` in `managed-settings.json` approve a Project's `.mcp.json` servers whether the Project is trusted or not", async () => {
    const seen = {};
    for (const [label, settings] of [["none", null], ["one named", { enabledMcpjsonServers: ["cd-throwaway-a"] }], ["all", { enableAllProjectMcpServers: true }]]) {
      await deploy(settings ? { file: JSON.stringify(settings) } : {});
      for (const [where, dir] of Object.entries(projects)) seen[`${label}, ${where}`] = await approved(dir);
    }
    const none = ["pending", "pending"];
    const one = ["approved", "pending"];
    const all = ["approved", "approved"];
    return same(seen, {
      "none, no state": none, "none, state untrusted": none, "none, trusted": none,
      "one named, no state": one, "one named, state untrusted": one, "one named, trusted": one,
      "all, no state": all, "all, state untrusted": all, "all, trusted": all,
    });
  });
}

await clear();
p.finish();
