// The tester's own data the UAT tools must never touch: config-digger's real
// data folders, the configuration files of the Agents the steps open, and the
// checkout's working tree. A snapshot before a walk is compared with one
// after. Claude Code's files are left out: an agent running the walk writes
// them itself.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

// The hash of each file of the tester's data, by path, under the APPDATA,
// LOCALAPPDATA and USERPROFILE of `env`.
export function realData(env) {
  const files = [];
  const walkDir = (dir) => {
    if (!existsSync(dir)) return;
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) walkDir(path);
      else files.push(path);
    }
  };
  walkDir(join(env.APPDATA, "config-digger"));
  walkDir(join(env.LOCALAPPDATA, "config-digger"));
  files.push(join(env.USERPROFILE, ".codex", "config.toml"), join(env.APPDATA, "Claude", "claude_desktop_config.json"));
  const packages = join(env.LOCALAPPDATA, "Packages");
  if (existsSync(packages)) {
    for (const name of readdirSync(packages).filter((n) => n.startsWith("Claude_"))) {
      files.push(join(packages, name, "LocalCache", "Roaming", "Claude", "claude_desktop_config.json"));
    }
  }
  return new Map(files.filter((f) => existsSync(f)).map((f) => [f, hash(f)]));
}

// The keys of two maps of hashes, such as two of `realData`, whose hash
// differs: changed, added or gone.
export const changedFiles = (before, after) => [...new Set([...before.keys(), ...after.keys()])].filter((f) => before.get(f) !== after.get(f));

// The tester's data and the checkout's working tree, as JSON can hold them:
// each line of its `git status`, with a hash of the file it names, so that a
// file changed before the run and changed again shows too.
export function snapshot({ env, checkout }) {
  return { files: Object.fromEntries(realData(env)), checkout, worktree: worktree(checkout) };
}

// Each line of `git status --porcelain`, as "XY path", with the hash of that
// file, or null when it is gone. With -z a rename's entry is followed by the
// path it had, which is skipped.
function worktree(checkout) {
  const entries = execFileSync("git", ["-C", checkout, "status", "--porcelain", "-z", "--untracked-files=all"], { encoding: "utf8" }).split("\0");
  const lines = {};
  for (let i = 0; i < entries.length; i++) {
    const entry = entries[i];
    if (!entry) continue;
    if (/[RC]/.test(entry.slice(0, 2))) i++;
    const file = join(checkout, entry.slice(3));
    lines[entry] = existsSync(file) && statSync(file).isFile() ? hash(file) : null;
  }
  return lines;
}

const hash = (file) => createHash("sha256").update(readFileSync(file)).digest("hex");

// What changed between two snapshots: each file of the tester's data, and the
// checkout with each line of its `git status` that is new, gone, or names a
// file that changed.
export function changed(before, after) {
  const files = changedFiles(new Map(Object.entries(before.files)), new Map(Object.entries(after.files)));
  const lines = changedFiles(new Map(Object.entries(before.worktree)), new Map(Object.entries(after.worktree)));
  return lines.length ? [...files, `the working tree of ${after.checkout}: ${lines.join(", ")}`] : files;
}

// What run.ps1 runs before and after its tools:
//
//   node uat/auto/untouched.mjs snapshot --checkout <dir> --out <file>
//   node uat/auto/untouched.mjs check --before <file>
//
// `check` names what it compared, or exits with 2 naming what changed.
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { positionals, values: opt } = parseArgs({
    allowPositionals: true,
    options: { checkout: { type: "string" }, out: { type: "string" }, before: { type: "string" } },
  });
  const [mode] = positionals;
  if (mode === "snapshot") writeFileSync(opt.out, JSON.stringify(snapshot({ env: process.env, checkout: opt.checkout }), null, 2) + "\n");
  else if (mode === "check") {
    const before = JSON.parse(readFileSync(opt.before, "utf8"));
    const changes = changed(before, snapshot({ env: process.env, checkout: before.checkout }));
    if (changes.length) {
      console.error(`The run changed the tester's own data:\n${changes.join("\n")}`);
      process.exitCode = 2;
    } else {
      const files = Object.keys(before.files).length;
      console.log(`The tester's own data is as before the run: the ${files} files of config-digger's real data and the real Codex and Claude Desktop configuration, and the working tree of ${before.checkout}.`);
    }
  } else throw new Error("usage: untouched.mjs snapshot --checkout <dir> --out <file> | untouched.mjs check --before <file>");
}
