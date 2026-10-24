# Shipping

Read by the `ship-tickets` skill (samfaina/agent-skills).

- **Base branch:** `main`
- **Merge method:** `merge`
- **Merge approval:** `auto` (the loop merges as soon as CI is green)
- **Branch naming:** the worktree `--name` is the issue title in kebab-case: "Action dialog" becomes `action-dialog`. Orca adds the `samfaina/` prefix to the branch.
- **After merge:** delete the remote branch with `git push origin --delete <branch>`.
- **CI:** `required` (PRs get checks, and the loop waits for them to pass)
- **Workers:** `claude` (all roles)

## Release gate

A `vX.Y.Z` tag runs `.github/workflows/release.yml`. Before the build, `cargo xtask unverified --gate` checks the `unverified` marks of the built-in definitions: it fails, listing them, while a mark is outside the exception. A mark is under the exception when its test needs an account or a deployment the project does not have, and says so with `needs` (`unverified = { sentence = "…", needs = "…" }`). The release notes list each mark under the exception with what its test needs (`cargo xtask unverified --notes <file>`), above GitHub's generated notes. CI on PRs does not run the gate.

## PR format

- **Title:** the issue title, unchanged.
- **Body**, in order:
  1. One paragraph saying what changed, in the vocabulary of `CONTEXT.md`.
  2. `## What's in it`: one bullet per change, each led by its term in bold.
  3. `## Testing`: each test file touched, with the cases it covers.
  4. `Closes #<n>`
- **No screenshots or other images**, even when the issue asks for them: the repo is public. Say in the body which UAT steps walk the look instead (see `docs/agents/issue-tracker.md`).
