// Tests Claude Desktop's facts about its two installers, which only Windows
// Sandbox may run: `uat/agents/sandbox.ps1` runs it there, with the
// installers claude.ai serves in `--installers`. Nobody signs in, unless
// `--signed-in` is given: then the probe installs the package alone, waits
// while the owner signs in from the Sandbox's window, and runs only the
// checks that need an account.
//
//   node uat/agents/claude-desktop.mjs --installers <folder holding
//     claude-exe.exe and claude-msix.msix> [--signed-in]
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
import { fresh, json, probe, run, same, sandboxOnly, write } from "./lib.mjs";

sandboxOnly();
const installers = process.argv.includes("--installers") ? process.argv[process.argv.indexOf("--installers") + 1] : ".";
const signedIn = process.argv.includes("--signed-in");
const probeDir = fresh("claude-desktop");
const appData = process.env.APPDATA;
const localAppData = process.env.LOCALAPPDATA;
const classicData = path.join(appData, "Claude");
let localCache = null;

const powershell = async (command) => (await run("powershell.exe", ["-NoProfile", "-Command", command], { timeout: 600_000 })).stdout.trim();
const version = await powershell(`(Get-Item '${path.join(installers, "claude-exe.exe")}').VersionInfo.ProductVersion`);
const p = probe("Claude Desktop", version);
// A check that runs only signed out, which `--signed-in` leaves to the run
// without it, and one that runs only with `--signed-in`.
const signedOut = (table, title, fn) => (signedIn ? null : p.check(table, title, fn));
const signedInOnly = (table, title, fn) => (signedIn ? p.check(table, title, fn) : null);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function waitFor(fn, seconds) {
  const end = Date.now() + seconds * 1000;
  while (Date.now() < end) {
    const value = await fn();
    if (value) return value;
    await sleep(2000);
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
await signedOut("(top)", "the classic installer puts the app in `%LOCALAPPDATA%\\AnthropicClaude`: a launcher with the app's version, `Update.exe`, `app-<version>`, and the user's uninstall key `AnthropicClaude`", async () => {
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

await signedOut("(top)", "the classic app runs from `app-<version>` and reads `%APPDATA%\\Claude`", async () => {
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

await signedOut("(top)", "the signed app ignores `CLAUDE_USER_DATA_DIR`: it reads `%APPDATA%\\Claude`, which its children name in `--user-data-dir`, and its main process names none", async () => {
  const userDataDir = path.join(probeDir, "user data dir");
  write(path.join(userDataDir, "claude_desktop_config.json"), config("userdatadir"));
  write(path.join(classicData, "claude_desktop_config.json"), config("ignored"));
  const seen = await started(() => detached(appExe(), [], { ...process.env, CLAUDE_USER_DATA_DIR: userDataDir }));
  return { ...same(folderFacts(seen, classicData), { tags: ["ignored"], children: true, main: [null], quoted: true }), observed: JSON.stringify(seen) };
});

// Started with the switch, the app reads `folder` and every one of its
// processes names it, quoted.
const readsSwitchFolder = (seen, folder, tag) => ({ ...same(folderFacts(seen, folder), { tags: [tag], children: true, main: [folder], quoted: true }), observed: JSON.stringify(seen) });

await signedOut("(top)", "the classic app started with `--user-data-dir=<folder>` reads that folder, and each of its processes names it there, quoted, the main one too", async () => {
  const userDataDir = path.join(probeDir, "switch data dir");
  write(path.join(userDataDir, "claude_desktop_config.json"), config("switch"));
  write(path.join(classicData, "claude_desktop_config.json"), config("notswitch"));
  return readsSwitchFolder(await started(() => detached(appExe(), [`--user-data-dir=${userDataDir}`])), userDataDir, "switch");
});

// ---------------------------------------------------------------- the MSIX package

let aumid = null;
let family = null;
await p.check("(top)", "the MSIX package installs beside the classic app", async () => {
  await powershell(`Add-AppxPackage -Path '${path.join(installers, "claude-msix.msix")}'`);
  family = await powershell("(Get-AppxPackage -Name Claude).PackageFamilyName");
  // The package holds helper apps too, such as `SshAskpass`: the app is the first.
  const app = (await powershell("(Get-AppxPackage -Name Claude | Get-AppxPackageManifest).Package.Applications.Application.Id")).split(/\s+/)[0];
  aumid = `${family}!${app}`;
  localCache = path.join(localAppData, "Packages", family, "LocalCache", "Roaming", "Claude");
  return { pass: family.startsWith("Claude_"), observed: JSON.stringify({ family, app, version: await powershell("(Get-AppxPackage -Name Claude).Version") }) };
});

const packaged = () => detached("explorer.exe", [`shell:AppsFolder\\${aumid}`]);
const movedData = () => path.join(localAppData, "Claude-Data", family.toLowerCase());
// The data folders the packaged app may read, in the order it picks one.
const dataFolders = () => [movedData(), localCache, classicData];

// The lines of the app's logs that match `pattern`: its real
// `%LOCALAPPDATA%\Claude\logs`, which no package virtualises, and those of
// each data folder.
function logLines(pattern) {
  const dirs = [path.join(localAppData, "Claude", "logs"), ...dataFolders().map((dir) => path.join(dir, "logs"))];
  return dirs
    .filter((dir) => fs.existsSync(dir))
    .flatMap((dir) => fs.readdirSync(dir).map((file) => path.join(dir, file)))
    .filter((file) => fs.statSync(file).isFile())
    .flatMap((file) => fs.readFileSync(file, "utf8").split(/\r?\n/))
    .filter((line) => pattern.test(line));
}

// ---------------------------------------------------------------- signed in

// The account's skills mirror, `skills-plugin\<org>\<account>`, in whichever
// data folder the app reads: the first one holding a `manifest.json` that
// lists skills, or null.
function mirror() {
  const roots = dataFolders().map((dir) => path.join(dir, "local-agent-mode-sessions", "skills-plugin"));
  for (const root of roots.filter((dir) => fs.existsSync(dir))) {
    for (const org of fs.readdirSync(root)) {
      if (!fs.statSync(path.join(root, org)).isDirectory()) continue;
      for (const account of fs.readdirSync(path.join(root, org))) {
        const dir = path.join(root, org, account);
        try {
          if (json(path.join(dir, "manifest.json")).skills?.length) return dir;
        } catch {}
      }
    }
  }
  return null;
}

// Signed out, the app writes no mirror. The owner signs in from the
// Sandbox's window, with the email code rather than Google, and the probe
// goes on once the app has synced the account's skills. A later run in the
// same Sandbox finds the app signed in already.
if (signedIn) {
  await stop();
  packaged();
  console.log("SIGN IN: sign in to Claude Desktop in the Sandbox's window, with your email and the code claude.ai sends; leave the window open");
  const synced = await waitFor(mirror, 20 * 60);
  console.log(synced ? `signed in: the mirror is ${synced}` : "no sign-in after 20 minutes");
  // Let the app write its cookies before it is stopped.
  await sleep(30_000);
  await stop();
}

// Two `SKILL.md` files the app cannot fully read, each with one fault: a
// frontmatter that is not YAML, and, under a frontmatter naming the skill as
// its manifest does, a body whose bytes are not UTF-8.
const notYaml = Buffer.from("---\nname: [unclosed\ndescription: cd-throwaway: not: yaml\n---\nbody\n");
const notUtf8 = (name) => Buffer.concat([Buffer.from(`---\nname: ${name}\ndescription: cd-throwaway\n---\n`), Buffer.from([0xff, 0xfe, 0xfd]), Buffer.from("\n")]);

// What became of a `SKILL.md` the probe planted, `planted`, over
// `original`: `kept` as planted, `restored` to the original, `removed`, or
// `other`.
function plantedFate(file, planted, original) {
  if (!fs.existsSync(file)) return "removed";
  const now = fs.readFileSync(file);
  if (now.equals(planted)) return "kept";
  return original && now.equals(original) ? "restored" : "other";
}

// The account syncs its skills into the mirror. The app seeds the skills it
// ships only for an organization that serves those alone, so they are left
// out. The account validates a skill on upload, so only a hand edit gives
// the mirror a `SKILL.md` the app cannot read. In the Sandbox's copy of the
// mirror, the probe plants a frontmatter that is not YAML in a skill the
// account syncs from Anthropic, a body that is not UTF-8 in another and in a
// user's skill, and a folder the manifest does not list, then starts the
// app and reads each after its next sync round.
await signedInOnly("locations[mirrored-skills]", "after its next sync round, the app keeps a `SKILL.md` whose frontmatter is not YAML, or whose body is not UTF-8, in a skill the account syncs, whether Anthropic's or a user's, its manifest still listing the skill on, and removes a folder the manifest does not list", async () => {
  const before = mirror();
  if (!before) return { pass: false, observed: "no mirror: nobody signed in" };
  const skillMd = (dir, name) => path.join(dir, "skills", name, "SKILL.md");
  const listed = json(path.join(before, "manifest.json")).skills.filter((e) => fs.existsSync(skillMd(before, e.name)));
  const anthropic = listed.filter((e) => e.creatorType === "anthropic");
  const user = listed.find((e) => e.creatorType === "user");
  const plants = [
    { label: "anthropic, not YAML", entry: anthropic[0], content: notYaml },
    { label: "anthropic, not UTF-8", entry: anthropic[1], content: anthropic[1] && notUtf8(anthropic[1].name) },
    { label: "user, not UTF-8", entry: user, content: user && notUtf8(user.name) },
  ];
  for (const plant of plants.filter((pl) => pl.entry)) {
    plant.original = fs.readFileSync(skillMd(before, plant.entry.name));
    write(skillMd(before, plant.entry.name), plant.content);
  }
  const unlisted = "cd-throwaway-unlisted";
  write(skillMd(before, unlisted), notYaml);

  const rounds = () => logLines(/\[SkillsPlugin\] Sync complete/).length;
  const earlier = rounds();
  packaged();
  const synced = await waitFor(async () => rounds() > earlier, 300);
  await sleep(20_000);
  await stop();

  const after = mirror() ?? before;
  const relisted = json(path.join(after, "manifest.json")).skills;
  const seen = { synced: !!synced };
  for (const plant of plants) {
    // An account without such a skill says so, and the check fails on it.
    if (!plant.entry) {
      seen[plant.label] = "the account syncs no such skill";
      continue;
    }
    const now = relisted.find((e) => e.name === plant.entry.name);
    seen[plant.label] = { file: plantedFate(skillMd(after, plant.entry.name), plant.content, plant.original), listed: !!now && now.enabled === plant.entry.enabled && now.skillId === plant.entry.skillId };
  }
  seen.unlisted = { file: plantedFate(skillMd(after, unlisted), notYaml, null), listed: relisted.some((e) => e.name === unlisted) };
  const expected = {
    synced: true,
    "anthropic, not YAML": { file: "kept", listed: true },
    "anthropic, not UTF-8": { file: "kept", listed: true },
    "user, not UTF-8": { file: "kept", listed: true },
    unlisted: { file: "removed", listed: false },
  };
  const skills = Object.fromEntries(plants.map((pl) => [pl.label, pl.entry?.name ?? null]));
  const log = logLines(/\[SkillsPlugin\]/).slice(-20);
  return { ...same(seen, expected), observed: JSON.stringify({ seen, skills, moved: after !== before, mirror: after, log }) };
});

// What each data folder holds of the app's own files, and whether the
// package moved its data to `Claude-Data`.
function folders() {
  const files = (dir) => (fs.existsSync(dir) && fs.statSync(dir).isDirectory() ? fs.readdirSync(dir).filter((f) => /config|Local State|Preferences|lockfile|user-data/.test(f)).sort() : null);
  const claudeData = path.join(localAppData, "Claude-Data");
  const data = fs.existsSync(claudeData) ? Object.fromEntries(fs.readdirSync(claudeData).map((f) => [f, files(path.join(claudeData, f))])) : null;
  return { classic: files(classicData), localCache: files(localCache), claudeData: data };
}

// The app names an entry it skips once: each launch gets new names.
await signedOut("(top)", "the packaged app, started beside a classic `%APPDATA%\\Claude`, reads the classic config and creates no `LocalCache`; once its `LocalCache` holds a config, it reads that one", async () => {
  write(path.join(classicData, "claude_desktop_config.json"), config("legacy"));
  const first = { ...(await started(packaged)), folders: folders() };
  write(path.join(classicData, "claude_desktop_config.json"), config("older"));
  write(path.join(localCache, "claude_desktop_config.json"), config("msix"));
  const both = { ...(await started(packaged)), folders: folders() };
  const seen = { first: first.tags, firstLocalCache: first.folders.localCache, both: both.tags };
  return { ...same(seen, { first: ["legacy"], firstLocalCache: null, both: ["msix"] }), observed: JSON.stringify({ first, both }) };
});

// An extension in the data folder `folder`, its id
// `local.mcpb.acme.cd-throwaway-<tag>`, its name `cd-throwaway-<name>` and
// its version `version`, switched on in its settings, whose server the app
// cannot start.
function extension(folder, tag, name, version) {
  const id = `local.mcpb.acme.cd-throwaway-${tag}`;
  write(path.join(folder, "Claude Extensions", id, "manifest.json"), {
    manifest_version: "0.2",
    name: `cd-throwaway-${name}`,
    version,
    server: { type: "binary", entry_point: "missing.exe", mcp_config: { command: "${__dirname}/missing.exe" } },
  });
  write(path.join(folder, "Claude Extensions Settings", `${id}.json`), { isEnabled: true });
}

// The lines of the app's logs that name a `cd-throwaway-ext*` extension.
const extensionLines = () => logLines(/cd-throwaway-ext/);

// The extensions those lines name, by the tag of their id or name, and the
// versions they name beside one.
function loggedExtensions(lines) {
  const tags = lines.flatMap((line) => line.match(/cd-throwaway-ext[a-z]+/g) ?? []).map((n) => n.slice("cd-throwaway-".length));
  const versions = lines.flatMap((line) => line.match(/\b[12]\.0\.0-(?:classic|cache)\b/g) ?? []);
  return { tags: [...new Set(tags)].sort(), versions: [...new Set(versions)].sort() };
}

// The folder roots of the overlay (#306): only the config file was tested
// before, and that a folder lists the entries of both by name is inferred
// from Microsoft's docs. The extension held in both has one id, so its copy
// is told by its name and its version. Signed out, as in Windows Sandbox,
// 3.6556.2 names no extension in its logs, so the check runs only with
// `--signed-in`, on an app someone signed in to.
await signedInOnly("homes[msix]", "the packaged app beside a classic `%APPDATA%\\Claude` sees an extension held only in the classic folder, one held only in its `LocalCache`, and the `LocalCache` copy of one held in both", async () => {
  extension(classicData, "extclassic", "extclassic", "1.0.0-classic");
  extension(localCache, "extcache", "extcache", "1.0.0-cache");
  extension(classicData, "extboth", "extbothclassic", "1.0.0-classic");
  extension(localCache, "extboth", "extbothcache", "2.0.0-cache");
  write(path.join(localCache, "claude_desktop_config.json"), config("overlay"));
  await stop();
  packaged();
  const seen = await waitFor(async () => {
    const logged = loggedExtensions(extensionLines());
    return logged.tags.includes("extclassic") && logged.tags.includes("extcache") ? logged : null;
  }, 120);
  // Whatever names the copy of the one held in both comes a moment later.
  await sleep(15_000);
  const lines = extensionLines();
  const logged = loggedExtensions(lines);
  await stop();
  const both = logged.tags.includes("extbothcache") || logged.versions.includes("2.0.0-cache");
  const classicBoth = logged.tags.includes("extbothclassic");
  const result = { classic: logged.tags.includes("extclassic"), cache: logged.tags.includes("extcache"), both, classicBoth };
  return { ...same(result, { classic: true, cache: true, both: true, classicBoth: false }), observed: JSON.stringify({ seen: !!seen, logged, lines: lines.slice(-40), folders: folders() }) };
});

// UAT step 7.6 starts the packaged app from its folder when no classic app
// is installed.
await signedOut("(top)", "the packaged app started from its folder with `--user-data-dir=<folder>` reads that folder, and each of its processes names it there, quoted, the main one too", async () => {
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
const answer = Number(await powershell("[Environment]::OSVersion.Version.Build")) < 22000 ? "userDataFolderAnswer" : "userDataOutsidePackageAnswer";

await signedOut("(top)", "the packaged app with no classic folder keeps its data in its `LocalCache` over several launches and writes no `Claude-Data\\<family>`", async () => {
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
await signedOut("(top)", "with the move's answer latched true in its config, the packaged app moves its data folder to `%LOCALAPPDATA%\\Claude-Data\\<family>` in lower case, beside a `README.txt`, leaves `.user-data-left.<family>` in its `LocalCache`, and reads and names the moved folder from then on", async () => {
  write(path.join(localCache, "claude_desktop_config.json"), { ...config("tomove"), preferences: { [answer]: true } });
  const launches = [];
  for (let i = 0; i < 3 && !fs.existsSync(path.join(movedData(), "claude_desktop_config.json")); i++) {
    launches.push({ ...(await started(packaged)), folders: folders() });
  }
  const copied = fs.existsSync(path.join(movedData(), "claude_desktop_config.json"));
  if (copied) write(path.join(movedData(), "claude_desktop_config.json"), { ...config("moved"), preferences: { [answer]: true } });
  moved = copied ? { ...(await started(packaged)), folders: folders() } : null;
  const left = `.user-data-left.${family.toLowerCase()}`;
  const seen = {
    copied,
    readme: fs.existsSync(path.join(localAppData, "Claude-Data", "README.txt")),
    left: fs.existsSync(path.join(localCache, left)),
    tags: moved?.tags ?? null,
    dir: moved?.dir ?? null,
  };
  return { ...same(seen, { copied: true, readme: true, left: true, tags: ["moved"], dir: movedData() }), observed: JSON.stringify({ answer, launches, moved }) };
});

await signedOut("(top)", "the packaged app reads its moved data folder before a classic `%APPDATA%\\Claude` beside it", async () => {
  if (!moved) return { pass: false, observed: "the app never moved its data folder" };
  write(path.join(classicData, "claude_desktop_config.json"), config("besideclassic"));
  write(path.join(movedData(), "claude_desktop_config.json"), { ...config("besidemoved"), preferences: { [answer]: true } });
  const seen = await started(packaged);
  return { ...same({ tags: seen.tags, dir: seen.dir }, { tags: ["besidemoved"], dir: movedData() }), observed: JSON.stringify({ ...seen, folders: folders() }) };
});

await stop();
p.finish();
