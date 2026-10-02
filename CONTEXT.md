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

**Mirrored home**:
An Agent home whose content another tool copies from a source Agent home, such as the Codex home Orca keeps in sync with `~/.codex`. config-digger never writes to it: actions on its Entries go to the Entry they were copied from.
_Avoid_: Managed home, runtime home, shadow home

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
_Avoid_: Overridden, hidden, inactive

**State**:
What one Agent does with an Entry for a given Agent home and Project (or none): Effective, Shadowed, Merged, Disabled, Blocked, Unread or Invalid. An Entry has exactly one State per context, set by the first of the Agent's own checks that it fails. Any other checks it also fails are listed as secondary reasons.
_Avoid_: Status, condition

**Merged**:
Said of an Entry that supplies some fields of a server the Agent builds by combining several same-named Entries field by field, so that no single Entry is Effective.
_Avoid_: Partial, combined, partly Effective

**Disabled**:
Said of an Entry that the Agent skips because it was Disabled, natively or by config-digger. When the Agent's mechanism works by name, every Entry of the Item is Disabled.
_Avoid_: Off, inactive

**Blocked**:
Said of an Entry that the Agent will not use because of a gate outside the Entry that Enable cannot lift: an untrusted folder, a project server not yet approved, or managed policy. config-digger never lifts these gates.
_Avoid_: Pending, denied, untrusted

**Unread**:
Said of an Entry that the Agent never reads in any context, such as one in an Agent home its lookup order skips or one keyed to a missing Project. It competes with nothing and can only be Removed.
_Avoid_: Orphaned, stale, ignored

**Invalid**:
Said of an Entry the Agent rejects, on its own or in combination: a server missing what the Agent requires, a skill whose `SKILL.md` the Agent skips, or Codex Entries that merge into a server with two transports, which stops the Agent from starting.
_Avoid_: Broken, conflicting, error

**Read failure**:
A configuration location that, for one Agent, config-digger could not turn into Entries because it cannot be opened, decoded into text, or parsed in that Agent's dialect. It contributes no Entries to that Agent and has no State; a missing location is not one, and a broken link is an Unread Linked Entry.
_Avoid_: Unreadable, broken file, corrupt file, invalid file

### Actions

**Disable**:
Make an Entry stop taking effect for one Agent while keeping it recoverable. The write can reach other Agents that share the same mechanism.
_Avoid_: Deactivate, turn off, mute

**Enable**:
Make a Disabled Entry take effect again for one Agent — the inverse of Disable, whoever disabled it (the Agent itself or config-digger).
_Avoid_: Re-enable, restore, activate

**Remove**:
Delete an Entry permanently from where it lives, for every Agent that reads it.
_Avoid_: Delete, uninstall, purge

**Operation**:
A set of Disables, Enables and Removes confirmed together in one preview. It takes effect entirely or not at all, and is undone as a unit.
_Avoid_: Change, transaction, batch

**Undo**:
Revert one Operation, as long as nothing it touched has changed since. Any Operation in the history can be undone, not only the last one, and an Undo is itself an Operation.
_Avoid_: Rollback, revert, restore

**Rollback**:
The automatic return of an Operation's locations to what they were before it, when one of its writes fails or its result cannot be verified. It happens before the Operation completes and never overwrites a location someone else changed after config-digger wrote it; when it cannot return every location, the Operation is left needing manual recovery.
_Avoid_: Undo, revert, abort
