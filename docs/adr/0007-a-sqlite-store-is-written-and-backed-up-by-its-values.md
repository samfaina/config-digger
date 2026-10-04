# A SQLite store is written and backed up by its values

Some Disable switches live in a SQLite store rather than a file, such as VS Code's `mcp.enablement` in `state.vscdb`, the JSON text of one row of its `ItemTable`. config-digger writes such a store only while its Agent is closed, and then only the rows an Operation's edits change. Edits, diffs, Verify, Rollback, History and Undo see those rows as one JSON document of each key and its value's text, so a row is edited, checked and undone like a key of a file (ADR 0003). The backup is that document, not a copy of the store. We rejected copying the store whole: it holds everything else the Agent keeps, such as its caches and window state, it can sit beside a journal that a copy misses, and writing it back would roll back whatever the Agent stored since, which ADR 0003 already rules out for files.

This narrows ADR 0003, whose Consequences say a full-file backup is taken before every Operation: for a SQLite store, the backup holds the values the Operation writes, which is all a Rollback restores.

## Consequences

- A write to a SQLite store is blocked while its Agent runs, whatever is acknowledged.
- config-digger never creates a store: a switch whose store is missing is skipped for the next mechanism, and a plan or Undo that needs a store that is gone is refused.
- A Rollback or Undo puts back the rows the Operation wrote, never the rest of the store.
