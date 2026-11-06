# The trial of pod-dns and pod-services in podnet: unfinished

This directory keeps the unfinished trial of v0.4.0 in podnet, so that a
new session can continue it. No tool reads it: `adoption.yml` and
`scripts/adoption.sh` read only `adoption/podnet/*.patch`. Remove the
directory when `adoption/podnet/pod-dns.patch` exists.

## Files

- `commit`: the podnet commit of the diff (`a0e1ffb`).
- `pod-dns.diff`: the change to podnet, not compiled in its last state.
  Its manifests name the crates by the tag `v0.4.0` (git form).
- `pod-dns.run`: the steps of the trial. It becomes
  `adoption/podnet/pod-dns.run`.
- `togit.sh`: changes path dependencies on a podcrates tree to the git
  form at a tag.
- `envs.sh`: the environment of a trial build (`CARGO_INCREMENTAL=0`; the
  incremental cache filled 6.7 GB).
- `secrets_like.py`: an emulation of two detectors of TruffleHog (URI,
  CloudflareApiToken). Run it over a new patch to find the lines that
  `.github/secrets-allow.txt` must list.

## Done in the diff

- `src/resolve.rs` is glue over pod-dns (about 190 lines; it had 591):
  podnet's settings, `set_pins` and `parse_pins`, `plain()`,
  `with_doh()`, `resolve`, `answering_step`, `doh_query`.
- New `src/services.rs`: the reads of pod-services.
- `src/egress.rs`: `Way` holds the resolver; `dialer_and_resolver`; the old
  `Resolver` glue is removed.
- `src/main.rs` sets the pins before `--proxy` and `--no-proxy` make the
  first dialer. Without this order, the pins of `--resolve` are lost.
- Each constant of the adoption map of podnet reads pod-services. The fuzz
  target `doh_json` is removed. `SECURITY.md`, `docs/protocol.md`,
  `docs/target-environment.md` and the module table of
  `docs/maintenance.md` follow.

## To do

1. Clone podnet at `commit`, apply the diff, and make the manifests path
   dependencies on this tree (`scripts/adoption.sh` does this).
2. Remove the unused `use super::*;` of the tests of `src/trust.rs`. The
   last edits (`trace()` is now `trace_endpoint()` in `src/warp/exit.rs`;
   `services::endpoints_of` filters `endpoints()`) were not compiled.
3. `cargo check --all-targets`, clippy, rustfmt, `cargo test --lib` and
   the integration tests of `pod-dns.run`.
4. Lower the count of each file in `scripts/size-allow.txt` of podnet that
   got smaller.
5. `cargo run -p podnet-todo -- remap` and `check`: give each stale
   citation the prefix `PodUtils/podnet:`.
6. podnet's `deny.toml` has `multiple-versions = "warn"`: duplicate
   versions do not fail `cargo deny`. It has not run yet.
7. Make `adoption/podnet/pod-dns.patch` against the podnet head of that
   day (not only `a0e1ffb`), run `sh scripts/adoption.sh podnet pod-dns`,
   and commit with `adoption/podnet/commit`.
8. Run `secrets_like.py` over the patch, then add the entries of
   `.github/secrets-allow.txt` with the commit of the patch.
