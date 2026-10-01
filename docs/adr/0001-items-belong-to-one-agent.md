# Items belong to one Agent

An Item is scoped to a single Agent: two Entries are the same Item only when that Agent's own rules make them compete, so the same MCP server configured for Claude Code, VS Code and Cursor is three Items. We rejected one global Item per server because no cross-Agent identity rule exists. Claude Code matches exact names across Scopes but endpoints against plugins, VS Code matches names case-insensitively, and Cursor never makes Entries compete. Any shared rule would sometimes call two Entries "the same" when the Agent disagrees, and then Effective and Shadowed would be wrong.

## Consequences

- An Entry is a location, and several Agents may read it. Its state and Scope are given per Agent, so one `.mcp.json` key can be Effective for Claude Code and Shadowed for VS Code.
- Remove targets an Entry and affects every Agent that reads it. Disable targets an Entry for one Agent. Both previews list every Agent the write reaches.
- "Turn off `github` everywhere" is not a domain action. A view may group same-named Items across Agents, but that grouping is presentation, not identity.
