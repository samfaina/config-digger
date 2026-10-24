// Tests how Orca copies `~\.codex` into its Codex home,
// `%APPDATA%\orca\codex-runtime-home\home`, which `codex.toml` reads as a
// Mirrored home. That is Orca's behaviour, not Codex's, so the probe runs
// Orca's own code: the main-process modules an Orca install unpacks, loaded
// in Node with `USERPROFILE` and `ORCA_USER_DATA_PATH` in throwaway folders,
// so that `~\.codex` and Orca's data folder are throwaway too. Each launch
// runs what Orca runs before it starts a Codex on that home; when Orca
// starts one there is read in its bundle, not tested.
//
//   node uat/agents/orca-codex.mjs [--orca <Orca's install folder>]
//
// Driving the Orca that is open is not safe: it copies from the `~\.codex`
// of the user it runs as, whatever `CODEX_HOME` says, and writes settings
// back into it. A second Orca on throwaway folders was not tried: it starts
// a Codex on this home only for a launch whose `CODEX_HOME` is not
// `~\.codex`, which would have to be driven in its window. What Orca's home
// holds is read from its files.
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { fresh, probe, same, skill, write } from "./lib.mjs";

const install = process.argv.includes("--orca") ? process.argv[process.argv.indexOf("--orca") + 1] : path.join(process.env.LOCALAPPDATA, "Programs", "orca");
const asar = path.join(install, "resources", "app.asar");
const chunks = path.join(install, "resources", "app.asar.unpacked", "out", "main", "chunks");

// A file of an asar archive: a header, the JSON index of its files, then
// their contents.
function asarFile(archive, name) {
  const fd = fs.openSync(archive, "r");
  try {
    const head = Buffer.alloc(16);
    fs.readSync(fd, head, 0, 16, 0);
    const index = Buffer.alloc(head.readUInt32LE(12));
    fs.readSync(fd, index, 0, index.length, 16);
    const file = JSON.parse(index.toString("utf8")).files[name];
    const content = Buffer.alloc(file.size);
    fs.readSync(fd, content, 0, file.size, 8 + head.readUInt32LE(4) + Number(file.offset));
    return content.toString("utf8");
  } finally {
    fs.closeSync(fd);
  }
}

// The chunk whose file name starts with `prefix`, as Orca's bundler names
// it, `<name>-<hash>.js`.
function chunk(prefix) {
  const file = fs.readdirSync(chunks).find((f) => f.startsWith(`${prefix}-`) && f.endsWith(".js"));
  if (!file) throw new Error(`no ${prefix} chunk in ${chunks}`);
  return createRequire(import.meta.url)(path.join(chunks, file));
}

const version = JSON.parse(asarFile(asar, "package.json")).version;
const p = probe("Orca", version);
const base = fresh("orca-codex");

// The exports as Orca 1.4.239's bundler names them; another version may
// name them otherwise, which `setup` catches through the paths they give.
// `codex-home-paths` holds the homes and the links between them;
// `managed-agent-hook-controls` holds, among Orca's hook code, the merge of
// `config.toml`.
const homes = chunk("codex-home-paths");
const hookControls = chunk("managed-agent-hook-controls");
const systemHome = () => homes.a();
const orcaHome = () => homes.o();
const linkSharedFolders = () => homes.c();
const mergeConfig = () => hookControls.X();

// A throwaway user for one check: their home folder, Orca's data folder,
// and a `CODEX_HOME` of their own, which Orca must not copy from.
function setup(name) {
  const dir = fresh(name, base);
  const user = { home: fresh("user", dir), data: fresh("orca-data", dir), codexHome: fresh("codex-home", dir) };
  process.env.USERPROFILE = user.home;
  process.env.HOME = user.home;
  process.env.ORCA_USER_DATA_PATH = user.data;
  process.env.CODEX_HOME = user.codexHome;
  user.codex = path.join(user.home, ".codex");
  user.orca = path.join(user.data, "codex-runtime-home", "home");
  if (systemHome() !== user.codex || orcaHome() !== user.orca) {
    throw new Error(`Orca ${version} names its exports otherwise: ${systemHome()}, ${orcaHome()}`);
  }
  user.config = path.join(user.codex, "config.toml");
  user.copy = path.join(user.orca, "config.toml");
  return user;
}

// What Orca 1.4.239 runs before it starts a Codex on its home: it links
// `~\.codex`'s folders into it, then merges `~\.codex\config.toml` with it.
function launch() {
  linkSharedFolders();
  mergeConfig();
}

const stdio = (name, tag = name) => `[mcp_servers.${name}]\ncommand = "cmd"\nargs = ["/c", "${tag}"]\n`;
const read = (file) => (fs.existsSync(file) ? fs.readFileSync(file, "utf8") : null);

// The servers a `config.toml` declares, by name.
function servers(file) {
  return [...(read(file) ?? "").matchAll(/^\[mcp_servers\.([^\].]+)\]/gm)].map((m) => m[1]).sort();
}

// ---------------------------------------------------------------- config.toml

await p.check("mirrored_homes[orca]", "Orca copies `~\\.codex\\config.toml` into its home when it prepares a Codex launch, never from `CODEX_HOME`", async () => {
  const user = setup("source");
  write(user.config, stdio("cd-throwaway-user"));
  write(path.join(user.codexHome, "config.toml"), stdio("cd-throwaway-codex-home"));
  const before = read(user.copy);
  launch();
  return same({ before, after: servers(user.copy) }, { before: null, after: ["cd-throwaway-user"] });
});

await p.check("mirrored_homes[orca]", "Orca's copy of `config.toml` follows a change to `~\\.codex\\config.toml` at the next launch, not before", async () => {
  const user = setup("follows");
  write(user.config, stdio("cd-throwaway-user"));
  launch();
  write(user.config, stdio("cd-throwaway-user") + stdio("cd-throwaway-later"));
  const between = servers(user.copy);
  launch();
  return same({ between, next: servers(user.copy) }, { between: ["cd-throwaway-user"], next: ["cd-throwaway-later", "cd-throwaway-user"] });
});

// What config-digger's Disable and Remove of an Entry of Orca's home write,
// in the Entry it was copied from.
await p.check("mirrored_homes[orca]", "a server Disabled or Removed in `~\\.codex\\config.toml` is Disabled or gone in Orca's copy at the next launch", async () => {
  const user = setup("disable");
  write(user.config, stdio("cd-throwaway-off") + stdio("cd-throwaway-gone"));
  launch();
  write(user.config, `${stdio("cd-throwaway-off")}enabled = false\n`);
  launch();
  return same({ servers: servers(user.copy), off: /^\[mcp_servers\.cd-throwaway-off\]\n(?:(?!\n\[).)*\nenabled = false$/ms.test(read(user.copy)) }, { servers: ["cd-throwaway-off"], off: true });
});

await p.check("mirrored_homes[orca]", "a server only Orca's home holds stays in it across launches, and Orca never copies it to `~\\.codex`", async () => {
  const user = setup("orca-only");
  write(user.config, stdio("cd-throwaway-user"));
  launch();
  write(user.copy, `${read(user.copy)}\n${stdio("cd-throwaway-orca-only")}`);
  launch();
  return same({ orca: servers(user.copy), user: servers(user.config) }, { orca: ["cd-throwaway-orca-only", "cd-throwaway-user"], user: ["cd-throwaway-user"] });
});

// The other way: Orca keeps a baseline of a few settings, `model` among
// them, in `.orca-config-settings-baseline.json`, and of the plugin tables.
await p.check("mirrored_homes[orca]", "Orca writes back to `~\\.codex\\config.toml` a setting such as `model` changed in its home since the last launch, and a plugin table added there", async () => {
  const user = setup("write-back");
  write(user.config, `model = "cd-throwaway-first"\n\n${stdio("cd-throwaway-user")}`);
  launch();
  write(user.copy, `${read(user.copy).replace("cd-throwaway-first", "cd-throwaway-second")}\n[plugins."cd-throwaway-p@cd-throwaway-m"]\nenabled = true\n`);
  launch();
  const source = read(user.config);
  return same(
    { model: source.match(/^model = "(.*)"$/m)?.[1], plugin: source.includes('[plugins."cd-throwaway-p@cd-throwaway-m"]'), servers: servers(user.config) },
    { model: "cd-throwaway-second", plugin: true, servers: ["cd-throwaway-user"] },
  );
});

// ---------------------------------------------------------------- folders

await p.check("mirrored_homes[orca]", "Orca links each folder of `~\\.codex` it shares into its home, and none `~\\.codex` lacks, so a skill added after the launch is in its home at once", async () => {
  const user = setup("folders");
  const shared = ["skills", "plugins", "hooks", "plugin-state", "profile-v2", "prompts"];
  for (const folder of shared) fs.mkdirSync(path.join(user.codex, folder), { recursive: true });
  launch();
  const linked = fs.readdirSync(user.orca).filter((f) => {
    const target = path.join(user.orca, f);
    return fs.lstatSync(target).isSymbolicLink() && fs.realpathSync(target) === fs.realpathSync(path.join(user.codex, f));
  });
  skill(path.join(user.codex, "skills", "cd-throwaway-later"), ["name: cd-throwaway-later", "description: d-cd-throwaway-later"]);
  const later = fs.existsSync(path.join(user.orca, "skills", "cd-throwaway-later", "SKILL.md"));
  return same({ linked: linked.sort(), themes: fs.existsSync(path.join(user.orca, "themes")), later }, { linked: [...shared].sort(), themes: false, later: true });
});

// A link to a file needs Developer Mode or an administrator on Windows; when
// Orca cannot make one, it copies the file and records the copy.
await p.check("mirrored_homes[orca]", "Orca links `~\\.codex\\AGENTS.md` into its home, or copies it when Windows refuses the link, and a copy follows a change at the next launch", async () => {
  const user = setup("agents-md");
  const source = path.join(user.codex, "AGENTS.md");
  const copy = path.join(user.orca, "AGENTS.md");
  write(source, "cd-throwaway-first\n");
  launch();
  const kind = fs.lstatSync(copy).isSymbolicLink() ? "link" : fs.existsSync(path.join(user.orca, ".orca-resource-copies", "AGENTS.md.json")) ? "copy" : "other";
  write(source, "cd-throwaway-second\n");
  const between = read(copy);
  launch();
  const seen = { kind, between, next: read(copy) };
  return { pass: kind !== "other" && same(seen, { kind, between: kind === "link" ? "cd-throwaway-second\n" : "cd-throwaway-first\n", next: "cd-throwaway-second\n" }).pass, observed: JSON.stringify(seen) };
});

p.finish();
