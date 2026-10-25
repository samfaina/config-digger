// Tests Claude Desktop's facts about its two installers, which only Windows
// Sandbox may run: `uat/agents/sandbox.ps1` runs it there, with the
// installers claude.ai serves in `--installers`. Nobody signs in.
//
//   node uat/agents/claude-desktop.mjs --installers <folder holding
//     claude-exe.exe and claude-msix.msix>
//
// Claude Desktop starts no server before someone signs in, but it reads
// `claude_desktop_config.json` at start and names the entries it skips in a
// dialog, "Some MCP servers couldn't be loaded". So each config declares a
// server it skips, a `url` alone, named after the config's folder, and the
// probe reads the dialog with UI Automation. The data folder the app chose
// is also on its Chromium processes, as `--user-data-dir`, which
// config-digger reads to tell whether a running app reads a home the user
// added (#211).
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fresh, probe, run, same, sandboxOnly, write } from "./lib.mjs";

sandboxOnly();
const installers = process.argv.includes("--installers") ? process.argv[process.argv.indexOf("--installers") + 1] : ".";
const probeDir = fresh("claude-desktop");
const appData = process.env.APPDATA;
const localAppData = process.env.LOCALAPPDATA;
const classicData = path.join(appData, "Claude");
let localCache = null;

const powershell = async (command) => (await run("powershell.exe", ["-NoProfile", "-Command", command], { timeout: 600_000 })).stdout.trim();
const version = await powershell(`(Get-Item '${path.join(installers, "claude-exe.exe")}').VersionInfo.ProductVersion`);
const p = probe("Claude Desktop", version);

async function waitFor(fn, seconds) {
  const end = Date.now() + seconds * 1000;
  while (Date.now() < end) {
    const value = await fn();
    if (value) return value;
    await new Promise((r) => setTimeout(r, 2000));
  }
  return null;
}
const stop = () => run("taskkill.exe", ["/im", "claude.exe", "/t", "/f"]);
const detached = (command, args, env = process.env) => spawn(command, args, { detached: true, stdio: "ignore", env }).unref();

// A config whose one server, `cd-throwaway-<tag>`, the app skips.
const config = (tag) => ({ mcpServers: { [`cd-throwaway-${tag}`]: { url: "http://127.0.0.1:9/mcp" } } });

// The probe servers the open windows of `claude.exe` name.
async function named() {
  const texts = await powershell(`
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$ids = @(Get-Process claude -ErrorAction SilentlyContinue | ForEach-Object Id)
$windows = [Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children, [Windows.Automation.Condition]::TrueCondition)
foreach ($w in $windows) {
  if ($ids -notcontains $w.Current.ProcessId) { continue }
  foreach ($e in $w.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition)) { $e.Current.Name }
}`);
  return [...new Set(texts.match(/cd-throwaway-[a-z]+/g) ?? [])].map((n) => n.slice("cd-throwaway-".length)).sort();
}

// The folder a command line's `--user-data-dir` names, the whole argument
// quoted, the value quoted, or neither; null when it has none.
const switchDir = (line) =>
  (line.match(/"--user-data-dir=([^"]*)"/) ?? line.match(/--user-data-dir="([^"]*)"/) ?? line.match(/--user-data-dir=(\S+)/))?.[1] ?? null;

// The executables the app runs as, the data folder its Chromium processes
// were given, and the folder each process's `--user-data-dir` names.
async function processes() {
  const paths = await powershell("Get-CimInstance Win32_Process -Filter \"Name='claude.exe'\" | ForEach-Object { $_.ExecutablePath }");
  const lines = await powershell("Get-CimInstance Win32_Process -Filter \"Name='claude.exe'\" | ForEach-Object { $_.CommandLine }");
  const images = [...new Set(paths.split(/\r?\n/).filter(Boolean))].sort();
  const dirs = lines.split(/\r?\n/).filter(Boolean).map(switchDir);
  return { images, dir: lines.match(/--user-data-dir="?([^"\r\n]+?)"?(?= --| \/|\r?$)/m)?.[1] ?? null, dirs };
}

// Starts the app with `launch`, waits for its dialog, and stops it: the
// folders its dialog named, the data folder it chose, and the folder each
// of its processes' `--user-data-dir` names.
async function started(launch, seconds = 90) {
  await stop();
  launch();
  const tags = (await waitFor(async () => {
    const tags = await named();
    return tags.length ? tags : null;
  }, seconds)) ?? [];
  const { images, dir, dirs } = await processes();
  await stop();
  return { tags, images, dir, dirs };
}

// ---------------------------------------------------------------- the classic installer

let classicExe = null;
await p.check("(top)", "the classic installer puts the app in `%LOCALAPPDATA%\\AnthropicClaude`: a launcher with the app's version, `Update.exe`, `app-<version>`, and the user's uninstall key `AnthropicClaude`", async () => {
  await run(path.join(installers, "claude-exe.exe"), ["--silent"], { timeout: 600_000 });
  await waitFor(() => fs.existsSync(path.join(localAppData, "AnthropicClaude")), 120);
  await stop();
  const found = await powershell(
    `Get-ChildItem '${localAppData}', '${process.env.ProgramFiles}', '${appData}' -Recurse -Depth 3 -Filter claude.exe -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName + ' ' + $_.VersionInfo.ProductVersion }`,
  );
  const uninstall = await powershell(
    "Get-ChildItem HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall | Get-ItemProperty | Where-Object DisplayName -like '*Claude*' | ForEach-Object { $_.PSChildName + ' | ' + $_.InstallLocation + ' | ' + $_.DisplayVersion + ' | ' + $_.UninstallString }",
  );
  const root = path.join(localAppData, "AnthropicClaude");
  classicExe = path.join(root, "claude.exe");
  const layout = fs.existsSync(root) ? fs.readdirSync(root).sort() : [];
  const seen = {
    launcher: await powershell(`(Get-Item '${classicExe}').VersionInfo.ProductVersion`),
    update: layout.includes("Update.exe"),
    app: fs.existsSync(path.join(root, `app-${version}`, "claude.exe")),
    // The key, its InstallLocation and its UninstallString.
    uninstall: uninstall.split(" | ").filter((_, i) => i !== 2).join(" | "),
  };
  const expected = { launcher: version, update: true, app: true, uninstall: `AnthropicClaude | ${root} | "${path.join(root, "Update.exe")}" --uninstall` };
  return { ...same(seen, expected), observed: JSON.stringify({ ...seen, found: found.split(/\r?\n/), layout }) };
});

await p.check("(top)", "the classic app runs from `app-<version>` and reads `%APPDATA%\\Claude`", async () => {
  write(path.join(classicData, "claude_desktop_config.json"), config("classic"));
  const { tags, images, dir } = await started(() => detached(classicExe, []));
  return same({ tags, images, dir }, { tags: ["classic"], images: [path.join(localAppData, "AnthropicClaude", `app-${version}`, "claude.exe")], dir: classicData });
});

// A data folder the user picks, which config-digger can know only from the
// running app's processes (#211): a space in it shows how the switch quotes
// it. Both checks start the app itself, not the launcher, and say how many
// of its processes name no folder.
const appExe = () => path.join(localAppData, "AnthropicClaude", `app-${version}`, "claude.exe");
const switches = (seen, folder) => {
  const named = seen.dirs.filter(Boolean);
  return { tags: seen.tags, named: named.length > 0, every: named.every((dir) => dir === folder), unnamed: seen.dirs.length - named.length };
};

await p.check("(top)", "the signed app ignores `CLAUDE_USER_DATA_DIR`: it reads `%APPDATA%\\Claude`, which its children name in `--user-data-dir`", async () => {
  const userDataDir = path.join(probeDir, "user data dir");
  write(path.join(userDataDir, "claude_desktop_config.json"), config("userdatadir"));
  write(path.join(classicData, "claude_desktop_config.json"), config("ignored"));
  const seen = await started(() => detached(appExe(), [], { ...process.env, CLAUDE_USER_DATA_DIR: userDataDir }));
  return { ...same(switches(seen, classicData), { tags: ["ignored"], named: true, every: true, unnamed: 1 }), observed: JSON.stringify(seen) };
});

await p.check("(top)", "the app started with `--user-data-dir=<folder>` reads that folder, and each of its processes names it there, the main one too", async () => {
  const userDataDir = path.join(probeDir, "switch data dir");
  write(path.join(userDataDir, "claude_desktop_config.json"), config("switch"));
  write(path.join(classicData, "claude_desktop_config.json"), config("notswitch"));
  const seen = await started(() => detached(appExe(), [`--user-data-dir=${userDataDir}`]));
  return { ...same(switches(seen, userDataDir), { tags: ["switch"], named: true, every: true, unnamed: 0 }), observed: JSON.stringify(seen) };
});

// ---------------------------------------------------------------- the MSIX package

let aumid = null;
await p.check("(top)", "the MSIX package installs beside the classic app", async () => {
  await powershell(`Add-AppxPackage -Path '${path.join(installers, "claude-msix.msix")}'`);
  const family = await powershell("(Get-AppxPackage -Name Claude).PackageFamilyName");
  // The package holds helper apps too, such as `SshAskpass`: the app is the first.
  const app = (await powershell("(Get-AppxPackage -Name Claude | Get-AppxPackageManifest).Package.Applications.Application.Id")).split(/\s+/)[0];
  aumid = `${family}!${app}`;
  localCache = path.join(localAppData, "Packages", family, "LocalCache", "Roaming", "Claude");
  return { pass: family.startsWith("Claude_"), observed: JSON.stringify({ family, app, version: await powershell("(Get-AppxPackage -Name Claude).Version") }) };
});

const packaged = () => detached("explorer.exe", [`shell:AppsFolder\\${aumid}`]);

// What each data folder holds of the app's own files, and whether the
// package moved its data to `Claude-Data`.
function folders() {
  const files = (dir) => (fs.existsSync(dir) ? fs.readdirSync(dir).filter((f) => /config|Local State|Preferences|lockfile|user-data/.test(f)).sort() : null);
  const claudeData = path.join(localAppData, "Claude-Data");
  const data = fs.existsSync(claudeData) ? Object.fromEntries(fs.readdirSync(claudeData).map((f) => [f, files(path.join(claudeData, f))])) : null;
  return { classic: files(classicData), localCache: files(localCache), claudeData: data };
}

// The app names an entry it skips once: each launch gets new names.
await p.check("(top)", "the packaged app, started beside a classic `%APPDATA%\\Claude`, reads the classic config and creates no `LocalCache`; once its `LocalCache` holds a config, it reads that one", async () => {
  write(path.join(classicData, "claude_desktop_config.json"), config("legacy"));
  const first = { ...(await started(packaged)), folders: folders() };
  write(path.join(classicData, "claude_desktop_config.json"), config("older"));
  write(path.join(localCache, "claude_desktop_config.json"), config("msix"));
  const both = { ...(await started(packaged)), folders: folders() };
  const seen = { first: first.tags, firstLocalCache: first.folders.localCache, both: both.tags };
  return { ...same(seen, { first: ["legacy"], firstLocalCache: null, both: ["msix"] }), observed: JSON.stringify({ first, both }) };
});

await stop();
p.finish();
