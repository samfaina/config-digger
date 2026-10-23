// What every probe shares: throwaway folders, running an Agent's CLI, a local
// stand-in for a model API, and the checks a probe reports.
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

// A new empty folder under `base`, which defaults to the probe root.
export function fresh(name, base = root) {
  const dir = path.join(base, name);
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

export function write(file, content) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, typeof content === "string" ? content : JSON.stringify(content, null, 2));
}

export function json(file) {
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

// A skill folder holding a `SKILL.md` with `frontmatter`, given as YAML lines.
export function skill(dir, frontmatter, body = "body") {
  write(path.join(dir, "SKILL.md"), `---\n${frontmatter.join("\n")}\n---\n${body}\n`);
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

// A local server that answers every request with `reply` and keeps each
// request's body, so an Agent's request to its model never leaves the machine.
export async function modelServer(reply) {
  const bodies = [];
  const server = http.createServer((req, res) => {
    let body = "";
    req.on("data", (d) => (body += d)).on("end", () => {
      bodies.push(body);
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(reply));
    });
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  return { url: `http://127.0.0.1:${server.address().port}`, bodies, close: () => server.close() };
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
