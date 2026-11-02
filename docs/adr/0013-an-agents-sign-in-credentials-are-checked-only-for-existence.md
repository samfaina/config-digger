# An Agent's sign-in credentials are checked only for existence

Claude Code runs on its claude.ai account by the tokens it keeps, `.credentials.json` in its config folder on Windows and Linux, not by `oauthAccount` in `~/.claude.json` (#491). With `oauthAccount` and no credentials file it is signed out; with the file and no `oauthAccount` it reads nothing offline and gets the account back at its next start online. So the Signed-in account needs both: the file naming the account and the credentials. config-digger checks only that the credentials file exists. It never opens it, and it never asks an OS store such as the macOS keychain. We rejected reading the file to tell a live token from an expired or empty one, since the whole file is a Secret and config-digger would then hold sign-in tokens it has no use for, and asking the keychain, which raises a system prompt the user did not ask for.

## Consequences

- A credentials file whose token is expired or revoked still counts as signed in. Claude Code refreshes such a token or signs the user out at its next start, which config-digger cannot see.
- A home whose credentials file exists but whose state file names no account is signed in to none. config-digger does not guess the account from the account folders it finds: the reason it gives says Claude Code gets the account back at its next start online.
- Where an Agent keeps its credentials outside the home, such as Claude Code in the macOS keychain, the file naming the account decides alone, as before #491.
- Codex's `auth.json` and its MCP `.credentials.json` fall under the same rule, should a definition ever need them.
