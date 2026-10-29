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

// A command line's `--user-data-dir` as written, the whole argument quoted,
// the value quoted, or neither, and the folder it names; nulls when it has
// none.
function userDataDir(line) {
  const raw = line.match(/"--user-data-dir=[^"]*"|--user-data-dir="[^"]*"|--user-data-dir=\S+/)?.[0] ?? null;
  return { raw, dir: raw?.replace(/"/g, "").slice("--user-data-dir=".length) ?? null };
}

// The executables the app runs as, the data folder its Chromium processes
// were given, and each process's `--user-data-dir`, a Chromium child's
// being one with a `--type`.
async function processes() {
  const listed = await powershell("@(Get-CimInstance Win32_Process -Filter \"Name='claude.exe'\" | Select-Object ExecutablePath, CommandLine) | ConvertTo-Json -Compress");
  const found = [listed ? JSON.parse(listed) : []].flat();
  const images = [...new Set(found.map((p) => p.ExecutablePath).filter(Boolean))].sort();
  const switches = found.map((p) => ({ child: /--type=/.test(p.CommandLine ?? ""), ...userDataDir(p.CommandLine ?? "") }));
  return { images, dir: switches.find((s) => s.dir)?.dir ?? null, switches };
}

// Starts the app with `launch`, waits for its dialog, and stops it: the
// folders its dialog named, the data folder it chose, and each of its
// processes' `--user-data-dir`.
async function started(launch, seconds = 90) {
  await stop();
  launch();
  const tags = (await waitFor(async () => {
    const tags = await named();
    return tags.length ? tags : null;
  }, seconds)) ?? [];
  const { images, dir, switches } = await processes();
  await stop();
  return { tags, images, dir, switches };
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
// running app's processes (#211). These checks start the app itself, not
// the launcher; a space in the folder shows the switch quoted.
const appExe = () => path.join(localAppData, "AnthropicClaude", `app-${version}`, "claude.exe");

// What a launch's processes say of `folder`: the tags its dialog named,
// whether it has children and each names `folder`, the folder each main
// process names, and whether every switch naming a folder with a space is
// quoted.
function folderFacts(seen, folder) {
  const children = seen.switches.filter((s) => s.child);
  const spaced = seen.switches.filter((s) => s.dir?.includes(" "));
  return {
    tags: seen.tags,
    children: children.length > 0 && children.every((s) => s.dir === folder),
    main: seen.switches.filter((s) => !s.child).map((s) => s.dir),
    quoted: spaced.every((s) => s.raw.includes('"')),
  };
}

await p.check("(top)", "the signed app ignores `CLAUDE_USER_DATA_DIR`: it reads `%APPDATA%\\Claude`, which its children name in `--user-data-dir`, and its main process names none", async () => {
  const userDataDir = path.join(probeDir, "user data dir");
  write(path.join(userDataDir, "claude_desktop_config.json"), config("userdatadir"));
  write(path.join(classicData, "claude_desktop_config.json"), config("ignored"));
  const seen = await started(() => detached(appExe(), [], { ...process.env, CLAUDE_USER_DATA_DIR: userDataDir }));
  return { ...same(folderFacts(seen, classicData), { tags: ["ignored"], children: true, main: [null], quoted: true }), observed: JSON.stringify(seen) };
});

// Started with the switch, the app reads `folder` and every one of its
// processes names it, quoted.
const readsSwitchFolder = (seen, folder, tag) => ({ ...same(folderFacts(seen, folder), { tags: [tag], children: true, main: [folder], quoted: true }), observed: JSON.stringify(seen) });

await p.check("(top)", "the classic app started with `--user-data-dir=<folder>` reads that folder, and each of its processes names it there, quoted, the main one too", async () => {
  const userDataDir = path.join(probeDir, "switch data dir");
  write(path.join(userDataDir, "claude_desktop_config.json"), config("switch"));
  write(path.join(classicData, "claude_desktop_config.json"), config("notswitch"));
  return readsSwitchFolder(await started(() => detached(appExe(), [`--user-data-dir=${userDataDir}`])), userDataDir, "switch");
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
  const files = (dir) => (fs.existsSync(dir) && fs.statSync(dir).isDirectory() ? fs.readdirSync(dir).filter((f) => /config|Local State|Preferences|lockfile|user-data/.test(f)).sort() : null);
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

// UAT step 7.6 starts the packaged app from its folder when no classic app
// is installed.
await p.check("(top)", "the packaged app started from its folder with `--user-data-dir=<folder>` reads that folder, and each of its processes names it there, quoted, the main one too", async () => {
  const exe = path.join(await powershell("(Get-AppxPackage -Name Claude).InstallLocation"), "app", "claude.exe");
  const userDataDir = path.join(probeDir, "packaged data dir");
  write(path.join(userDataDir, "claude_desktop_config.json"), config("packaged"));
  write(path.join(localCache, "claude_desktop_config.json"), config("notpackaged"));
  return readsSwitchFolder(await started(() => detached(exe, [`--user-data-dir=${userDataDir}`])), userDataDir, "packaged");
});

// The move out of the package (#305). The app moves its data folder to
// `%LOCALAPPDATA%\Claude-Data\<family>` once a rollout flag, which it
// latches into its config's `preferences.userDataOutsidePackageAnswer`
// (`userDataFolderAnswer` before Windows 11), reads on [bundle]. Nobody
// signs in here, so the probe writes the latched answer itself.
const family = () => path.basename(path.dirname(path.dirname(path.dirname(localCache))));
const movedData = () => path.join(localAppData, "Claude-Data", family().toLowerCase());
const answer = process.env.CD_PROBE_MOVE_ANSWER ?? (Number((await powershell("[Environment]::OSVersion.Version.Build")) || 0) < 22000 ? "userDataFolderAnswer" : "userDataOutsidePackageAnswer");

await p.check("(top)", "the packaged app with no classic folder keeps its data in its `LocalCache` over several launches and writes no `Claude-Data\\<family>`", async () => {
  fs.rmSync(classicData, { recursive: true, force: true });
  const launches = [];
  for (const tag of ["cachea", "cacheb", "cachec"]) {
    write(path.join(localCache, "claude_desktop_config.json"), config(tag));
    launches.push({ ...(await started(packaged)), folders: folders() });
  }
  const seen = { tags: launches.map((l) => l.tags), dirs: launches.map((l) => l.dir), moved: fs.existsSync(movedData()) };
  return { ...same(seen, { tags: [["cachea"], ["cacheb"], ["cachec"]], dirs: [classicData, classicData, classicData], moved: false }), observed: JSON.stringify(launches) };
});

let moved = null;
await p.check("(top)", "with the move's answer latched true in its config, the packaged app moves its data folder to `%LOCALAPPDATA%\\Claude-Data\\<family>` in lower case, beside a `README.txt`, leaves `.user-data-left.<family>` in its `LocalCache`, and reads and names the moved folder from then on", async () => {
  write(path.join(localCache, "claude_desktop_config.json"), { ...config("tomove"), preferences: { [answer]: true } });
  const launches = [];
  for (let i = 0; i < 3 && !fs.existsSync(path.join(movedData(), "claude_desktop_config.json")); i++) {
    launches.push({ ...(await started(packaged)), folders: folders() });
  }
  const copied = fs.existsSync(path.join(movedData(), "claude_desktop_config.json"));
  if (copied) write(path.join(movedData(), "claude_desktop_config.json"), { ...config("moved"), preferences: { [answer]: true } });
  moved = copied ? { ...(await started(packaged)), folders: folders() } : null;
  const left = `.user-data-left.${family().toLowerCase()}`;
  const seen = {
    copied,
    readme: fs.existsSync(path.join(localAppData, "Claude-Data", "README.txt")),
    left: fs.existsSync(path.join(localCache, left)),
    tags: moved?.tags ?? null,
    dir: moved?.dir ?? null,
  };
  return { ...same(seen, { copied: true, readme: true, left: true, tags: ["moved"], dir: movedData() }), observed: JSON.stringify({ answer, launches, moved }) };
});

await p.check("(top)", "the packaged app reads its moved data folder before a classic `%APPDATA%\\Claude` beside it", async () => {
  if (!moved) return { pass: false, observed: "the app never moved its data folder" };
  write(path.join(classicData, "claude_desktop_config.json"), config("besideclassic"));
  write(path.join(movedData(), "claude_desktop_config.json"), { ...config("besidemoved"), preferences: { [answer]: true } });
  const seen = await started(packaged);
  return { ...same({ tags: seen.tags, dir: seen.dir }, { tags: ["besidemoved"], dir: movedData() }), observed: JSON.stringify({ ...seen, folders: folders() }) };
});

await stop();
p.finish();
