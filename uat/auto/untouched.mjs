// The tester's own data the UAT tools must never touch: config-digger's real
// data folders, the configuration files of the Agents the steps open, and the
// checkout's working tree. A snapshot before a walk is compared with one
// after. Claude Code's files are left out: an agent running the walk writes
// them itself.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

// A hash of each file of the tester's data, by path, under the APPDATA,
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
  return new Map(files.filter((f) => existsSync(f)).map((f) => [f, createHash("sha256").update(readFileSync(f)).digest("hex")]));
}

// The files of `realData` that differ between two of its maps: changed,
// added or gone.
export const changedFiles = (before, after) => [...new Set([...before.keys(), ...after.keys()])].filter((f) => before.get(f) !== after.get(f));

// The tester's data and the checkout's `git status`, as JSON can hold them.
export function snapshot({ env, checkout }) {
  const status = execFileSync("git", ["-C", checkout, "status", "--porcelain", "--untracked-files=all"], { encoding: "utf8" });
  return { files: Object.fromEntries(realData(env)), checkout, status };
}

// What changed between two snapshots: each file, and the checkout with the
// lines of its `git status` that are new.
export function changed(before, after) {
  const files = changedFiles(new Map(Object.entries(before.files)), new Map(Object.entries(after.files)));
  const was = new Set(before.status.split("\n"));
  const lines = after.status.split("\n").filter((line) => line && !was.has(line));
  return lines.length ? [...files, `the working tree of ${after.checkout}: ${lines.join(", ")}`] : files;
}

// What run.ps1 runs before and after its tools:
//
//   node uat/auto/untouched.mjs snapshot --checkout <dir> --out <file>
//   node uat/auto/untouched.mjs check --before <file> --where "<what the run may write>"
//
// `check` exits with 2 when something changed, naming it.
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { positionals, values: opt } = parseArgs({
    allowPositionals: true,
    options: { checkout: { type: "string" }, out: { type: "string" }, before: { type: "string" }, where: { type: "string" } },
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
      console.log(`Nothing outside ${opt.where} changed: ${files} files of config-digger's real data and the Codex and Claude Desktop configuration compared, and the working tree of ${before.checkout}.`);
    }
  } else throw new Error('usage: untouched.mjs snapshot --checkout <dir> --out <file> | untouched.mjs check --before <file> --where "<text>"');
}
