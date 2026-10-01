# config-digger

config-digger inspects the effective configuration of AI coding agents — which MCP servers and skills are configured, where each one comes from, and which one actually wins — and removes or disables them according to how they were installed.

## Language

### Agents

**Agent**:
An AI coding tool whose configuration config-digger reads, such as Claude Code, Cursor or Codex.
_Avoid_: Client, host, tool

**Agent definition**:
A data file that describes where an Agent keeps its configuration, in which format, and with what precedence. Supporting a new Agent means writing an Agent definition, not code.
_Avoid_: Adapter, plugin, driver

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
The level at which an Agent applies an Entry, such as managed, user, project or local. Each Agent has its own Scopes, so an Entry read by several Agents has a Scope for each of them.
_Avoid_: Level, layer, tier

**Provenance**:
How an Entry got where it is — a file entry, a versioned project file, a plugin or extension, a skill manager, or managed/cloud configuration. Provenance decides how the Entry can be taken away.
_Avoid_: Origin, source, installer

**Effective**:
Said of the Entry that wins, for a given Agent home and Project (or none), when several Entries of the same Item compete.
_Avoid_: Active, winning, resolved

**Shadowed**:
Said of an Entry that loses, for a given Agent home and Project (or none), to the Effective Entry of the same Item and therefore has no effect in that Agent.
_Avoid_: Overridden, hidden, inactive

### Actions

**Disable**:
Make an Entry stop taking effect for one Agent while keeping it recoverable. The write can reach other Agents that share the same mechanism.
_Avoid_: Deactivate, turn off, mute

**Remove**:
Delete an Entry permanently from where it lives, for every Agent that reads it.
_Avoid_: Delete, uninstall, purge
