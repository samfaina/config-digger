# A server of a git-versioned layer is Disabled through a Stand-in

Codex turns off a server with `enabled = false` in any layer, but Disable never writes to a git-versioned Project layer, and a user table holding `enabled = false` alone stops Codex with "invalid transport" in every folder where no other layer declares the server. So Disable writes a Stand-in in the user's `config.toml`: the one field that fixes the server's transport, `command` as the layer has it or `url` without its credentials and query, and `enabled = false`. In the Project the layer's fields win; elsewhere Codex starts, with a Disabled server of its own. We rejected offering no Disable, which leaves the user editing a file every clone shares, a table with `enabled` alone, which stops Codex, a copy of the whole transport, which spreads Secrets into another file, and a fixed placeholder transport, which shows a made-up server in every folder.

## Consequences

- Enable deletes a Stand-in whole, recognised by its shape whoever wrote it, since deleting only `enabled` would turn the server on in every folder. It is offered only in a Project whose layer gives the server its transport; elsewhere only Remove is. A table of that shape that no known Project's layer declares is the user's own server with `enabled = false`, which Enable turns on again by deleting the field.
- A Stand-in turns the server off in every Project that declares it. The preview names the known ones, and Disable is not offered when one of them declares the server with the other transport, since Codex would stop there.
- When the layer later changes its transport, Codex stops in the Project; the merged Entries show as Invalid, and the preview of the Disable warns of it.
- When the user's layer already declares the server, Disable writes `enabled = false` in that table instead, which turns the server off in every folder.
