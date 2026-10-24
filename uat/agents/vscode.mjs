// Tests VS Code's facts that `vscode.toml` marked unverified, on throwaway
// profiles and Projects. Nothing reaches the user's own VS Code: each launch
// gets its own `--user-data-dir`, `--extensions-dir`, `--shared-data-dir`
// and `--agent-plugins-dir`, and its own `USERPROFILE` and `APPDATA`, so
// that what VS Code reads in the home folder, such as `~\.vscode\argv.json`
// and Claude Desktop's config, is throwaway too. It runs beside a VS Code
// the user has open, and stops only the processes of its own launches.
//
//   node uat/agents/vscode.mjs [--bin <Code.exe>]
//
// `CD_PROBE_ONLY=files,broken-dot` runs those launches only: `plugins`,
// `files`, `broken-<folder|dot|user|plugin|workspace>` and `discovery`.
//
// VS Code starts an MCP server only when asked. A probe extension, loaded
// with `--extensionDevelopmentPath`, asks it to start every server it
// loaded, `workbench.mcp.startServer *`, trusting the servers of other apps
// it discovers, then quits VS Code. Each server is this machine's Node
// running `server.mjs <tag>`, which writes a marker named after its tag,
// with the arguments and the `CD_PROBE_*` variables it got. The extension
// also gives VS Code servers of its own, which VS Code caches in the
// workspace's `state.vscdb`.
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { fresh, json, probe, run, same, write } from "./lib.mjs";

// `node:sqlite` warns that it is experimental on every run.
process.removeAllListeners("warning");
const { DatabaseSync } = await import("node:sqlite");

const bin = process.argv.includes("--bin")
  ? process.argv[process.argv.indexOf("--bin") + 1]
  : path.join(process.env.LOCALAPPDATA, "Programs", "Microsoft VS Code", "Code.exe");
// `product.json` sits in the folder of the commit `Code.exe` runs.
const commit = fs.readdirSync(path.dirname(bin)).find((f) => fs.existsSync(path.join(path.dirname(bin), f, "resources", "app", "product.json")));
const version = json(path.join(path.dirname(bin), commit, "resources", "app", "product.json")).version;
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
`,
);
const markers = path.join(base, "markers");
// A server that leaves the marker `tag` when VS Code starts it.
const server = (tag, extra = {}) => ({ command: process.execPath, args: [script, tag], ...extra });
// Servers `cd-throwaway-<tag>`, one per tag, with `,,` between them, which
// no JSON parser accepts.
const broken = (tags) => `{ ${tags.map((t) => `"cd-throwaway-${t}": ${JSON.stringify(server(t))}`).join(",, ")} }`;
const fileUri = (file) => pathToFileURL(file).href;

// The probe extension: it gives VS Code `provider`'s servers, starts every
// server VS Code loaded, writes `done.json` and quits VS Code.
function extension(dir, provider) {
  write(path.join(dir, "package.json"), {
    name: "probe",
    publisher: "cd-throwaway",
    version: "0.0.1",
    engines: { vscode: "^1.100.0" },
    main: "extension.js",
    activationEvents: ["onStartupFinished"],
    contributes: { mcpServerDefinitionProviders: [{ id: "cd-throwaway-provider", label: "cd-throwaway-provider" }] },
  });
  write(path.join(dir, "probe.json"), { provider, done: path.join(dir, "done.json") });
  write(
    path.join(dir, "extension.js"),
    `const vscode = require("vscode");
const fs = require("fs");
const config = require("./probe.json");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
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
    await Promise.race([vscode.commands.executeCommand("workbench.mcp.startServer", "*", { autoTrustChanges: true }), sleep(20000)]);
  } catch (e) {
    error = String(e);
  }
  await sleep(4000);
  fs.writeFileSync(config.done, JSON.stringify({
    error,
    folders: (vscode.workspace.workspaceFolders ?? []).map((f) => f.uri.fsPath),
    workspaceFile: vscode.workspace.workspaceFile ? vscode.workspace.workspaceFile.fsPath : null,
  }));
  await vscode.commands.executeCommand("workbench.action.quit");
};
`,
  );
}

const powershell = async (command) => (await run("powershell.exe", ["-NoProfile", "-Command", command])).stdout.trim();
// Stops what is left of a launch: the processes given its data folder only.
const stop = (data) =>
  powershell(`Get-CimInstance Win32_Process -Filter "Name='Code.exe'" | Where-Object { $_.CommandLine -like '*${data}*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }`);

// The folders of a launch in `dir`.
const folders = (dir) => ({
  data: path.join(dir, "data"),
  user: path.join(dir, "data", "User"),
  home: path.join(dir, "home"),
  appData: path.join(dir, "home", "AppData", "Roaming"),
  plugins: path.join(dir, "home", ".vscode", "agent-plugins"),
  probeExt: path.join(dir, "probe-ext"),
});

// Launches VS Code on `open`, a folder or a `.code-workspace` file, with the
// user's `settings`, and resolves once the probe extension is done: the
// servers VS Code started, by tag, what the extension saw, and the
// workspace's `mcp.extCachedServers`.
async function launch(dir, open, { settings = {}, provider = null, env = {}, seconds = 120 } = {}) {
  const d = folders(dir);
  write(path.join(d.user, "settings.json"), {
    "security.workspace.trust.enabled": false,
    "telemetry.telemetryLevel": "off",
    "update.mode": "none",
    "extensions.autoUpdate": false,
    "extensions.autoCheckUpdates": false,
    "workbench.startupEditor": "none",
    "chat.mcp.autostart": "never",
    ...settings,
  });
  extension(d.probeExt, provider);
  // 1.129.1 starts a profile's servers in the home folder, and fails to
  // when it does not exist.
  fs.mkdirSync(d.home, { recursive: true });
  fs.rmSync(markers, { recursive: true, force: true });
  const args = [
    `--user-data-dir=${d.data}`,
    `--extensions-dir=${path.join(dir, "ext")}`,
    `--shared-data-dir=${path.join(dir, "shared")}`,
    `--agent-plugins-dir=${d.plugins}`,
    `--extensionDevelopmentPath=${d.probeExt}`,
    "--disable-workspace-trust",
    "--skip-welcome",
    "--skip-release-notes",
    "--log=trace",
    "--new-window",
    open,
  ];
  spawn(bin, args, { detached: true, stdio: "ignore", env: { ...process.env, USERPROFILE: d.home, APPDATA: d.appData, ...env } }).unref();
  const doneFile = path.join(d.probeExt, "done.json");
  const end = Date.now() + seconds * 1000;
  while (!fs.existsSync(doneFile) && Date.now() < end) await new Promise((r) => setTimeout(r, 1000));
  await new Promise((r) => setTimeout(r, 3000));
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

p.finish();
