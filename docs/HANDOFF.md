# Handoff

The state of the unfinished work, for the next session of podcrates.
Keep it current; remove a line when its work is done. `TODO/PROGRESS.md`
has the work order; this page has only what is in flight.

## In flight: the triage of GitHub #20 to #36

- #20 to #34: the reports of the owner and of an audit (pod-proxy,
  pod-dns, `SOURCES`, `check-sources.py`, the watches of `pod-services`,
  the static probe, the README and `docs/design.md`). #24 to #26 ask for a
  new crate, `pod-gsr`.
- #35 (cfb, 84 asks) and #36 (githome, 12 asks): meta requests of the
  owner's other projects. Each ask goes to a crate of podcrates, to a
  consumer, or to a new project. New crate names go to GitHub #1 (the
  owner reserves them).
- The sessions of podssh, podnet and podvm run in parallel, each in its own
  repository. Each triages its own issues and reports to this session.
  The reconciliation of all reports comes before new work; the questions
  for the owner go to `TODO/PROGRESS.md`.

## The red scheduled runs

- `record-issues`: `TODO/github.txt` lags GitHub until the triage maps
  #20 to #36.
- `upstream`, job `sources`: commits of podssh, podnet and podvm touch
  files that feed `pod-services`. Each needs a port entry, or a new
  snapshot when the facts did not change.

## Channels

- Workflows: dispatch them with the GitHub MCP tool
  (`docs/maintenance.md`, "A run of a workflow").
- Messages: COORD issues (`docs/session-protocol.md`) and
  `sh scripts/agent/watch-org.sh` under the Monitor tool.
