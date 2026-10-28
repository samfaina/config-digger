// Tests Gemini CLI's facts that `gemini-cli.toml` marked unverified. Its home
// is a throwaway `GEMINI_CLI_HOME` on the system drive, in the temp folder,
// and its Projects sit under the probe root on another drive, so that a rule
// for the home folder and one for a Project are on different drives. A
// session gets the fake key `not-a-real-secret` and a closed local port for
// its model, so no request leaves the machine.
//
//   node uat/agents/gemini-cli.mjs [--bin <gemini.cmd>]
//
// Whether an extension is on in a folder is read from `gemini extensions
// list`, its `Enabled (Workspace)` line.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fresh, json, probe, root, run, same, write } from "./lib.mjs";

const bin = process.argv.includes("--bin") ? process.argv[process.argv.indexOf("--bin") + 1] : "gemini.cmd";
const home = fresh("cd-throwaway-gemini-home", os.tmpdir());
if (path.parse(home).root.toLowerCase() === path.parse(root).root.toLowerCase()) {
  throw new Error(`the Gemini CLI home (${home}) and the probe root (${root}) must be on different drives`);
}
const env = { GEMINI_CLI_HOME: home, GEMINI_API_KEY: "not-a-real-secret", GOOGLE_GEMINI_BASE_URL: "http://127.0.0.1:9" };
const gemini = (cwd, ...args) => run(bin, args, { cwd, env });
const version = (await gemini(home, "--version")).stdout.trim();
const p = probe("Gemini CLI", version);
const base = fresh("gemini-cli");

write(path.join(home, ".gemini", "settings.json"), { security: { folderTrust: { enabled: false }, auth: { selectedType: "gemini-api-key" } } });
const extensions = path.join(home, ".gemini", "extensions");
const enablement = path.join(extensions, "extension-enablement.json");
const sources = path.join(base, "sources");
for (const name of ["cd-throwaway-e1", "cd-throwaway-e2", "cd-throwaway-e3"]) {
  write(path.join(sources, name, "gemini-extension.json"), { name, version: "1.0.0", mcpServers: { [`${name}-srv`]: { command: "cmd", args: ["/c", name] } } });
  await gemini(sources, "extensions", "install", path.join(sources, name), "--consent");
}
const proj = path.join(base, "proj");
const deeper = path.join(proj, "deeper");
fs.mkdirSync(deeper, { recursive: true });

// Whether `name` is on in `cwd`.
async function on(cwd, name = "cd-throwaway-e1") {
  const { stdout, stderr } = await gemini(cwd, "extensions", "list");
  const block = (stdout + stderr).split(/\n(?=\S+ cd-throwaway-)/).find((b) => b.includes(`${name} (`));
  return block?.match(/Enabled \(Workspace\): (\w+)/)?.[1] === "true";
}
const rules = (name, overrides) => write(enablement, { ...json(enablement), [name]: { overrides } });

// ---------------------------------------------------------------- rules

await p.check("items.mcp.disable[1]", "a rule for a folder on another drive than the home folder's, or for that drive's root, reaches the folders below it", async () => {
  rules("cd-throwaway-e1", []);
  const seen = { "no rule": [await on(proj), await on(deeper)] };
  for (const rule of ["!/E:/cd-throwaway-agents/*", "!/E:/*"].map((r) => r.replace("E:", proj.slice(0, 2)).replace("cd-throwaway-agents", path.basename(root)))) {
    rules("cd-throwaway-e1", [rule]);
    seen[rule] = [await on(proj), await on(deeper)];
  }
  rules("cd-throwaway-e1", []);
  return same(Object.values(seen), [[true, true], [false, false], [false, false]]);
});

await p.check("items.mcp.disable[1]", "rules compare folders with regard to case, the drive letter included", async () => {
  const folder = `/${proj.replaceAll("\\", "/")}`;
  const spellings = {
    "as written": `!${folder}/*`,
    "lower-case drive": `!${folder[0]}${folder[1].toLowerCase()}${folder.slice(2)}/*`,
    "upper-case folder": `!${folder.toUpperCase()}/*`,
  };
  const off = {};
  for (const [label, rule] of Object.entries(spellings)) {
    rules("cd-throwaway-e1", [rule]);
    off[label] = !(await on(proj));
  }
  rules("cd-throwaway-e1", []);
  return same(off, { "as written": true, "lower-case drive": false, "upper-case folder": false });
});

await p.check("items.mcp.disable[1]", "Gemini CLI spells the drive letter as the folder it starts in is spelled: upper case from PowerShell, lower case after `cd /d e:\\` in cmd", async () => {
  const lower = proj[0].toLowerCase() + proj.slice(1);
  const fromFolder = path.join(base, "disable-from.cmd");
  write(fromFolder, '@cd /d "%~1" && "%~2" extensions disable cd-throwaway-e2 --scope workspace\r\n');
  await run(fromFolder, [lower, bin], { env, cwd: home });
  const fromCmd = json(enablement)["cd-throwaway-e2"].overrides.at(-1);
  rules("cd-throwaway-e2", []);
  await run("powershell.exe", ["-NoProfile", "-Command", `Set-Location '${lower}'; & '${bin}' extensions disable cd-throwaway-e2 --scope workspace`], { env, cwd: home });
  const fromPowerShell = json(enablement)["cd-throwaway-e2"].overrides.at(-1);
  rules("cd-throwaway-e2", []);
  return same({ fromCmd: fromCmd?.slice(1, 4), fromPowerShell: fromPowerShell?.slice(1, 4) }, { fromCmd: `/${lower.slice(0, 2)}`, fromPowerShell: `/${proj.slice(0, 2)}` });
});

// ---------------------------------------------------------------- manifests

await p.check("plugins[extensions]", "two extensions whose manifests give one `name` stop Gemini CLI from starting at all", async () => {
  const copy = path.join(extensions, "cd-throwaway-zdup");
  fs.cpSync(path.join(extensions, "cd-throwaway-e1"), copy, { recursive: true });
  const session = await gemini(proj, "-p", "hi");
  const list = await gemini(proj, "mcp", "list");
  fs.rmSync(copy, { recursive: true });
  const stopped = (r) => /critical error occurred.*Extension with name cd-throwaway-e1 already was loaded/s.test(r.stdout + r.stderr);
  return same({ session: stopped(session), mcpList: stopped(list) }, { session: true, mcpList: true });
});

await p.check("locations[extension-mcp]", "an extension manifest that does not parse skips that extension with a warning, and the others load", async () => {
  const file = path.join(extensions, "cd-throwaway-e2", "gemini-extension.json");
  const kept = fs.readFileSync(file, "utf8");
  write(file, '{ "name": broken');
  const { stdout, stderr } = await gemini(proj, "mcp", "list");
  write(file, kept);
  const out = stdout + stderr;
  return same({ warning: /Skipping extension in .*cd-throwaway-e2/.test(out), servers: (out.match(/cd-throwaway-e\d-srv/g) ?? []).sort() }, { warning: true, servers: ["cd-throwaway-e1-srv", "cd-throwaway-e3-srv"] });
});

// ---------------------------------------------------------------- skills

// Frontmatters that are not YAML, each in the folder `cd-throwaway-fm-<case>`
// of `~\.gemini\skills`.
const frontmatters = {
  colon: "name: cd-throwaway-fm-colon\ndescription: Use when: x\n---",
  stray: "name: cd-throwaway-fm-stray\njust text\ndescription: d-stray\n---",
  continued: "name: cd-throwaway-fm-continued\ndescription: Use when: one\n  two\n\n  three\n---",
  number: "name: 123\ndescription: d-number\n---",
  indented: "  name: cd-throwaway-fm-indented\ndescription: Use when: x\n---",
  twonames: "name: cd-throwaway-fm-first\nname: cd-throwaway-fm-second\ndescription: d-twonames\n---",
  quoted: "name: \"cd-throwaway-fm:quoted\"\ndescription: Use when: x\n---",
  noname: "description: Use when: x\n---",
  unclosed: "name: cd-throwaway-fm-unclosed\ndescription: Use when: x",
};
for (const [name, frontmatter] of Object.entries(frontmatters)) {
  write(path.join(home, ".gemini", "skills", `cd-throwaway-fm-${name}`, "SKILL.md"), `---\n${frontmatter}\nbody\n`);
}
// Each `cd-throwaway-fm-<case>` skill `gemini skills list` lists, as
// `{ name, description }`.
const listed = await (async () => {
  const { stdout, stderr } = await gemini(proj, "skills", "list");
  const blocks = [...(stdout + stderr).matchAll(/^(.*) \[\w+\]\r?\n\s*Description: (.*?)\r?\n\s*Location: .*\\cd-throwaway-fm-([a-z]+)\\SKILL\.md/gm)];
  return Object.fromEntries(blocks.map(([, name, description, folder]) => [folder, { name, description: description.trim() }]));
})();
const listedOf = (...folders) => Object.fromEntries(folders.map((folder) => [folder, listed[folder] ?? null]));

await p.check("items.skill", "a frontmatter YAML refuses is read line by line: `description: Use when: x`, a stray line and an indented `name` load the skill", async () => {
  return same(listedOf("colon", "stray", "indented"), {
    colon: { name: "cd-throwaway-fm-colon", description: "Use when: x" },
    stray: { name: "cd-throwaway-fm-stray", description: "d-stray" },
    indented: { name: "cd-throwaway-fm-indented", description: "Use when: x" },
  });
});
await p.check("items.skill", "read line by line, a `description` goes on over the indented lines up to a blank one, and a later `name` wins", async () => {
  return same(listedOf("continued", "twonames"), {
    continued: { name: "cd-throwaway-fm-continued", description: "Use when: one two" },
    twonames: { name: "cd-throwaway-fm-second", description: "d-twonames" },
  });
});
await p.check("items.skill", "`name: 123`, no string to YAML, is read line by line as `123`", async () => {
  return same(listedOf("number"), { number: { name: "123", description: "d-number" } });
});
await p.check("items.skill", "read line by line, a name keeps its quotes, and `path-chars` rewrites them", async () => {
  return same(listedOf("quoted"), { quoted: { name: "-cd-throwaway-fm-quoted-", description: "Use when: x" } });
});
await p.check("items.skill", "a frontmatter without a `name` line, or that never closes, skips the skill", async () => {
  return same(listedOf("noname", "unclosed"), { noname: null, unclosed: null });
});

p.finish();
