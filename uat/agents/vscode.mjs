// Tests VS Code's facts that `vscode.toml` marked unverified, on throwaway
// profiles and Projects. Nothing reaches the user's own VS Code: each launch
// gets its own `--user-data-dir`, `--extensions-dir`, `--shared-data-dir`
// and `--agent-plugins-dir`, and its own `USERPROFILE` and `APPDATA`, so
// that what VS Code reads in the home folder, such as `~\.vscode\argv.json`
// and Claude Desktop's config, is throwaway too. It runs beside a VS Code
// the user has open, and stops only the processes of its own launches.
//
//   node uat/agents/vscode.mjs [--bin <Code.exe>] [--cli <config-digger.exe>] [--sign-in]
//
// `CD_PROBE_ONLY=files,broken-dot` runs those launches only: `plugins`,
// `manifests`, `install`, `files`,
// `broken-<folder|dot|user|plugin|workspace>`, `discovery`,
// `experiments`, `trust`, `nonce`, `enablement`, `skills`, `extensions`,
// `profiles` and `policy`.
//
// VS Code starts an MCP server only when asked. A probe extension, loaded
// with `--extensionDevelopmentPath`, asks it to start every server it
// loaded, `workbench.mcp.startServer *`, trusting the servers of other apps
// it discovers, then quits VS Code. Each server is this machine's Node
// running `server.mjs <tag>`, which writes a marker named after its tag,
// with the arguments and the `CD_PROBE_*` variables it got. The extension
// also gives VS Code servers of its own, which VS Code caches in the
// workspace's `state.vscdb`, and reports what the window saw through the
// proposed API `chatPromptFiles`, `--enable-proposed-api` letting it.
//
// Only the `policy` check runs signed in to a GitHub account, on the
// profile `--sign-in` opens for the owner: `vscode-account` in the probe
// root, or the folder `CD_PROBE_VSCODE_ACCOUNT` names there, whose name
// starts the same. The other launches sign in to nothing.
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { fresh, json, probe, root, run, same, skill, write } from "./lib.mjs";

// `node:sqlite` warns that it is experimental on every run.
process.removeAllListeners("warning");
const { DatabaseSync } = await import("node:sqlite");

const bin = process.argv.includes("--bin")
  ? process.argv[process.argv.indexOf("--bin") + 1]
  : path.join(process.env.LOCALAPPDATA, "Programs", "Microsoft VS Code", "Code.exe");
// `product.json` sits in the folder of the commit `Code.exe` runs.
const commit = fs.readdirSync(path.dirname(bin)).find((f) => fs.existsSync(path.join(path.dirname(bin), f, "resources", "app", "product.json")));
// The Installation's `resources\app`, which holds `product.json`, the CLI
// and the built-in extensions.
const app = path.join(path.dirname(bin), commit, "resources", "app");
const version = json(path.join(app, "product.json")).version;
// Whether the version is 1.`minor` or later.
const fromMinor = (minor) => {
  const [major, own] = version.split(".").map(Number);
  return major > 1 || own >= minor;
};
// The CLI of config-digger that the `nonce` launch asks, `--cli`, or else
// the debug build of this checkout, which `cargo build -p
// config-digger-cli` makes.
let cliPath = process.argv.includes("--cli") ? process.argv[process.argv.indexOf("--cli") + 1] : null;
const configDigger = () => {
  if (!cliPath) {
    const metadata = spawnSync("cargo", ["metadata", "--format-version", "1", "--no-deps"], { encoding: "utf8", maxBuffer: 1 << 26 });
    cliPath = path.join(JSON.parse(metadata.stdout).target_directory, "debug", "config-digger.exe");
  }
  if (!fs.existsSync(cliPath)) throw new Error(`no config-digger CLI at ${cliPath}: run cargo build -p config-digger-cli, or pass --cli`);
  return cliPath;
};

const powershell = async (command) => (await run("powershell.exe", ["-NoProfile", "-Command", command])).stdout.trim();
// The PowerShell that lists the processes given the data folder `data`.
const processesOf = (data) => `Get-CimInstance Win32_Process -Filter "Name='Code.exe'" | Where-Object { $_.CommandLine -like '*${data}*' }`;
// Stops what is left of a launch: the processes given its data folder only.
const stop = (data) => powershell(`${processesOf(data)} | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }`);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// The folders of a launch in `dir`.
const folders = (dir) => ({
  data: path.join(dir, "data"),
  user: path.join(dir, "data", "User"),
  global: path.join(dir, "data", "User", "globalStorage", "state.vscdb"),
  home: path.join(dir, "home"),
  appData: path.join(dir, "home", "AppData", "Roaming"),
  plugins: path.join(dir, "home", ".vscode", "agent-plugins"),
  shared: path.join(dir, "shared"),
  probeExt: path.join(dir, "probe-ext"),
});
// The flags that give a launch in `dir` its own profile and folders.
const profileArgs = (dir) => {
  const d = folders(dir);
  return [`--user-data-dir=${d.data}`, `--extensions-dir=${path.join(dir, "ext")}`, `--shared-data-dir=${d.shared}`, `--agent-plugins-dir=${d.plugins}`];
};

// The rows of `keys` in the SQLite store `file`, each value parsed, or
// `{}` when the file does not exist.
function rows(file, keys) {
  if (!fs.existsSync(file)) return {};
  const db = new DatabaseSync(file, { readOnly: true });
  const found = {};
  for (const key of keys) {
    const row = db.prepare("SELECT value FROM ItemTable WHERE key = ?").get(key);
    if (row) found[key] = JSON.parse(row.value);
  }
  db.close();
  return found;
}
// Sets the rows of `values` in the SQLite store `file`, as VS Code lays one
// out, each value as JSON text.
function plant(file, values) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const db = new DatabaseSync(file);
  db.exec("CREATE TABLE IF NOT EXISTS ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB)");
  for (const [key, value] of Object.entries(values)) db.prepare("INSERT INTO ItemTable (key, value) VALUES (?, ?)").run(key, JSON.stringify(value));
  db.close();
}

// The profile signed in to a GitHub account, a folder named
// `vscode-account*` in the probe root: `--sign-in` opens it for the owner.
const account = path.resolve(process.env.CD_PROBE_VSCODE_ACCOUNT ?? path.join(root, "vscode-account"));
if (path.dirname(account) !== root || !path.basename(account).startsWith("vscode-account")) {
  throw new Error(`CD_PROBE_VSCODE_ACCOUNT must be a folder named vscode-account* in ${root}, not ${account}`);
}
// Whether `dir`'s profile holds a GitHub session, which VS Code keeps
// encrypted in the shared data folder's store on Windows.
function accountSignedIn(dir) {
  const file = path.join(folders(dir).shared, "sharedStorage", "state.vscdb");
  if (!fs.existsSync(file)) return false;
  const db = new DatabaseSync(file, { readOnly: true });
  const row = db.prepare("SELECT key FROM ItemTable WHERE key LIKE 'secret://%vscode.github-authentication%github.auth%'").get();
  db.close();
  return Boolean(row);
}

// VS Code opens GitHub's sign-in page in the default browser, which gets
// the owner's own app data here, and so their own browser profile. The
// owner signs in from the Accounts menu; the probe only waits.
if (process.argv.includes("--sign-in")) {
  const d = folders(account);
  write(path.join(d.user, "settings.json"), { "telemetry.telemetryLevel": "off", "update.mode": "none", "workbench.startupEditor": "none" });
  const proj = path.join(account, "proj");
  fs.mkdirSync(proj, { recursive: true });
  fs.mkdirSync(d.home, { recursive: true });
  spawn(bin, [...profileArgs(account), "--new-window", proj], { detached: true, stdio: "ignore", env: { ...process.env, USERPROFILE: d.home } }).unref();
  console.log(`Sign in to GitHub from the Accounts menu of the window that opened, on the profile in ${account}, then close the window.`);
  const running = async () => Number(await powershell(`@(${processesOf(d.data)}).Count`));
  while (!(await running())) await sleep(1000);
  while (await running()) await sleep(2000);
  const done = accountSignedIn(account);
  console.log(done ? "The profile is signed in." : "The profile is not signed in.");
  process.exit(done ? 0 : 1);
}

const p = probe("VS Code", version);
const base = fresh("vscode");
const only = process.env.CD_PROBE_ONLY?.split(",");
const runs = (name) => !only || only.includes(name);

const script = path.join(base, "server.mjs");
write(
  script,
  `import fs from "node:fs";
import path from "node:path";
const [tag, ...args] = process.argv.slice(2);
const env = Object.fromEntries(Object.entries(process.env).filter(([k]) => k.startsWith("CD_PROBE_")));
fs.mkdirSync(path.join(import.meta.dirname, "markers"), { recursive: true });
fs.writeFileSync(path.join(import.meta.dirname, "markers", tag + ".json"), JSON.stringify({ args, env }));
// A \`serve-\` server answers VS Code as an MCP server with one tool, named
// after its tag, so that VS Code records what it saw of it and lists its
// tool.
if (tag.startsWith("serve-")) {
  const reply = (id, result) => process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id, result }) + "\\n");
  let buffer = "";
  process.stdin.on("data", (d) => {
    buffer += d;
    let end;
    while ((end = buffer.indexOf("\\n")) >= 0) {
      const message = JSON.parse(buffer.slice(0, end));
      buffer = buffer.slice(end + 1);
      if (message.method === "initialize") {
        reply(message.id, { protocolVersion: message.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: tag, version: "1.0.0" } });
      } else if (message.method === "tools/list") {
        reply(message.id, { tools: [{ name: tag.replaceAll("-", "_"), description: "probe", inputSchema: { type: "object", properties: {} } }] });
      } else if (message.id !== undefined) reply(message.id, {});
    }
  });
}
`,
);
const markers = path.join(base, "markers");
// A server that leaves the marker `tag` when VS Code starts it.
const server = (tag, extra = {}) => ({ command: process.execPath, args: [script, tag], ...extra });
// Servers `cd-throwaway-<tag>`, one per tag, with `,,` between them, which
// no JSON parser accepts.
const broken = (tags) => `{ ${tags.map((t) => `"cd-throwaway-${t}": ${JSON.stringify(server(t))}`).join(",, ")} }`;
const fileUri = (file) => pathToFileURL(file).href;
// A file's URI as VS Code spells it as text, its drive in lower case and
// its `:` encoded.
const vscodeUri = (file) => fileUri(file).replace(/^file:\/\/\/([A-Za-z]):/, (_, drive) => `file:///${drive.toLowerCase()}%3A`);
// A file's URI as VS Code writes one in a store, which it reads back with
// its scheme and path.
const storedUri = (file) => ({ $mid: 1, path: pathToFileURL(file).pathname, scheme: "file" });

// The probe extension: it gives VS Code `provider`'s servers, starts every
// server VS Code loaded with the options `start`, writes `done.json` and
// quits VS Code. `done.json` also holds whether the window is trusted, the
// default value of each setting of `inspect`, and the skills of the
// proposed API `chatPromptFiles`: those VS Code lists to its model,
// `chat.getSkills`, and those the user can run, `chat.getSlashCommands`.
// With `ask`, it first runs that command, which asks for a text, writes
// `asking.json` for the probe to type it, and waits for the command,
// whose result `done.json` holds as `answered`.
function extension(dir, { provider, start = { autoTrustChanges: true }, inspect = [], ask = null }) {
  write(path.join(dir, "package.json"), {
    name: "probe",
    publisher: "cd-throwaway",
    version: "0.0.1",
    engines: { vscode: "^1.100.0" },
    main: "extension.js",
    activationEvents: ["onStartupFinished"],
    enabledApiProposals: ["chatPromptFiles"],
    capabilities: { untrustedWorkspaces: { supported: true } },
    contributes: { mcpServerDefinitionProviders: [{ id: "cd-throwaway-provider", label: "cd-throwaway-provider" }] },
  });
  write(path.join(dir, "probe.json"), { provider, start, inspect, ask, asking: path.join(dir, "asking.json"), done: path.join(dir, "done.json") });
  write(
    path.join(dir, "extension.js"),
    `const vscode = require("vscode");
const fs = require("fs");
const config = require("./probe.json");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const skillList = (list) => (list ?? []).map((s) => ({ name: s.name, path: s.uri.fsPath, description: s.description ?? null }));
exports.activate = async () => {
  const p = config.provider;
  if (p) {
    vscode.lm.registerMcpServerDefinitionProvider("cd-throwaway-provider", {
      provideMcpServerDefinitions: () => [
        new vscode.McpStdioServerDefinition("cd-throwaway-ext-stdio", p.command, p.args, p.env, "1.0.0"),
        new vscode.McpHttpServerDefinition("cd-throwaway-ext-http", vscode.Uri.parse(p.uri), p.headers, "1.0.0"),
      ],
    });
  }
  await sleep(8000);
  let error = null;
  let answered = null;
  if (config.ask) {
    const asked = vscode.commands.executeCommand(config.ask, { skipReveal: true });
    fs.writeFileSync(config.asking, "{}");
    answered = await Promise.race([asked.then((r) => r ?? null, (e) => "threw: " + String(e)), sleep(90000).then(() => "timeout")]);
    await sleep(8000);
  }
  try {
    await Promise.race([vscode.commands.executeCommand("workbench.mcp.startServer", "*", config.start), sleep(20000)]);
  } catch (e) {
    error = String(e);
  }
  await sleep(4000);
  const settings = vscode.workspace.getConfiguration();
  let skills = null;
  let commands = null;
  try {
    skills = skillList(await vscode.chat.getSkills(new vscode.CancellationTokenSource().token));
    commands = skillList(await vscode.chat.getSlashCommands(new vscode.CancellationTokenSource().token));
  } catch (e) {
    error = (error ? error + "; " : "") + String(e);
  }
  fs.writeFileSync(config.done, JSON.stringify({
    error,
    answered,
    folders: (vscode.workspace.workspaceFolders ?? []).map((f) => f.uri.fsPath),
    workspaceFile: vscode.workspace.workspaceFile ? vscode.workspace.workspaceFile.fsPath : null,
    trusted: vscode.workspace.isTrusted,
    defaults: Object.fromEntries(config.inspect.map((key) => [key, settings.inspect(key)?.defaultValue ?? null])),
    skills,
    commands,
    tools: vscode.lm.tools.map((t) => t.name),
  }));
  await vscode.commands.executeCommand("workbench.action.quit");
};
`,
  );
}

// Launches VS Code on `open`, a folder or a `.code-workspace` file, with the
// user's `settings`, and resolves once the probe extension is done: the
// servers VS Code started, by tag, what the extension saw, and the
// workspace's `mcp.extCachedServers`. A launch with `trust` keeps
// workspace trust on, and one with `shared: false` leaves the shared data
// folder where VS Code puts it, `~\.vscode-shared` of the launch's home.
// One with `answer`, `{ command, text }`, has the extension run `command`
// and types `text` into the input box it opens, over the Chrome DevTools
// Protocol.
async function launch(dir, open, { settings = {}, provider = null, start, inspect, env = {}, args: extra = [], trust = false, shared = true, answer = null, seconds = 120 } = {}) {
  const d = folders(dir);
  write(path.join(d.user, "settings.json"), {
    "security.workspace.trust.enabled": trust,
    "security.workspace.trust.startupPrompt": "never",
    "telemetry.telemetryLevel": "off",
    "update.mode": "none",
    "extensions.autoUpdate": false,
    "extensions.autoCheckUpdates": false,
    "workbench.startupEditor": "none",
    "chat.mcp.autostart": "never",
    ...settings,
  });
  extension(d.probeExt, { provider, start, inspect, ask: answer?.command });
  const doneFile = path.join(d.probeExt, "done.json");
  const askingFile = path.join(d.probeExt, "asking.json");
  fs.rmSync(doneFile, { force: true });
  fs.rmSync(askingFile, { force: true });
  const port = answer && (await freePort());
  // 1.129.1 starts a profile's servers in the home folder, and fails to
  // when it does not exist.
  fs.mkdirSync(d.home, { recursive: true });
  fs.rmSync(markers, { recursive: true, force: true });
  const args = [
    ...profileArgs(dir).filter((a) => shared || !a.startsWith("--shared-data-dir=")),
    `--extensionDevelopmentPath=${d.probeExt}`,
    "--enable-proposed-api=cd-throwaway.probe",
    ...(trust ? [] : ["--disable-workspace-trust"]),
    "--skip-welcome",
    "--skip-release-notes",
    "--log=trace",
    ...(port ? [`--remote-debugging-port=${port}`] : []),
    ...extra,
    "--new-window",
    open,
  ];
  spawn(bin, args, { detached: true, stdio: "ignore", env: { ...process.env, USERPROFILE: d.home, APPDATA: d.appData, ...env } }).unref();
  const end = Date.now() + seconds * 1000;
  if (answer) {
    while (!fs.existsSync(askingFile) && Date.now() < end) await sleep(1000);
    if (fs.existsSync(askingFile)) await typeAnswer(port, answer.text);
  }
  while (!fs.existsSync(doneFile) && Date.now() < end) await sleep(1000);
  await sleep(3000);
  await stop(d.data);
  const done = fs.existsSync(doneFile) ? json(doneFile) : { timeout: true };
  const started = fs.existsSync(markers)
    ? Object.fromEntries(fs.readdirSync(markers).map((f) => [f.slice(0, -".json".length), json(path.join(markers, f))]))
    : {};
  return { done, started, tags: Object.keys(started).sort(), cache: extCache(d.data) };
}

const freePort = () =>
  new Promise((resolve) => {
    const s = net.createServer().listen(0, "127.0.0.1", () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });

// Types `text` into the input box the window on the DevTools port `port`
// shows, once it shows one, and presses Enter.
async function typeAnswer(port, text) {
  const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
  const page = targets.find((t) => t.type === "page" && t.url.includes("workbench"));
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((r) => ws.addEventListener("open", r));
  let id = 0;
  const pending = new Map();
  ws.addEventListener("message", (e) => {
    const m = JSON.parse(e.data);
    pending.get(m.id)?.(m);
    pending.delete(m.id);
  });
  const send = (method, params) =>
    new Promise((resolve) => {
      pending.set(++id, resolve);
      ws.send(JSON.stringify({ id, method, params }));
    });
  const evaluate = async (expression) => (await send("Runtime.evaluate", { expression, returnByValue: true })).result?.result?.value;
  const input = `[...document.querySelectorAll(".quick-input-widget input")].find((i) => i.offsetParent !== null)`;
  const end = Date.now() + 20_000;
  while (!(await evaluate(`Boolean(${input})`)) && Date.now() < end) await sleep(500);
  await evaluate(`${input}?.focus()`);
  await send("Input.insertText", { text });
  await sleep(500);
  for (const type of ["keyDown", "keyUp"]) await send("Input.dispatchKeyEvent", { type, key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 });
  ws.close();
}

// The `mcp.extCachedServers` of the workspace the launch opened.
function extCache(data) {
  const storage = path.join(data, "User", "workspaceStorage");
  if (!fs.existsSync(storage)) return null;
  for (const ws of fs.readdirSync(storage)) {
    const cached = rows(path.join(storage, ws, "state.vscdb"), ["mcp.extCachedServers"])["mcp.extCachedServers"];
    if (cached) return cached;
  }
  return null;
}

// A marketplace laid out the way VS Code's code clones one from a git URL,
// into `agent-plugins\<host>\<owner>\<repo>`, holding a plugin per `[tag,
// files]`, each listed in `installed.json`. `files` maps a path in the
// plugin's folder to its content. The folder of each plugin, by tag.
const marketUrl = "https://cd-throwaway.invalid/cd-throwaway/market.git";
function marketplace(plugins, list) {
  const market = path.join(plugins, "cd-throwaway.invalid", "cd-throwaway", "market");
  const installed = [];
  const folders = {};
  for (const [tag, files] of list) {
    const name = `cd-throwaway-${tag}`;
    const folder = path.join(market, "plugins", tag);
    for (const [file, content] of Object.entries(files)) write(path.join(folder, file), content);
    installed.push({ pluginUri: fileUri(folder), marketplace: marketUrl, name });
    folders[tag] = folder;
  }
  const listed = list.map(([tag]) => ({ name: `cd-throwaway-${tag}`, source: `./plugins/${tag}`, description: "probe plugin", version: "1.0.0" }));
  write(path.join(market, ".github", "plugin", "marketplace.json"), { name: "cd-throwaway-market", owner: { name: "cd-throwaway" }, plugins: listed });
  write(path.join(plugins, "installed.json"), { version: 1, installed });
  return folders;
}
const manifest = (tag, extra = {}) => ({ name: `cd-throwaway-${tag}`, version: "1.0.0", description: "probe plugin", ...extra });
const dotMcp = (tag) => ({ mcpServers: { [`cd-throwaway-${tag}`]: server(tag) } });
// Claude Desktop's config in a launch's `APPDATA`, with the server `desktop`
// or the servers of `config`.
const claudeDesktop = (d, config = dotMcp("desktop")) => write(path.join(d.appData, "Claude", "claude_desktop_config.json"), config);

// ---------------------------------------------------------------- agent plugins

if (runs("plugins")) {
  const dir = fresh("plugins", base);
  const { plugins } = folders(dir);
  marketplace(plugins, [
    ["plugin-root", { "plugin.json": manifest("plugin-root"), ".mcp.json": dotMcp("plugin-root") }],
    ["plugin-github", { ".github/plugin/plugin.json": manifest("plugin-github"), ".mcp.json": dotMcp("plugin-github") }],
    ["plugin-claude", { ".claude-plugin/plugin.json": manifest("plugin-claude"), ".mcp.json": dotMcp("plugin-claude") }],
    ["plugin-open", { ".plugin/plugin.json": manifest("plugin-open"), ".mcp.json": dotMcp("plugin-open") }],
    ["plugin-bare", { ".mcp.json": dotMcp("plugin-bare") }],
    // A manifest that names another file of servers.
    ["plugin-named", { "plugin.json": manifest("plugin-named", { mcpServers: "./servers.json" }), ".mcp.json": dotMcp("named-dot"), "servers.json": dotMcp("named-file") }],
  ]);
  // A plugin directly in `agent-plugins`, which `installed.json` does not list.
  write(path.join(plugins, "direct", "plugin.json"), manifest("plugin-direct"));
  write(path.join(plugins, "direct", ".mcp.json"), dotMcp("plugin-direct"));
  const seen = await launch(dir, fresh("proj", dir));
  const pluginTags = seen.tags.filter((t) => t.startsWith("plugin-"));

  await p.check("plugins[agent-plugins]", "VS Code reads the plugins `installed.json` lists, here in a marketplace's clone `<host>\\<owner>\\<repo>`, whatever their manifest, and not a folder directly in `agent-plugins` it does not list", async () =>
    same(pluginTags, ["plugin-bare", "plugin-claude", "plugin-github", "plugin-open", "plugin-root"]),
  );
  await p.check("locations[plugin-mcp]", "a plugin's servers are in the `.mcp.json` at its root, and a manifest's `mcpServers` adds the file it names", async () =>
    same(seen.tags.filter((t) => t.startsWith("named-")), ["named-dot", "named-file"]),
  );
}

// ---------------------------------------------------------------- what a manifest's `mcpServers` holds

// Plugins like `plugin-named` of the `plugins` launch, each with a
// `.mcp.json`: one per other place of the manifest, naming a file; one
// whose manifest holds its servers inline, one a list of a path and an
// object; and one whose `.mcp.json` and named file both name
// `cd-throwaway-clash`, with `chat.mcp.collisionBehavior` at `disable`, so
// that only one of the two can run. `tests/vscode.rs` reads the same
// plugins.
if (runs("manifests")) {
  const dir = fresh("manifests", base);
  const { plugins } = folders(dir);
  const named = (tag, place) => [tag, { [place]: manifest(tag, { mcpServers: "./servers.json" }), ".mcp.json": dotMcp(`${tag}-dot`), "servers.json": dotMcp(`${tag}-file`) }];
  marketplace(plugins, [
    named("github", ".github/plugin/plugin.json"),
    named("claude", ".claude-plugin/plugin.json"),
    named("open", ".plugin/plugin.json"),
    ["inline", { "plugin.json": manifest("inline", { mcpServers: { "cd-throwaway-inline-object": server("inline-object") } }), ".mcp.json": dotMcp("inline-dot") }],
    ["list", { "plugin.json": manifest("list", { mcpServers: ["./servers.json", { "cd-throwaway-list-object": server("list-object") }] }), ".mcp.json": dotMcp("list-dot"), "servers.json": dotMcp("list-file") }],
    [
      "clash",
      {
        "plugin.json": manifest("clash", { mcpServers: "./servers.json" }),
        ".mcp.json": { mcpServers: { "cd-throwaway-clash": server("clash-dot") } },
        "servers.json": { mcpServers: { "cd-throwaway-clash": server("clash-file") } },
      },
    ],
  ]);
  const seen = await launch(dir, fresh("proj", dir), { settings: { "chat.mcp.collisionBehavior": "disable" } });
  const of = (tag) => seen.tags.filter((t) => t.startsWith(`${tag}-`));

  await p.check("locations[plugin-manifest-mcp]", "a manifest in `.claude-plugin` or `.plugin` that names a file in `mcpServers` adds that file's servers to the `.mcp.json`'s, as the root `plugin.json` does", async () =>
    same({ claude: of("claude"), open: of("open") }, { claude: ["claude-dot", "claude-file"], open: ["open-dot", "open-file"] }),
  );
  // 1.143.1 reads it, and fails this check.
  await p.check("locations[plugin-manifest-mcp]", "a manifest in `.github\\plugin` is not read: the file its `mcpServers` names adds nothing to the `.mcp.json`'s servers", async () =>
    same(of("github"), ["github-dot"]),
  );
  await p.check("locations[plugin-manifest-mcp]", "a manifest's inline `mcpServers` object takes the place of the plugin's `.mcp.json`: its servers start, and the `.mcp.json`'s do not", async () =>
    same(of("inline"), ["inline-object"]),
  );
  await p.check("locations[plugin-manifest-mcp]", "of a manifest's `mcpServers` list of a path and an object, VS Code reads the path, beside the `.mcp.json`, and skips the object", async () =>
    same(of("list"), ["list-dot", "list-file"]),
  );
  await p.check("locations[plugin-manifest-mcp]", "of a server the plugin's `.mcp.json` and the file its manifest names both name, the `.mcp.json`'s starts, and the named file's does not", async () =>
    same(of("clash"), ["clash-dot"]),
  );
}

// ---------------------------------------------------------------- a plugin VS Code installs from a marketplace

// VS Code installs a plugin itself: the extension runs `Install Plugin from
// Source`, `workbench.action.chat.installPluginFromSource`, and the probe
// types the marketplace's URL into the box it opens. The marketplace is a
// git repo of the launch, which `chat.plugins.marketplaces` names by an
// `https` URL on a host that does not exist: git's `insteadOf`, in a
// throwaway global config that `GIT_CONFIG_GLOBAL` gives VS Code, clones
// it from the repo's folder, so that no request leaves the machine. The
// marketplace is trusted beforehand in the profile's store, as VS Code
// trusts it once the user agrees to its dialog; its key is the marketplace's
// `canonicalId` in VS Code's code [binary]. A marketplace of one plugin
// installs that plugin without asking which.
if (runs("install")) {
  const dir = fresh("install", base);
  const d = folders(dir);
  const repo = path.join(dir, "market-repo");
  write(path.join(repo, ".github", "plugin", "marketplace.json"), {
    name: "cd-throwaway-market",
    owner: { name: "cd-throwaway" },
    plugins: [{ name: "cd-throwaway-installed", source: "./plugins/installed", description: "probe plugin", version: "1.0.0" }],
  });
  write(path.join(repo, "plugins", "installed", "plugin.json"), manifest("installed"));
  write(path.join(repo, "plugins", "installed", ".mcp.json"), dotMcp("installed"));
  const gitconfig = path.join(dir, "gitconfig");
  fs.writeFileSync(
    gitconfig,
    `[user]\n\tname = cd-throwaway\n\temail = cd-throwaway@cd-throwaway.invalid\n[url "${pathToFileURL(repo).href}"]\n\tinsteadOf = ${marketUrl}\n`,
  );
  const gitEnv = { GIT_CONFIG_GLOBAL: gitconfig, GIT_CONFIG_NOSYSTEM: "1", GIT_TERMINAL_PROMPT: "0" };
  for (const args of [["init", "-q"], ["add", "-A"], ["commit", "-q", "-m", "cd-throwaway"]]) {
    const done = await run("git", args, { cwd: repo, env: gitEnv });
    if (done.code !== 0) throw new Error(`git ${args[0]} failed: ${done.stderr}`);
  }
  plant(d.global, { "chat.plugins.trustedMarketplaces.v1": ["git:cd-throwaway.invalid/cd-throwaway/market.git"] });
  const seen = await launch(dir, fresh("proj", dir), {
    settings: { "chat.plugins.marketplaces": [marketUrl] },
    env: gitEnv,
    answer: { command: "workbench.action.chat.installPluginFromSource", text: marketUrl },
    seconds: 240,
  });
  const listed = fs.existsSync(path.join(d.plugins, "installed.json")) ? json(path.join(d.plugins, "installed.json")) : null;
  const clone = path.join(d.plugins, "cd-throwaway.invalid", "cd-throwaway", "market");
  const folder = path.join(clone, "plugins", "installed");
  // 1.143.1 adds the folder's path below `agent-plugins`, which 1.141.0
  // does not write.
  const relativePath = fromMinor(143) ? { relativePath: path.relative(d.plugins, folder).replaceAll("\\", "/") } : {};

  await p.check("plugins[agent-plugins]", "VS Code clones a marketplace it installs a plugin from into `agent-plugins\\<host>\\<owner>\\<repo>`, and lists the plugin in `installed.json` as `{\"version\": 1, \"installed\": [{\"pluginUri\", \"marketplace\", \"name\"}]}`, its `pluginUri` the `file:` URI of the plugin's folder in the clone, from 1.143.1 with a `relativePath` too", async () =>
    same(
      {
        answered: seen.done.answered,
        clone: fs.existsSync(path.join(clone, ".git")),
        listed,
      },
      {
        answered: true,
        clone: true,
        listed: { version: 1, installed: [{ pluginUri: vscodeUri(folder), ...relativePath, marketplace: marketUrl, name: "cd-throwaway-installed" }] },
      },
    ),
  );
  await p.check("plugins[agent-plugins]", "VS Code starts the server of the `.mcp.json` of the plugin it installed", async () => same(seen.tags, ["installed"]));
}

// ---------------------------------------------------------------- the MCP files, read whole

if (runs("files")) {
  const dir = fresh("files", base);
  const d = folders(dir);
  const proj = path.join(dir, "proj");
  write(path.join(proj, ".vscode", "mcp.json"), { servers: { "cd-throwaway-folder": server("folder") } });
  write(path.join(proj, ".mcp.json"), {
    mcpServers: {
      "cd-throwaway-dot": server("dot", {
        args: [script, "dot", "${CD_PROBE_REF}", "${env:CD_PROBE_REF}", "$CD_PROBE_REF"],
        env: { CD_PROBE_BARE: "${CD_PROBE_REF}", CD_PROBE_ENV: "${env:CD_PROBE_REF}" },
      }),
    },
  });
  write(path.join(d.user, "mcp.json"), { servers: { "cd-throwaway-user": server("user") } });
  claudeDesktop(d);
  const seen = await launch(dir, proj, {
    settings: { "chat.mcp.discovery.enabled": { "claude-desktop": true }, "chat.mcp.workspaceRootConfig.enabled": false },
    provider: {
      command: process.execPath,
      args: [script, "ext-stdio", "--token=not-a-real-secret"],
      env: { CD_PROBE_KEY: "not-a-real-secret" },
      uri: "http://cd-throwaway:not-a-real-secret@127.0.0.1:9/mcp?key=not-a-real-secret&mode=x#t=not-a-real-secret",
      headers: { Authorization: "Bearer not-a-real-secret" },
    },
    env: { CD_PROBE_REF: "from-env" },
  });

  await p.check("locations[dot-mcp]", "VS Code takes every value of `.mcp.json` as written: `${NAME}`, `${env:NAME}` and `$NAME` reach the server unchanged", async () => {
    const dot = seen.started.dot;
    return same(dot && { args: dot.args, bare: dot.env.CD_PROBE_BARE, env: dot.env.CD_PROBE_ENV }, {
      args: ["${CD_PROBE_REF}", "${env:CD_PROBE_REF}", "$CD_PROBE_REF"],
      bare: "${CD_PROBE_REF}",
      env: "${env:CD_PROBE_REF}",
    });
  });
  await p.check("locations[dot-mcp]", "`chat.mcp.workspaceRootConfig.enabled: false` does not stop VS Code reading `.mcp.json`", async () =>
    same(["dot", "folder", "user"].filter((t) => seen.tags.includes(t)), ["dot", "folder", "user"]),
  );
  await p.check("locations[claude-desktop-mcp]", "the user's `chat.mcp.discovery.enabled` turns Claude Desktop's config on", async () => same(seen.tags.includes("desktop"), true));
  // `external` is there from 1.135.0 on, and not in 1.129.1.
  const hasExternal = fromMinor(135);
  await p.check("locations[extension-mcp]", "the cache spells a launch's `env` as an object, its `headers` as `[name, value]` pairs, its `args` as an array, and its `uri` as a URI object whose `authority`, `query` and `fragment` hold the userinfo, the query and the fragment, and, from 1.135.0, whose `external` holds the whole URL with `=` and `&` encoded", async () => {
    const servers = seen.cache?.["cd-throwaway.probe/cd-throwaway-provider"]?.servers ?? [];
    const launchOf = (label) => servers.find((s) => s.label === label)?.launch;
    const stdio = launchOf("cd-throwaway-ext-stdio");
    const http = launchOf("cd-throwaway-ext-http");
    const spelled = {
      stdio: stdio && { type: stdio.type, args: stdio.args.slice(1), env: stdio.env },
      http: http && {
        type: http.type,
        headers: http.headers,
        uri: Object.keys(http.uri).filter((k) => k !== "external").sort(),
        external: http.uri.external ?? null,
        authority: http.uri.authority,
        query: http.uri.query,
        fragment: http.uri.fragment,
        path: http.uri.path,
      },
    };
    const expected = {
      stdio: { type: 1, args: ["ext-stdio", "--token=not-a-real-secret"], env: { CD_PROBE_KEY: "not-a-real-secret" } },
      http: {
        type: 2,
        headers: [["Authorization", "Bearer not-a-real-secret"]],
        uri: ["$mid", "authority", "fragment", "path", "query", "scheme"],
        external: hasExternal ? "http://cd-throwaway:not-a-real-secret@127.0.0.1:9/mcp?key%3Dnot-a-real-secret%26mode%3Dx#t%3Dnot-a-real-secret" : null,
        authority: "cd-throwaway:not-a-real-secret@127.0.0.1:9",
        query: "key=not-a-real-secret&mode=x",
        fragment: "t=not-a-real-secret",
        path: "/mcp",
      },
    };
    return { ...same(spelled, expected), observed: JSON.stringify(servers) };
  });
}

// ---------------------------------------------------------------- files that do not parse

// Each launch breaks one location's file and keeps a server in two others,
// `folder-ok` in `.vscode\mcp.json` and `user-ok` in the profile's
// `mcp.json`, unless the broken file is one of them.
async function brokenLaunch(where, setup) {
  const dir = fresh(`broken-${where}`, base);
  const d = folders(dir);
  const proj = path.join(dir, "proj");
  write(path.join(proj, ".vscode", "mcp.json"), { servers: { "cd-throwaway-folder-ok": server("folder-ok") } });
  write(path.join(d.user, "mcp.json"), { servers: { "cd-throwaway-user-ok": server("user-ok") } });
  const { file, open = proj, settings } = setup({ dir, d, proj });
  const text = fs.readFileSync(file, "utf8");
  const seen = await launch(dir, open, { settings });
  return { ...seen, untouched: fs.readFileSync(file, "utf8") === text };
}

if (runs("broken-folder")) {
  const seen = await brokenLaunch("folder", ({ proj }) => {
    const file = path.join(proj, ".vscode", "mcp.json");
    write(file, `{ "servers": ${broken(["folder-a", "folder-b"])} }`);
    return { file };
  });
  await p.check("locations[folder-mcp]", "VS Code skips a `.vscode\\mcp.json` it cannot parse, loads the other files, and leaves the file as it is", async () =>
    same({ tags: seen.tags, untouched: seen.untouched }, { tags: ["user-ok"], untouched: true }),
  );
}
if (runs("broken-dot")) {
  const seen = await brokenLaunch("dot", ({ proj }) => {
    const file = path.join(proj, ".mcp.json");
    write(file, `{ "mcpServers": ${broken(["dot-a", "dot-b"])} }`);
    return { file };
  });
  await p.check("locations[dot-mcp]", "VS Code skips a `.mcp.json` it cannot parse and loads the other files", async () =>
    same({ tags: seen.tags, untouched: seen.untouched }, { tags: ["folder-ok", "user-ok"], untouched: true }),
  );
}
if (runs("broken-user")) {
  const seen = await brokenLaunch("user", ({ d }) => {
    const file = path.join(d.user, "mcp.json");
    write(file, `{ "servers": ${broken(["user-a", "user-b"])} }`);
    return { file };
  });
  await p.check("locations[user-mcp]", "VS Code skips a profile's `mcp.json` it cannot parse, loads the other files, and leaves the file as it is", async () =>
    same({ tags: seen.tags, untouched: seen.untouched }, { tags: ["folder-ok"], untouched: true }),
  );
}

// Four errors besides `,,`, each in a plugin's `.mcp.json` and in a
// `.code-workspace` file, one member per line, and what VS Code keeps of
// each, as `node-jsonc-parser`'s code reads it (#437). `tests/vscode.rs`
// reads the same texts, its servers and folders named apart.
const member = (key, value) => `"${key}": ${JSON.stringify(value)}`;
// `items` as code spans in a sentence, "`a`, `b` and `c`".
const codeList = (items) => items.map((i) => `\`${i}\``).join(", ").replace(/, ([^,]*)$/, " and $1");
const pluginServer = (t) => member(`cd-throwaway-plugin-${t}`, server(`plugin-${t}`));
// The server `plugin-b` with a member whose string is left open: it runs
// to the end of the line, swallowing the `}` and `,` after it.
const openServer = `${pluginServer("b").slice(0, -1)}, "note": "open },`;
const pluginErrors = [
  { name: "comma", error: "a missing comma between two servers", kept: ["a", "b", "c"], text: `{ "mcpServers": {\n  ${pluginServer("a")}\n  ${pluginServer("b")},\n  ${pluginServer("c")}\n} }` },
  { name: "value", error: "a key with no value", kept: ["b", "c"], text: `{ "mcpServers": {\n  "cd-throwaway-plugin-a": ,\n  ${pluginServer("b")},\n  ${pluginServer("c")}\n} }` },
  { name: "open-object", error: "an object left open at the end of the file", kept: ["a", "b"], text: `{ "mcpServers": {\n  ${pluginServer("a")},\n  ${pluginServer("b")}\n` },
  { name: "open-string", error: "a string left open inside a server", kept: ["a", "b"], text: `{ "mcpServers": {\n  ${pluginServer("a")},\n  ${openServer}\n  ${pluginServer("c")}\n} }` },
];
const workspaceSettings = {
  "chat.mcp.discovery.enabled": { "claude-desktop": true },
  mcp: { servers: { "cd-throwaway-workspace": server("workspace") } },
};
const workspaceFolders = member("folders", [{ path: "proj" }, { path: "other" }]);
const workspaceErrors = [
  { name: "comma", error: "a missing comma between `folders` and `settings`", kept: ["other", "proj"], text: `{\n  ${workspaceFolders}\n  ${member("settings", workspaceSettings)}\n}` },
  {
    name: "value",
    error: "a key with no value between `folders` and `settings`",
    kept: ["other", "proj"],
    text: `{\n  ${workspaceFolders},\n  "cd-throwaway-a": ,\n  ${member("settings", workspaceSettings)}\n}`,
  },
  {
    name: "open-object",
    error: "`settings` left open at the end of the file",
    kept: ["other", "proj"],
    text: `{\n  ${workspaceFolders},\n  "settings": {\n    ${Object.entries(workspaceSettings).map(([k, v]) => member(k, v)).join(",\n    ")}\n`,
  },
  {
    name: "open-string",
    error: "a string left open inside the first folder",
    kept: ["proj"],
    text: `{\n  "folders": [\n    { "path": "proj", "name": "open },\n    { "path": "other" }\n  ],\n  ${member("settings", workspaceSettings)}\n}`,
  },
];

if (runs("broken-plugin")) {
  const seen = await brokenLaunch("plugin", ({ d }) => {
    const plugin = marketplace(d.plugins, [["plugin-p", { "plugin.json": manifest("plugin-p"), ".mcp.json": `{ "mcpServers": ${broken(["plugin-a", "plugin-b"])} }` }]]);
    return { file: path.join(plugin["plugin-p"], ".mcp.json") };
  });
  await p.check("locations[plugin-mcp]", "VS Code reads an agent plugin's `.mcp.json` it cannot parse around the error: both servers start", async () =>
    same(seen.tags, ["folder-ok", "plugin-a", "plugin-b", "user-ok"]),
  );
  for (const { name, error, kept, text } of pluginErrors) {
    const seen = await brokenLaunch(`plugin-${name}`, ({ d }) => {
      const plugin = marketplace(d.plugins, [["plugin-p", { "plugin.json": manifest("plugin-p"), ".mcp.json": text }]]);
      return { file: path.join(plugin["plugin-p"], ".mcp.json") };
    });
    const started = kept.map((t) => `plugin-${t}`);
    await p.check("locations[plugin-mcp]", `VS Code reads an agent plugin's \`.mcp.json\` around ${error}: of \`plugin-a\`, \`plugin-b\` and \`plugin-c\`, ${codeList(started)} start`, async () =>
      same(seen.tags, ["folder-ok", ...started, "user-ok"]),
    );
  }
}
if (runs("broken-workspace")) {
  const seen = await brokenLaunch("workspace", ({ dir, d }) => {
    const file = path.join(dir, "cd-throwaway.code-workspace");
    // The one error is outside `settings.mcp`.
    const settings = { "chat.mcp.discovery.enabled": { "claude-desktop": true }, mcp: { servers: { "cd-throwaway-workspace": server("workspace") } } };
    write(file, `{ "folders": [{ "path": "proj" }],, "settings": ${JSON.stringify(settings)} }`);
    claudeDesktop(d);
    return { file, open: file };
  });
  await p.check("locations[workspace-mcp]", "VS Code skips the servers of a `.code-workspace` file it cannot parse, and still opens its folders and applies its other settings", async () =>
    same(
      { tags: seen.tags, folders: seen.done.folders?.map((f) => path.basename(f)), untouched: seen.untouched },
      { tags: ["desktop", "folder-ok", "user-ok"], folders: ["proj"], untouched: true },
    ),
  );
  // `desktop` starts when VS Code applies `chat.mcp.discovery.enabled`,
  // and `workspace` would if it read the file's servers.
  for (const { name, error, kept, text } of workspaceErrors) {
    const seen = await brokenLaunch(`workspace-${name}`, ({ dir, d }) => {
      const file = path.join(dir, "cd-throwaway.code-workspace");
      fs.mkdirSync(path.join(dir, "other"), { recursive: true });
      write(file, text);
      claudeDesktop(d);
      return { file, open: file };
    });
    await p.check("locations[workspace-mcp]", `VS Code reads a \`.code-workspace\` file around ${error}: it opens ${codeList(kept)}, applies \`chat.mcp.discovery.enabled\`, starts none of the file's servers and leaves the file as it is`, async () =>
      same(
        { tags: seen.tags, folders: seen.done.folders?.map((f) => path.basename(f)).sort(), untouched: seen.untouched },
        { tags: ["desktop", "folder-ok", "user-ok"], folders: kept, untouched: true },
      ),
    );
  }
}

// ---------------------------------------------------------------- which settings turn discovery on

// Whether VS Code reads Claude Desktop's config, with
// `chat.mcp.discovery.enabled` set in a Project folder's settings, a
// workspace file's or the user's. `workspace` opens the folder in a
// `.code-workspace` file holding those settings.
async function discovers(name, { folder, workspace, user }) {
  const on = (value) => ({ "chat.mcp.discovery.enabled": { "claude-desktop": value } });
  const dir = fresh(`discovery-${name}`, base);
  const d = folders(dir);
  const proj = path.join(dir, "proj");
  fs.mkdirSync(proj, { recursive: true });
  if (folder !== undefined) write(path.join(proj, ".vscode", "settings.json"), on(folder));
  let open = proj;
  if (workspace !== undefined) {
    open = path.join(dir, "cd-throwaway.code-workspace");
    write(open, { folders: [{ path: "proj" }], settings: workspace === null ? {} : on(workspace) });
  }
  claudeDesktop(d);
  return (await launch(dir, open, { settings: user === undefined ? {} : on(user) })).tags.includes("desktop");
}

if (runs("discovery")) {
  await p.check("locations[claude-desktop-mcp]", "a folder's `.vscode\\settings.json` turns discovery on, and off over the user's", async () =>
    same({ on: await discovers("folder", { folder: true }), overUser: await discovers("folder-over-user", { folder: false, user: true }) }, { on: true, overUser: false }),
  );
  await p.check("locations[claude-desktop-mcp]", "a workspace file's `settings` turn discovery on, and off over the user's", async () =>
    same({ on: await discovers("workspace", { workspace: true }), overUser: await discovers("workspace-over-user", { workspace: false, user: true }) }, { on: true, overUser: false }),
  );
  await p.check("locations[claude-desktop-mcp]", "in a workspace file, a member folder's `.vscode\\settings.json` is not read, on or off", async () =>
    same(
      { on: await discovers("member-on", { folder: true, workspace: null }), off: await discovers("member-off", { folder: false, workspace: null, user: true }) },
      { on: false, off: true },
    ),
  );
}

// ---------------------------------------------------------------- the ExP experiment cache

// VS Code reads its cache of ExP assignments only while the telemetry level
// is `all`. The launch sends every request to a closed local port, so that
// neither its telemetry nor a request for new assignments leaves the
// machine. `control` is a setting whose schema marks it for experiments in
// every version tested; `assigned` holds the settings `vscode.toml` once
// read in the cache, each assigned a value other than its default. Claude
// Desktop's config holds a server VS Code would start were discovery on.
const control = "editor.inlineSuggest.experimental.suppressInlineSuggestions";
if (runs("experiments")) {
  const dir = fresh("experiments", base);
  const d = folders(dir);
  const proj = path.join(dir, "proj");
  write(path.join(proj, ".vscode", "mcp.json"), { servers: { "cd-throwaway-folder": server("folder") } });
  claudeDesktop(d);
  const assigned = {
    "chat.mcp.access": "none",
    "chat.mcp.collisionBehavior": "suffix",
    "chat.mcp.discovery.enabled": { "claude-desktop": true, "cursor-global": true, "cursor-workspace": true, windsurf: true },
    "chat.useAgentSkills": false,
    "security.workspace.trust.enabled": false,
  };
  plant(d.global, {
    "memento/experiment.service.memento": {
      "VSCode.ABExp.FeatureData": {
        features: [],
        assignmentContext: "",
        configs: [
          { Id: "cd-throwaway", Parameters: { [`config.${control}`]: "cd-throwaway-first" } },
          { Id: "vscode", Parameters: { [`config.${control}`]: "cd-throwaway-vscode", ...Object.fromEntries(Object.entries(assigned).map(([k, v]) => [`config.${k}`, v])) } },
        ],
      },
    },
  });
  const seen = await launch(dir, proj, {
    settings: { "telemetry.telemetryLevel": "all", "http.proxy": "http://127.0.0.1:9" },
    args: ["--proxy-server=http://127.0.0.1:9"],
    inspect: [control, ...Object.keys(assigned)],
  });

  await p.check("(top)", "VS Code takes a setting's ExP default from the `Parameters` of the item of `configs` whose `Id` is `vscode`, as `config.<setting>`, here the second item", async () =>
    same(seen.done.defaults?.[control], "cd-throwaway-vscode"),
  );
  await p.check("(top)", "VS Code runs no experiment on `chat.mcp.access`, `chat.mcp.collisionBehavior`, `chat.mcp.discovery.enabled`, `chat.useAgentSkills` or `security.workspace.trust.enabled`: an assignment leaves their defaults, and the servers run as without it", async () =>
    same(
      {
        defaults: Object.entries(assigned).map(([k, v]) => JSON.stringify(seen.done.defaults?.[k]) === JSON.stringify(v)),
        started: seen.tags,
      },
      { defaults: [false, false, false, false, false], started: ["folder"] },
    ),
  );
}

// ---------------------------------------------------------------- the trusted folders

if (runs("trust")) {
  const dir = fresh("trust", base);
  const d = folders(dir);
  const trusted = path.join(dir, "trusted");
  const other = path.join(dir, "other");
  fs.mkdirSync(trusted, { recursive: true });
  fs.mkdirSync(other, { recursive: true });
  plant(path.join(d.home, ".vscode-shared", "sharedStorage", "state.vscdb"), { "content.trust.model.key": { uriTrustInfo: [{ uri: storedUri(trusted), trusted: true }] } });
  const opened = await launch(dir, trusted, { trust: true, shared: false });
  const elsewhere = await launch(dir, other, { trust: true, shared: false });

  await p.check("items.mcp.gates[0]", "without `--shared-data-dir`, VS Code reads the folders it trusts from `~\\.vscode-shared\\sharedStorage\\state.vscdb`: a folder trusted there opens trusted, another does not", async () =>
    same({ trusted: opened.done.trusted, other: elsewhere.done.trusted }, { trusted: true, other: false }),
  );
}

// ---------------------------------------------------------------- the trust of a discovered server

// A server of Claude Desktop's config, which VS Code trusts at the nonce of
// its launch configuration. It answers VS Code, which records the nonce of
// the configuration it last ran in the profile's `mcpToolCache`. The CLI of
// config-digger, built from this checkout, reads the launch's profile as a
// home it added, with the launch's home folder and app data as the user's,
// and says whether the server's nonce, as it computes it, is the one VS
// Code trusted.
if (runs("nonce")) {
  const dir = fresh("nonce", base);
  const d = folders(dir);
  const proj = path.join(dir, "proj");
  fs.mkdirSync(proj, { recursive: true });
  const name = "cd-throwaway-serve-desktop";
  const id = `claude-desktop.null.${name}`;
  const desktop = (...extra) =>
    claudeDesktop(d, { mcpServers: { [name]: server("serve-desktop", { args: [script, "serve-desktop", ...extra], env: { CD_PROBE_SECRET: "not-a-real-secret" } }) } });
  const record = () => rows(d.global, ["mcpToolCache"]).mcpToolCache?.serverTools?.find(([key]) => key === id)?.[1] ?? null;
  const matching = (r) => typeof r?.nonce === "string" && r.nonce === r.trustedAtNonce;
  const settings = { "chat.mcp.discovery.enabled": { "claude-desktop": true } };
  // Starting with `promptType: "never"` starts a trusted server only.
  const untrusting = { settings, start: { promptType: "never" } };
  const env = { USERPROFILE: d.home, APPDATA: d.appData, LOCALAPPDATA: path.join(d.home, "AppData", "Local") };
  // The State config-digger gives the server, and its reason's code.
  const seen = async () => {
    const { stdout } = await run(configDigger(), ["--json", "--no-project", "show", name, "--agent", "vscode", "--home", d.user], { env });
    const entry = JSON.parse(stdout).item?.entries?.[0];
    return [entry?.state, entry?.reason?.code].filter(Boolean).join(" ");
  };
  desktop();
  const trusting = await launch(dir, proj, { settings });
  const trusted = record();
  const added = await run(configDigger(), ["homes", "add", d.user, "--agent", "vscode"], { env });
  if (added.code !== 0) throw new Error(`config-digger homes add failed: ${added.stdout}${added.stderr}`);
  const trustedSeen = await seen();
  const again = await launch(dir, proj, untrusting);
  desktop("changed");
  const before = record();
  const changedSeen = await seen();
  const changed = await launch(dir, proj, untrusting);
  const after = record();
  const retrusting = await launch(dir, proj, { settings });
  const retrusted = record();
  const retrustedSeen = await seen();

  await p.check("items.mcp.gates[1]", "once trusted, a discovered server starts without asking while its file is unchanged, its record's `trustedAtNonce` and `nonce` matching", async () =>
    same(
      { trusted: trusting.tags, matching: matching(trusted), again: again.tags },
      { trusted: ["serve-desktop"], matching: true, again: ["serve-desktop"] },
    ),
  );
  await p.check("items.mcp.gates[1]", "once its file changes, VS Code no longer starts it without asking, though its record's two nonces still match, before that launch and after it", async () =>
    same({ started: changed.tags, before: matching(before), after: matching(after) }, { started: [], before: true, after: true }),
  );
  await p.check("items.mcp.gates[1]", "the `trustedAtNonce` VS Code writes is the nonce config-digger computes for a stdio server, with its `args` and `env`, in the home folder: Effective once trusted, Blocked with `not_approved` once its file changes, and Effective again once VS Code trusts the new launch at a new nonce", async () =>
    same(
      { trusted: trustedSeen, changed: changedSeen, retrusted: retrustedSeen, started: retrusting.tags, renewed: typeof retrusted?.trustedAtNonce === "string" && retrusted.trustedAtNonce !== trusted?.trustedAtNonce },
      { trusted: "effective", changed: "blocked not_approved", retrusted: "effective", started: ["serve-desktop"], renewed: true },
    ),
  );
}

// ---------------------------------------------------------------- servers turned off in a store

// Each location's server `serve-<location>`, which a pair of
// `mcp.enablement` turns off, in the workspace's store for a Project's
// file and in the profile's for the others, and `serve-on`, which none
// names. `workbench.mcp.startServer` starts a server that is off all the
// same, but VS Code gives the model no tool of it: the oracle is the tools
// `lm.tools` lists, each server's named after its tag. One launch opens a
// folder, the other a `.code-workspace` file.
const serving = (tag) => ({ mcpServers: { [`cd-throwaway-${tag}`]: server(tag) } });
// The tags of `seen`'s servers that VS Code lists a tool of, of `tags`.
const tooled = (seen, tags) => tags.filter((tag) => seen.done.tools?.some((t) => new RegExp(`(^|_)serve_${tag.replaceAll("-", "_")}$`).test(t)));
// Launches VS Code on `open` twice, with `options`: before and after it
// plants each store's `false` pairs, `{ store: [id, ...] }`, the
// workspace's store found once the first launch made it.
async function enablement(dir, open, options, pairs) {
  const d = folders(dir);
  const first = await launch(dir, open, options);
  plant(path.join(d.user, "workspaceStorage", workspaceStorage(d, open), "state.vscdb"), { "mcp.enablement": pairs.workspace.map((id) => [id, false]) });
  plant(d.global, { "mcp.enablement": pairs.profile.map((id) => [id, false]) });
  return { first, second: await launch(dir, open, options) };
}

if (runs("enablement")) {
  const dir = fresh("enablement", base);
  const d = folders(dir);
  const proj = path.join(dir, "proj");
  write(path.join(proj, ".vscode", "mcp.json"), { servers: { "cd-throwaway-serve-folder": server("serve-folder"), "cd-throwaway-serve-on": server("serve-on") } });
  write(path.join(proj, ".mcp.json"), serving("serve-dot"));
  write(path.join(d.user, "mcp.json"), { servers: { "cd-throwaway-serve-user": server("serve-user") } });
  claudeDesktop(d, serving("serve-desktop"));
  const plugin = marketplace(d.plugins, [["serve-plugin", { "plugin.json": manifest("serve-plugin"), ".mcp.json": serving("serve-plugin") }]])["serve-plugin"];
  // The ids the definition's `switch_id`s make.
  const folder = await enablement(
    dir,
    proj,
    {
      settings: { "chat.mcp.discovery.enabled": { "claude-desktop": true } },
      provider: { command: process.execPath, args: [script, "serve-extension"], env: {}, uri: "http://127.0.0.1:9/mcp", headers: {} },
    },
    {
      workspace: ["mcp.config.ws0.cd-throwaway-serve-folder", "workspace-dot-mcp.0.cd-throwaway-serve-dot"],
      profile: [
        "mcp.config.usrlocal.cd-throwaway-serve-user",
        "claude-desktop.null.cd-throwaway-serve-desktop",
        `plugin.${vscodeUri(plugin)}.cd-throwaway-serve-plugin`,
        "cd-throwaway.probe/cd-throwaway-ext-stdio",
      ],
    },
  );
  const tags = ["desktop", "dot", "extension", "folder", "on", "plugin", "user"];

  const wsDir = fresh("enablement-workspace", base);
  const w = folders(wsDir);
  write(path.join(wsDir, "proj", ".cursor", "mcp.json"), serving("serve-cursor-workspace"));
  write(path.join(w.home, ".cursor", "mcp.json"), serving("serve-cursor"));
  write(path.join(w.home, ".codeium", "windsurf", "mcp_config.json"), serving("serve-windsurf"));
  const file = path.join(wsDir, "cd-throwaway.code-workspace");
  write(file, { folders: [{ path: "proj" }], settings: { mcp: { servers: { "cd-throwaway-serve-workspace": server("serve-workspace"), "cd-throwaway-serve-on": server("serve-on") } } } });
  const workspace = await enablement(
    wsDir,
    file,
    { settings: { "chat.mcp.discovery.enabled": { "cursor-workspace": true, "cursor-global": true, windsurf: true } } },
    {
      workspace: ["mcp.config.workspace.cd-throwaway-serve-workspace", "cursor-workspace.0.cd-throwaway-serve-cursor-workspace"],
      profile: ["cursor.null.cd-throwaway-serve-cursor", "windsurf.null.cd-throwaway-serve-windsurf"],
    },
  );
  const wsTags = ["cursor", "cursor-workspace", "on", "windsurf", "workspace"];

  await p.check("items.mcp.disable[0]", "VS Code reads `mcp.enablement` in a workspace's store and the profile's as the JSON text of `[id, enabled]` pairs, each id the server's collection followed by its name, as each location's `switch_id` makes it, and gives the model no tool of a server a `false` names", async () =>
    same(
      {
        folder: [tooled(folder.first, tags), tooled(folder.second, tags)],
        workspace: [tooled(workspace.first, wsTags), tooled(workspace.second, wsTags)],
      },
      { folder: [tags, ["on"]], workspace: [wsTags, ["on"]] },
    ),
  );
}

// The folder of `d`'s `workspaceStorage` that holds the store of `open`, a
// folder or a `.code-workspace` file.
function workspaceStorage(d, open) {
  const storage = path.join(d.user, "workspaceStorage");
  const uri = decodeURIComponent(fileUri(open)).toLowerCase();
  return fs.readdirSync(storage).find((ws) => {
    const file = path.join(storage, ws, "workspace.json");
    if (!fs.existsSync(file)) return false;
    const opened = json(file);
    return decodeURIComponent(opened.folder ?? opened.workspace ?? "").toLowerCase() === uri;
  });
}

// ---------------------------------------------------------------- skills

// The oracle is the proposed API `chatPromptFiles`: `chat.getSkills` is
// the list VS Code builds its model's `<skills>` from, dropping only those
// without a `description` or with `disable-model-invocation` [binary], and
// `chat.getSlashCommands` the skills the user can run.
if (runs("skills")) {
  const dir = fresh("skills", base);
  const d = folders(dir);
  const proj = path.join(dir, "proj");
  const at = (folder, name) => path.join(proj, folder, "skills", name);
  for (const folder of [".agents", ".github", ".claude"]) skill(at(folder, "cd-throwaway-order"), [`description: ${folder}`]);
  for (const folder of [".github", ".claude"]) skill(at(folder, "cd-throwaway-order-b"), [`description: ${folder}`]);
  write(path.join(at(".agents", "cd-throwaway-bare"), "SKILL.md"), "A skill with no frontmatter.\n");
  skill(at(".agents", "cd-throwaway-broken"), ["description: [unclosed"]);
  skill(at(".agents", "cd-throwaway-broken-field"), ["description: probe", "argument-hint: [unclosed"]);
  skill(at(".agents", "cd-throwaway-broken-line"), ["description: probe", "not a key"]);
  skill(at(".agents", "cd-throwaway-broken-colon"), ["description: a: b"]);
  write(path.join(at(".agents", "cd-throwaway-unclosed"), "SKILL.md"), "---\ndescription: probe\nbody\n");
  // A personal skill, which the same code reads.
  skill(path.join(d.home, ".copilot", "skills", "cd-throwaway-broken-user"), ["description: probe", "not a key"]);
  skill(at(".agents", "cd-throwaway-off"), ["description: probe"]);
  skill(at(".agents", "cd-throwaway-off-folder"), ["description: probe"]);
  plant(d.global, {
    "chat.disabledPromptFiles.skill": [storedUri(path.join(at(".agents", "cd-throwaway-off"), "SKILL.md")), storedUri(at(".agents", "cd-throwaway-off-folder"))],
  });
  const seen = await launch(dir, proj);
  const named = (list, name) => (list ?? []).filter((s) => s.name === name);
  const where = (name) => named(seen.done.skills, name).map((s) => path.relative(proj, s.path));

  await p.check("locations[folder-agents-skills]", "of a workspace folder's skills of one name, VS Code keeps the `.agents` one, then the `.github` one, over the `.claude` one", async () =>
    same(
      { order: where("cd-throwaway-order"), b: where("cd-throwaway-order-b") },
      { order: [path.join(".agents", "skills", "cd-throwaway-order", "SKILL.md")], b: [path.join(".github", "skills", "cd-throwaway-order-b", "SKILL.md")] },
    ),
  );
  await p.check("items.skill", "VS Code loads a `SKILL.md` with no frontmatter under its folder's name, with no description, and the user can run it", async () =>
    same(
      { skill: named(seen.done.skills, "cd-throwaway-bare").map((s) => s.description), command: named(seen.done.commands, "cd-throwaway-bare").length },
      { skill: [null], command: 1 },
    ),
  );
  await p.check("items.skill", "VS Code loads a `SKILL.md` whose frontmatter is not YAML, or never closes, under its folder's name, a workspace folder's or a personal one, keeping the values it can read", async () => {
    const broken = ["broken", "broken-field", "broken-line", "broken-colon", "unclosed", "broken-user"];
    return same(
      Object.fromEntries(broken.map((tag) => [tag, named(seen.done.skills, `cd-throwaway-${tag}`).map((s) => s.description)])),
      { broken: [null], "broken-field": ["probe"], "broken-line": ["probe"], "broken-colon": ["a: b"], unclosed: ["probe"], "broken-user": ["probe"] },
    );
  });
  await p.check("items.skill.disable[0]", "`chat.disabledPromptFiles.skill` in the profile's store names a skill by its `SKILL.md`: the user can no longer run it, while a skill named by its folder stays", async () =>
    same(
      { off: named(seen.done.commands, "cd-throwaway-off").length, folder: named(seen.done.commands, "cd-throwaway-off-folder").length },
      { off: 0, folder: 1 },
    ),
  );
  await p.check("items.skill.disable[0]", "a skill turned off there, with a description, stays in `chat.getSkills`, the list VS Code builds its model's `<skills>` from", async () =>
    same(named(seen.done.skills, "cd-throwaway-off").length, 1),
  );
}

// ---------------------------------------------------------------- built-in, installed and updated extensions

// An extension's folder in `dir`, as `publisher.name` at `version`, which
// contributes one skill per name of `skills` in its `chatSkills`, each with
// the description `from`.
function extensionFolder(dir, { publisher = "cd-throwaway", name, version, skills, from }) {
  write(path.join(dir, "package.json"), {
    name,
    publisher,
    version,
    engines: { vscode: "^1.100.0" },
    contributes: { chatSkills: skills.map((s) => ({ path: `./skills/${s}/SKILL.md` })) },
  });
  for (const s of skills) skill(path.join(dir, "skills", s), [`description: ${from}`]);
}

// A VSIX of the extension `extension` describes, as `extensionFolder` takes
// it: a zip archive holding it in `extension`, which VS Code's CLI installs.
// VS Code names the folder of an extension installed from a VSIX with no
// target platform [test].
async function vsix(file, extension) {
  const src = fresh(path.basename(file, ".vsix"), path.join(path.dirname(file), "vsix-src"));
  extensionFolder(path.join(src, "extension"), extension);
  const packed = await run(path.join(process.env.SystemRoot, "System32", "tar.exe"), ["--format", "zip", "-cf", file, "-C", src, "extension"]);
  if (packed.code !== 0) throw new Error(`tar failed: ${packed.stderr}`);
  return file;
}

// Runs VS Code's CLI on the profile of `dir`, as `bin\code.cmd` does:
// `Code.exe` as Node, outside development mode, running `cli.js`.
async function cli(dir, args) {
  const d = folders(dir);
  fs.mkdirSync(d.home, { recursive: true });
  return run(bin, [path.join(app, "out", "cli.js"), ...profileArgs(dir), ...args], { env: { ELECTRON_RUN_AS_NODE: "1", VSCODE_DEV: "", USERPROFILE: d.home, APPDATA: d.appData } });
}

// The built-in extensions are the folder `--builtin-extensions-dir` names,
// here a copy of the Installation's `resources\app\extensions` with the
// probe's added, so that the Installation stays as it is. Each extension
// installed in the launch's `--extensions-dir` comes from VS Code's CLI, as
// a VSIX. The CLI refuses to install a built-in extension's id, so the
// extensions that have one are installed before the probe adds the
// built-in ones, as when VS Code starts to ship an extension the user had
// installed, as it did GitHub Copilot Chat. The oracle is the skills
// `chat.getSkills` lists, each named after the extension that contributes
// it.
if (runs("extensions")) {
  const dir = fresh("extensions", base);
  const builtin = path.join(dir, "builtin");
  fs.cpSync(path.join(app, "extensions"), builtin, { recursive: true });
  const extensions = path.join(dir, "ext");
  const packs = fresh("vsix", dir);
  const builtinArg = `--builtin-extensions-dir=${builtin}`;
  const install = async (extension) => {
    const file = await vsix(path.join(packs, `${extension.name}-${extension.version}.vsix`), extension);
    return cli(dir, [builtinArg, "--install-extension", file, "--force"]);
  };

  // One skill name in a built-in extension and an installed one, whose
  // folder sorts first. Another in two built-in extensions, and another in
  // two installed ones, the one whose folder sorts last installed first: in
  // each pair, the folder that sorts first holds the id that sorts last.
  await install({ name: "rank-b", version: "1.0.0", skills: ["cd-throwaway-rank", "cd-throwaway-rank-installed"], from: "installed b" });
  await install({ name: "rank-b-0", version: "1.0.0", skills: ["cd-throwaway-rank-installed"], from: "installed b-0" });
  extensionFolder(path.join(builtin, "zz-cd-throwaway-rank"), { name: "rank-builtin", version: "1.0.0", skills: ["cd-throwaway-rank", "cd-throwaway-rank-builtin"], from: "built-in" });
  extensionFolder(path.join(builtin, "zz-cd-throwaway-rank-2"), { name: "aa-rank", version: "1.0.0", skills: ["cd-throwaway-rank-builtin"], from: "built-in 2" });
  // An installed extension with a built-in one's id, at a higher version,
  // the same one and a lower one; and a built-in extension whose id the CLI
  // is asked to install, at a higher version, with none of that id
  // installed.
  const twins = [
    { name: "newer", builtin: "1.0.0", installed: "2.0.0" },
    { name: "same", builtin: "1.0.0", installed: "1.0.0" },
    { name: "older", builtin: "2.0.0", installed: "1.0.0" },
  ];
  const twinInstalls = [];
  for (const { name, installed } of twins) {
    const installing = await install({ name, version: installed, skills: [`cd-throwaway-${name}-installed`], from: "installed" });
    twinInstalls.push(installing.code);
  }
  for (const { name, builtin: version } of twins) {
    extensionFolder(path.join(builtin, `cd-throwaway-${name}`), { name, version, skills: [`cd-throwaway-${name}-builtin`], from: "built-in" });
  }
  extensionFolder(path.join(builtin, "cd-throwaway-alone"), { name: "alone", version: "1.0.0", skills: ["cd-throwaway-alone-builtin"], from: "built-in" });
  const refused = await install({ name: "alone", version: "2.0.0", skills: ["cd-throwaway-alone-installed"], from: "installed" });
  // An extension updated, version 1.0.0 then 2.0.0, its publisher in mixed
  // case.
  const updated = { publisher: "CD-Throwaway", name: "updated", from: "installed" };
  await install({ ...updated, version: "1.0.0", skills: ["cd-throwaway-updated-1"] });
  const update = await install({ ...updated, version: "2.0.0", skills: ["cd-throwaway-updated-2"] });
  const obsoleteFile = path.join(extensions, ".obsolete");
  const updatedFolders = () => fs.readdirSync(extensions).filter((f) => f.startsWith("cd-throwaway.updated-")).sort();
  const obsolete = fs.existsSync(obsoleteFile) ? fs.readFileSync(obsoleteFile, "utf8") : null;
  const kept = updatedFolders();

  const seen = await launch(dir, fresh("proj", dir), { args: [builtinArg] });
  // The descriptions of the skills named `name` that VS Code lists.
  const descriptions = (name) => (seen.done.skills ?? []).filter((s) => s.name === name).map((s) => s.description);
  const listed = (names) => names.filter((name) => descriptions(name).length);

  await p.check("plugins[built-in-extensions]", "of two extensions' skills of one name, VS Code keeps the built-in extension's over the installed one's, whose folder sorts first, and within each group the one whose folder sorts first, not its id", async () =>
    same(
      { over: descriptions("cd-throwaway-rank"), builtin: descriptions("cd-throwaway-rank-builtin"), installed: descriptions("cd-throwaway-rank-installed") },
      { over: ["built-in"], builtin: ["built-in"], installed: ["installed b-0"] },
    ),
  );
  await p.check("plugins[built-in-extensions]", "VS Code loads an installed extension with a built-in one's id in its place when its version is higher, and the built-in one when its version is the same or lower; its CLI refuses to install such an extension", async () =>
    same(
      {
        installed: twinInstalls,
        loaded: Object.fromEntries(twins.map(({ name }) => [name, listed([`cd-throwaway-${name}-builtin`, `cd-throwaway-${name}-installed`])])),
        refused: { failed: refused.code !== 0, said: `${refused.stdout}${refused.stderr}`.includes("is a built-in extension"), listed: listed(["cd-throwaway-alone-builtin", "cd-throwaway-alone-installed"]) },
      },
      {
        installed: [0, 0, 0],
        loaded: { newer: ["cd-throwaway-newer-installed"], same: ["cd-throwaway-same-builtin"], older: ["cd-throwaway-older-builtin"] },
        refused: { failed: true, said: true, listed: ["cd-throwaway-alone-builtin"] },
      },
    ),
  );
  await p.check("plugins[extensions]", "updating an extension leaves its old version's folder, named `<id>-<version>` in lower case, and `.obsolete`, a JSON object holding `true` under that name; VS Code then loads the new version only, and deletes the folder and the file", async () =>
    same(
      {
        update: update.code,
        obsolete: obsolete && JSON.parse(obsolete),
        kept,
        skills: listed(["cd-throwaway-updated-1", "cd-throwaway-updated-2"]),
        after: { folders: updatedFolders(), obsolete: fs.existsSync(obsoleteFile) },
      },
      {
        update: 0,
        obsolete: { "cd-throwaway.updated-1.0.0": true },
        kept: ["cd-throwaway.updated-1.0.0", "cd-throwaway.updated-2.0.0"],
        skills: ["cd-throwaway-updated-2"],
        after: { folders: ["cd-throwaway.updated-2.0.0"], obsolete: false },
      },
    ),
  );
}

// ---------------------------------------------------------------- the profile a workspace was last opened in

// Opens `target` in a launch in `dir` as a user would, without the probe
// extension, and stops it once its `storage.json` holds `until`, or after
// `seconds`. It resolves to what `storage.json` then holds.
async function open(dir, target, { args: extra = [], until, seconds = 90 }) {
  const d = folders(dir);
  fs.mkdirSync(d.home, { recursive: true });
  const args = [...profileArgs(dir), "--disable-workspace-trust", "--skip-welcome", "--skip-release-notes", ...extra, "--new-window", target];
  spawn(bin, args, { detached: true, stdio: "ignore", env: { ...process.env, USERPROFILE: d.home, APPDATA: d.appData } }).unref();
  const file = path.join(d.user, "globalStorage", "storage.json");
  const read = () => (fs.existsSync(file) ? json(file) : {});
  const end = Date.now() + seconds * 1000;
  while (!until(read()) && Date.now() < end) await sleep(1000);
  await sleep(2000);
  await stop(d.data);
  return read();
}

// A folder and a `.code-workspace` file, each opened with `--profile Work`,
// which creates the profile: `storage.json` then associates each with it
// under `profileAssociations.workspaces`, keyed by the workspace's URI. An
// extension development window, as the probe extension's, is associated
// with nothing.
if (runs("profiles")) {
  const dir = fresh("profiles", base);
  const d = folders(dir);
  const proj = path.join(dir, "proj");
  fs.mkdirSync(proj, { recursive: true });
  fs.mkdirSync(path.join(dir, "member"), { recursive: true });
  const workspace = path.join(dir, "multi.code-workspace");
  write(workspace, { folders: [{ path: "member" }] });
  await launch(dir, proj, { args: ["--profile", "Work"] });
  const developed = json(path.join(d.user, "globalStorage", "storage.json")).profileAssociations?.workspaces ?? {};
  const associated = (uri) => (storage) => Boolean(storage.profileAssociations?.workspaces?.[uri]);
  await open(dir, proj, { args: ["--profile", "Work"], until: associated(vscodeUri(proj)) });
  const storage = await open(dir, workspace, { args: ["--profile", "Work"], until: associated(vscodeUri(workspace)) });
  const work = (storage.userDataProfiles ?? []).find((profile) => profile.name === "Work");
  const associations = storage.profileAssociations?.workspaces ?? {};

  await p.check("projects", "an extension development window opened with `--profile Work` associates its folder with no profile", async () => same(developed, {}));
  await p.check("projects", "`profileAssociations.workspaces` keys a folder by its URI and a `.code-workspace` file by the file's URI, as VS Code spells a URI as text", async () =>
    same(Object.keys(associations).sort(), [vscodeUri(proj), vscodeUri(workspace)].sort()),
  );
  await p.check("projects", "the value of a workspace opened with `--profile Work` is the id of the profile `userDataProfiles` names Work, its `location`", async () =>
    same(
      { location: typeof work?.location === "string" && !work.location.includes("/"), values: Object.values(associations) },
      { location: true, values: [work?.location, work?.location] },
    ),
  );
}

// ---------------------------------------------------------------- the account's policy

// VS Code fetches the account's entitlements at each start, then the
// Copilot token's, which bring `policyData.mcp`, and, while `mcp` is not
// `false`, the MCP registry's, which bring `policyData.mcpAccess`, each of
// the last two only once its last fetch is an hour old [binary]. Two
// launches in a row tell the times apart. The account's policy must allow
// MCP, `mcp: true`, or VS Code fetches no registry data to date.
if (runs("policy") && !accountSignedIn(account)) {
  await p.check("items.mcp.rules[0]", "the account profile is signed in", async () => ({ pass: false, observed: `${account} is not: run node uat/agents/vscode.mjs --sign-in` }));
} else if (runs("policy")) {
  const policy = () => rows(folders(account).global, ["defaultAccount.cachedPolicyData"])["defaultAccount.cachedPolicyData"]?.accountPolicyData;
  await launch(account, path.join(account, "proj"));
  const first = policy();
  const startedAt = Date.now();
  await launch(account, path.join(account, "proj"));
  const endedAt = Date.now();
  const second = policy();
  const kept = (key) => typeof second?.[key] === "number" && second[key] === first?.[key] && second[key] < startedAt;

  await p.check("items.mcp.rules[0]", "of two launches in a row, the second fetches the account's entitlements again, `entitlementsFetchedAt`, and keeps the time of the token's, `tokenEntitlementsFetchedAt`, and of the MCP registry's, `mcpRegistryDataFetchedAt`, from the first: each part of the cached policy has a time of its own", async () =>
    same(
      {
        entitlements: typeof second?.entitlementsFetchedAt === "number" && second.entitlementsFetchedAt >= startedAt && second.entitlementsFetchedAt <= endedAt,
        token: kept("tokenEntitlementsFetchedAt"),
        mcp: second?.policyData?.mcp,
        registry: kept("mcpRegistryDataFetchedAt"),
      },
      { entitlements: true, token: true, mcp: true, registry: true },
    ),
  );
}

p.finish();
