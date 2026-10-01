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

### Inventory

**Item**:
A configured thing of a given Item kind that an Agent can use.
_Avoid_: Element, resource, thing

**Item kind**:
The category of an Item. v0.1 knows two: MCP server and skill.
_Avoid_: Type, category

**Entry**:
One declaration of an Item at one concrete location — a key inside a configuration file, or a skill folder on disk. An Item may have several Entries.
_Avoid_: Record, definition, setting

**Scope**:
The level at which an Entry applies: managed, user, project or local.
_Avoid_: Level, layer, tier

**Provenance**:
How an Entry got where it is — a file entry, a versioned project file, a plugin or extension, a skill manager, or managed/cloud configuration. Provenance decides how the Entry can be taken away.
_Avoid_: Origin, source, installer

**Effective**:
Said of the Entry that wins when several Entries compete for the same Item.
_Avoid_: Active, winning, resolved

**Shadowed**:
Said of an Entry that loses to an Effective Entry and therefore has no effect.
_Avoid_: Overridden, hidden, inactive

### Actions

**Disable**:
Make an Entry stop taking effect while keeping it recoverable.
_Avoid_: Deactivate, turn off, mute

**Remove**:
Delete an Entry permanently from where it lives.
_Avoid_: Delete, uninstall, purge
