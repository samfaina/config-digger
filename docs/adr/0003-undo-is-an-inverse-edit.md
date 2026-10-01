# Undo is an inverse edit, checked semantically

Undo reverts an Operation by applying the inverse edit to each file as it is now, and refuses only when the Entries or keys the Operation touched have changed since. It does not restore whole files from the backup. The same semantic check runs between preview and execute. We rejected whole-file hashes because Claude Code rewrites `~/.claude.json` every few minutes and Codex rewrites `config.toml` while running, so a hash check would refuse almost every Undo and invalidate any preview left open for a while. Restoring the backup would also roll back whatever the Agent wrote in between.

## Consequences

- Full-file backups are still taken before every Operation, but they serve rollback of a half-applied Operation and manual recovery, not Undo.
- The write layer must be able to compute, for each edit, its inverse and the set of keys it depends on.
- Any Operation in the history can be undone, not only the last one, and the history survives app restarts.
