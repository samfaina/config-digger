# Shipping

Read by `/ship-tickets` and `/ship-tickets-reviewed` (samfaina/agent-skills).

- **Base branch:** `main`
- **Merge method:** `merge`
- **Branch naming:** the worktree `--name` is the issue title in kebab-case, e.g. `stale-previews-and-rollback`. Orca adds the `samfaina/` prefix to the branch.
- **After merge:** delete the remote branch with `git push origin --delete <branch>`.

## PR format

- **Title:** the issue title, unchanged.
- **Body**, in order:
  1. One paragraph saying what changed, in the vocabulary of `CONTEXT.md`.
  2. `## What's in it`: one bullet per change, each led by its term in bold.
  3. `## Testing`: each test file touched, with the cases it covers.
  4. `Closes #<n>`
