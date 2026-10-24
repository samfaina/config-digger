// Tests VS Code's facts that `vscode.toml` marked unverified, on throwaway
// profiles and Projects. Nothing reaches the user's own VS Code: each launch
// gets its own `--user-data-dir`, `--extensions-dir`, `--shared-data-dir`
// and `--agent-plugins-dir`, and its own `USERPROFILE` and `APPDATA`, so
// that what VS Code reads in the home folder, such as `~\.vscode\argv.json`
// and Claude Desktop's config, is throwaway too. It runs beside a VS Code
// the user has open, and stops only the processes of its own launches.
//
//   node uat/agents/vscode.mjs [--bin <Code.exe>] [--sign-in]
//
// `CD_PROBE_ONLY=files,broken-dot` runs those launches only: `plugins`,
// `files`, `broken-<folder|dot|user|plugin|workspace>`, `discovery`,
// `experiments`, `trust`, `nonce`, `enablement`, `skills` and `policy`.
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
import { spawn } from "node:child_process";
import fs from "node:fs";
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
const version = json(path.join(path.dirname(bin), commit, "resources", "app", "product.json")).version;

const powershell = async (command) => (await run("powershell.exe", ["-NoProfile", "-Command", command])).stdout.trim();
// Stops what is left of a launch: the processes given its data folder only.
const stop = (data) =>
  powershell(`Get-CimInstance Win32_Process -Filter "Name='Code.exe'" | Where-Object { $_.CommandLine -like '*${data}*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }`);
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
  const running = async () => Number(await powershell(`@(Get-CimInstance Win32_Process -Filter "Name='Code.exe'" | Where-Object { $_.CommandLine -like '*${d.data}*' }).Count`));
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
const uriOf = (file) => ({ $mid: 1, path: pathToFileURL(file).pathname, scheme: "file" });

// The probe extension: it gives VS Code `provider`'s servers, starts every
// server VS Code loaded with the options `start`, writes `done.json` and
// quits VS Code. `done.json` also holds whether the window is trusted, the
// default value of each setting of `inspect`, and the skills of the
// proposed API `chatPromptFiles`: those VS Code lists to its model,
// `chat.getSkills`, and those the user can run, `chat.getSlashCommands`.
function extension(dir, { provider, start = { autoTrustChanges: true }, inspect = [] }) {
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
  write(path.join(dir, "probe.json"), { provider, start, inspect, done: path.join(dir, "done.json") });
  write(
    path.join(dir, "extension.js"),
    `const vscode = require("vscode");
const fs = require("fs");
const config = require("./probe.json");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const files = (list) => (list ?? []).map((s) => ({ name: s.name, path: s.uri.fsPath, description: s.description ?? null }));
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
    skills = files(await vscode.chat.getSkills(new vscode.CancellationTokenSource().token));
    commands = files(await vscode.chat.getSlashCommands(new vscode.CancellationTokenSource().token));
  } catch (e) {
    error = (error ? error + "; " : "") + String(e);
  }
  fs.writeFileSync(config.done, JSON.stringify({
    error,
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
async function launch(dir, open, { settings = {}, provider = null, start, inspect, env = {}, args: extra = [], trust = false, shared = true, seconds = 120 } = {}) {
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
  extension(d.probeExt, { provider, start, inspect });
  const doneFile = path.join(d.probeExt, "done.json");
  fs.rmSync(doneFile, { force: true });
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
    ...extra,
    "--new-window",
    open,
  ];
  spawn(bin, args, { detached: true, stdio: "ignore", env: { ...process.env, USERPROFILE: d.home, APPDATA: d.appData, ...env } }).unref();
  const end = Date.now() + seconds * 1000;
  while (!fs.existsSync(doneFile) && Date.now() < end) await sleep(1000);
  await sleep(3000);
  await stop(d.data);
  const done = fs.existsSync(doneFile) ? json(doneFile) : { timeout: true };
  const started = fs.existsSync(markers)
    ? Object.fromEntries(fs.readdirSync(markers).map((f) => [f.slice(0, -".json".length), json(path.join(markers, f))]))
    : {};
  return { done, started, tags: Object.keys(started).sort(), cache: extCache(d.data) };
}

// The `mcp.extCachedServers` of the workspace the launch opened.
function extCache(data) {
  const storage = path.join(data, "User", "workspaceStorage");
  if (!fs.existsSync(storage)) return null;
  for (const ws of fs.readdirSync(storage)) {
    const file = path.join(storage, ws, "state.vscdb");
    if (!fs.existsSync(file)) continue;
    const db = new DatabaseSync(file, { readOnly: true });
    const row = db.prepare("SELECT value FROM ItemTable WHERE key = 'mcp.extCachedServers'").get();
    db.close();
    if (row) return JSON.parse(row.value);
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
// Claude Desktop's config in a launch's `APPDATA`, with the server `desktop`.
const claudeDesktop = (d) => write(path.join(d.appData, "Claude", "claude_desktop_config.json"), dotMcp("desktop"));

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
  const [major, minor] = version.split(".").map(Number);
  const hasExternal = major > 1 || minor >= 135;
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
if (runs("broken-plugin")) {
  const seen = await brokenLaunch("plugin", ({ d }) => {
    const plugin = marketplace(d.plugins, [["plugin-p", { "plugin.json": manifest("plugin-p"), ".mcp.json": `{ "mcpServers": ${broken(["plugin-a", "plugin-b"])} }` }]]);
    return { file: path.join(plugin["plugin-p"], ".mcp.json") };
  });
  await p.check("locations[plugin-mcp]", "VS Code reads an agent plugin's `.mcp.json` it cannot parse around the error: both servers start", async () =>
    same(seen.tags, ["folder-ok", "plugin-a", "plugin-b", "user-ok"]),
  );
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

  await p.check("(top)", "VS Code takes a setting's ExP default from the `Parameters` of the config whose `Id` is `vscode`, as `config.<setting>`, wherever that config sits in `configs`", async () =>
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
  plant(path.join(d.home, ".vscode-shared", "sharedStorage", "state.vscdb"), { "content.trust.model.key": { uriTrustInfo: [{ uri: uriOf(trusted), trusted: true }] } });
  const opened = await launch(dir, trusted, { trust: true, shared: false });
  const elsewhere = await launch(dir, other, { trust: true, shared: false });

  await p.check("items.mcp.gates[0]", "without `--shared-data-dir`, VS Code reads the folders it trusts from `~\\.vscode-shared\\sharedStorage\\state.vscdb`: a folder trusted there opens trusted, another does not", async () =>
    same({ trusted: opened.done.trusted, other: elsewhere.done.trusted }, { trusted: true, other: false }),
  );
}

// ---------------------------------------------------------------- the trust of a discovered server

// A server of Claude Desktop's config, which VS Code trusts at the nonce of
// its launch configuration. It answers VS Code, which records the nonce of
// the configuration it last ran in the profile's `mcpToolCache`.
if (runs("nonce")) {
  const dir = fresh("nonce", base);
  const d = folders(dir);
  const proj = path.join(dir, "proj");
  fs.mkdirSync(proj, { recursive: true });
  const id = "claude-desktop.null.cd-throwaway-serve-desktop";
  const desktop = (...extra) =>
    write(path.join(d.appData, "Claude", "claude_desktop_config.json"), { mcpServers: { "cd-throwaway-serve-desktop": server("serve-desktop", { args: [script, "serve-desktop", ...extra] }) } });
  const record = () => rows(d.global, ["mcpToolCache"]).mcpToolCache?.serverTools?.find(([key]) => key === id)?.[1] ?? null;
  const matching = (r) => typeof r?.nonce === "string" && r.nonce === r.trustedAtNonce;
  const settings = { "chat.mcp.discovery.enabled": { "claude-desktop": true } };
  // Starting with `promptType: "never"` starts a trusted server only.
  const untrusting = { settings, start: { promptType: "never" } };
  desktop();
  const trusting = await launch(dir, proj, { settings });
  const trusted = record();
  const again = await launch(dir, proj, untrusting);
  desktop("changed");
  const before = record();
  const changed = await launch(dir, proj, untrusting);
  const after = record();

  await p.check("items.mcp.gates[1]", "once trusted, a discovered server starts without asking while its file is unchanged, its record's `trustedAtNonce` and `nonce` matching", async () =>
    same(
      { trusted: trusting.tags, matching: matching(trusted), again: again.tags },
      { trusted: ["serve-desktop"], matching: true, again: ["serve-desktop"] },
    ),
  );
  await p.check("items.mcp.gates[1]", "once its file changes, VS Code no longer starts it without asking, though its record's two nonces still match, before that launch and after it", async () =>
    same({ started: changed.tags, before: matching(before), after: matching(after) }, { started: [], before: true, after: true }),
  );
}

// ---------------------------------------------------------------- servers turned off in a store

// Each location's server `serve-<location>`, which a pair of
// `mcp.enablement` turns off, in the workspace's store for a Project's
// file and in the profile's for the others, and `serve-on`, which none
// names. `workbench.mcp.startServer` starts a server that is off all the
// same, but VS Code gives the model no tool of it: the oracle is the tools
// `lm.tools` lists, each server's named after its tag.
if (runs("enablement")) {
  const dir = fresh("enablement", base);
  const d = folders(dir);
  const proj = path.join(dir, "proj");
  const serving = (tag) => ({ mcpServers: { [`cd-throwaway-${tag}`]: server(tag) } });
  write(path.join(proj, ".vscode", "mcp.json"), { servers: { "cd-throwaway-serve-folder": server("serve-folder"), "cd-throwaway-serve-on": server("serve-on") } });
  write(path.join(proj, ".mcp.json"), serving("serve-dot"));
  write(path.join(d.user, "mcp.json"), { servers: { "cd-throwaway-serve-user": server("serve-user") } });
  write(path.join(d.appData, "Claude", "claude_desktop_config.json"), serving("serve-desktop"));
  const plugin = marketplace(d.plugins, [["serve-plugin", { "plugin.json": manifest("serve-plugin"), ".mcp.json": serving("serve-plugin") }]])["serve-plugin"];
  const options = {
    settings: { "chat.mcp.discovery.enabled": { "claude-desktop": true } },
    provider: { command: process.execPath, args: [script, "serve-extension"], env: {}, uri: "http://127.0.0.1:9/mcp", headers: {} },
  };
  const tools = (seen) => ["desktop", "dot", "extension", "folder", "on", "plugin", "user"].filter((tag) => seen.done.tools?.some((t) => t.endsWith(`serve_${tag}`)));
  // The first launch makes the workspace's store.
  const first = await launch(dir, proj, options);
  const workspaceStore = path.join(d.user, "workspaceStorage", workspaceFolder(d, proj), "state.vscdb");
  // The ids the definition's `switch_id`s make.
  const ids = {
    folder: "mcp.config.ws0.cd-throwaway-serve-folder",
    dot: "workspace-dot-mcp.0.cd-throwaway-serve-dot",
    user: "mcp.config.usrlocal.cd-throwaway-serve-user",
    desktop: "claude-desktop.null.cd-throwaway-serve-desktop",
    plugin: `plugin.${vscodeUri(plugin)}.cd-throwaway-serve-plugin`,
    extension: "cd-throwaway.probe/cd-throwaway-ext-stdio",
  };
  plant(workspaceStore, { "mcp.enablement": [[ids.folder, false], [ids.dot, false]] });
  plant(d.global, { "mcp.enablement": [ids.user, ids.desktop, ids.plugin, ids.extension].map((key) => [key, false]) });
  const second = await launch(dir, proj, options);

  await p.check("items.mcp.disable[0]", "VS Code reads `mcp.enablement` in a workspace's store and the profile's as the JSON text of `[id, enabled]` pairs, each id the server's collection followed by its name, as each location's `switch_id` makes it, and gives the model no tool of a server a `false` names", async () =>
    same(
      { first: tools(first), second: tools(second) },
      { first: ["desktop", "dot", "extension", "folder", "on", "plugin", "user"], second: ["on"] },
    ),
  );
}

// The folder of `d`'s `workspaceStorage` that holds the store of the
// workspace folder `folder`.
function workspaceFolder(d, folder) {
  const storage = path.join(d.user, "workspaceStorage");
  const uri = fileUri(folder).toLowerCase();
  return fs.readdirSync(storage).find((ws) => {
    const file = path.join(storage, ws, "workspace.json");
    return fs.existsSync(file) && decodeURIComponent(json(file).folder ?? "").toLowerCase() === decodeURIComponent(uri);
  });
}

// ---------------------------------------------------------------- skills

// The oracle is the proposed API `chatPromptFiles`: `chat.getSkills` is
// the list of skills VS Code gives its model, before it drops those
// without a `description` or with `disable-model-invocation`, and
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
  skill(at(".agents", "cd-throwaway-off"), ["description: probe"]);
  skill(at(".agents", "cd-throwaway-off-folder"), ["description: probe"]);
  plant(d.global, {
    "chat.disabledPromptFiles.skill": [uriOf(path.join(at(".agents", "cd-throwaway-off"), "SKILL.md")), uriOf(at(".agents", "cd-throwaway-off-folder"))],
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
  await p.check("items.skill", "VS Code loads a `SKILL.md` with no frontmatter under its folder's name, with no description, so that its model does not get it while the user can run it", async () =>
    same(
      { skill: named(seen.done.skills, "cd-throwaway-bare").map((s) => s.description), command: named(seen.done.commands, "cd-throwaway-bare").length },
      { skill: [null], command: 1 },
    ),
  );
  await p.check("items.skill", "VS Code loads a `SKILL.md` whose frontmatter is not YAML, or never closes, under its folder's name, keeping the values it can read", async () => {
    const broken = ["broken", "broken-field", "broken-line", "broken-colon", "unclosed"];
    return same(
      Object.fromEntries(broken.map((tag) => [tag, named(seen.done.skills, `cd-throwaway-${tag}`).map((s) => s.description)])),
      { broken: [null], "broken-field": ["probe"], "broken-line": ["probe"], "broken-colon": ["a: b"], unclosed: ["probe"] },
    );
  });
  await p.check("items.skill.disable[0]", "`chat.disabledPromptFiles.skill` in the profile's store names a skill by its `SKILL.md`: the user can no longer run it, while a skill named by its folder stays", async () =>
    same(
      { off: named(seen.done.commands, "cd-throwaway-off").length, folder: named(seen.done.commands, "cd-throwaway-off-folder").length },
      { off: 0, folder: 1 },
    ),
  );
  await p.check("items.skill.disable[0]", "a skill turned off there still reaches the model's list of skills", async () =>
    same(named(seen.done.skills, "cd-throwaway-off").length, 1),
  );
}

// ---------------------------------------------------------------- the account's policy

// VS Code fetches the account's entitlements at each start, then the
// Copilot token's, which bring `policyData.mcp`, and, while `mcp` is not
// `false`, the MCP registry's, which bring `policyData.mcpAccess`, each of
// the last two only once its last fetch is an hour old [binary]. Two
// launches in a row tell the times apart.
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

  await p.check("items.mcp.rules[0]", "VS Code caches each part of the account's policy with the time of the request that brought it: `entitlementsFetchedAt`, fetched again at each start, and `tokenEntitlementsFetchedAt`, which brings `mcp`, and `mcpRegistryDataFetchedAt`, which brings `mcpAccess`, both kept from an earlier launch", async () =>
    same(
      {
        entitlements: typeof second?.entitlementsFetchedAt === "number" && second.entitlementsFetchedAt >= startedAt && second.entitlementsFetchedAt <= endedAt,
        token: kept("tokenEntitlementsFetchedAt"),
        mcp: typeof second?.policyData?.mcp,
        registry: second?.policyData?.mcp === false || kept("mcpRegistryDataFetchedAt"),
      },
      { entitlements: true, token: true, mcp: "boolean", registry: true },
    ),
  );
}

p.finish();
