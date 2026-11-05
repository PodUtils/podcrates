# Security

## Report a vulnerability

Do not open a public issue for a vulnerability. Use the private report of
GitHub: open <https://github.com/PodUtils/podcrates/security/advisories/new>
and write these items:

* the crate and its version (or the commit);
* the consumer and its version, when a consumer shows the fault;
* the steps that show the fault;
* what an attacker can do, and what the attacker must have first.

The maintainer replies in the report. A fix comes in a new version of the
crate, and the report becomes a public advisory after the release. The
maintainer tells each consumer that uses the crate.

## Supported versions

Only the latest version of each crate and `main` get fixes.

## Dependencies

* `.github/workflows/deny.yml` (step M0 of `docs/plan.md`) runs
  `cargo deny` on each push and each day: advisories, licenses, bans and
  sources (`deny.toml`).
* An advisory stops CI. Update the crate, or replace it. If no fixed
  version exists and no crate uses the affected part, add the advisory to
  `[advisories] ignore` in `deny.toml` with the reason and the date it was
  read.
