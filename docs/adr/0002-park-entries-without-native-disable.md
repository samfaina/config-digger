# Park Entries that have no native Disable

Some Entries have no native way to be Disabled: a server in Claude Desktop's `claude_desktop_config.json`, or a user skill folder in Cursor. For these, config-digger Disables by moving the Entry out of the Agent's reach into its own store, keeping it listed as Disabled, and putting it back on Enable. We rejected offering only Remove, because users who want to try life without a server would lose it, and we rejected Agent-specific tricks such as renaming keys or folders, because they rely on undocumented behaviour that breaks when Agents update.

Parking never applies to an Entry in a git-versioned file or folder: moving it would show up as a deletion in git and reach every clone. Disable on a versioned Entry always writes to a machine-local mechanism (the one the Agent's own UI uses), and when the Agent has none, Disable is not offered.

## Consequences

- The parking store is part of the inventory, not a cache: parked Entries are never pruned with backups.
- Enable of a parked Entry refuses rather than overwrite when a same-named Entry has appeared in the meantime, or when the Project folder is gone.
- Parking in Claude Desktop needs the app closed, since it rewrites its config from memory while running. An Agent home the user added outside its definition's homes is read by no launch, so parking there does not.
