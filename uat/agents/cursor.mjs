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
// `broken-<user|project>`, `project-plugins` and `sign-in`.
//
// Cursor starts the servers of `~\.cursor\mcp.json` and of the plugins
// without asking, and a Project's once approved: the probe approves them in
// the profile's `state.vscdb` before the launch. Each server is this
// machine's Node running `server.mjs <tag>`, which writes a marker named
// after its tag. Only the `project-plugins` checks run signed in to a
// Cursor account, on the profile `--sign-in` opens for the owner.
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
import { fresh, json, probe, root, run, same, write } from "./lib.mjs";

// `node:sqlite` warns that it is experimental on every run.
process.removeAllListeners("warning");
const { DatabaseSync } = await import("node:sqlite");

const bin = process.argv.includes("--bin")
  ? process.argv[process.argv.indexOf("--bin") + 1]
  : path.join(process.env.LOCALAPPDATA, "Programs", "cursor", "Cursor.exe");
const version = json(path.join(path.dirname(bin), "resources", "app", "product.json")).version;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const powershell = async (command) => (await run("powershell.exe", ["-NoProfile", "-Command", command])).stdout.trim();
// Stops what is left of a launch: the processes given its data folder only.
const stop = (data) =>
  powershell(`Get-CimInstance Win32_Process -Filter "Name='Cursor.exe'" | Where-Object { $_.CommandLine -like '*${data}*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }`);

// The folders of a launch in `dir`.
const folders = (dir) => ({
  data: path.join(dir, "data"),
  user: path.join(dir, "data", "User"),
  home: path.join(dir, "home"),
  cursor: path.join(dir, "home", ".cursor"),
  claude: path.join(dir, "home", ".claude"),
  proj: path.join(dir, "proj"),
});

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
function signedIn(dir) {
  const file = path.join(folders(dir).user, "globalStorage", "state.vscdb");
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
  const running = async () => Number(await powershell(`@(Get-CimInstance Win32_Process -Filter "Name='Cursor.exe'" | Where-Object { $_.CommandLine -like '*${d.data}*' }).Count`));
  while (!(await running())) await sleep(1000);
  while (await running()) await sleep(2000);
  const done = signedIn(account);
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

// Launches Cursor on the Project folder of `dir`, approving the Project
// servers `approved` names by identifier, and resolves once it has loaded
// its servers and `during`, given the window, is done: the servers it
// started, by tag. The profile is left in `dir`, so a launch can follow
// another on it.
async function launch(dir, { approved = [], during, seconds = 90 } = {}) {
  const d = folders(dir);
  write(path.join(d.user, "settings.json"), {
    "security.workspace.trust.enabled": false,
    "telemetry.telemetryLevel": "off",
    "update.mode": "none",
    "extensions.autoUpdate": false,
    "workbench.startupEditor": "none",
    "workbench.externalBrowser": path.join(process.env.SystemRoot, "System32", "curl.exe"),
  });
  if (approved.length) {
    const file = path.join(d.user, "globalStorage", "state.vscdb");
    fs.mkdirSync(path.dirname(file), { recursive: true });
    const db = new DatabaseSync(file);
    db.exec("CREATE TABLE IF NOT EXISTS ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB)");
    db.prepare("INSERT INTO ItemTable VALUES (?, ?)").run("cursor/approvedProjectMcpServers", JSON.stringify(approved));
    db.close();
  }
  fs.mkdirSync(d.proj, { recursive: true });
  fs.rmSync(markers, { recursive: true, force: true });
  const port = await freePort();
  const sessions = fs.existsSync(path.join(d.data, "logs")) ? fs.readdirSync(path.join(d.data, "logs")).length : 0;
  const args = [`--user-data-dir=${d.data}`, `--extensions-dir=${path.join(dir, "ext")}`, `--remote-debugging-port=${port}`, "--log=trace", "--new-window", d.proj];
  spawn(bin, args, { detached: true, stdio: "ignore", env: env(d) }).unref();
  // Cursor logs a census of the servers once it has read their files.
  const census = () => (windowLog(d.data, "workbench.mcp.files.log").match(/Plugin MCP census/g) ?? []).length;
  const end = Date.now() + seconds * 1000;
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
  const evaluate = (expression) =>
    new Promise((resolve) => {
      pending.set(++id, (m) => resolve(m.result?.result?.value));
      ws.send(JSON.stringify({ id, method: "Runtime.evaluate", params: { expression, returnByValue: true, awaitPromise: true } }));
    });
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
    close: () => ws.close(),
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
// with its server, which needs the account: these checks run on the
// signed-in profile. The plugin is the Marketplace's `superdesign`, which
// holds skills only, so no server of its runs. Beside it the probe plants
// two copies Cursor's cache could hold: one in a marketplace whose name
// comes first, and a complete commit whose name comes last, each with a
// server that leaves a marker.
if (runs("project-plugins") && !signedIn(account)) {
  await p.check("plugins[project]", "the account profile is signed in", async () => ({ pass: false, observed: `${account} is not: run node uat/agents/cursor.mjs --sign-in` }));
} else if (runs("project-plugins")) {
  const d = folders(account);
  const cache = path.join(d.cursor, "plugins", "cache");
  fs.rmSync(cache, { recursive: true, force: true });
  fs.rmSync(d.proj, { recursive: true, force: true });
  write(path.join(d.proj, ".cursor", "settings.json"), { plugins: { superdesign: { enabled: true } } });
  const decoy = (marketplace, commit, tag) => {
    const folder = path.join(cache, marketplace, "superdesign", commit);
    write(path.join(folder, ".cursor-plugin", "plugin.json"), { name: "superdesign" });
    write(path.join(folder, "mcp.json"), servers(tag));
    write(path.join(folder, ".cache-complete"), "");
  };
  decoy("aaa-cd-throwaway", "a".repeat(40), "first-marketplace");
  decoy("cursor-public", "f".repeat(40), "last-commit");
  const tags = await launch(account);
  // The commits of `superdesign` in each marketplace's cache, `*` marking
  // a complete one.
  const commits = Object.fromEntries(
    fs.readdirSync(cache).map((m) => {
      const plugin = path.join(cache, m, "superdesign");
      return [m, fs.readdirSync(plugin).map((c) => (fs.existsSync(path.join(plugin, c, ".cache-complete")) ? `${c}*` : c))];
    }),
  );
  const log = windowLog(d.data, path.join("exthost", "anysphere.cursor-agent-exec", "Cursor Plugins.log"));
  // The commit Cursor's server named, and the folders Cursor loaded.
  const named = log.match(/Adding enabled plugin: superdesign from ([0-9a-f]{40})/)?.[1];
  const loaded = [...new Set([...log.matchAll(/Found cached plugin: superdesign at (.+)/g)].map((m) => path.relative(cache, m[1].trim())))];

  await p.check("plugins[project]", "Cursor's server resolves a key that names no marketplace to `cursor-public`, and Cursor loads the plugin from that marketplace's cache, not from another marketplace's that holds a plugin of the same name and comes first", async () =>
    same(
      { tags, loaded, firstMarketplace: commits["aaa-cd-throwaway"] },
      { tags: [], loaded: [path.join("cursor-public", "superdesign", named ?? "?")], firstMarketplace: [`${"a".repeat(40)}*`] },
    ),
  );
  await p.check("plugins[project]", "Cursor downloads the commit its server names, and deletes the plugin's other commits, a complete one that comes last included, as it loads it", async () =>
    same({ named: Boolean(named), commits: commits["cursor-public"] }, { named: true, commits: [`${named}*`] }),
  );
}

// ---------------------------------------------------------------- sign-in

// A local MCP server behind OAuth. It registers any client, grants the
// token `not-a-real-secret` for any code, and answers the authorization
// request itself by calling the redirect it names, as a user who signs in
// would. `phase` labels the requests that follow, and `signedIn(phase)`
// says whether one of them carried the token.
async function oauthServer() {
  const requests = [];
  const s = {
    phase: "start",
    signedIn: (phase) => requests.some((r) => r.phase === phase && r.bearer),
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
  while (!oauth.signedIn("signed-in") && Date.now() < end) await sleep(1000);
  return oauth.signedIn("signed-in");
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
        { signedIn, restarted: oauth.signedIn("restarted"), again: oauth.signedIn("started-with-again"), repaired: oauth.signedIn("started-repaired") },
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
      same({ signedIn, putBack: oauth.signedIn("put-back"), asked: oauth.asked("put-back") }, { signedIn: true, putBack: false, asked: true }),
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
      same({ signedIn, repaired: oauth.signedIn("repaired"), asked: oauth.asked("repaired") }, { signedIn: true, repaired: false, asked: true }),
    );
  }
  oauth.close();
}

p.finish();
