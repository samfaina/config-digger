# config-digger

config-digger inspects the effective configuration of AI coding agents — which MCP servers and skills are configured, where each one comes from, and which one actually wins — and removes or disables them according to how they were installed.

## Language

### Agents

**Agent**:
An AI coding tool whose configuration config-digger reads, such as Claude Code, Cursor or Codex.
_Avoid_: Client, host, tool

**Agent definition**:
A data file that describes where an Agent keeps its configuration, in which format, and with what precedence. It is assembled from building blocks config-digger already implements, so supporting a new Agent means writing an Agent definition, not code, unless the Agent needs a building block that does not exist yet. Agent definitions ship with the app.
_Avoid_: Adapter, plugin, driver

**Skill manager definition**:
A data file that describes how a skill manager, such as `npx skills`, lays out the skills it installs, so that the Provenance of a skill Entry can be recognised in any Agent.
_Avoid_: Installer definition, package manager

**Agent home**:
The root folder from which one instance of an Agent reads its user-level configuration. An Agent can have several (another `CODEX_HOME`, a VS Code profile, Claude Desktop's Store and classic folders); Entries in different Agent homes never compete. A home whose folder the Agent's lookup order skips is still shown, as not read.
_Avoid_: Profile, instance, installation

**Installation**:
A launchable copy of an Agent, with its own version, such as the native Claude Code, the Claude Code that Claude Desktop downloads, or a VS Code user setup. Several Installations can read the same Agent home, and one Installation can read several. Older version files an installer keeps around are not Installations.
_Avoid_: Install, binary, copy

**Tested range**:
The lowest and highest versions of an Agent that its Agent definition was tested against. An Installation outside it, older or newer, is noted on each Agent home it reads and in the preview of every Operation on that Agent; nothing is turned off. A definition without one is untested.
_Avoid_: Supported versions, compatible range, verified range

**Unverified mark**:
A sentence in an Agent definition or skill manager definition saying what one of its facts assumes, for a fact taken from inference or source code alone, or a default taken because the fact is not known. It changes nothing config-digger does; the marks of the built-in definitions list what the release gate still has to test. A mark whose test needs an account or a deployment the project does not have is under the exception: it says what the test needs, it lets a release ship, and the release notes list it. Any other mark stops a release.
_Avoid_: Assumption, caveat, TODO

**Running Agent**:
An Agent that config-digger detects, on a best-effort basis, as running for an Agent home that reads a location an Operation writes, or a skill folder it moves, or keeps a Disable switch there. It is matched through the `[process]` section of its Agent definition, which an Agent home may replace with its own. An Agent that picks its home by lookup order, such as Claude Desktop, never runs for an Agent home the user added outside the homes of its definition, since no launch reads it; one added in the folder of a definition's home, however spelled, is that home. config-digger never closes an Agent; what a Running Agent means for a write depends on the location's Store class.
_Avoid_: Open Agent, live Agent, active Agent

**Feature gate**:
A switch on the Agent vendor's side that changes how the Agent treats its configuration, such as whether Cursor collapses servers with the same URL. config-digger sees at most the value the Agent last cached, as of when it cached it; otherwise it assumes the Agent's default.
_Avoid_: Flag, experiment, rollout

**Mirrored home**:
An Agent home whose content another tool copies from a source Agent home, such as the Codex home Orca keeps in sync with `~/.codex`. config-digger never writes to it: actions on its Entries go to the Entry they were copied from.
_Avoid_: Managed home, runtime home, shadow home

**Last-opened home**:
The Agent home an Agent last opened a Project in, as the Agent itself records it, such as the VS Code profile a workspace was last opened in. config-digger shows that Project in this home first. The Agent can open the Project in another home next time, so the Entries of its other homes are not Unread for it.
_Avoid_: Default home, preferred home, profile association

**Project**:
What an Agent can be started on — a folder or a workspace file. Which locations around it contribute Entries is up to each Agent (Codex reads every folder from the repository root down; Gemini CLI reads only the folder itself). A Project whose folder no longer exists is missing, but Entries keyed to it in user files still exist.
_Avoid_: Workspace, repository, folder

### Inventory

**Item**:
A configured thing of a given Item kind that one Agent can use. An Item belongs to exactly one Agent home of one Agent: Entries are the same Item when that Agent's own rules make them compete, so the same server configured for three Agents is three Items.
_Avoid_: Element, resource, thing

**Item kind**:
The category of an Item. v0.1 knows two: MCP server and skill.
_Avoid_: Type, category

**Entry**:
One declaration at one concrete location — a key inside a configuration file, or a skill folder on disk. A location is its real path once links in its parent folders are resolved, so a skill reached through a linked `skills` folder is one Entry, not two. An Item may have several Entries. Several Agents may read the same Entry; it then belongs to one Item in each of them, and its state is given per Agent.
_Avoid_: Record, definition, setting

**Linked Entry**:
An Entry whose location is a junction or symlink to another location. It is its own Entry, separate from its target: taking it away removes the link, not what it points to.
_Avoid_: Alias, shortcut, mirror

**Git-versioned**:
Said of an Entry or location that is in the index of the git repo that contains it; ignored and untracked files don't count, and nothing does when git cannot be asked. A change to one shows in the git diff and reaches every clone once committed, so a Remove, or any other write inside one, needs its own acknowledgement. Disable never writes to one, not even to move an Entry out of it into config-digger's store: when every way to Disable an Entry would write to one, Disable is not offered. config-digger never stages or commits.
_Avoid_: Tracked, committed, in the repo

**Scope**:
The level at which an Agent applies an Entry, such as managed, user, project or local. Each Agent has its own Scopes, so an Entry read by several Agents has a Scope for each of them. A Scope does not decide on its own which Entry wins: an Agent can rank two locations of the same Scope, as VS Code ranks `.vscode/mcp.json` above `.mcp.json`.
_Avoid_: Level, layer, tier

**Provenance**:
How an Entry got where it is — a file entry, a versioned project file, a plugin or extension, a skill manager, something built into the Agent, something installed through the user's account, or managed/cloud configuration. Provenance decides how the Entry can be taken away.
_Avoid_: Origin, source, installer

**Effective**:
Said of the Entry that wins, for a given Agent home and Project (or none), when several Entries of the same Item compete.
_Avoid_: Active, winning, resolved

**Shadowed**:
Said of an Entry that loses, for a given Agent home and Project (or none), to the Effective Entry of the same Item and therefore has no effect in that Agent.
_Avoid_: Overridden, hidden, inactive. "Hide" is the user's verb for Agent homes and Projects (`homes hide`, `projects hide`), never for Entries.

**State**:
What one Agent does with an Entry for a given Agent home and Project (or none): Effective, Shadowed, Merged, Disabled, Blocked, Unread or Invalid. An Entry has exactly one State per context, set by the first of the Agent's own checks that it fails. Any other checks it also fails are listed as secondary reasons.
_Avoid_: Status, condition

**Merged**:
Said of an Entry that supplies some fields of a server the Agent builds by combining several same-named Entries field by field, so that no single Entry is Effective.
_Avoid_: Partial, combined, partly Effective

**Disabled**:
Said of an Entry that the Agent skips because it was Disabled, natively or by config-digger. When the Agent's mechanism works by name, every Entry of the Item is Disabled. A rule in a file the user writes, such as a deny list, or a setting that turns off everything it covers, Disables too; config-digger never lifts these, so Enable is not offered. The same rule in managed policy Blocks instead.
_Avoid_: Off, inactive

**Parked**:
Said of a Disabled Entry that config-digger moved out of its Agent's reach into its own store, because the Agent has no native way to Disable it; Enable puts it back where it was parked from, its origin: an Agent, one of its Agent homes, a location and, when keyed to one, a Project. The store is never pruned.
_Avoid_: Stashed, archived, moved

**Partial switch**:
A setting that limits part of an Entry, such as some of its tools, while the Agent still uses it. The Entry keeps its State, with a note.
_Avoid_: Partial disable, partly off

**Blocked**:
Said of an Entry that the Agent will not use because of a gate outside the Entry that Enable cannot lift: an untrusted folder, a project server not yet approved, or managed policy. config-digger never lifts these gates.
_Avoid_: Pending, denied, untrusted

**Unread**:
Said of an Entry that the Agent never reads in any context, such as one in an Agent home its lookup order skips, one keyed to a missing Project, or one in a location the Agent reads only while a Feature gate, or one of the Agent's settings, has another value, such as another app's configuration VS Code discovers only once told to. It competes with nothing and can only be Removed.
_Avoid_: Orphaned, stale, ignored

**Leftover**:
An Unread Entry keyed to a missing Project in a user file, such as an MCP server under a Claude Code `projects["<path>"]` key. The leftovers of every missing Project are listed together and can be Removed in one Operation, or all of one Project's at once. Removing them never touches the Project's key itself, nor any trust it records.
_Avoid_: Orphan, stale entry, remnant

**Stranded**:
Said of a Parked Entry whose origin config-digger cannot find: no definition of its Agent is loaded, its Agent home is not found, or its location is no longer in the definition. It has no State, since no Agent home holds it; it is listed apart, with the first of these reasons, and can only be Removed. It becomes Parked again once its origin is found.
_Avoid_: Orphan, lost, dangling

**Invalid**:
Said of an Entry the Agent rejects, on its own or in combination: a server missing what the Agent requires, a skill whose `SKILL.md` the Agent skips, or Codex Entries that merge into a server with two transports, which stops the Agent from starting.
_Avoid_: Broken, conflicting, error

**Read failure**:
A configuration location that, for one Agent, config-digger could not turn into Entries because it cannot be opened, decoded into text, or parsed in that Agent's dialect, or because it holds, where its Entries are, a value of a shape config-digger cannot enumerate, such as a list where servers should be. A file that cannot be opened, decoded or parsed is a Read failure for every location in it; a shape is one only for the location whose Entries it holds, so the other locations in the same file are still read. It contributes no Entries to that Agent and has no State; a missing location is not one, and a broken link is an Unread Linked Entry. config-digger's parked-Entry store is one too when it cannot be opened or parsed, and so is each Entry in it that does not read as a Parked Entry; config-digger never writes to the store because of one.
_Avoid_: Unreadable, broken file, corrupt file, invalid file

**Secret**:
A literal value in an Entry that config-digger never shows, such as a token in an env or header value, a URL credential, or a field an Agent definition marks. It is masked everywhere configuration appears, keeping only a recognised prefix such as `Bearer ` or `ghp_`. A reference to an environment variable is not a Secret.
_Avoid_: Credential, sensitive value, token

### Actions

**Disable**:
Make an Entry stop taking effect for one Agent while keeping it recoverable. The write can reach other Agents that share the same mechanism.
_Avoid_: Deactivate, turn off, mute

**Enable**:
Make a Disabled Entry take effect again for one Agent — the inverse of Disable, whoever disabled it (the Agent itself or config-digger). It lifts switches, never a rule.
_Avoid_: Re-enable, restore, activate

**Remove**:
Delete an Entry permanently from where it lives, for every Agent that reads it.
_Avoid_: Delete, uninstall, purge

**Operation**:
A set of Disables, Enables and Removes confirmed together in one preview. It takes effect entirely or not at all, and is undone as a unit.
_Avoid_: Change, transaction, batch

**Store class**:
How safe a location is to write while its Agent runs, declared per location and per Disable switch in the Agent definition. `plain`: the Agent reads it and leaves it alone, so a write warns that the Agent may not see the change until it restarts. `rewritten-while-running`: the Agent rewrites it from memory, such as `~/.claude.json`, so a write warns strongly that the Agent may overwrite it, and moving an Entry out of it into config-digger's store, or back, is blocked. `sqlite`: the Agent holds it open as a SQLite database, so a write is blocked, and no acknowledgement lifts it. The blocks apply to any Running Agent, the warnings only when an Installation config-digger found reads the Agent home it runs for. A file shared by locations or switches of several classes takes the strongest.
_Avoid_: Write class, store type, safety level

**Undo**:
Revert one Operation, as long as nothing it touched has changed since. Any Operation in the history can be undone, not only the last one, and an Undo is itself an Operation.
_Avoid_: Rollback, revert, restore

**Rollback**:
The automatic return of an Operation's locations to what they were before it, when one of its writes fails or its result cannot be verified. It happens before the Operation completes and never overwrites a location someone else changed after config-digger wrote it; when it cannot return every location, the Operation is left needing manual recovery.
_Avoid_: Undo, revert, abort
