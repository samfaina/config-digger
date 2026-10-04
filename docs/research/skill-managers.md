# How skill managers lay out skills on disk

Research for [#14](https://github.com/samfaina/config-digger/issues/14). Researched 2026-10-01 on Windows 11 Pro 26200, Node 22.23.1.

Each fact carries one of three marks:

- **[disk]** verified on disk, either on this machine (read-only) or in a scratch home directory inside the worktree.
- **[source]** read in the source code or docs, not observed.
- **[contradicts]** the docs or a message in the tool say one thing, the code or the disk shows another.

`npx skills` means the `skills` npm package, version 1.7.0, from [vercel-labs/skills](https://github.com/vercel-labs/skills) at tag `v1.7.0` (commit `7407f38`). Source links below point at that tag.

## Summary

- `npx skills` keeps one canonical copy per skill in `.agents/skills/<name>` (project Scope) or `~/.agents/skills/<name>` (user Scope), and links it into the folders of Agents that don't read `.agents/skills`. **[disk]**
- On Windows the "symlink" is a directory **junction** with an absolute target, never a true symlink. Junctions need neither admin rights nor Developer Mode. **[disk]** for the junction, **[source]** for the privilege claim.
- If the junction fails, the installer silently copies instead and prints "On Windows, enable Developer Mode for symlink support". Developer Mode has nothing to do with junctions, so that hint is wrong. **[contradicts]**
- Agents whose project folder is `.agents/skills` (Codex, Cursor, Gemini CLI, GitHub Copilot, OpenCode, Amp and others) get **no** Entry in their own user folder. A user-Scope install for Codex or Gemini CLI writes only `~/.agents/skills`, whatever the README's "Global Path" column says. **[disk]**
- When the selected Agents share a single target folder (for example only Claude Code), `skills` skips the canonical copy and writes a **real directory** straight into the Agent folder. **[disk]**
- There are two lock files with different schemas. User Scope uses `~/.agents/.skill-lock.json` (version 3; `$XDG_STATE_HOME/skills/.skill-lock.json` if that variable is set). Project Scope uses `<project>/skills-lock.json` (version 1). The README documents neither. **[disk]** + **[source]**
- `skills remove -g <name>` deletes a folder named `<name>` from **every** known Agent's user skills folder and from the canonical folder, whatever its Provenance. A hand-made `~/.claude/skills/handmade` was deleted in the scratch test. **[disk]**
- Other managers can be told apart on disk. skillfish puts `.skillfish.json` in each copy. openskills puts `.openskills.json` in each copy. Claude Code's claude.ai sync lives under `~/.claude/skills/synced/<org>_<account>/` with a `manifest.json`. graphify leaves `.graphify_version`. **[disk]** for skillfish, synced and graphify; **[source]** for openskills.

## 1. `npx skills`: which folders it writes

### Canonical folder and "universal" Agents

- The canonical folder is `join(base, '.agents', 'skills')`, where `base` is `os.homedir()` for user Scope and `cwd` for project Scope ([installer.ts L128-L131](https://github.com/vercel-labs/skills/blob/v1.7.0/src/installer.ts#L128-L131), [constants.ts](https://github.com/vercel-labs/skills/blob/v1.7.0/src/constants.ts)). **[source]**
- `skills` treats an Agent as universal when its project `skillsDir` is `.agents/skills` ([agents.ts L910](https://github.com/vercel-labs/skills/blob/v1.7.0/src/agents.ts#L910)). For universal Agents, `getAgentBaseDir` returns the canonical folder in **both** Scopes, and the installer returns early without linking into the Agent's `globalSkillsDir` ([installer.ts L151-L179](https://github.com/vercel-labs/skills/blob/v1.7.0/src/installer.ts#L151-L179), [L392-L401](https://github.com/vercel-labs/skills/blob/v1.7.0/src/installer.ts#L392-L401)). **[source]**
- In 1.7.0 the universal Agents are: Amp, Antigravity, Antigravity CLI, Cline, Codex, Cursor, Deep Agents, Dexto, Droid, Firebender, Gemini CLI, GitHub Copilot, Kilo Code, Kimi Code CLI, Loaf, OpenCode, PromptScript, Replit, Sarvam Code, Warp, Zed, plus the pseudo-Agent `universal` ([agents.ts](https://github.com/vercel-labs/skills/blob/v1.7.0/src/agents.ts)). **[source]**
- Scratch test: `skills add <local skill> -g -a claude-code codex gemini-cli cursor -y` with `USERPROFILE` pointing at a scratch home produced `~/.agents/skills/demo-skill` (real dir) and `~/.claude/skills/demo-skill` (junction). Nothing appeared under `~/.codex`, `~/.gemini` or `~/.cursor`. **[disk]**
- On this machine `~/.agents/.skill-lock.json` has `lastSelectedAgents` including `codex`, `gemini-cli`, `cursor` and `github-copilot`, but `~/.gemini/skills`, `~/.copilot/skills` and `$CODEX_HOME/skills` hold no `skills`-managed Entries. Only `~/.claude/skills` has junctions to the three Orca skills. **[disk]**
- The README's agent table lists a "Global Path" per Agent, e.g. `~/.codex/skills/` for Codex and `~/.gemini/skills/` for Gemini CLI ([README.md](https://github.com/vercel-labs/skills/blob/v1.7.0/README.md#supported-agents)). That column is the `globalSkillsDir` the tool *scans* (for `list` and `remove`). It is not where `add` writes for universal Agents. **[contradicts]**

### Agent-specific folders

- Non-universal Agents get a link at `<agent globalSkillsDir>/<name>` (user Scope) or `<cwd>/<agent skillsDir>/<name>` (project Scope). Examples from [agents.ts](https://github.com/vercel-labs/skills/blob/v1.7.0/src/agents.ts): Claude Code `.claude/skills` and `$CLAUDE_CONFIG_DIR/skills` or `~/.claude/skills`; Windsurf `.windsurf/skills`; Roo `.roo/skills`; Goose `.goose/skills` and `$XDG_CONFIG_HOME/goose/skills`. **[source]**
- `globalSkillsDir` honours env overrides read at module load: `CODEX_HOME`, `CLAUDE_CONFIG_DIR`, `VIBE_HOME`, `HERMES_HOME`, `AUTOHAND_HOME`, `GROK_HOME`, `SARVAM_HOME`, `XDG_CONFIG_HOME` (via `xdg-basedir`) and `APPDATA` for Zed detection ([agents.ts L7-L18](https://github.com/vercel-labs/skills/blob/v1.7.0/src/agents.ts#L7-L18)). On this machine `CODEX_HOME` is set (to an Orca path), so config-digger must resolve these variables the same way. The README's "Environment Variables" table lists none of them, nor `XDG_STATE_HOME` for the lock. **[source]** + **[disk]** for the env var.
- In project Scope, a non-universal Agent whose root folder (e.g. `.windsurf`) does not exist is skipped, unless the user named that Agent with `-a`, or the Agent sets `createProjectSkillsDirByDefault` (only Claude Code does) ([installer.ts L95-L112](https://github.com/vercel-labs/skills/blob/v1.7.0/src/installer.ts#L95-L112), [add.ts L1969](https://github.com/vercel-labs/skills/blob/v1.7.0/src/add.ts#L1969)). Scratch test: `-a claude-code codex windsurf` created `.windsurf/skills/find-skills` even though `.windsurf` did not exist. **[disk]**

### Folder names

- The folder name is `sanitizeName(name)`: lowercase, runs of characters outside `[a-z0-9._]` become `-`, leading and trailing `.`/`-` are stripped, max 255 chars ([installer.ts L61-L76](https://github.com/vercel-labs/skills/blob/v1.7.0/src/installer.ts#L61-L76)). Lock keys keep the original name, so a plugin skill `ce:review` lives in folder `ce-review` under lock key `ce:review` ([remove.ts L27-L39](https://github.com/vercel-labs/skills/blob/v1.7.0/src/remove.ts#L27-L39)). To match lock entries to folders, config-digger has to sanitize the key the same way. **[source]**
- The Agent Skills spec requires `name` to match the parent folder ([agentskills.io/specification](https://agentskills.io/specification)). `sanitizeName` allows `.` and `_`, which the spec forbids, so a lenient folder name can still come out of `skills`. **[source]**
- The spec says nothing about install locations, links or lock files. Those are all tool conventions. **[source]**

## 2. Copy or link

### Link mode (default)

1. `rm -rf` the canonical folder, then copy the skill into it ([installer.ts L388-L390](https://github.com/vercel-labs/skills/blob/v1.7.0/src/installer.ts#L388-L390)). **[source]**
2. Create the link at the Agent folder ([installer.ts L227-L292](https://github.com/vercel-labs/skills/blob/v1.7.0/src/installer.ts#L227-L292)). On `win32` it calls `fs.symlink(absoluteTarget, linkPath, 'junction')`. On other platforms it creates a relative symlink with no type. **[source]**

Verified on this machine:

```
C:\Users\kgon1\.claude\skills\computer-use   Junction  C:\Users\kgon1\.agents\skills\computer-use
C:\Users\kgon1\.claude\skills\orca-cli       Junction  C:\Users\kgon1\.agents\skills\orca-cli
C:\Users\kgon1\.claude\skills\orchestration  Junction  C:\Users\kgon1\.agents\skills\orchestration
```

`cmd /c dir /AL` shows the scratch install as `<JUNCTION>` with an absolute target. **[disk]** The README only ever says "symlinks" ([Installation Methods](https://github.com/vercel-labs/skills/blob/v1.7.0/README.md#installation-methods)), so on Windows the docs and the disk disagree on the link type. **[contradicts]**

Project-Scope installs also use absolute junction targets, e.g. `.claude/skills/find-skills -> E:\...\proj\.agents\skills\find-skills`. A junction inside a committed repo therefore points at one machine's path. **[disk]**

### Why junctions avoid Developer Mode

- Creating a true symlink needs `SeCreateSymbolicLinkPrivilege`, which by default only Administrators hold ([Microsoft: Create symbolic links](https://learn.microsoft.com/en-us/windows/security/threat-protection/security-policy-settings/create-symbolic-links)). Developer Mode is the other common way to lift that requirement. **[source]**
- Junctions are a separate reparse-point type. They link directories, including across local volumes, but cannot target mapped network drives ([Microsoft: Hard links and junctions](https://learn.microsoft.com/en-us/windows/win32/fileio/hard-links-and-junctions)). Node requires an absolute target for `'junction'` and normalizes it ([Node fs docs](https://nodejs.org/api/fs.html#fssymlinktarget-path-type-callback)). **[source]**
- This machine has Developer Mode on (`AllowDevelopmentWithoutDevLicense=1`) and the shell is not elevated, so I could not reproduce the no-Developer-Mode case directly. The junction itself was created unelevated. **[disk]** That junction creation also works with Developer Mode off is **[source]** only.

### Fallback to copy

- `createSymlink` wraps everything in `try/catch` and returns `false` on any error. The installer then `rm -rf`s the Agent folder and copies the skill there as a real directory, reporting `symlinkFailed: true` ([installer.ts L426-L439](https://github.com/vercel-labs/skills/blob/v1.7.0/src/installer.ts#L426-L439)). **[source]**
- The user then sees "Files were copied instead. On Windows, enable Developer Mode for symlink support." ([add.ts L2294](https://github.com/vercel-labs/skills/blob/v1.7.0/src/add.ts#L2294)). The code never asks for a true symlink on Windows, so Developer Mode would not change the outcome. Junction failures come from things like a network-share home or a locked folder. **[contradicts]**
- After a fallback, the canonical copy and the Agent copy are two real directories with identical content. Nothing on disk marks the Agent copy as fallback output. **[source]**

### Copy mode

- `--copy`, or a single unique target folder, gives copy mode. With `-y` and more than one target folder, link mode is used without asking ([add.ts L1776-L1811](https://github.com/vercel-labs/skills/blob/v1.7.0/src/add.ts#L1776-L1811)). **[source]**
- In copy mode non-universal Agents get a real directory and **no canonical copy is written** ([installer.ts L366-L376](https://github.com/vercel-labs/skills/blob/v1.7.0/src/installer.ts#L366-L376)). Scratch test: `skills add vercel-labs/skills --skill find-skills -g -a claude-code -y` produced a real `~/.claude/skills/find-skills` and nothing under `~/.agents/skills`, yet the lock still got an entry. **[disk]**
- Files are copied with `dereference: true`. `.git`, `__pycache__`, `__pypackages__` and `metadata.json` are skipped ([installer.ts L452-L548](https://github.com/vercel-labs/skills/blob/v1.7.0/src/installer.ts#L452-L548)). **[source]**

### How Node sees a junction

A scratch test in Node 22 (`fs.symlinkSync(target, link, 'junction')`):

- `lstat(link).isSymbolicLink() === true`, `isDirectory() === false`
- `Dirent.isDirectory() === false`, `Dirent.isSymbolicLink() === true`
- `readlink` returns the absolute target
- `fs.rm(link, {recursive: true, force: true})` removes the link and leaves the target's files intact

**[disk]**. This is Node's view. How Rust's `std::fs` classifies the same junction is listed under open questions.

## 3. Lock files

### User Scope: `~/.agents/.skill-lock.json`

- Path is `$XDG_STATE_HOME/skills/.skill-lock.json` if `XDG_STATE_HOME` is set, otherwise `~/.agents/.skill-lock.json` ([skill-lock.ts L62-L73](https://github.com/vercel-labs/skills/blob/v1.7.0/src/skill-lock.ts#L62-L73)). **[source]**; default path **[disk]**.
- Schema version 3 ([skill-lock.ts L13-L60](https://github.com/vercel-labs/skills/blob/v1.7.0/src/skill-lock.ts#L13-L60)):

  ```jsonc
  {
    "version": 3,
    "skills": {
      "<original skill name>": {
        "source": "stablyai/orca",            // owner/repo or normalized source
        "sourceType": "github",
        "sourceUrl": "https://github.com/stablyai/orca.git",
        "ref": "main",                         // optional
        "skillPath": "skills/orca-cli/SKILL.md", // optional
        "skillFolderHash": "<GitHub tree SHA>",  // may be ""
        "installedAt": "<ISO>",
        "updatedAt": "<ISO>",
        "pluginName": "...",                   // optional
        "sourceBaseUrl": "...",                // optional
        "wellKnownDigest": "..."               // optional
      }
    },
    "dismissed": { "findSkillsPrompt": true },
    "lastSelectedAgents": ["claude-code", "..."]
  }
  ```

  This machine's file matches the schema, with three `stablyai/orca` entries. **[disk]**
- The lock records **neither the target Agents nor link-vs-copy mode**. `lastSelectedAgents` is only the last prompt answer. To know which Agent folders hold a locked skill, config-digger must scan for them. **[source]** + **[disk]**
- `skills` writes this lock only for user-Scope installs with a remote source (`getOwnerRepo` returns `null` for local paths and direct downloads) ([add.ts L2086-L2127](https://github.com/vercel-labs/skills/blob/v1.7.0/src/add.ts#L2086-L2127), [source-parser.ts L11-L14](https://github.com/vercel-labs/skills/blob/v1.7.0/src/source-parser.ts#L11-L14)). Scratch test: a local-path user install left no lock file at all. **[disk]** Upstream [#399](https://github.com/vercel-labs/skills/issues/399), "Skills not tracked in lock file on Windows", is this same rule: the reporter did a project install, which goes to the project lock.
- A file with `version < 3` or no `skills` key reads as empty, and the next write overwrites it, silently dropping older entries ([skill-lock.ts L80-L103](https://github.com/vercel-labs/skills/blob/v1.7.0/src/skill-lock.ts#L80-L103)). **[source]**

### Project Scope: `<project>/skills-lock.json`

- Path is `join(cwd, 'skills-lock.json')`. The file is meant for version control, with keys sorted alphabetically and no timestamps ([local-lock.ts L5-L67](https://github.com/vercel-labs/skills/blob/v1.7.0/src/local-lock.ts#L5-L67)). **[source]**
- Schema version 1:

  ```jsonc
  {
    "version": 1,
    "skills": {
      "find-skills": {
        "source": "vercel-labs/skills",
        "sourceType": "github",
        "sourceUrl": "...",     // optional
        "ref": "...",           // optional
        "skillPath": "skills/find-skills/SKILL.md", // optional
        "computedHash": "<sha256 of relative paths + file bytes>",
        "subagents": [""],      // optional, Eve only
        "wellKnownDigest": "..." // optional
      }
    }
  }
  ```

  The scratch project install wrote exactly the four keys `source`, `sourceType`, `skillPath`, `computedHash`. **[disk]**
- For `sourceType: "local"` the `source` is stored relative to the project (`./...`), or absolute with forward slashes when it is on a different drive ([local-lock.ts L125-L138](https://github.com/vercel-labs/skills/blob/v1.7.0/src/local-lock.ts#L125-L138)). **[source]**
- `computedHash` hashes raw bytes, so CRLF checkouts on Windows give a different hash than Linux ([upstream #781](https://github.com/vercel-labs/skills/issues/781)). **[source]**
- `skills experimental_install` restores skills from this file into `.agents/skills/` ([install.ts](https://github.com/vercel-labs/skills/blob/v1.7.0/src/install.ts)). **[source]**

## 4. What `skills remove` deletes

From [remove.ts L62-L392](https://github.com/vercel-labs/skills/blob/v1.7.0/src/remove.ts#L62-L392):

1. **Scan.** User Scope reads `~/.agents/skills` plus every Agent's `globalSkillsDir`. Project Scope reads `.agents/skills` plus `<cwd>/<skillsDir>` for every Agent. A folder counts if it is a real directory holding `SKILL.md` and its name does not start with `.`. Junction entries are skipped here (`Dirent.isDirectory()` is false), but their canonical targets get picked up. **[source]**
2. **Delete Agent Entries.** Without `-a`, it targets **all** Agents in the table, not only detected ones. For each Agent it `rm -rf`s `getInstallPath(...)` and `<globalSkillsDir or skillsDir>/<sanitized name>` when they exist. Provenance is not checked. **[source]**
3. **Delete the canonical folder** unless an installed Agent outside the `-a` list still has the skill at its install path. **[source]**
4. **Update the lock.** Under the same condition it deletes the key from `~/.agents/.skill-lock.json` (user) or `skills-lock.json` (project). The file stays, possibly with `"skills": {}`. **[source]**

Scratch tests **[disk]**:

- `remove handmade -g -y` deleted the hand-made real directories `~/.claude/skills/handmade` and `~/.gemini/skills/handmade`. `skills` never installed either of them.
- `remove demo-skill find-skills -g -y` deleted the junction, the canonical folder and the copy-mode folder, and left `~/.agents/.skill-lock.json` as `{"version": 3, "skills": {}, "dismissed": {}}`.
- Project `remove find-skills -y` deleted the canonical folder and both junctions, and left `skills-lock.json` as `{"version": 1, "skills": {}}`. Empty `skills/` folders and `.windsurf/` were left behind.
- `rm` of a junction does not follow it (Node test above), so a junction pointing outside `.agents` (for example at a user's dev checkout) only loses the link.

For config-digger this means a `skills remove` call is not a safe way to Remove one Entry. It deletes every same-named Entry in every Agent's folder for that Scope.

## 5. Other skill managers

| Manager | Copy or link | Per-skill marker | Index file | Notes |
| --- | --- | --- | --- | --- |
| `skills` (vercel-labs) | junction on Windows, copy fallback, copy mode | none | `~/.agents/.skill-lock.json`, `skills-lock.json` | Canonical folder `.agents/skills` |
| [skillfish](https://github.com/knoxgraeme/skillfish) 1.0.39 | copy per Agent, skips symlinks in source | `.skillfish.json` in each copy | optional `skillfish.json` (project) or `~/skillfish.json` from `skillfish bundle` | **[disk]** + **[source]** |
| [openskills](https://github.com/numman-ali/openskills) 1.5.0 | copy (`cpSync`, dereference) | `.openskills.json` in each copy | none | Targets `.claude/skills` or `.agent/skills` (singular), project or `~` **[source]** |
| Claude Code claude.ai sync | written by Claude Code | `manifest.json` per account folder | `~/.claude/skills/synced/<org>_<account>/manifest.json` | **[disk]**; deleted skills land in `~/.claude/skills/.trash/<ts>-<pid>-<rand>/` |
| graphify | own installer | `.graphify_version` | none | **[disk]** only; installer not researched |
| Claude Code plugins | plugin cache | n/a | `~/.claude/plugins/installed_plugins.json` | Plugin Provenance, out of scope here. Skills live in `~/.claude/plugins/cache/<marketplace>/<plugin>/<version>/skills` **[disk]** |

### skillfish

- Writes `.skillfish.json` (schema version 2: `version`, `name`, `owner`, `repo`, `path`, `branch`, `sha`, optional `ref`, `source: "manual" | "manifest"`) into every installed skill folder ([manifest.ts](https://github.com/knoxgraeme/skillfish/blob/main/src/lib/manifest.ts)). **[source]**
- Agent folders come from a separate table with some differences from `skills`. For example Codex is `.codex/skills` (not `.agents/skills`), GitHub Copilot is `.github/skills` in a project and `~/.copilot/skills` in the user Scope, and Augment is `.augment/rules` (npm package `skillfish@1.0.39`, `dist/lib/agents.js`). **[source]**
- This machine has `jetpack-compose` as three identical real directories in `~/.claude/skills`, `~/.gemini/skills` and `~/.copilot/skills`, each with the same `.skillfish.json` (`owner: thebushidocollective`, `repo: han`, `source: "manual"`). **[disk]**
- `skillfish remove` runs `rmSync(path, {recursive: true})` on each matching folder in the selected Agents. It warns, without editing it, when `skillfish.json` still lists the skill ([remove.ts](https://github.com/knoxgraeme/skillfish/blob/main/src/commands/remove.ts)). **[source]**
- The GitHub repo's `package.json` says 1.0.31 while npm ships 1.0.39. I read the agent table from the npm tarball. **[source]**

### openskills

- Installs into `./.claude/skills` by default, `./.agent/skills` with `--universal`, or the same under `~` with `--global`. Note `.agent`, not `.agents` ([install.ts L84-L88](https://github.com/numman-ali/openskills/blob/main/src/commands/install.ts#L84-L88), [dirs.ts](https://github.com/numman-ali/openskills/blob/main/src/utils/dirs.ts)). **[source]**
- Writes `.openskills.json` (`source`, `sourceType`, `repoUrl`, `subpath`, `localPath`, `installedAt`) into each copy ([skill-metadata.ts](https://github.com/numman-ali/openskills/blob/main/src/utils/skill-metadata.ts)). **[source]**
- Not present on this machine.

### Telling them apart

In priority order, a skill folder's manager Provenance can be inferred from:

1. It is a junction or symlink into `.agents/skills` → `skills` (link mode).
2. It contains `.skillfish.json` → skillfish. It contains `.openskills.json` → openskills.
3. It sits under `~/.claude/skills/synced/` → claude.ai sync (managed/cloud Provenance).
4. It sits in `.agents/skills`, or its sanitized name matches a key in the lock for that Scope → `skills` (canonical copy, copy mode or copy fallback). This is the weakest signal. A hand-made folder with the same name as a locked skill looks identical.
5. Anything else → file Entry.

## Implications for config-digger

- Treat a junction and its canonical folder as one Item with two Entries. The junction's Agent Entry is Effective for that Agent. Disabling it means removing the junction, not the target.
- Codex, Gemini CLI, Cursor, Copilot and the other universal Agents read `~/.agents/skills` themselves, so a `skills` user install is one Entry visible to all of them. Their own user folders (`~/.gemini/skills`, `$CODEX_HOME/skills`, ...) can still hold other managers' copies, which may be Shadowed by or shadow the `.agents` one depending on each Agent's precedence. That precedence is not part of this ticket.
- Do not shell out to `skills remove` to Remove a single Entry. It sweeps every Agent folder. Deleting the link or folder directly and then editing the right lock file gives the same lock state.
- Read env overrides (`CODEX_HOME`, `CLAUDE_CONFIG_DIR`, `XDG_STATE_HOME`, `XDG_CONFIG_HOME`, ...) the same way `skills` does, or the lock and Agent folders will be looked up in the wrong place.

## Sources

- npm registry: `npm view skills` → repository `git+https://github.com/vercel-labs/skills.git`, version 1.7.0, tarball `https://registry.npmjs.org/skills/-/skills-1.7.0.tgz`
- vercel-labs/skills at `v1.7.0`: [README.md](https://github.com/vercel-labs/skills/blob/v1.7.0/README.md), [src/installer.ts](https://github.com/vercel-labs/skills/blob/v1.7.0/src/installer.ts), [src/agents.ts](https://github.com/vercel-labs/skills/blob/v1.7.0/src/agents.ts), [src/add.ts](https://github.com/vercel-labs/skills/blob/v1.7.0/src/add.ts), [src/remove.ts](https://github.com/vercel-labs/skills/blob/v1.7.0/src/remove.ts), [src/skill-lock.ts](https://github.com/vercel-labs/skills/blob/v1.7.0/src/skill-lock.ts), [src/local-lock.ts](https://github.com/vercel-labs/skills/blob/v1.7.0/src/local-lock.ts), [src/install.ts](https://github.com/vercel-labs/skills/blob/v1.7.0/src/install.ts), [src/source-parser.ts](https://github.com/vercel-labs/skills/blob/v1.7.0/src/source-parser.ts), [src/constants.ts](https://github.com/vercel-labs/skills/blob/v1.7.0/src/constants.ts)
- Upstream issues: [#399](https://github.com/vercel-labs/skills/issues/399), [#781](https://github.com/vercel-labs/skills/issues/781)
- Agent Skills spec: [agentskills.io/specification](https://agentskills.io/specification)
- skillfish: [knoxgraeme/skillfish](https://github.com/knoxgraeme/skillfish) (`main` at `b34c068`), npm `skillfish@1.0.39`
- openskills: [numman-ali/openskills](https://github.com/numman-ali/openskills) (`main` at `57d933a`), npm `openskills@1.5.0`
- Microsoft: [Hard links and junctions](https://learn.microsoft.com/en-us/windows/win32/fileio/hard-links-and-junctions), [Create symbolic links](https://learn.microsoft.com/en-us/windows/security/threat-protection/security-policy-settings/create-symbolic-links)
- Node.js: [fs.symlink](https://nodejs.org/api/fs.html#fssymlinktarget-path-type-callback)
- On-disk checks (read-only) on this machine: `C:\Users\kgon1\.agents\skills`, `C:\Users\kgon1\.agents\.skill-lock.json`, `C:\Users\kgon1\.claude\skills`, `C:\Users\kgon1\.gemini\skills`, `C:\Users\kgon1\.copilot\skills`, `C:\Users\kgon1\.claude\plugins\cache`, `HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock`
- Scratch runs: `skills@1.7.0` with `USERPROFILE`/`HOME`/`APPDATA`/`LOCALAPPDATA` pointing at a scratch home inside the worktree and `CODEX_HOME`, `CLAUDE_CONFIG_DIR` and `XDG_*` unset (scratch not committed)

## Open questions

- Does junction creation really succeed with Developer Mode **off** and no admin? Every source says yes, but this machine has Developer Mode on. Needs a run on a clean Windows account or VM.
- What makes the copy fallback fire in practice on Windows? Candidates: home on a network share or redirected folder, OneDrive-redirected folders, antivirus locks. Not reproduced.
- How does Rust's `std::fs::symlink_metadata` / `FileType` classify junctions on the toolchain config-digger uses? It decides whether the scanner sees a link or a directory.
- Each universal Agent's precedence between `~/.agents/skills`, its own user folder and project folders (needed for Effective/Shadowed). Belongs to the per-Agent research tickets.
- How does Claude Code treat `~/.claude/skills/synced/` and `.trash/`? This session shows synced skills with an `anthropic-skills:` prefix, so a synced `unslop` and a local `~/.claude/skills/unslop` appear side by side rather than one shadowing the other. Needs Claude Code docs.
- Does git on Windows handle a committed project folder that contains junctions with absolute targets? `skills` creates them in project Scope.
- Are there other skill managers with real adoption (Python, Go, or editor-bundled ones)? I only checked the ones found on this machine plus openskills.
