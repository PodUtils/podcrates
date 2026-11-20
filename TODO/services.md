

# T-930: `pod-services`: the Syncthing relay DVWOU27 moved to 61.223.4.2; cloudflared 2026.11.1 changed `quick_tunnel.go` (services 44282821174)

**Source:** the scheduled run 44282821174 of `services.yml` at `4161d8d`: two `MOVED` lines and one `FAILED` line.
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

- `MOVED syncthing-relay http-json`: the endpoint `relay-dvwou27` of `crates/pod-services/services/syncthing-relay.toml` is `1.168.112.83`; the service gives the host `61.223.4.2` (source: https://relays.syncthing.net/endpoint/full).
- `MOVED cloudflare-tunnel-edge github-files`: `cmd/cloudflared/tunnel/quick_tunnel.go` of the fact `cloudflared-files` is `637457fc...` at `2026.10.7`; the service gives `9cdabc42...` at `2026.11.1`.
- `FAILED node-xhttp-193-160-208-240 proxy-banner`: the TLS handshake at `193.160.208.240:443` ended with an EOF.

## Premise

- Read: the log of the job `watch`; `quick_tunnel.go` at both tags (the mirror of the forge): the one change is that a quick tunnel no longer sets the protocol to `quic` when the user gives none, so the protocol stays `auto`. `pod-cloudflare` has `Protocol::Auto` as its default, so no change of code follows.
- The same file sets `HaConnections` to 1 for a quick tunnel, at both tags. `Config::default()` of `pod-cloudflare` has `ha_connections: 2` (T-915 saw the second leg). That difference is T-931.
- The `FAILED` line: the new dispatch of `services.yml` with `service=node-xhttp-193-160-208-240` (run 44284797019, 2026-11-18) passed: a failure of the host of the run, no fault of the node.

## Approach

Set the host of `relay-dvwou27` to `61.223.4.2` with the read date 2026-11-18 (the device id stays). Set the sum of `quick_tunnel.go` to `9cdabc42...`, the tag of the measure to `2026.11.1`, and the read date.

## Prove

```sh
cargo test -p pod-services --all-features
```

A dispatch of `services.yml` with `service=syncthing-relay` and `service=cloudflare-tunnel-edge` gives `holds`.

## Done

2026-11-18: `crates/pod-services/services/syncthing-relay.toml`: the endpoint `relay-dvwou27` is `61.223.4.2` (read 2026-11-18). The pin of cloudflared moves to `2026.11.1` (`518d7f7`, released 2026-11-17) in `crates/pod-cloudflare/src/cloudflare/cloudflared-pin.txt` and in the facts `cloudflared-tag`, `cloudflared-commit`, `cloudflared-released` and `cloudflared-files`: each pinned file has the same sum at `2026.11.1` but `quick_tunnel.go` (read through the mirror of the forge: 9 of 10 the same, and `connection/protocol.go` the same). The one change of `quick_tunnel.go` (no forced `quic` for a quick tunnel) asks nothing of `pod-cloudflare`, whose default is `Protocol::Auto`. A test that read the old tag as text reads `speaks()` now; the test of the date reads 2026-11-17. The `FAILED` node passed on a new dispatch (run 44284797019). `cargo test -p pod-services --all-features` and `cargo test -p pod-cloudflare --all-features --lib` (195) pass. The tests: `crates/pod-services/tests/api.rs::a_fact_is_read_by_its_type` (the date of the pin) and `speaks_names_the_tag_of_the_pin` of `crates/pod-cloudflare/src/cloudflare/mod.rs` (the tag).
