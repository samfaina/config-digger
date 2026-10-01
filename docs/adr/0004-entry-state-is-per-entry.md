# Entry State is per Entry, with Merged for field-level merges

Every Entry gets exactly one State per Agent, Agent home and Project (or none), even where the Agent combines same-named Entries field by field. Codex deep-merges `[mcp_servers.<name>]` across its layers, and Gemini CLI merges a settings Entry into a same-named extension server. In both cases the Entries that take part are Merged, and the Item's detail shows the server the Agent actually builds, with the Entry each field came from. We rejected deciding Effective and Shadowed per field. It describes these two Agents exactly, but it would make every list, filter and action for all six Agents work on fields, when only two Agents ever merge. An Entry is also what Disable, Enable and Remove act on, so its State has to be per Entry too.

The State is the first of the Agent's own checks that the Entry fails, in the Agent's order (for example policy, then Disable, then folder trust). Any other checks it fails are kept as secondary reasons, not as further States.

## Consequences

- Removing a Merged Entry can change the server the Agent runs even when another Entry with the same name remains. The Remove preview must show the server before and after.
- An Entry that only overrides fields, such as a Codex `[mcp_servers.<name>] enabled = false` with no transport, is still an Entry. It is Merged, or Disabled together with the rest of the Item when the field it supplies turns the server off.
- A merge the Agent rejects, such as Codex Entries combining `command` and `url`, makes every Entry in it Invalid, and the Agent is flagged as unable to start in that context.
- Agent definitions must declare whether same-named Entries compete whole or merge per field, and the order of the checks that sets each State.
