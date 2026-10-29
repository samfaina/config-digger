# A home no found Installation reads, shown as not read

config-digger decides whether an Agent home is read from its Agent's lookup orders and the folders it finds, never from the Installations it finds. A home whose folder exists and that a lookup order picks shows as read even when no Installation that reads it is found. The home carries the `NoInstallationFound` note instead.

## Why this is out of scope

Finding Installations is best-effort, and an Unread Entry can only be Removed. A home shown as not read when its Agent reads it would offer the user to delete live configuration. The wrong answer today is a duplicate listing, which costs much less.

- Gemini CLI's definition declares no Installation, so every one of its homes would turn Unread.
- An Agent run from a path its definition does not know has no Installation found: Claude Code installed with npm under another prefix, a portable VS Code, a build run from a checkout.
- A Claude Desktop MSIX package's Installation is never found today, because listing `C:\Program Files\WindowsApps` fails with `EPERM` (#469). Every package home would turn Unread.

The case that raised it is Claude Desktop's classic `%APPDATA%\Claude` left behind after the classic app is uninstalled: its Entries the package holds no copy of are listed both in the classic home, as read, and in the package's Overlay home. ADR 0010 accepts that as a consequence of Overlay homes.

A narrower rule, limited to a home that an Overlay home lies over and applied only when the overlay's Installation is found and none of the home's own is, could only take effect once #469 lands, and would only remove that accepted duplicate. It was not taken up either.

## Prior requests

- #435: "Show a home that no found Installation reads as not read"
