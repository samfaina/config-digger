// Tests Cursor's facts that `cursor.toml` marked unverified, on throwaway
// profiles and Projects. Nothing reaches the user's own Cursor: each launch
// gets its own `--user-data-dir` and `--extensions-dir`, and its own
// `USERPROFILE`, `APPDATA` and `LOCALAPPDATA`, so that `~\.cursor` and
// `~\.claude`, which Cursor reads in the home folder, are throwaway too. It
// runs beside a Cursor the user has open, and stops only the processes of
// its own launches.
//
//   node uat/agents/cursor.mjs [--bin <Cursor.exe>] [--sign-in]
//
// `CD_PROBE_ONLY=plugins,broken-user` runs those launches only: `plugins`,
// `broken-<user|project>`, `project-plugins`, `sign-in`, `gates`, `skills`
// and `nested-skills`.
//
// Cursor takes a Statsig gate from the account's cache, or signed out from
// its code's default, unless an override in the profile's
// `workbench.experiments.featureFlagOverrides` says otherwise, which it
// reads only when it runs an extension under development. The `gates`
// launches that turn gates off write such overrides, and load an empty
// extension with `--extensionDevelopmentPath`.
//
// Cursor starts the servers of `~\.cursor\mcp.json` and of the plugins
// without asking, and a Project's once approved: the probe approves them in
// the profile's `state.vscdb` before the launch. Each server is this
// machine's Node running `server.mjs <tag>`, which writes a marker named
// after its tag. Only the `project-plugins` checks run signed in to a
// Cursor account, on the profile `--sign-in` opens for the owner:
// `cursor-account` in the probe root, or the folder
// `CD_PROBE_CURSOR_ACCOUNT` names there, whose name starts the same.
//
// The sign-in checks run a local MCP server behind OAuth, with dynamic
// client registration, which takes any request and grants the token
// `not-a-real-secret`. The probe drives the window over the Chrome DevTools
// Protocol, `--remote-debugging-port`, and clicks the server's Authenticate
// button in Customize; `workbench.externalBrowser` hands the authorization
// URL to `curl.exe`, so no browser opens, and the server answers it by
// calling Cursor's loopback redirect.
import { spawn } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { fresh, gitInit, json, probe, root, run, same, skill, write } from "./lib.mjs";

// `node:sqlite` warns that it is experimental on every run.
process.removeAllListeners("warning");
const { DatabaseSync } = await import("node:sqlite");

const bin = process.argv.includes("--bin")
  ? process.argv[process.argv.indexOf("--bin") + 1]
  : path.join(process.env.LOCALAPPDATA, "Programs", "cursor", "Cursor.exe");
const version = json(path.join(path.dirname(bin), "resources", "app", "product.json")).version;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const powershell = async (command) => (await run("powershell.exe", ["-NoProfile", "-Command", command])).stdout.trim();
// PowerShell's processes given the data folder `data`.
const processesOf = (data) => `Get-CimInstance Win32_Process -Filter "Name='Cursor.exe'" | Where-Object { $_.CommandLine -like '*${data}*' }`;
// The processes given the data folder `data`, how many.
const running = async (data) => Number(await powershell(`@(${processesOf(data)}).Count`));
// Stops what is left of a launch: the processes given its data folder only.
// Resolves once they are gone and their stores closed, or after 30 s.
async function stop(data) {
  await powershell(`${processesOf(data)} | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }`);
  const end = Date.now() + 30_000;
  while ((await running(data)) && Date.now() < end) await sleep(1000);
  await sleep(2000);
}

// The folders of a launch in `dir`, its home folder `home` unless said.
const folders = (dir, home = path.join(dir, "home")) => ({
  data: path.join(dir, "data"),
  user: path.join(dir, "data", "User"),
  home,
  cursor: path.join(home, ".cursor"),
  claude: path.join(home, ".claude"),
  proj: path.join(dir, "proj"),
});
// The profile's store, the application's state, in the folders `d`.
const globalState = (d) => path.join(d.user, "globalStorage", "state.vscdb");

// What a launch with the folders `d` runs with: its own home folder, so
// that `~\.cursor` and `~\.claude` are its own, and its own app data.
const env = (d) => {
  const appData = path.join(d.home, "AppData");
  return { ...process.env, USERPROFILE: d.home, APPDATA: path.join(appData, "Roaming"), LOCALAPPDATA: path.join(appData, "Local") };
};

// The profile signed in to a Cursor account, a folder named
// `cursor-account*` in the probe root: `--sign-in` opens it for the owner.
const account = path.resolve(process.env.CD_PROBE_CURSOR_ACCOUNT ?? path.join(root, "cursor-account"));
if (path.dirname(account) !== root || !path.basename(account).startsWith("cursor-account")) {
  throw new Error(`CD_PROBE_CURSOR_ACCOUNT must be a folder named cursor-account* in ${root}, not ${account}`);
}
// Whether `dir`'s profile is signed in to a Cursor account.
function accountSignedIn(dir) {
  const file = globalState(folders(dir));
  if (!fs.existsSync(file)) return false;
  const db = new DatabaseSync(file, { readOnly: true });
  const token = db.prepare("SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken'").get();
  db.close();
  return Boolean(token);
}

// Cursor opens its sign-in page in the default browser, which gets the
// owner's own app data here, and so their own browser profile: a browser
// given the launch's would start a new profile in it.
if (process.argv.includes("--sign-in")) {
  const d = folders(account);
  write(path.join(d.user, "settings.json"), { "telemetry.telemetryLevel": "off", "update.mode": "none", "workbench.startupEditor": "none" });
  fs.mkdirSync(d.proj, { recursive: true });
  spawn(bin, [`--user-data-dir=${d.data}`, `--extensions-dir=${path.join(account, "ext")}`, "--new-window", d.proj], {
    detached: true,
    stdio: "ignore",
    env: { ...process.env, USERPROFILE: d.home },
  }).unref();
  console.log(`Sign in to Cursor in the window that opened, on the profile in ${account}, then close the window.`);
  while (!(await running(d.data))) await sleep(1000);
  while (await running(d.data)) await sleep(2000);
  const done = accountSignedIn(account);
  console.log(done ? "The profile is signed in." : "The profile is not signed in.");
  process.exit(done ? 0 : 1);
}

const p = probe("Cursor", version);
const base = fresh("cursor");
const only = process.env.CD_PROBE_ONLY?.split(",");
const runs = (name) => !only || only.includes(name);

const script = path.join(base, "server.mjs");
write(
  script,
  `import fs from "node:fs";
import path from "node:path";
const [tag] = process.argv.slice(2);
fs.mkdirSync(path.join(import.meta.dirname, "markers"), { recursive: true });
fs.writeFileSync(path.join(import.meta.dirname, "markers", tag + ".json"), "{}");
`,
);
const markers = path.join(base, "markers");
// A server that leaves the marker `tag` when Cursor starts it.
const server = (tag) => ({ command: process.execPath, args: [script, tag] });
// `{ "mcpServers": … }` with a server `cd-throwaway-<tag>` per tag.
const servers = (...tags) => ({ mcpServers: Object.fromEntries(tags.map((t) => [`cd-throwaway-${t}`, server(t)])) });
// The same, with `,,` between the servers, which no JSON parser accepts.
const broken = (...tags) => `{ "mcpServers": { ${tags.map((t) => `"cd-throwaway-${t}": ${JSON.stringify(server(t))}`).join(",, ")} } }`;

const freePort = () =>
  new Promise((resolve) => {
    const s = net.createServer().listen(0, "127.0.0.1", () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });

// The text of a log Cursor writes for the window, such as
// `workbench.mcp.files.log`, across the launches of `data`.
function windowLog(data, name) {
  const logs = path.join(data, "logs");
  if (!fs.existsSync(logs)) return "";
  return fs
    .readdirSync(logs)
    .map((session) => path.join(logs, session, "window1", name))
    .filter((f) => fs.existsSync(f))
    .map((f) => fs.readFileSync(f, "utf8"))
    .join("");
}

// The store Cursor keeps for the Project folder of `d`, if it has one.
function workspaceState(d) {
  const storage = path.join(d.user, "workspaceStorage");
  if (!fs.existsSync(storage)) return undefined;
  const uri = `file:///${d.proj.replace(/\\/g, "/").replace(/^([A-Za-z]):/, (_, drive) => `${drive.toLowerCase()}%3A`)}`;
  const id = fs.readdirSync(storage).find((w) => fs.existsSync(path.join(storage, w, "workspace.json")) && json(path.join(storage, w, "workspace.json")).folder === uri);
  return id && path.join(storage, id, "state.vscdb");
}
// The value of `key` in the store `file`, parsed, or null.
function stored(file, key) {
  if (!file || !fs.existsSync(file)) return null;
  const db = new DatabaseSync(file, { readOnly: true });
  const row = db.prepare("SELECT value FROM ItemTable WHERE key = ?").get(key);
  db.close();
  return row ? JSON.parse(String(row.value)) : null;
}
// Sets `key` to `value`, as JSON, in the store `file`.
function store(file, key, value) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const db = new DatabaseSync(file);
  db.exec("CREATE TABLE IF NOT EXISTS ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB)");
  db.prepare("INSERT INTO ItemTable VALUES (?, ?)").run(key, JSON.stringify(value));
  db.close();
}

// An extension with nothing in it: running it under development lets
// Cursor read the overrides of its gates.
const devExtension = path.join(base, "devext");
write(path.join(devExtension, "package.json"), { name: "cd-throwaway-devext", publisher: "cd-throwaway", version: "0.0.1", engines: { vscode: "*" } });

// Launches Cursor on the Project folder of `dir`, approving the Project
// servers `approved` names by identifier, with each gate of `gates`, a
// name and a value, overridden, and with its home folder `home` if given,
// and resolves once it has loaded its servers and `during`, given the
// window, is done: the servers it started, by tag. The profile is left in
// `dir`, so a launch can follow another on it.
async function launch(dir, { approved = [], gates, home, during } = {}) {
  const d = folders(dir, home);
  write(path.join(d.user, "settings.json"), {
    "security.workspace.trust.enabled": false,
    "telemetry.telemetryLevel": "off",
    "update.mode": "none",
    "extensions.autoUpdate": false,
    "workbench.startupEditor": "none",
    "workbench.externalBrowser": path.join(process.env.SystemRoot, "System32", "curl.exe"),
  });
  if (approved.length) store(globalState(d), "cursor/approvedProjectMcpServers", approved);
  if (gates) {
    const overrides = Object.fromEntries(Object.entries(gates).map(([gate, value]) => [gate, { value, expiresAt: null }]));
    store(globalState(d), "workbench.experiments.featureFlagOverrides", overrides);
  }
  fs.mkdirSync(d.proj, { recursive: true });
  fs.rmSync(markers, { recursive: true, force: true });
  const port = await freePort();
  const sessions = fs.existsSync(path.join(d.data, "logs")) ? fs.readdirSync(path.join(d.data, "logs")).length : 0;
  const args = [`--user-data-dir=${d.data}`, `--extensions-dir=${path.join(dir, "ext")}`, `--remote-debugging-port=${port}`, "--log=trace", "--new-window"];
  if (gates) args.push(`--extensionDevelopmentPath=${devExtension}`);
  args.push(d.proj);
  spawn(bin, args, { detached: true, stdio: "ignore", env: env(d) }).unref();
  // Cursor logs a census of the servers once it has read their files.
  const census = () => (windowLog(d.data, "workbench.mcp.files.log").match(/Plugin MCP census/g) ?? []).length;
  const end = Date.now() + 90_000;
  const before = sessions ? census() : 0;
  while (census() <= before && Date.now() < end) await sleep(1000);
  await sleep(15_000);
  try {
    if (during) await during(await devtools(port));
  } finally {
    await stop(d.data);
  }
  return fs.existsSync(markers) ? fs.readdirSync(markers).map((f) => f.slice(0, -".json".length)).sort() : [];
}

// The window of a launch, over the Chrome DevTools Protocol.
async function devtools(port) {
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
  const evaluate = async (expression) => (await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true })).result?.result?.value;
  const key = async (name, code, keyCode, modifiers = 0) => {
    for (const type of ["keyDown", "keyUp"]) await send("Input.dispatchKeyEvent", { type, key: name, code, windowsVirtualKeyCode: keyCode, modifiers });
  };
  // Opens Customize on its tab `tab`, such as `mcps` or `skills`.
  const customize = async (tab) => {
    await evaluate(`(() => {
      const open = [...document.querySelectorAll(".tab")].find((t) => t.getAttribute("aria-label") === "Customize");
      const button = [...document.querySelectorAll("*")].find((e) => e.offsetParent !== null && [...e.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim() === "Customize"));
      (open ?? button)?.click();
    })()`);
    await sleep(2000);
    await evaluate(`document.querySelector("[data-customize-manage-tab-id=${tab}]")?.click()`);
    await sleep(2500);
  };
  // Types `text` into the agent's prompt, cleared first.
  const type = async (text) => {
    await evaluate(`[...document.querySelectorAll(".aislash-editor-input")].find((e) => e.offsetParent !== null)?.focus()`);
    await key("a", "KeyA", 65, 2);
    await key("Backspace", "Backspace", 8);
    for (const ch of text) {
      await send("Input.insertText", { text: ch });
      await sleep(80);
    }
    await sleep(2000);
  };
  // The skills of the `/` menu the prompt shows, each `/<name>` and its
  // description.
  const menu = () =>
    evaluate(`[...document.querySelectorAll("[aria-label='Slash commands'] [role=menuitem]")].filter((e) => e.offsetParent !== null)
      .map((e) => e.innerText.split("\\n").map((s) => s.trim()).filter(Boolean)).filter(([name]) => name.startsWith("/"))`);
  return {
    // Clicks the visible element whose own text or `aria-label` is `label`.
    click: (label) =>
      evaluate(`(() => {
        const el = [...document.querySelectorAll("*")].find((e) => e.offsetParent !== null
          && ([...e.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim() === ${JSON.stringify(label)}) || e.getAttribute("aria-label") === ${JSON.stringify(label)}));
        el?.click();
        return Boolean(el);
      })()`),
    // Clicks the Authenticate button of the server `name` in Customize's
    // list of MCP servers.
    authenticate: (name) =>
      evaluate(`(() => {
        const button = [...document.querySelectorAll("button[data-mcp-connect]")].find((b) => {
          for (let e = b; e; e = e.parentElement) if (e.textContent.includes(${JSON.stringify(name)})) return e.querySelectorAll("button[data-mcp-connect]").length === 1;
        });
        button?.click();
        return Boolean(button);
      })()`),
    // The servers Customize lists, by name.
    async mcpServers() {
      await customize("mcps");
      return evaluate(`[...document.querySelectorAll("[data-mcp-manage-server]")].map((e) => e.getAttribute("data-mcp-manage-server")).sort()`);
    },
    // The sources of the server `name` in Customize, each with whether
    // Cursor runs the server from it.
    async sources(name) {
      await customize("mcps");
      await evaluate(`document.querySelector(${JSON.stringify(`[aria-label="Configure ${name}"]`)})?.click()`);
      await sleep(2000);
      const sources = await evaluate(`Object.fromEntries([...document.querySelectorAll("[role=switch]")].filter((e) => e.offsetParent !== null)
        .map((e) => [(e.getAttribute("aria-label") ?? "").match(/^(?:Disable|Enable) .+ for (.+) source$/)?.[1], e.getAttribute("aria-checked") === "true"])
        .filter(([source]) => source))`);
      await evaluate(`[...document.querySelectorAll("button")].find((e) => e.offsetParent !== null && e.innerText.trim() === "Done")?.click()`);
      await sleep(1500);
      return sources;
    },
    // Turns the server `name` off with its toggle in Customize, and
    // resolves with the toggle's label once it says Enable.
    async toggleOff(name) {
      await customize("mcps");
      await evaluate(`document.querySelector(${JSON.stringify(`[aria-label="Configure ${name}"]`)})?.click()`);
      await sleep(2000);
      const label = await evaluate(`(async () => {
        const toggle = [...document.querySelectorAll("[role=switch]")].find((e) => e.offsetParent !== null && (e.getAttribute("aria-label") ?? "").startsWith(${JSON.stringify(`Disable ${name} `)}));
        toggle?.click();
        await new Promise((r) => setTimeout(r, 2000));
        return toggle?.getAttribute("aria-label");
      })()`);
      await sleep(3000);
      return label;
    },
    // Customize's skills, each its folder's name and the line below it, its
    // description or an error, sorted, with every group shown whole.
    async skills() {
      await customize("skills");
      await evaluate(`[...document.querySelectorAll("*")].filter((e) => e.offsetParent !== null && /^Show \\d+ more$/.test(e.innerText?.trim() ?? "") && e.children.length === 0).forEach((e) => e.click())`);
      await sleep(1500);
      return evaluate(`[...document.querySelectorAll("[data-customize-manage-kind=skill]")].filter((e) => e.offsetParent !== null)
        .map((e) => [e.getAttribute("data-customize-manage-row"), e.innerText.split("\\n").map((s) => s.trim()).filter(Boolean)[1] ?? ""]).sort()`);
    },
    // The skills the `/` menu lists for `query`.
    async slash(query) {
      await type(query);
      return menu();
    },
    // Picks from the `/` menu for `query` the `down`th entry below the
    // first, with Enter.
    async pick(query, down = 0) {
      await type(query);
      for (let i = 0; i < down; i++) await key("ArrowDown", "ArrowDown", 40);
      await key("Enter", "Enter", 13);
      await sleep(2000);
    },
  };
}

// ---------------------------------------------------------------- plugins

// A Claude Code plugin per `[name, files]`, installed for the user and
// turned on in `~\.claude\settings.json`, as Claude Code records it.
function claudePlugins(claude, list) {
  const plugins = {};
  for (const [name, files] of list) {
    const folder = path.join(claude, "plugins", "cache", "cd-throwaway-market", name, "1.0.0");
    for (const [file, content] of Object.entries(files)) write(path.join(folder, file), content);
    plugins[`${name}@cd-throwaway-market`] = [{ scope: "user", installPath: folder, version: "1.0.0" }];
  }
  write(path.join(claude, "plugins", "installed_plugins.json"), { version: 2, plugins });
  write(path.join(claude, "settings.json"), { enabledPlugins: Object.fromEntries(Object.keys(plugins).map((id) => [id, true])) });
}
// A local plugin per `[name, files]`, a folder of `~\.cursor\plugins\local`.
function localPlugins(cursor, list) {
  for (const [name, files] of list) for (const [file, content] of Object.entries(files)) write(path.join(cursor, "plugins", "local", name, file), content);
}
// A manifest declaring `tags`' servers, with `,,` in it when `broken`.
const manifest = (name, tags, { broken = false } = {}) =>
  broken ? `{ "name": "${name}",, "mcpServers": ${JSON.stringify(servers(...tags).mcpServers)} }` : { name, mcpServers: servers(...tags).mcpServers };

if (runs("plugins")) {
  const dir = fresh("plugins", base);
  const d = folders(dir);
  localPlugins(d.cursor, [
    ["cd-throwaway-bad-manifest", { ".cursor-plugin/plugin.json": manifest("cd-throwaway-bad-manifest", ["bm-manifest"], { broken: true }), ".mcp.json": servers("bm-dot") }],
    // A manifest that does not parse, before one that does.
    ["cd-throwaway-next", { ".cursor-plugin/plugin.json": manifest("cd-throwaway-next", ["next-first"], { broken: true }), ".claude-plugin/plugin.json": manifest("cd-throwaway-next", ["next-second"]) }],
    ["cd-throwaway-bad-mcp", { "mcp.json": broken("bad-mcp-a", "bad-mcp-b") }],
  ]);
  claudePlugins(d.claude, [
    // The manifest and `.mcp.json` both declare `cd-throwaway-shared`.
    ["cd-throwaway-cc-both", {
      ".claude-plugin/plugin.json": { name: "cd-throwaway-cc-both", mcpServers: { "cd-throwaway-shared": server("shared-manifest"), ...servers("cc-manifest").mcpServers } },
      ".mcp.json": { mcpServers: { "cd-throwaway-shared": server("shared-dot"), ...servers("cc-dot").mcpServers } },
    }],
    ["cd-throwaway-cc-bad-manifest", { ".claude-plugin/plugin.json": manifest("cd-throwaway-cc-bad-manifest", ["ccbm-manifest"], { broken: true }), ".mcp.json": servers("ccbm-dot") }],
    ["cd-throwaway-cc-next", {
      ".cursor-plugin/plugin.json": manifest("cd-throwaway-cc-next", ["cc-next-first"], { broken: true }),
      ".claude-plugin/plugin.json": manifest("cd-throwaway-cc-next", ["cc-next-second"]),
    }],
    ["cd-throwaway-cc-bad-mcp", { ".claude-plugin/plugin.json": { name: "cd-throwaway-cc-bad-mcp" }, ".mcp.json": broken("cc-bad-mcp-a", "cc-bad-mcp-b") }],
  ]);
  const tags = await launch(dir);
  const of = (...want) => want.filter((t) => tags.includes(t));

  await p.check("locations[cursor-plugin-manifest-mcp]", "Cursor skips a plugin manifest it cannot parse, and still reads the plugin's `.mcp.json`", async () =>
    same(of("bm-manifest", "bm-dot"), ["bm-dot"]),
  );
  await p.check("locations[cursor-plugin-manifest-mcp]", "Cursor reads the next manifest of a plugin whose first one does not parse", async () =>
    same(of("next-first", "next-second"), ["next-second"]),
  );
  await p.check("locations[cursor-plugin-mcp]", "Cursor reads a plugin's `mcp.json` it cannot parse around the error: both servers start", async () =>
    same(of("bad-mcp-a", "bad-mcp-b"), ["bad-mcp-a", "bad-mcp-b"]),
  );
  await p.check("locations[claude-plugin-manifest-mcp]", "Cursor reads a Claude Code plugin's `.mcp.json` beside the servers its manifest declares, and of a server both declare, the manifest's starts", async () =>
    same(of("shared-manifest", "shared-dot", "cc-manifest", "cc-dot"), ["shared-manifest", "cc-manifest", "cc-dot"]),
  );
  await p.check("locations[claude-plugin-manifest-mcp]", "Cursor skips a Claude Code plugin's manifest it cannot parse, and still reads the plugin's `.mcp.json`", async () =>
    same(of("ccbm-manifest", "ccbm-dot"), ["ccbm-dot"]),
  );
  await p.check("locations[claude-plugin-manifest-mcp]", "Cursor reads the next manifest of a Claude Code plugin whose first one does not parse", async () =>
    same(of("cc-next-first", "cc-next-second"), ["cc-next-second"]),
  );
  await p.check("locations[claude-plugin-mcp]", "Cursor reads a Claude Code plugin's `.mcp.json` it cannot parse around the error: both servers start", async () =>
    same(of("cc-bad-mcp-a", "cc-bad-mcp-b"), ["cc-bad-mcp-a", "cc-bad-mcp-b"]),
  );
}

// ---------------------------------------------------------------- MCP files that do not parse

// Each launch breaks one MCP file and keeps a server in the other and in a
// local plugin: `user-ok` in `~\.cursor\mcp.json`, `project-ok` in the
// Project's `.cursor\mcp.json` and `plugin-ok`.
async function brokenLaunch(where, setup) {
  const dir = fresh(`broken-${where}`, base);
  const d = folders(dir);
  const user = path.join(d.cursor, "mcp.json");
  const project = path.join(d.proj, ".cursor", "mcp.json");
  write(user, servers("user-ok"));
  write(project, servers("project-ok"));
  localPlugins(d.cursor, [["cd-throwaway-ok", { ".mcp.json": servers("plugin-ok") }]]);
  const file = setup({ user, project });
  const text = fs.readFileSync(file, "utf8");
  // Approved, the servers of a Project file Cursor read around its error
  // would start too.
  const approved = ["ok", "a", "b"].map((t) => `project-0-proj-cd-throwaway-project-${t}`);
  const tags = await launch(dir, { approved });
  return { tags, untouched: fs.readFileSync(file, "utf8") === text };
}

if (runs("broken-user")) {
  const seen = await brokenLaunch("user", ({ user }) => {
    write(user, broken("user-a", "user-b"));
    return user;
  });
  await p.check("locations[user-mcp]", "Cursor skips a `~\\.cursor\\mcp.json` it cannot parse, loads the other files, and leaves the file as it is", async () =>
    same(seen, { tags: ["plugin-ok", "project-ok"], untouched: true }),
  );
}
if (runs("broken-project")) {
  const seen = await brokenLaunch("project", ({ project }) => {
    write(project, broken("project-a", "project-b"));
    return project;
  });
  await p.check("locations[project-mcp]", "Cursor skips a Project's `.cursor\\mcp.json` it cannot parse, loads the other files, and leaves the file as it is", async () =>
    same(seen, { tags: ["plugin-ok", "user-ok"], untouched: true }),
  );
}

// ---------------------------------------------------------------- a Project's plugins

// Cursor resolves the plugin keys of a Project's `.cursor\settings.json`
// with its server, which needs an account. The plugin is the Marketplace's
// `superdesign`, which holds skills only, so no server of its runs: the
// one real Entry of the probe, since the server resolves real keys only.
// The probe plants copies of it Cursor's cache could hold, each with a
// server that leaves a marker: signed out, a complete one in
// `cursor-public`; signed in, one in a marketplace whose name comes first,
// a complete commit whose name comes last and an incomplete one.
// A copy of `superdesign` at `commit` in `marketplace` of `cache`.
function decoy(cache, marketplace, commit, tag, { complete = true } = {}) {
  const folder = path.join(cache, marketplace, "superdesign", commit);
  write(path.join(folder, ".cursor-plugin", "plugin.json"), { name: "superdesign" });
  write(path.join(folder, "mcp.json"), servers(tag));
  if (complete) write(path.join(folder, ".cache-complete"), "");
}
const enablesSuperdesign = (d) => write(path.join(d.proj, ".cursor", "settings.json"), { plugins: { superdesign: { enabled: true } } });
const pluginLog = (d) => windowLog(d.data, path.join("exthost", "anysphere.cursor-agent-exec", "Cursor Plugins.log"));

if (runs("project-plugins")) {
  const dir = fresh("signed-out", base);
  const d = folders(dir);
  enablesSuperdesign(d);
  decoy(path.join(d.cursor, "plugins", "cache"), "cursor-public", "c".repeat(40), "signed-out");
  const tags = await launch(dir);
  await p.check("plugins[project]", "signed out, Cursor loads no plugin a Project's key names, even one complete in the cache", async () =>
    same({ tags, loaded: /superdesign/.test(pluginLog(d)) }, { tags: [], loaded: false }),
  );
}

if (runs("project-plugins") && !accountSignedIn(account)) {
  await p.check("plugins[project]", "the account profile is signed in", async () => ({ pass: false, observed: `${account} is not: run node uat/agents/cursor.mjs --sign-in` }));
} else if (runs("project-plugins")) {
  const d = folders(account);
  const cache = path.join(d.cursor, "plugins", "cache");
  fs.rmSync(cache, { recursive: true, force: true });
  fs.rmSync(d.proj, { recursive: true, force: true });
  enablesSuperdesign(d);
  decoy(cache, "aaa-cd-throwaway", "a".repeat(40), "first-marketplace");
  decoy(cache, "cursor-public", "f".repeat(40), "last-commit");
  decoy(cache, "cursor-public", "e".repeat(40), "incomplete-commit", { complete: false });
  const tags = await launch(account);
  // The commits of `superdesign` in each marketplace's cache, `*` marking
  // a complete one.
  const commits = Object.fromEntries(
    fs.readdirSync(cache).map((m) => {
      const plugin = path.join(cache, m, "superdesign");
      return [m, fs.readdirSync(plugin).map((c) => (fs.existsSync(path.join(plugin, c, ".cache-complete")) ? `${c}*` : c))];
    }),
  );
  const log = pluginLog(d);
  // The commit Cursor's server named, and the folders Cursor loaded.
  const named = log.match(/Adding enabled plugin: superdesign from ([0-9a-f]{40})/)?.[1];
  const loaded = [...new Set([...log.matchAll(/Found cached plugin: superdesign at (.+)/g)].map((m) => path.relative(cache, m[1].trim())))];

  await p.check("plugins[project]", "Cursor's server resolves a key that names no marketplace to `cursor-public`, and Cursor loads the plugin from that marketplace's cache, not from another marketplace's that holds a plugin of the same name and comes first", async () =>
    same(
      { tags, loaded, firstMarketplace: commits["aaa-cd-throwaway"] },
      { tags: [], loaded: [path.join("cursor-public", "superdesign", named ?? "?")], firstMarketplace: [`${"a".repeat(40)}*`] },
    ),
  );
  await p.check("plugins[project]", "Cursor downloads the commit its server names, and deletes the plugin's other commits, complete or not, a complete one that comes last included, as it loads it", async () =>
    same({ named: Boolean(named), commits: commits["cursor-public"] }, { named: true, commits: [`${named}*`] }),
  );
}

// ---------------------------------------------------------------- sign-in

// A local MCP server behind OAuth. It registers any client, grants the
// token `not-a-real-secret` for any code, and answers the authorization
// request itself by calling the redirect it names, as a user who signs in
// would. `phase` labels the requests that follow, and `authorized(phase)`
// says whether one of them carried the token.
async function oauthServer() {
  const requests = [];
  const s = {
    phase: "start",
    authorized: (phase) => requests.some((r) => r.phase === phase && r.bearer),
    asked: (phase) => requests.some((r) => r.phase === phase),
  };
  const srv = http.createServer((req, res) => {
    let body = "";
    req.on("data", (d) => (body += d)).on("end", () => {
      const u = new URL(req.url, s.url);
      const reply = (code, value, headers = {}) => {
        res.writeHead(code, { "content-type": "application/json", ...headers });
        res.end(JSON.stringify(value));
      };
      if (u.pathname.startsWith("/.well-known/oauth-protected-resource")) return reply(200, { resource: `${s.url}/mcp`, authorization_servers: [s.url] });
      if (u.pathname.startsWith("/.well-known/oauth-authorization-server")) {
        return reply(200, {
          issuer: s.url,
          authorization_endpoint: `${s.url}/authorize`,
          token_endpoint: `${s.url}/token`,
          registration_endpoint: `${s.url}/register`,
          response_types_supported: ["code"],
          code_challenge_methods_supported: ["S256"],
          grant_types_supported: ["authorization_code", "refresh_token"],
          token_endpoint_auth_methods_supported: ["none"],
        });
      }
      if (u.pathname === "/register") return reply(201, { ...JSON.parse(body || "{}"), client_id: "cd-throwaway-client" });
      if (u.pathname === "/authorize") {
        reply(200, {});
        const back = new URL(u.searchParams.get("redirect_uri"));
        back.searchParams.set("code", "cd-throwaway-code");
        back.searchParams.set("state", u.searchParams.get("state"));
        fetch(back).catch(() => {});
        return;
      }
      if (u.pathname === "/token") return reply(200, { access_token: "not-a-real-secret", token_type: "Bearer", expires_in: 3600, refresh_token: "not-a-real-secret" });
      if (u.pathname !== "/mcp") return reply(404, {});
      const bearer = req.headers.authorization === "Bearer not-a-real-secret";
      requests.push({ phase: s.phase, bearer });
      if (!bearer) return reply(401, {}, { "www-authenticate": `Bearer resource_metadata="${s.url}/.well-known/oauth-protected-resource"` });
      if (req.method !== "POST") return reply(405, {});
      const message = JSON.parse(body);
      if (message.id === undefined) return reply(202, {});
      const result =
        message.method === "initialize"
          ? { protocolVersion: message.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: "cd-throwaway-oauth", version: "1.0.0" } }
          : message.method === "tools/list"
            ? { tools: [{ name: "cd_throwaway_tool", description: "probe tool", inputSchema: { type: "object" } }] }
            : {};
      reply(200, { jsonrpc: "2.0", id: message.id, result });
    });
  });
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  s.url = `http://127.0.0.1:${srv.address().port}`;
  s.close = () => srv.close();
  return s;
}

// Signs in to the server `name` in the window: Customize lists it as
// needing authentication. Resolves once it connected with the token.
async function signIn(win, oauth, name) {
  oauth.phase = "signed-in";
  for (const label of ["Open Cursor Settings", "Open Customize", "MCPs"]) {
    await win.click(label);
    await sleep(3000);
  }
  await win.authenticate(name);
  const end = Date.now() + 30_000;
  while (!oauth.authorized("signed-in") && Date.now() < end) await sleep(1000);
  return oauth.authorized("signed-in");
}

// Writes `~\.cursor\mcp.json` while Cursor runs, then labels what follows.
async function change(oauth, file, content, phase) {
  write(file, content);
  oauth.phase = phase;
  await sleep(15_000);
}

if (runs("sign-in")) {
  const oauth = await oauthServer();
  const remote = () => ({ mcpServers: { "cd-throwaway-oauth": { url: `${oauth.url}/mcp` } } });
  // The same, with `,,` after the server.
  const brokenRemote = () => `{ "mcpServers": { "cd-throwaway-oauth": ${JSON.stringify({ url: `${oauth.url}/mcp` })},, "cd-throwaway-other": ${JSON.stringify(server("other"))} } }`;

  // Across restarts: signed in, then started again with the server, then
  // without it, then with it again.
  {
    const dir = fresh("sign-in-closed", base);
    const file = path.join(folders(dir).cursor, "mcp.json");
    write(file, remote());
    let signedIn = false;
    await launch(dir, { during: async (win) => (signedIn = await signIn(win, oauth, "cd-throwaway-oauth")) });
    oauth.phase = "restarted";
    await launch(dir);
    write(file, { mcpServers: {} });
    oauth.phase = "started-without";
    await launch(dir);
    write(file, remote());
    oauth.phase = "started-with-again";
    await launch(dir);
    write(file, brokenRemote());
    oauth.phase = "started-broken";
    await launch(dir);
    write(file, remote());
    oauth.phase = "started-repaired";
    await launch(dir);
    await p.check("locations[user-mcp]", "Cursor keeps the sign-in of a remote server across restarts, and of one that was gone from `mcp.json`, or in a `mcp.json` that did not parse, while it started", async () =>
      same(
        { signedIn, restarted: oauth.authorized("restarted"), again: oauth.authorized("started-with-again"), repaired: oauth.authorized("started-repaired") },
        { signedIn: true, restarted: true, again: true, repaired: true },
      ),
    );
  }

  // While Cursor runs: signed in, then the server taken out of `mcp.json`
  // and put back.
  {
    const dir = fresh("sign-in-removed", base);
    const file = path.join(folders(dir).cursor, "mcp.json");
    write(file, remote());
    let signedIn = false;
    await launch(dir, {
      during: async (win) => {
        signedIn = await signIn(win, oauth, "cd-throwaway-oauth");
        await change(oauth, file, { mcpServers: {} }, "removed");
        await change(oauth, file, remote(), "put-back");
      },
    });
    await p.check("locations[user-mcp]", "Cursor forgets the sign-in of a remote server taken out of `mcp.json` while it runs: put back, it needs signing in again", async () =>
      same({ signedIn, putBack: oauth.authorized("put-back"), asked: oauth.asked("put-back") }, { signedIn: true, putBack: false, asked: true }),
    );
  }

  // While Cursor runs: signed in, then `mcp.json` made unparsable, then
  // repaired.
  {
    const dir = fresh("sign-in-broken", base);
    const file = path.join(folders(dir).cursor, "mcp.json");
    write(file, remote());
    let signedIn = false;
    await launch(dir, {
      during: async (win) => {
        signedIn = await signIn(win, oauth, "cd-throwaway-oauth");
        await change(oauth, file, brokenRemote(), "broken");
        await change(oauth, file, remote(), "repaired");
      },
    });
    await p.check("locations[user-mcp]", "Cursor forgets the sign-in of the remote servers of a `mcp.json` that stops parsing while it runs: repaired, it needs signing in again", async () =>
      same({ signedIn, repaired: oauth.authorized("repaired"), asked: oauth.asked("repaired") }, { signedIn: true, repaired: false, asked: true }),
    );
  }
  oauth.close();
}

// ---------------------------------------------------------------- gates and the Disabled store

const gateNames = ["enable_cc_plugin_import", "dedupe_mcp_servers", "mcp_profile_scoped_global_disabled_state"];

// Two profiles, one with Cursor's gates as it takes them without an
// account, from its code, and one with the three overridden off. Each has a
// user's server and a Project's, a Claude Code plugin's server, and two
// remote servers at one URL on a closed local port, the user's and a local
// plugin's. The probe reads the servers Customize lists, turns the user's
// and the Project's servers off with their toggles, reads both stores, and
// starts Cursor again.
if (runs("gates")) {
  const url = { url: `http://127.0.0.1:${await freePort()}/mcp` };
  const project = "project-0-proj-cd-throwaway-project";
  const seen = {};
  for (const [label, gates] of [["default", undefined], ["off", Object.fromEntries(gateNames.map((g) => [g, false]))]]) {
    const dir = fresh(`gates-${label}`, base);
    const d = folders(dir);
    write(path.join(d.cursor, "mcp.json"), { mcpServers: { ...servers("user").mcpServers, "cd-throwaway-url-user": url } });
    write(path.join(d.proj, ".cursor", "mcp.json"), servers("project"));
    localPlugins(d.cursor, [["cd-throwaway-url", { ".mcp.json": { mcpServers: { "cd-throwaway-url-plugin": url } } }]]);
    claudePlugins(d.claude, [["cd-throwaway-cc", { ".claude-plugin/plugin.json": { name: "cd-throwaway-cc" }, ".mcp.json": servers("cc") }]]);
    const s = {};
    s.first = await launch(dir, {
      approved: [project],
      gates,
      during: async (win) => {
        s.remote = Object.fromEntries((await win.mcpServers()).filter((name) => /url/i.test(name)).map((name) => [name, null]));
        for (const name of Object.keys(s.remote)) s.remote[name] = await win.sources(name);
        s.toggles = [await win.toggleOff("cd-throwaway-user"), await win.toggleOff("cd-throwaway-project")];
      },
    });
    s.stores = { profile: stored(globalState(d), "cursor/disabledGlobalMcpServers"), workspace: stored(workspaceState(d), "cursor/disabledMcpServers") };
    s.again = await launch(dir, { approved: [project], gates });
    seen[label] = s;
  }
  const { default: on, off } = seen;
  const toggled = ["Enable cd-throwaway-user for User source", "Enable cd-throwaway-project for proj source"];

  await p.check("items.mcp.disable[0]", "Cursor's toggle of a Project's server writes its identifier to the workspace's `cursor/disabledMcpServers`, a JSON list, and Cursor no longer starts it there", async () =>
    same(
      [on, off].map((s) => ({ toggles: s.toggles, workspace: s.stores.workspace?.includes(project), started: s.first.includes("project"), again: s.again.includes("project") })),
      [on, off].map(() => ({ toggles: toggled, workspace: true, started: true, again: false })),
    ),
  );
  await p.check("feature_gates[mcp_profile_scoped_global_disabled_state]", "on, as Cursor takes it without an account, the toggle of a user's server writes its identifier to the profile's `cursor/disabledGlobalMcpServers`, not the workspace's list, and Cursor no longer starts it", async () =>
    same(
      { stores: on.stores, started: on.first.includes("user"), again: on.again.includes("user") },
      { stores: { profile: ["user-cd-throwaway-user"], workspace: [project] }, started: true, again: false },
    ),
  );
  await p.check("feature_gates[mcp_profile_scoped_global_disabled_state]", "off, the toggle of a user's server writes the workspace's `cursor/disabledMcpServers`, as a Project's does, and Cursor no longer starts it there", async () =>
    same(
      { stores: off.stores, started: off.first.includes("user"), again: off.again.includes("user") },
      { stores: { profile: null, workspace: ["user-cd-throwaway-user", project] }, started: true, again: false },
    ),
  );
  await p.check("feature_gates[enable_cc_plugin_import]", "Cursor starts a Claude Code plugin's servers while this gate is on, as Cursor takes it without an account, and not while it is off", async () =>
    same({ on: [on.first.includes("cc"), on.again.includes("cc")], off: [off.first.includes("cc"), off.again.includes("cc")] }, { on: [true, true], off: [false, false] }),
  );
  await p.check("feature_gates[dedupe_mcp_servers]", "while this gate is on, as Cursor takes it without an account, Cursor makes one server of two remote servers at one URL, the user's and a plugin's, from the user's source; off, it keeps both", async () =>
    same(
      { on: on.remote, off: off.remote },
      {
        on: { "Cd Throwaway URL": { User: true, Plugin: false } },
        off: { "cd-throwaway-url-plugin": { Plugin: true }, "cd-throwaway-url-user": { User: true } },
      },
    ),
  );

  // What config-digger reads when Cursor is signed in: the account's cache
  // of its gates, each under the djb2 hash of its name.
  const djb2 = (name) => String([...name].reduce((h, c) => (Math.imul(h, 31) + c.charCodeAt(0)) | 0, 0) >>> 0);
  await p.check("feature_gates[mcp_profile_scoped_global_disabled_state]", "the owner's account, in no team, has the three gates on in Cursor's cache", async () => {
    if (!accountSignedIn(account)) return { pass: false, observed: `${account} is not signed in: run node uat/agents/cursor.mjs --sign-in` };
    const cache = stored(globalState(folders(account)), "workbench.experiments.statsigBootstrap");
    return same(Object.fromEntries(gateNames.map((g) => [g, cache?.feature_gates?.[djb2(g)]?.value])), Object.fromEntries(gateNames.map((g) => [g, true])));
  });
}

// ---------------------------------------------------------------- skills

// One launch whose user's and Project's skill folders hold skills Cursor
// cannot parse, two skills of one name at different paths, and skills of a
// user and of the Project at the same path below their folders. The probe
// reads the `/` menu of the agent's prompt and Customize's list of skills,
// picks skills from the menu, and reads the skills Cursor records as picked
// in the profile's `cursor.skills.recentlyUsed`, each by its path below its
// `skills` folder, most recent first.
//
// Cursor's code takes a skill under `<drive>:\Users\<name>` for a user's,
// so the launch's home folder is `<drive>:\Users\cd-throwaway`, on a drive
// `subst` maps to a folder of the launch, which the probe takes off again.
//
// The letter of a drive mapped to a folder of the launch in `dir`, and the
// home folder on it.
async function usersDrive(dir) {
  const letter = [..."ZYXWVUTSRQPONMLKJIHG"].find((l) => !fs.existsSync(`${l}:\\`));
  const drive = path.join(dir, "drive");
  fs.mkdirSync(path.join(drive, "Users", "cd-throwaway"), { recursive: true });
  const mapped = await run("subst.exe", [`${letter}:`, drive]);
  if (mapped.code !== 0) throw new Error(`subst ${letter}: failed: ${mapped.stdout}${mapped.stderr}`);
  return { letter, home: `${letter}:\\Users\\cd-throwaway` };
}
const unmap = (letter) => run("subst.exe", [`${letter}:`, "/d"]);

if (runs("skills")) {
  const dir = fresh("skills", base);
  const { letter, home } = await usersDrive(dir);
  const d = folders(dir, home);
  const userSkills = path.join(d.cursor, "skills");
  const skillAt = (folder, text) => write(path.join(folder, "SKILL.md"), text);
  skill(path.join(userSkills, "cd-throwaway-ok"), ["name: cd-throwaway-ok", "description: parses"]);
  skillAt(path.join(userSkills, "cd-throwaway-bare"), "No frontmatter.\n");
  skill(path.join(userSkills, "cd-throwaway-yaml"), ["name: [unclosed", "description: YAML that does not parse"]);
  skillAt(path.join(userSkills, "cd-throwaway-unclosed"), "---\nname: cd-throwaway-unclosed\ndescription: never closes\nbody\n");
  skill(path.join(userSkills, "cd-throwaway-no-key"), ["name: cd-throwaway-no-key", "a line that is no key"]);
  skill(path.join(userSkills, "cd-throwaway-no-description"), ["name: cd-throwaway-no-description"]);
  // One name, two paths below `.cursor`.
  skill(path.join(userSkills, "cd-throwaway-twin"), ["name: cd-throwaway-twin", "description: user top"]);
  skill(path.join(d.proj, ".cursor", "skills", "nested", "cd-throwaway-twin"), ["name: cd-throwaway-twin", "description: project nested"]);
  // One path below two folders, whatever the folder holding each, and the
  // names.
  skill(path.join(d.home, ".agents", "skills", "cd-throwaway-path"), ["name: cd-throwaway-path", "description: user agents"]);
  skill(path.join(d.proj, ".claude", "skills", "cd-throwaway-path"), ["name: cd-throwaway-path", "description: project claude"]);
  skill(path.join(userSkills, "cd-throwaway-folder"), ["name: cd-throwaway-user-name", "description: user named"]);
  skill(path.join(d.proj, ".cursor", "skills", "cd-throwaway-folder"), ["name: cd-throwaway-project-name", "description: project named"]);

  // Skills whose frontmatter Cursor cannot read, and one it reads that has
  // no description.
  const unparsed = ["cd-throwaway-bare", "cd-throwaway-yaml", "cd-throwaway-unclosed", "cd-throwaway-no-key", "cd-throwaway-no-description"];
  let seen;
  try {
    await launch(dir, {
      home,
      during: async (win) => {
        const listed = {};
        for (const name of [...unparsed, "cd-throwaway-ok"]) listed[name] = (await win.slash(`/${name}`)).map(([n]) => n);
        seen = { listed, customize: await win.skills(), twins: [] };
        // The menu before any pick, after a pick of its first entry, and
        // after a pick of its second.
        seen.twins.push(await win.slash("/cd-throwaway-twin"));
        await win.pick("/cd-throwaway-twin");
        seen.twins.push(await win.slash("/cd-throwaway-twin"));
        await win.pick("/cd-throwaway-twin", 1);
        seen.twins.push(await win.slash("/cd-throwaway-twin"));
        Object.assign(seen, {
          path: await win.slash("/cd-throwaway-path"),
          userName: await win.slash("/cd-throwaway-user-name"),
          projectName: await win.slash("/cd-throwaway-project-name"),
        });
      },
    });
  } finally {
    await unmap(letter);
  }
  const picked = stored(globalState(d), "cursor.skills.recentlyUsed");
  // Customize's line below each skill of `names`, by folder.
  const customized = (names) => Object.fromEntries((seen?.customize ?? []).filter(([n]) => names.includes(n)));

  await p.check("items.skill", "Cursor lists a `SKILL.md` with no frontmatter, one whose YAML does not parse, one whose frontmatter never closes and one with a line that is no key under their folder's names in the `/` menu, and Customize marks each \"Error parsing skill\", as it does a skill with no description", async () =>
    same(
      { listed: seen?.listed, customize: customized([...unparsed, "cd-throwaway-ok"]) },
      {
        listed: Object.fromEntries([...unparsed, "cd-throwaway-ok"].map((n) => [n, [`/${n}`]])),
        customize: Object.fromEntries([...unparsed, "cd-throwaway-ok"].sort().map((n) => [n, n === "cd-throwaway-ok" ? "parses" : "Error parsing skill"])),
      },
    ),
  );
  const user = ["/cd-throwaway-twin", "user top"];
  const project = ["/cd-throwaway-twin", "project nested"];
  // Enter on the first entry picks the user's; the record of that pick puts
  // the Project's first, and a pick of the second entry is the user's
  // again, which the record shows unchanged.
  await p.check("items.skill", "Cursor lists both of two skills of one name at different paths under `/name`, the user's first until one is picked; once the user's was picked, the Project's comes first, so which one Enter takes depends on earlier picks", async () =>
    same({ twins: seen?.twins, picked }, { twins: [[user, project], [project, user], [project, user]], picked: ["cd-throwaway-twin/SKILL.md"] }),
  );
  // The `/` menu names a skill by its frontmatter's `name`, Customize by its
  // folder.
  await p.check("items.skill", "Of a user's skill and a Project's at the same path below their `.cursor`, `.claude` or `.agents` folders, whatever their names, Cursor lists the user's only, in the `/` menu and in Customize", async () =>
    same(
      { path: seen?.path, userName: seen?.userName, projectName: seen?.projectName, customize: seen?.customize.filter(([n]) => ["cd-throwaway-path", "cd-throwaway-folder"].includes(n)) },
      {
        path: [["/cd-throwaway-path", "user agents"]],
        userName: [["/cd-throwaway-user-name", "user named"]],
        projectName: [],
        customize: [["cd-throwaway-folder", "user named"], ["cd-throwaway-path", "user agents"]],
      },
    ),
  );
}

// ---------------------------------------------------------------- nested skills

// Skills folders deeper in a Project than its own `.cursor\skills`: one
// git Project, launched with Include Third-Party Plugins, Skills, and
// Other Configs on, as Cursor has it unless told, and again with it off,
// and a Project in no git work tree. Each nested skill is named after the
// case it tests; the `/` menu is queried one name at a time and Customize
// read once. The home folder is on a mapped drive, as for `skills`, so
// that Cursor takes `~\.cursor\skills` for the user's.
if (runs("nested-skills")) {
  const dir = fresh("nested-skills", base);
  const { letter, home } = await usersDrive(dir);
  const d = folders(dir, home);
  const at = (folder, name, description) => skill(path.join(d.proj, ...folder.split("/"), name), [`name: ${name}`, `description: ${description}`]);
  at("sub/.cursor/skills", "cd-throwaway-n-cursor-1", "cursor depth 1");
  at("sub/deep/er/.cursor/skills", "cd-throwaway-n-cursor-3", "cursor depth 3");
  at("a/.agents/skills", "cd-throwaway-n-agents-1", "agents depth 1");
  at("sub/deep/er/.agents/skills", "cd-throwaway-n-agents-3", "agents depth 3");
  at("sub/.claude/skills", "cd-throwaway-n-claude", "claude depth 1");
  at("sub/.codex/skills", "cd-throwaway-n-codex", "codex depth 1");
  at("sub/.grok/skills", "cd-throwaway-n-grok", "grok depth 1");
  // A junction below the Project's folder to a folder outside it.
  const elsewhere = path.join(dir, "elsewhere");
  skill(path.join(elsewhere, ".cursor", "skills", "cd-throwaway-n-linked"), ["name: cd-throwaway-n-linked", "description: through a junction"]);
  fs.symlinkSync(elsewhere, path.join(d.proj, "linked"), "junction");
  at("node_modules/pkg/.cursor/skills", "cd-throwaway-n-node-modules", "excluded");
  at("dist/.cursor/skills", "cd-throwaway-n-dist", "excluded");
  // A `.gitignore` that leaves out a folder holding a skills folder, one
  // that leaves out the skills folder itself, and one its `.cursor` folder;
  // an `.ignore` as the first.
  write(path.join(d.proj, ".gitignore"), "gi-folder/\ngi-skills/.cursor/skills/\ngi-marker/.cursor/\n");
  write(path.join(d.proj, ".ignore"), "ig-folder/\n");
  at("gi-folder/.cursor/skills", "cd-throwaway-n-ignored-folder", "gitignored folder");
  at("gi-skills/.cursor/skills", "cd-throwaway-n-ignored-skills", "gitignored skills folder");
  at("gi-marker/.cursor/skills", "cd-throwaway-n-ignored-marker", "gitignored marker folder");
  at("ig-folder/.cursor/skills", "cd-throwaway-n-dot-ignore", "ignored by .ignore");
  // One path below the marker folder: the root's and a nested one, the
  // user's and a nested one, and two nested ones.
  at(".cursor/skills", "cd-throwaway-n-root", "root");
  at("sub/.cursor/skills", "cd-throwaway-n-root", "nested");
  skill(path.join(d.cursor, "skills", "cd-throwaway-n-user"), ["name: cd-throwaway-n-user", "description: user"]);
  at("sub/.agents/skills", "cd-throwaway-n-user", "nested");
  at("x/.cursor/skills", "cd-throwaway-n-pair", "x");
  at("y/.cursor/skills", "cd-throwaway-n-pair", "y");
  // Two nested ones whose folders' order and paths' order disagree, and one
  // deeper whose path comes first.
  at("x/.agents/skills", "cd-throwaway-n-kinds", "x agents");
  at("y/.cursor/skills", "cd-throwaway-n-kinds", "y cursor");
  at("x/.cursor/skills", "cd-throwaway-n-holder", "x cursor");
  at("x/.agents/skills", "cd-throwaway-n-holder", "x agents");
  at("b/.cursor/skills", "cd-throwaway-n-deep", "b");
  at("a/b/c/.cursor/skills", "cd-throwaway-n-deep", "a/b/c");
  // An excluded name below the skills folder.
  at("sub/.cursor/skills/build", "cd-throwaway-n-below-build", "below build");
  await gitInit(d.proj);

  const names = [
    "cd-throwaway-n-cursor-1",
    "cd-throwaway-n-cursor-3",
    "cd-throwaway-n-agents-1",
    "cd-throwaway-n-agents-3",
    "cd-throwaway-n-claude",
    "cd-throwaway-n-codex",
    "cd-throwaway-n-grok",
    "cd-throwaway-n-linked",
    "cd-throwaway-n-node-modules",
    "cd-throwaway-n-dist",
    "cd-throwaway-n-ignored-folder",
    "cd-throwaway-n-ignored-skills",
    "cd-throwaway-n-dot-ignore",
    "cd-throwaway-n-root",
    "cd-throwaway-n-user",
    "cd-throwaway-n-pair",
    "cd-throwaway-n-kinds",
    "cd-throwaway-n-holder",
    "cd-throwaway-n-deep",
    "cd-throwaway-n-below-build",
    "cd-throwaway-n-ignored-marker",
    "cd-throwaway-n-nogit",
    "cd-throwaway-n-home",
  ];
  // The `/` menu for each name, and Customize's rows of the nested skills.
  const read = async (win) => {
    const slash = {};
    for (const name of names) slash[name] = await win.slash(`/${name}`);
    const customize = (await win.skills()).filter(([n]) => n.startsWith("cd-throwaway-n-"));
    return { slash, customize };
  };
  const seen = {};
  try {
    await launch(dir, { home, during: async (win) => (seen.on = await read(win)) });
    store(globalState(d), "cursor/thirdPartyExtensibilityEnabled", false);
    await launch(dir, { home, during: async (win) => (seen.off = await read(win)) });
  } finally {
    await unmap(letter);
  }
  const noGit = fresh("nested-skills-no-git", base);
  const n = folders(noGit);
  skill(path.join(n.proj, "sub", ".cursor", "skills", "cd-throwaway-n-nogit"), ["name: cd-throwaway-n-nogit", "description: no git"]);
  await launch(noGit, { during: async (win) => (seen.noGit = await read(win)) });
  // A git Project that is the user's home folder.
  const atHome = fresh("nested-skills-home", base);
  const h = folders(atHome, path.join(atHome, "proj"));
  skill(path.join(h.proj, "sub", ".cursor", "skills", "cd-throwaway-n-home"), ["name: cd-throwaway-n-home", "description: at home"]);
  await gitInit(h.proj);
  await launch(atHome, { home: h.home, during: async (win) => (seen.home = await read(win)) });

  // What the `/` menu listed under `/name` itself, each entry's
  // description: the menu also lists names close to the query.
  const listed = (s, name) => s?.slash[name]?.filter(([entry]) => entry === `/${name}`).map(([, description]) => description);
  const cases = (s, list) => Object.fromEntries(list.map((name) => [name, listed(s, name)]));
  const rows = (s, list) => s?.customize.filter(([name]) => list.includes(name));
  const depths = ["cd-throwaway-n-cursor-1", "cd-throwaway-n-cursor-3", "cd-throwaway-n-agents-1", "cd-throwaway-n-agents-3"];
  await p.check("locations[nested-cursor-skills]", "In a git Project, Cursor lists the skills of a `.cursor\\skills` and an `.agents\\skills` folder one folder below the Project's and three, in the `/` menu and in Customize", async () =>
    same(
      { slash: cases(seen.on, depths), customize: rows(seen.on, depths) },
      {
        slash: { "cd-throwaway-n-cursor-1": ["cursor depth 1"], "cd-throwaway-n-cursor-3": ["cursor depth 3"], "cd-throwaway-n-agents-1": ["agents depth 1"], "cd-throwaway-n-agents-3": ["agents depth 3"] },
        customize: [["cd-throwaway-n-agents-1", "agents depth 1"], ["cd-throwaway-n-agents-3", "agents depth 3"], ["cd-throwaway-n-cursor-1", "cursor depth 1"], ["cd-throwaway-n-cursor-3", "cursor depth 3"]],
      },
    ),
  );
  const thirdParty = ["cd-throwaway-n-claude", "cd-throwaway-n-codex", "cd-throwaway-n-grok"];
  await p.check("locations[nested-claude-skills]", "Cursor lists the skills of nested `.claude\\skills`, `.codex\\skills` and `.grok\\skills` folders while Include Third-Party Plugins, Skills, and Other Configs is on, and not while it is off", async () =>
    same(
      { on: [cases(seen.on, thirdParty), rows(seen.on, thirdParty)], off: [cases(seen.off, thirdParty), rows(seen.off, thirdParty)] },
      {
        on: [
          { "cd-throwaway-n-claude": ["claude depth 1"], "cd-throwaway-n-codex": ["codex depth 1"], "cd-throwaway-n-grok": ["grok depth 1"] },
          [["cd-throwaway-n-claude", "claude depth 1"], ["cd-throwaway-n-codex", "codex depth 1"], ["cd-throwaway-n-grok", "grok depth 1"]],
        ],
        off: [Object.fromEntries(thirdParty.map((name) => [name, []])), []],
      },
    ),
  );
  await p.check("locations[nested-cursor-skills]", "Cursor lists a nested skill reached through a junction below the Project's folder", async () =>
    same({ slash: listed(seen.on, "cd-throwaway-n-linked"), customize: rows(seen.on, ["cd-throwaway-n-linked"]) }, { slash: ["through a junction"], customize: [["cd-throwaway-n-linked", "through a junction"]] }),
  );
  const excluded = ["cd-throwaway-n-node-modules", "cd-throwaway-n-dist", "cd-throwaway-n-below-build"];
  await p.check("locations[nested-cursor-skills]", "Cursor lists no nested skill whose path passes through a `node_modules`, a `dist` or a `build` folder, above its skills folder or below it", async () =>
    same({ slash: cases(seen.on, excluded), customize: rows(seen.on, excluded) }, { slash: Object.fromEntries(excluded.map((name) => [name, []])), customize: [] }),
  );
  await p.check("locations[nested-cursor-skills]", "Cursor lists no nested skill of a Project in no git work tree", async () =>
    same({ slash: listed(seen.noGit, "cd-throwaway-n-nogit"), customize: rows(seen.noGit, ["cd-throwaway-n-nogit"]) }, { slash: [], customize: [] }),
  );
  // Its code means to skip a workspace folder that is the user's home
  // folder, comparing the two paths as spelled.
  await p.check("locations[nested-cursor-skills]", "Cursor lists the nested skills of a git Project that is the user's home folder too", async () =>
    same({ slash: listed(seen.home, "cd-throwaway-n-home"), customize: rows(seen.home, ["cd-throwaway-n-home"]) }, { slash: ["at home"], customize: [["cd-throwaway-n-home", "at home"]] }),
  );
  const ignored = ["cd-throwaway-n-ignored-folder", "cd-throwaway-n-ignored-skills", "cd-throwaway-n-ignored-marker", "cd-throwaway-n-dot-ignore"];
  await p.check("locations[nested-cursor-skills]", "Cursor lists no skill of a nested skills folder below a folder that a `.gitignore` or an `.ignore` leaves out, nor in a `.cursor` folder one leaves out, but lists one whose skills folder itself a `.gitignore` leaves out", async () =>
    same(
      { slash: cases(seen.on, ignored), customize: rows(seen.on, ignored) },
      {
        slash: { "cd-throwaway-n-ignored-folder": [], "cd-throwaway-n-ignored-skills": ["gitignored skills folder"], "cd-throwaway-n-ignored-marker": [], "cd-throwaway-n-dot-ignore": [] },
        customize: [["cd-throwaway-n-ignored-skills", "gitignored skills folder"]],
      },
    ),
  );
  const paths = ["cd-throwaway-n-root", "cd-throwaway-n-user"];
  await p.check("items.skill", "Of a nested skill at the same path below its marker folder as the Project's own or a user's skill, Cursor lists the nested one only, in the `/` menu and in Customize", async () =>
    same(
      { slash: cases(seen.on, paths), customize: rows(seen.on, paths) },
      { slash: { "cd-throwaway-n-root": ["nested"], "cd-throwaway-n-user": ["nested"] }, customize: [["cd-throwaway-n-root", "nested"], ["cd-throwaway-n-user", "nested"]] },
    ),
  );
  const nested = ["cd-throwaway-n-pair", "cd-throwaway-n-kinds", "cd-throwaway-n-holder", "cd-throwaway-n-deep"];
  await p.check("items.skill", "Of two nested skills at the same path below their marker folders, Cursor lists one only: the one whose folder holding the marker comes first by path, whatever the markers, and of one folder's, the `.cursor` one over the `.agents` one", async () =>
    same(
      { slash: cases(seen.on, nested), customize: rows(seen.on, nested) },
      {
        slash: { "cd-throwaway-n-pair": ["x"], "cd-throwaway-n-kinds": ["x agents"], "cd-throwaway-n-holder": ["x cursor"], "cd-throwaway-n-deep": ["a/b/c"] },
        customize: [["cd-throwaway-n-deep", "a/b/c"], ["cd-throwaway-n-holder", "x cursor"], ["cd-throwaway-n-kinds", "x agents"], ["cd-throwaway-n-pair", "x"]],
      },
    ),
  );
}

p.finish();
