# A removed link comes back as it was

Remove of a Linked Entry records the link's kind (a junction, a folder symlink or a file symlink) and its target as written, relative if it was relative, and Undo creates that same link again. Until now Undo created a junction to the absolute target, whatever had been there: a relative symlink committed in a repo came back as a junction to one machine's path, which shows in the git diff of a Git-versioned Entry and breaks when the tree moves. The standard library creates a symlink without unsafe code, but it creates no junction and cannot tell a junction from a folder symlink with an absolute target, so config-digger depends on the `junction` crate on Windows to tell them apart and to create a junction. The crate holds the unsafe code; config-digger's own crates still forbid it. We rejected keeping `mklink /J` and parsing `fsutil reparsepoint query` or `dir /AL` to tell the kinds apart: they start a process for each link, their output is translated into the system's language, and `cmd` cannot quote a path that holds `"` or `%`.

Creating a symlink on Windows takes Developer Mode or admin rights. When Undo cannot create a folder symlink, it creates a junction to the absolute target instead, and its result sentence says so: a skill that works again is worth more than an Undo that waits for a setting. A file symlink has no junction to fall back to, so its Undo is refused, with what is missing.

## Consequences

- A History record of a removed link with no kind is read as a junction to an absolute target, which is what Undo created before this decision.
- The Remove preview of a symlink warns that it may come back as a junction to the absolute target when config-digger finds, by creating a symlink of its own once per session, that it cannot create one.
- Verify after Undo checks the link's kind and its target as written, separators aside; after the fallback, it checks a junction to the absolute target.
