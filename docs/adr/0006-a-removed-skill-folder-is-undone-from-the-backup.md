# A removed skill folder is undone from the backup

Remove of a skill folder moves the folder whole into the Operation's backup, and Undo moves it back. This is the one place where the backup serves Undo, which ADR 0003 keeps it from doing for files. A skill folder has no inverse edit: it is not a key in a file the Agent rewrites, so there is nothing for an Agent to have written in between that a restore would roll back, and the copy in the backup is the only place the skill still exists. We rejected copying the folder and deleting the original, because a copy left next to the original would be loaded by Codex and Gemini CLI, and because a move keeps the file count and total size that Verify checks. Remove of a Linked Entry removes the link alone, and its Undo creates the link again from the kind and target recorded in History (ADR 0008).

## Consequences

- Undo refuses when anything is at the folder's location again, or when its copy in the backup is gone.
- Deleting the backup of a skill folder Remove deletes the skill for good, as the Delete backup preview lists.
- An Undo of that Undo moves the folder into its own backup again, so each Operation still owns the copies its manifest names.
