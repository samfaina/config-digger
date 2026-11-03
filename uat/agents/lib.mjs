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
// The folder is named as Claude Code names it in every version: `<name>`
// when `generation` is missing or 1 or less, and `<name>~g<generation>`
// when it is 2 or more. `manifest` stands for the plugin manifest's
// content, an object or raw text. Returns the plugin's folder.
// Claude Code reads it while the gate `tengu_claudeai_plugins_sync_enabled`
// is on, and a sync round deletes it.
export function syncedPlugin(bucket, name, { generation, manifest = { name, version: "1.0.0" }, skills = [] } = {}) {
  const folder = generation >= 2 ? `${name}~g${generation}` : name;
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

// A Windows pseudo console, driven from Windows PowerShell's `Add-Type`
// rather than a native npm package. The C# below starts the command line in
// it, copies the keys it reads on its standard input to the console, and
// what the console renders to its standard output, and returns the
// command's exit code once it exits. The two variables that carry it are
// taken out before the command starts. `STARTF_USESTDHANDLES` with no
// handles keeps the command off PowerShell's redirected pipes.
const pseudoConsoleSource = String.raw`
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;
using Microsoft.Win32.SafeHandles;

public static class CdPseudoConsole {
  [StructLayout(LayoutKind.Sequential)] struct Coord { public short X; public short Y; }
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  struct StartupInfo {
    public int cb; public string lpReserved; public string lpDesktop; public string lpTitle;
    public int dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars, dwFillAttribute, dwFlags;
    public short wShowWindow, cbReserved2; public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError;
  }
  [StructLayout(LayoutKind.Sequential)] struct StartupInfoEx { public StartupInfo StartupInfo; public IntPtr lpAttributeList; }
  [StructLayout(LayoutKind.Sequential)] struct ProcessInformation { public IntPtr hProcess, hThread; public int dwProcessId, dwThreadId; }

  [DllImport("kernel32.dll", SetLastError = true)] static extern bool CreatePipe(out SafeFileHandle read, out SafeFileHandle write, IntPtr attributes, int size);
  [DllImport("kernel32.dll")] static extern int CreatePseudoConsole(Coord size, SafeFileHandle input, SafeFileHandle output, uint flags, out IntPtr console);
  [DllImport("kernel32.dll")] static extern void ClosePseudoConsole(IntPtr console);
  [DllImport("kernel32.dll", SetLastError = true)] static extern bool InitializeProcThreadAttributeList(IntPtr list, int count, int flags, ref IntPtr size);
  [DllImport("kernel32.dll")] static extern void DeleteProcThreadAttributeList(IntPtr list);
  [DllImport("kernel32.dll", SetLastError = true)] static extern bool UpdateProcThreadAttribute(IntPtr list, uint flags, IntPtr attribute, IntPtr value, IntPtr size, IntPtr previous, IntPtr returned);
  [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
  static extern bool CreateProcessW(string application, string commandLine, IntPtr processAttributes, IntPtr threadAttributes, bool inherit, uint flags, IntPtr environment, string cwd, ref StartupInfoEx info, out ProcessInformation process);
  [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr handle, uint ms);
  [DllImport("kernel32.dll")] static extern bool GetExitCodeProcess(IntPtr process, out uint code);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);

  static Thread Copy(Stream from, Stream to) {
    Thread t = new Thread(() => {
      byte[] buffer = new byte[8192];
      int n;
      try { while ((n = from.Read(buffer, 0, buffer.Length)) > 0) { to.Write(buffer, 0, n); to.Flush(); } } catch (IOException) { } catch (ObjectDisposedException) { }
    });
    t.IsBackground = true;
    t.Start();
    return t;
  }

  public static int Run(string commandLine, string cwd, short cols, short rows) {
    SafeFileHandle inRead, inWrite, outRead, outWrite;
    if (!CreatePipe(out inRead, out inWrite, IntPtr.Zero, 0) || !CreatePipe(out outRead, out outWrite, IntPtr.Zero, 0)) throw new Exception("CreatePipe: " + Marshal.GetLastWin32Error());
    Coord size = new Coord(); size.X = cols; size.Y = rows;
    IntPtr console;
    int result = CreatePseudoConsole(size, inRead, outWrite, 0, out console);
    if (result != 0) throw new Exception("CreatePseudoConsole: " + result);
    inRead.Dispose();
    outWrite.Dispose();
    IntPtr listSize = IntPtr.Zero;
    InitializeProcThreadAttributeList(IntPtr.Zero, 1, 0, ref listSize);
    StartupInfoEx info = new StartupInfoEx();
    info.StartupInfo.cb = Marshal.SizeOf(typeof(StartupInfoEx));
    info.StartupInfo.dwFlags = 0x100;
    info.lpAttributeList = Marshal.AllocHGlobal(listSize);
    if (!InitializeProcThreadAttributeList(info.lpAttributeList, 1, 0, ref listSize)) throw new Exception("InitializeProcThreadAttributeList: " + Marshal.GetLastWin32Error());
    if (!UpdateProcThreadAttribute(info.lpAttributeList, 0, (IntPtr)0x20016, console, (IntPtr)IntPtr.Size, IntPtr.Zero, IntPtr.Zero)) throw new Exception("UpdateProcThreadAttribute: " + Marshal.GetLastWin32Error());
    Environment.SetEnvironmentVariable("CD_PSEUDO_CONSOLE", null);
    Environment.SetEnvironmentVariable("CD_PSEUDO_CONSOLE_LINE", null);
    ProcessInformation process;
    if (!CreateProcessW(null, commandLine, IntPtr.Zero, IntPtr.Zero, false, 0x80000, IntPtr.Zero, cwd, ref info, out process)) throw new Exception("CreateProcess: " + Marshal.GetLastWin32Error());
    DeleteProcThreadAttributeList(info.lpAttributeList);
    Marshal.FreeHGlobal(info.lpAttributeList);
    FileStream keys = new FileStream(inWrite, FileAccess.Write);
    FileStream rendered = new FileStream(outRead, FileAccess.Read);
    Thread output = Copy(rendered, Console.OpenStandardOutput());
    Copy(Console.OpenStandardInput(), keys);
    WaitForSingleObject(process.hProcess, 0xFFFFFFFF);
    uint code;
    GetExitCodeProcess(process.hProcess, out code);
    CloseHandle(process.hThread);
    CloseHandle(process.hProcess);
    ClosePseudoConsole(console);
    output.Join(5000);
    keys.Dispose();
    rendered.Dispose();
    return (int)code;
  }
}
`;

// An argument as the command line of `CreateProcess` quotes it.
function quoteArg(a) {
  if (a !== "" && !/[\s"]/.test(a)) return a;
  return `"${a.replace(/(\\*)"/g, '$1$1\\"').replace(/(\\+)$/, "$1$1")}"`;
}

// What a console's output shows as text: its escape sequences taken out, a
// cursor moved right written as that many spaces.
function consoleText(output) {
  return output
    .replace(/\x1b\[(\d*)C/g, (_, n) => " ".repeat(Number(n || 1)))
    .replace(/\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b\[[0-?]*[ -/]*[@-~]|\x1b[()][0-9A-Za-z]|\x1b[0-9=>@-_]/g, "");
}

// Starts `command`, an executable rather than a `.cmd` shim, in a Windows
// pseudo console of 120 by 40, with `cwd` and the probe's environment plus
// `env`, as `run` does. `send` types keys, such as `"\r"`
// for Enter; `output` is what the console rendered so far, and `text` that
// without its escape sequences; `until` resolves with the text once `test`
// holds of it, or with null after `ms`; `exited` resolves with the exit
// code; `stop` ends the process tree, as `timeout` does. The variables a
// Claude Code session hands the commands it runs, `CLAUDECODE` and
// `CLAUDE_*`, are left out unless `env` sets them: an interactive Claude
// Code would take itself for that session's child, and save no transcript.
export function pseudoConsole(command, args, { cwd, env = {}, timeout = 180_000 } = {}) {
  const script = "$ErrorActionPreference = 'Stop'; Add-Type -TypeDefinition $env:CD_PSEUDO_CONSOLE; exit [CdPseudoConsole]::Run($env:CD_PSEUDO_CONSOLE_LINE, (Get-Location).Path, 120, 40)";
  const inherited = Object.fromEntries(Object.entries(process.env).filter(([name]) => !/^CLAUDE/i.test(name)));
  const child = spawn("powershell.exe", ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script], {
    cwd,
    env: { ...inherited, ...env, CD_PSEUDO_CONSOLE: pseudoConsoleSource, CD_PSEUDO_CONSOLE_LINE: [command, ...args].map(quoteArg).join(" ") },
    windowsHide: true,
  });
  let output = "";
  let stderr = "";
  // A character can come split across two chunks.
  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (d) => (output += d));
  child.stderr.on("data", (d) => (stderr += d));
  child.stdin.on("error", () => {});
  const stop = () => spawn("taskkill", ["/pid", String(child.pid), "/t", "/f"], { windowsHide: true });
  const timer = setTimeout(stop, timeout);
  const exited = new Promise((resolve) => {
    child.on("error", (error) => resolve({ code: -1, stderr: stderr + String(error) }));
    child.on("close", (code) => resolve({ code, stderr }));
  }).finally(() => clearTimeout(timer));
  let done = false;
  exited.then(() => (done = true));
  return {
    send: (keys) => child.stdin.write(keys),
    output: () => output,
    text: () => consoleText(output),
    async until(test, ms = 60_000) {
      for (const end = Date.now() + ms; Date.now() < end; await new Promise((r) => setTimeout(r, 200))) {
        if (test(consoleText(output))) return consoleText(output);
        if (done) break;
      }
      return test(consoleText(output)) ? consoleText(output) : null;
    },
    exited,
    async stop() {
      if (!done) stop();
      return exited;
    },
  };
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
// `reply` returns for the request's body when it is a function, once that
// settles when it is a promise, and keeps each request's body, and in
// `headers` its headers, so an Agent's request to its model never leaves the
// machine.
export async function modelServer(reply) {
  const bodies = [];
  const headers = [];
  const server = http.createServer((req, res) => {
    let body = "";
    req.on("data", (d) => (body += d)).on("end", async () => {
      bodies.push(body);
      headers.push(req.headers);
      const answer = await (typeof reply === "function" ? reply(body) : reply);
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(answer));
    });
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  return { url: `http://127.0.0.1:${server.address().port}`, bodies, headers, close: () => server.close() };
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
