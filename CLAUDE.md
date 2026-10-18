# config-digger

## Agent skills

### Issue tracker

Issues and wayfinder maps live in GitHub Issues on samfaina/config-digger, managed with the `gh` CLI. See `docs/agents/issue-tracker.md`.

A reported bug: search issues and open PRs for it with `gh` first. Parallel workers may already have a fix in flight.

### Triage labels

The five default triage labels (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `CONTEXT.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.

### Manual UATs

The UAT kit in `uat/`: the plan, the runner page and the results. Add steps when a feature lands, publish a run, and turn its results into tickets as `docs/agents/uat.md` says.
