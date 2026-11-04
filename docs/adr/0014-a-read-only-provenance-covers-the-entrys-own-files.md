# A read-only Provenance covers the Entry's own files, not its switches

A Claude Code skill synced from claude.ai is a copy of the account's skill, which Claude Code rewrites as it syncs: config-digger never writes it, nor Removes it. Until #550 the same rule kept config-digger from Disabling and Enabling it, though Claude Code turns it off by `skillOverrides` in the settings files, which config-digger already writes for every other skill (#530). The owner decided that a read-only Provenance covers the Entry's own files, not the switches the Agent keeps for it in files config-digger writes. This follows the `plugin` and `extension` Provenance, whose Entries are Disabled through a switch and never Removed.

So a Provenance answers two questions apart: whether config-digger writes the Entries' files and Removes them, and which switches it writes for them. `cloud-mirror` is never written nor Removed, but its Entries are Disabled and Enabled through a switch that holds one of them by its name in a file outside the copy and any plugin's folder. `managed`, `built-in` and `account-managed` stay read-only on both counts.

## Consequences

- A synced Claude Code skill is Disabled by `off` in the Project's local settings and Enabled as #530 enables any skill, `"on"` there when the shared or user settings would still turn it off. Remove is still refused.
- A switch in the copy itself stays read-only, such as the `enabled` flag in Claude Desktop's mirrored `manifest.json`, which the app rewrites from the account. So does a switch of a whole synced plugin in `enabledPlugins`, which #550 left out of scope and no probe has tested: a synced plugin's servers and skills are still read-only.
- A `toml-fragments` location still requires a Provenance whose files are never written, and a merged field's fallback a layer whose files are written: both are about files, not switches.
- An Unread synced skill still offers nothing: Unread can only be Removed, and Remove is refused.
