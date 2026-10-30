// What every probe shares: throwaway folders, running an Agent's CLI, a local
// stand-in for a model API, a proxy that keeps an Agent off the network, and
// the checks a probe reports.
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";

// The folder every probe writes under. Its name must start with
// `cd-throwaway`, so `fresh` can never delete another folder; the probes
// that write machine-wide paths run only in Windows Sandbox (`sandboxOnly`).
// It sits outside the user's home folder, since Claude Code and Codex read
// files in every folder above the one they start in.
export const root = path.resolve(process.env.CD_PROBE_ROOT ?? "E:\\cd-throwaway-agents");
if (!path.basename(root).startsWith("cd-throwaway")) {
  throw new Error(`CD_PROBE_ROOT must be a folder named cd-throwaway*, not ${root}`);
}

// Orca's terminals set `CLAUDE_CODE_PLUGIN_DIRS`, which would load Orca's
// own plugin into every Claude Code launch.
delete process.env.CLAUDE_CODE_PLUGIN_DIRS;

// A new empty folder under `base`, which defaults to the probe root.
export function fresh(name, base = root) {
  const dir = path.join(base, name);
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

export function write(file, content) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, typeof content === "string" || Buffer.isBuffer(content) ? content : JSON.stringify(content, null, 2));
}

export function json(file) {
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

// A skill folder holding a `SKILL.md` with `frontmatter`, given as YAML lines.
export function skill(dir, frontmatter, body = "body") {
  write(path.join(dir, "SKILL.md"), `---\n${frontmatter.join("\n")}\n---\n${body}\n`);
}

// A plugin as Claude Code syncs it from claude.ai into `bucket`, an
// account's folder of `plugins/synced`: its folder, with its manifest and a
// skill per name of `skills`, its `.meta.json`, and its entry in the
// bucket's `manifest.json`, without which Claude Code does not load it.
// The folder is `<name>`, as 2.1.309 names it, or `<name>~g<generation>`
// with a `generation`, as 2.1.310 does. `manifest` stands for the plugin
// manifest's content, an object or raw text. Returns the plugin's folder.
// Claude Code reads it while the gate `tengu_claudeai_plugins_sync_enabled`
// is on, and a sync round deletes it.
export function syncedPlugin(bucket, name, { generation, manifest = { name, version: "1.0.0" }, skills = [] } = {}) {
  const folder = generation === undefined ? name : `${name}~g${generation}`;
  const dir = path.join(bucket, folder);
  write(path.join(dir, ".claude-plugin", "plugin.json"), manifest);
  for (const s of skills) skill(path.join(dir, "skills", s), [`name: ${s}`, `description: a skill of ${name}`]);
  write(path.join(bucket, `${folder}.meta.json`), { server_plugin_id: name, marketplace_name: "cd-throwaway-market", installation_preference: "available" });
  const list = path.join(bucket, "manifest.json");
  const listed = fs.existsSync(list) ? json(list) : { lastUpdated: Date.now(), plugins: [] };
  listed.plugins = listed.plugins.filter((p) => p.name !== name);
  listed.plugins.push({ pluginId: name, name, description: `${name}, planted by a probe`, version: "1.0.0", updatedAt: new Date().toISOString(), marketplaceName: "cd-throwaway-market", installationPreference: "available", ...(generation === undefined ? {} : { generation }) });
  write(list, listed);
  return dir;
}

// What `claude plugin list`'s `stdout` says of the plugin `name`: the words
// of its status, such as `disabled` or `failed to load`, its error up to
// the first colon, and the folder of its `Read from:`, each or null. The
// mark before the status differs from one terminal to another.
export function pluginStatus(stdout, name) {
  const block = stdout.split(/\n\s*\n/).find((b) => b.includes(name)) ?? "";
  return {
    status: block.match(/Status:[^a-z\n]*([^\n]*)/)?.[1].trim() ?? null,
    error: block.match(/Error: ([^:\n]*)/)?.[1] ?? null,
    readFrom: block.match(/Read from: ([^\n]*)/)?.[1].trim() ?? null,
  };
}

export async function gitInit(dir) {
  fs.mkdirSync(dir, { recursive: true });
  const { code, stderr } = await run("git", ["init", "-q"], { cwd: dir });
  if (code !== 0) throw new Error(`git init failed: ${stderr}`);
}

// Runs `command` and resolves with its output, never rejecting: a probe
// checks the exit code itself. A `.cmd` shim, as npm and Volta install for a
// CLI, runs through `cmd.exe`, each argument quoted.
export function run(command, args, { cwd, env = {}, input, timeout = 120_000 } = {}) {
  return new Promise((resolve) => {
    const options = { cwd, env: { ...process.env, ...env }, windowsHide: true };
    const child = /\.cmd$/i.test(command)
      ? spawn("cmd.exe", ["/d", "/s", "/c", `"${[command, ...args].map((a) => `"${a.replaceAll('"', '""')}"`).join(" ")}"`], { ...options, windowsVerbatimArguments: true })
      : spawn(command, args, options);
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (d) => (stdout += d));
    child.stderr.on("data", (d) => (stderr += d));
    // Killing `cmd.exe` alone would leave the CLI holding its output open.
    const timer = setTimeout(() => spawn("taskkill", ["/pid", String(child.pid), "/t", "/f"], { windowsHide: true }), timeout);
    child.on("error", (error) => {
      clearTimeout(timer);
      resolve({ code: -1, stdout, stderr: stderr + String(error) });
    });
    child.on("close", (code) => {
      clearTimeout(timer);
      resolve({ code, stdout, stderr });
    });
    child.stdin.end(input ?? "");
  });
}

// Maps a free drive letter to `folder` with `subst`, so a probe can reach
// the root of a drive without writing a real one, and resolves with the
// letter. `unsubst` takes it off again.
export async function subst(folder) {
  const letter = [..."ZYXWVUTSRQPONMLKJIHG"].find((l) => !fs.existsSync(`${l}:\\`));
  if (!letter) throw new Error("subst: no drive letter from G to Z is free");
  fs.mkdirSync(folder, { recursive: true });
  const mapped = await run("subst.exe", [`${letter}:`, folder]);
  if (mapped.code !== 0) throw new Error(`subst ${letter}: failed: ${mapped.stdout}${mapped.stderr}`);
  return letter;
}

export const unsubst = (letter) => run("subst.exe", [`${letter}:`, "/d"]);

// Stops a probe that writes machine-wide paths, `%ProgramFiles%`,
// `%ProgramData%` or `HKLM`, unless it runs in Windows Sandbox, whose only
// user is WDAGUtilityAccount. `uat/agents/sandbox.ps1` runs it there.
export function sandboxOnly() {
  if (os.userInfo().username !== "WDAGUtilityAccount") {
    throw new Error("this probe writes machine-wide paths: run it in Windows Sandbox with uat/agents/sandbox.ps1");
  }
}

// Sets the registry value `name` of `key`, such as
// `HKLM\SOFTWARE\Policies\ClaudeCode`, to the string `data`, or deletes it
// when `data` is null.
export async function registry(key, name, data) {
  const args = data === null ? ["delete", key, "/v", name, "/f"] : ["add", key, "/v", name, "/t", "REG_SZ", "/d", data, "/f"];
  const { code, stderr } = await run("reg.exe", args);
  if (code !== 0 && data !== null) throw new Error(`reg ${args.join(" ")} failed: ${stderr}`);
}

// A local server that answers every request with `reply`, or with what
// `reply` returns for the request's body when it is a function, and keeps
// each request's body, so an Agent's request to its model never leaves the
// machine.
export async function modelServer(reply) {
  const bodies = [];
  const server = http.createServer((req, res) => {
    let body = "";
    req.on("data", (d) => (body += d)).on("end", () => {
      bodies.push(body);
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(typeof reply === "function" ? reply(body) : reply));
    });
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  return { url: `http://127.0.0.1:${server.address().port}`, bodies, close: () => server.close() };
}

// A local proxy that refuses every request, and an ssh command for git that
// refuses every connection. An Agent launched with `env`, and the git it
// runs, fetches nothing over HTTP(S) or ssh, and `tried()` lists what it
// tried to reach: the host of each request, and `ssh` for each connection.
export async function refusingProxy() {
  const hosts = [];
  const server = http.createServer((req, res) => {
    hosts.push(new URL(req.url, "http://unknown").host);
    res.writeHead(502).end();
  });
  // A client may reset its socket before the refusal reaches it.
  server.on("connect", (req, socket) => {
    hosts.push(req.url);
    socket.on("error", () => {});
    socket.end("HTTP/1.1 502 Bad Gateway\r\n\r\n");
  });
  server.on("clientError", (_, socket) => socket.destroy());
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  const url = `http://127.0.0.1:${server.address().port}`;
  // git runs the command through `sh`, the connection's arguments after it,
  // which `#` leaves out.
  const sshLog = path.join(root, `refused-ssh-${server.address().port}.txt`);
  fs.rmSync(sshLog, { force: true });
  const ssh = () => (fs.existsSync(sshLog) ? fs.readFileSync(sshLog, "utf8").split("\n").filter(Boolean) : []);
  return {
    env: { HTTPS_PROXY: url, HTTP_PROXY: url, NO_PROXY: "127.0.0.1", GIT_SSH_COMMAND: `echo ssh >> '${sshLog.replaceAll("\\", "/")}'; exit 1 #` },
    tried: () => [...hosts, ...ssh()],
    close: () => {
      server.close();
      fs.rmSync(sshLog, { force: true });
    },
  };
}

// The checks of one probe. Each check names the definition's table it tests,
// as `cargo xtask unverified` prints it, and says what the Agent did.
export function probe(agent, version) {
  const results = [];
  return {
    async check(table, title, fn) {
      let result;
      try {
        result = await fn();
      } catch (error) {
        result = { pass: false, observed: `threw: ${error.stack ?? error}` };
      }
      results.push({ agent, version, table, title, ...result });
      console.log(`${result.pass ? "PASS" : "FAIL"}  ${table}: ${title}`);
      if (!result.pass || process.env.CD_PROBE_VERBOSE) console.log(`      observed: ${result.observed}`);
    },
    finish() {
      const failed = results.filter((r) => !r.pass).length;
      console.log(`\n${agent} ${version}: ${results.length - failed} passed, ${failed} failed`);
      if (process.env.CD_PROBE_JSON) write(process.env.CD_PROBE_JSON, results);
      process.exitCode = failed ? 1 : 0;
    },
  };
}

// `{ pass, observed }` for a value that must equal `expected`.
export function same(observed, expected) {
  const o = JSON.stringify(observed);
  return { pass: o === JSON.stringify(expected), observed: o };
}

// `same` on the `keys` of each case, for checks that share what one set of
// sessions saw: `observed` and `expected` map a case's label to an object.
export function sameIn(keys, observed, expected) {
  const cut = (cases) => Object.fromEntries(Object.entries(cases).map(([label, o]) => [label, Object.fromEntries(keys.map((k) => [k, o[k]]))]));
  return same(cut(observed), cut(expected));
}
