

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

# T-1093: `services.yml` 45009160652: three `.xyz` RDAP records refused, three Trojan nodes gone, the Syncthing relay DVWOU27 at a new address, and two hosts dark to the runner

**Source:** the scheduled run 45009160652 of `services.yml` at `50aa5e0b` (2026-11-22, job `watch` 137167379735), the scheduled run before it, 44891418606 (2026-11-21, job `watch` 136807841812), and the scheduled run after it, 45159148662 at `23248f8` (2026-11-23, job `watch` 137639710258).
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The step "Each watch" gave `210 hold, 0 skipped, 2 first with no answer, 7 moved or failed`:

- `FAILED node-trojan-66-163-112-121 proxy-banner`: "could not connect to 66.163.112.121:443: Connection refused (os error 111)".
- `FAILED freedos-image http-sha256`: "connecting to download.freedos.org:443 timed out after 29s".
- `FAILED localtunnel tls-name`: "connecting to localtunnel.me:443 timed out after 29s".
- `FAILED gosuda-portal rdap`, `FAILED openbare-xyz rdap`, `FAILED opentunnel rdap`: "https://rdap.centralnic.com/xyz/domain/NAME answered HTTP 403; https://rdap.org/domain/NAME answered HTTP 403".
- `MOVED syncthing-relay http-json`: "endpoint relay-dvwou27 is 61.223.4.2; the service gives host "1.168.95.18" (source: url https://relays.syncthing.net/endpoint/full, read 2026-11-18)".

The run of 2026-11-21 gave `214 hold, 0 skipped, 2 first with no answer, 3 moved or failed`: `FAILED (first no answer)` for `node-trojan-66-163-112-121` ("tls handshake eof") and `freedos-image` (the same time-out), `FAILED localtunnel tls-name` (the same time-out), `FAILED temp-sh http-status` (T-1034), and `MOVED syncthing-relay` for the relay YIRRKOD (not in the pool at that read). The three `rdap` watches held on 2026-11-21.

The run of 2026-11-23 gave `214 hold, 0 skipped, 1 first with no answer, 4 moved or failed`: `FAILED` for `node-trojan-66-163-112-121`, `node-trojan-169-197-140-218` and `node-trojan-216-238-104-154` (each "Connection refused (os error 111)"), `FAILED (first no answer) node-trojan-45-63-30-142` ("tls handshake eof"), and the same `MOVED syncthing-relay` line. `freedos-image`, `localtunnel tls-name` and the three `rdap` watches held.

## Premise

- Read: the logs of both jobs through the mirror of the forge; the scheduled runs 44715667153 (2026-11-20), 44282821174 (2026-11-18) and 44067490108 (2026-11-17); `crates/pod-services-watch/src/rdap.rs`; T-685, T-855 and T-930.
- RDAP, measured here on 2026-11-23: `https://rdap.centralnic.com/xyz/domain/openbare.xyz` and `.../opentunnel.xyz` answer 403 (an HTML page of nginx), also with the user agent of the watch; `https://rdap.org/domain/openbare.xyz` answers 302 to the same URL. On 2026-11-15 the same URL answered 200 here (T-685). Thus the registry refuses more than one cloud range, on more than one day. IANA's list of the registrar ids (`https://www.iana.org/assignments/registrar-ids/registrar-ids-1.csv`) gives the RDAP base of each registrar: 1068 (NameCheap) `https://rdap.namecheap.com/`, 1910 (Cloudflare) `https://rdap.cloudflare.com/rdap/v1/`. The registrar of `gosunuts.xyz` and `openbare.xyz` is 1068, of `opentunnel.xyz` 1910 (the registry's id `D...-CNIC` and the registrar, read through `who-dat.as93.net`). Here, `https://rdap.namecheap.com/domain/openbare.xyz` answers 200 with the event `registrar expiration` 2027-01-17 alone, `.../gosunuts.xyz` 2027-03-01; `https://rdap.cloudflare.com/rdap/v1/domain/opentunnel.xyz` answers 200 with `expiration` 2028-03-30. Each date is the date of the run of 2026-11-21.
- `node-trojan-66-163-112-121`: no answer on two scheduled runs in a row (2026-11-21 a TLS handshake that ended, 2026-11-22 a refused connect); it held on 2026-11-20. The link is still in the list of barry-far (4 lines with `@66.163.112.121:`, read 2026-11-23); the lists are made by a robot and keep dead nodes. The proxy of this sandbox resets a connect to an IP literal: no read of the node here.
- `syncthing-relay`: the pool (`https://relays.syncthing.net/endpoint/full`, read here 2026-11-23) gives DVWOU27 (the same device id, `providedBy=xlionjuan`, `globalLimitBps=3000000`, location TW) at `relay://1.168.95.18:443`, with `startTime` 2026-11-18T16:28:17Z and `uptimeSeconds` 408506 (4.7 days). Its address was `1.168.114.174` until 2026-11-12, `1.168.112.83` until 2026-11-16 (T-790), `61.223.4.2` until 2026-11-18 (T-930), and is `1.168.95.18` now: the relay restarts at each new address. YIRRKOD is in the pool again at `85.121.51.152:443`, as the file says.
- `node-trojan-169-197-140-218` and `node-trojan-216-238-104-154`: a refused connect on two scheduled runs in a row (2026-11-22 first no answer, 2026-11-23 `FAILED`); the four nodes that a refused connect or an ended handshake met are of the same lists and the same form of link.
- `localtunnel` and `freedos-image`: see T-1096.

## Approach

- RDAP: a second source, as the Done of T-685 says. A new optional argument `registrar` of the watch `rdap` (the IANA id of the registrar). Where the registry's server and `rdap.org` give no record, the watch reads the RDAP base of the registrar from IANA's list of the registrar ids and asks it; it takes `expiration`, else `registrar expiration`. The line of the run keeps the refusals. The three `.xyz` watches name their registrar. A test with a stand-in, and a plant.
- `node-trojan-66-163-112-121`, `node-trojan-169-197-140-218` and `node-trojan-216-238-104-154`: each leaves as `node-trojan-167-88-61-175` left (T-855): its file, the count of the rows `node-*` in `crates/pod-services/README.md` (6 to 3); the next hand-off tells podssh (PodUtils/podssh@T-574). `node-trojan-45-63-30-142` stays: its first no answer waits for the next scheduled run (call C153).
- `syncthing-relay`: the relay DVWOU27 leaves the set by call C104 (up 7 days or more at the read; it was up 4.7 days) and the new call C901 (a relay at a new address leaves the set). Its endpoint and its fact go; the count of the README row goes from 25 to 24. Its consumers (podssh, PodUtils/podssh@T-571; podnet) read the set at the next tag.

## Prove

```sh
cargo test -p pod-services-watch --all-features --test watches -- apis::rdap
cargo test -p pod-services --all-features
sh scripts/plant-each.sh services-rdap-registrar
```

A dispatch of `services.yml` with `kind=rdap`, and with `service=syncthing-relay`, gives `holds`.

## Done

2026-11-23. `crates/pod-services-watch/src/rdap.rs`: the optional argument `registrar` (an IANA registrar id); where the registry's server and `rdap.org` give no record, the watch reads the RDAP base of the registrar from IANA's list of the registrar ids (`Bases::rdap_registrars`) and asks it; the event `expiration`, else `registrar expiration`; a record after a refusal keeps the refusals in its line. `gosuda-portal` and `openbare-xyz` name the registrar 1068, `opentunnel` 1910. The test `apis::rdap_reads_the_registrar_where_each_server_refuses` (`crates/pod-services-watch/tests/watches/apis.rs`; it fails on the code before the fix, which has no argument `registrar`) and the plant `services-rdap-registrar` (`scripts/plants/services-rdap-registrar.plant`); the patch of the plant `services-rdap-any-end` follows the new lines. `node-trojan-66-163-112-121`, `node-trojan-169-197-140-218` and `node-trojan-216-238-104-154` left (their files; the README row `node-*` counts 3). The relay DVWOU27 left `syncthing-relay` by calls C104 and C901 (the README row counts 24). Here, through the proxy of this sandbox: `cargo run -q -p pod-services-watch -- --kind rdap --service openbare-xyz --service gosuda-portal --service opentunnel` gives 3 hold, each read at the registrar's server after the two 403 answers (`gosunuts.xyz` until 2027-03-01, `openbare.xyz` until 2027-01-17, `opentunnel.xyz` until 2028-03-30). `cargo test -p pod-services-watch -p pod-services --all-features` passes (182 tests) and clippy of both crates is clean. The consumers podssh (PodUtils/podssh@T-574, PodUtils/podssh@T-571) and podnet read the smaller sets at the next tag. A dispatch of `services.yml` with `kind=rdap` checks the registrar's servers from a runner.

# T-1096: `pod-services`: `localtunnel.me` and `download.freedos.org` give no answer to the runners of GitHub on each run, but answer other networks

**Source:** the scheduled runs of `services.yml` 44715667153 (2026-11-20), 44891418606 (2026-11-21) and 45009160652 (2026-11-22), found in T-1093.
**Category:** measurement
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`localtunnel tls-name` timed out on the TCP connect to `localtunnel.me:443` (29 s) on three scheduled runs in a row (first no answer on 2026-11-20, then `FAILED`). `freedos-image http-sha256` timed out on the TCP connect to `download.freedos.org:443` on two runs in a row (2026-11-21 and 2026-11-22). Both held on 2026-11-17 and 2026-11-18; `freedos-image` held on 2026-11-20. The rule of call C153 holds only the first day of no answer: a host that drops the address ranges of the runners makes each scheduled run red, and the other findings hide behind it. The consumers that fetch these hosts on a runner of GitHub meet the same: podvm downloads the FreeDOS zip (`PodUtils/podvm experiments/369-windows-tcg-dos.sh`), podssh opens localtunnel tunnels.

## Premise

- Measured here on 2026-11-23 through the proxy of this sandbox: `https://localtunnel.me/` answers 302, and `https://download.freedos.org/1.4/FD14-LiteUSB.zip` answers 200 with 17671175 bytes and the SHA-256 of the fact (`857dcd2e...`). Through `fetch.web.pkgforge.dev` (the egress of Cloudflare): `localtunnel.me` answers 503 "Tunnel Unavailable" from its own server (`x-localtunnel-status`), and its certificate (`domain.web.pkgforge.dev`) is valid for `localtunnel.me` until 2027-02-08. DNS over HTTPS (`dns.google`): `localtunnel.me` A `193.34.76.44`, `download.freedos.org` A `173.236.240.244` (DreamHost), and no AAAA for `download.freedos.org`: no fault of IPv6.
- The same zip (17671175 bytes) is on the archive of the FreeDOS project at ibiblio (`https://www.ibiblio.org/pub/micro/pc-stuff/freedos/files/distributions/1.4/FD14-LiteUSB.zip`, read here 2026-11-23: 200, the same length).
- Not measured: a connect from a runner by hand. This sandbox cannot dispatch a workflow.

## Approach

1. The coordinator dispatches `services.yml` with `service=localtunnel` and `service=freedos-image`: a fourth read from a runner, at another time of the day.
2. When the time-outs stay: `freedos-image` gets a second endpoint at ibiblio with its own `http-sha256` watch and the same sum, and podvm gets the mirror as a fallback in the hand-off. For a host that drops the runners and answers other networks, a watch argument (for example `runner-dark = "REASON"`) makes a no answer a `skipped` line with the reason (as the skips of `pod-services-watch`), while each other failure stays a failure; record it as a call. When a dispatch holds, the time-outs were of the host; close with that run.

## Prove

```sh
cargo test -p pod-services --all-features
cargo test -p pod-services-watch --all-features
```

A dispatch of `services.yml` with `service=localtunnel` and `service=freedos-image` passes.

## Done

2026-11-23, no change of code. The scheduled run 45159148662 of `services.yml` at `23248f8` (2026-11-23, job `watch` 137639710258) gave `holds freedos-image http-sha256: the SHA-256 of https://download.freedos.org/1.4/FD14-LiteUSB.zip is the fact's` and `holds localtunnel tls-name: localtunnel.me verified at localtunnel.me:443`. The time-outs of 2026-11-20 to 2026-11-22 were of the hosts or of their routes to the runners, not a block of the runners that stays; the watches stay as they are. Time-outs of the same watches on three scheduled runs in a row reopen this entry with step 2 of the Approach.

# T-1150: `pod-services-watch`: a watch of the facts of RFC 5780 of a STUN server (OTHER-ADDRESS, CHANGE-REQUEST, RESPONSE-PORT)

**Source:** the review of PodUtils/podcrates#256 (T-802).
**Category:** feature
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

`easybell-stun` has the facts `rfc5780` and `response-port`
(`crates/pod-services/services/easybell-stun.toml`), and `pod-natprobe`
takes a server with `rfc5780 = true` for the tests of filtering. Its only
watch is `stun-binding`, which checks a Binding success. No watch checks
that the server still gives OTHER-ADDRESS, answers a CHANGE-REQUEST from
the other address and port, and sends to the port of RESPONSE-PORT. The
fact `response-port` comes from a measurement of the author of
PodUtils/podcrates#256 alone, which this session cannot repeat (no UDP way
out).

## Premise

- Read: `crates/pod-services-watch/src/stun.rs` (the watch `stun-binding`);
  `crates/pod-turn/src/stun_nat.rs` (`NatRequestBuilder`, the readers of
  OTHER-ADDRESS and RESPONSE-ORIGIN); the list
  pradt2/always-online-stun:valid_nat_testing_hosts.txt (read 2026-11-24:
  `stun.easybell.de:3478`; the list checks OTHER-ADDRESS only).
- Measured 2026-11-24 from this session: `stun.easybell.de` A
  `138.201.243.186` (`dns.google`); a UDP Binding request got no answer (no
  UDP way out).
- Not measured: the answers of easybell to CHANGE-REQUEST and
  RESPONSE-PORT.

## Approach

1. A watch kind `stun-rfc5780` (arguments `endpoint`, `facts`): a Binding
   request; the answer has OTHER-ADDRESS with another IP and another port,
   and RESPONSE-ORIGIN equal to the source of the datagram. Then a
   request with CHANGE-REQUEST 0x06 to the primary address gets its answer
   from OTHER-ADDRESS, and one with 0x02 from the other port of the
   primary IP. With the fact `response-port`, a request with RESPONSE-PORT
   from a socket gets its answer at a second socket of the run.
2. A stand-in of two loopback sockets (two ports of 127.0.0.1) proves the
   watch, and a plant that skips the check of the source must fail its
   test.
3. Add the watch to `easybell-stun.toml`, the kind to the table of
   `crates/pod-services/README.md`, and the hosts to row L19 of
   `docs/live-calls.md`.

## Prove

```sh
cargo test -p pod-services-watch --all-features stun_rfc5780
cargo test -p pod-services --all-features
sh scripts/plant-each.sh --apply
```

A dispatch of `services.yml` with `service=easybell-stun` passes.

# T-1223: `services.yml` 45394042840: the Syncthing relay WMYMARB at a new address, the Trojan node `node-trojan-45-63-30-142` gone, and two hosts with a first no answer

**Source:** the scheduled run 45394042840 of `services.yml` at `6d3f754` (2026-11-24, job `watch` 138444977173).
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The step "Each watch" gave `213 hold, 0 skipped, 2 first with no answer, 2 moved or failed`:

- `MOVED syncthing-relay http-json`: the endpoint `relay-wmymarb` of `crates/pod-services/services/syncthing-relay.toml` is `189.146.18.236`; the service gives the host `187.142.242.141` (source: https://relays.syncthing.net/endpoint/full, read 2026-11-12).
- `FAILED node-trojan-45-63-30-142 proxy-banner`: "TLS for connectwithemployerstohireyouridealteam.com at 45.63.30.142:443: tls handshake eof". The run of 2026-11-23 gave `FAILED (first no answer)` for the same node and the same text (T-1093).
- `FAILED (first no answer) node-trojan-64-176-7-205 proxy-banner`: "64.176.7.205:443: connecting to 64.176.7.205:443 timed out after 29s".
- `FAILED (first no answer) freedos-image http-sha256`: "connecting to download.freedos.org:443 timed out after 29s".

`temp-sh http-status` held: `https://temp.sh/upload answered 405` (T-1034).

## Premise

- Read: the log of the job (the URL of the log from the GitHub MCP tool); `crates/pod-services/services/syncthing-relay.toml`; T-1093, T-1096; call C901.
- `syncthing-relay`: the pool (`https://relays.syncthing.net/endpoint/full`, read here 2026-11-24) gives the device WMYMARB (`providedBy=TacosAlPastor`, no rate limit, location MX) at `relay://187.142.242.141:443`, with `startTime` 2026-09-23 and `uptimeSeconds` 5328936 (61.7 days). The start time is the same as at the read of 2026-11-12 (49 days then): the relay did not restart, only its address changed. Call C901 removes a relay at a new address because the restart resets the uptime; here the uptime is far over the 7 days of call C104, so the relay stays with its new host.
- `node-trojan-45-63-30-142`: no answer on two scheduled runs in a row with the same text (2026-11-23 first no answer, 2026-11-24 `FAILED`). The link is still in the list of barry-far (7 lines with `@45.63.30.142:` or `@64.176.7.205:`, read here 2026-11-24): the lists are made by a robot and keep dead nodes. The proxy of this sandbox resets a connect to an IP literal: no read of the node here.
- `node-trojan-64-176-7-205`: a first no answer. It held on 2026-11-23. The rule of call C153 waits for the next scheduled run.
- `freedos-image`: a first no answer. It held on 2026-11-23 (T-1096) and timed out on 2026-11-21 and 2026-11-22. T-1096 reopens at three runs in a row; the tally is 3 of the last 4 runs, not in a row.

## Approach

- `syncthing-relay`: set the host of `relay-wmymarb` to `187.142.242.141` with the read date 2026-11-24 and the uptime of 61 days in its note (the device id stays).
- `node-trojan-45-63-30-142` leaves as `node-trojan-167-88-61-175` left (T-855) and the three nodes of T-1093: its file, and the count of the row `node-*` in `crates/pod-services/README.md` (3 to 2). The next hand-off tells podssh (PodUtils/podssh@T-574), the consumer of the nodes. Two files `node-*` stay: `node-trojan-64-176-7-205` and `node-xhttp-193-160-208-240`.
- `node-trojan-64-176-7-205` and `freedos-image` wait for the next scheduled run. When `node-trojan-64-176-7-205` fails again, it leaves in the same way. When `freedos-image` times out on the next run too, T-1096 reopens with its step 2.

## Prove

```sh
cargo test -p pod-services -p pod-services-watch --all-features
cargo run -q -p pod-services-watch -- --service syncthing-relay
```

## Done

2026-11-24. `crates/pod-services/services/syncthing-relay.toml`: the endpoint `relay-wmymarb` is `187.142.242.141` (read 2026-11-24, up 61 days) and the note of its fact names the new host. The file of the node `node-trojan-45-63-30-142` is gone; the row `node-*` of `crates/pod-services/README.md` counts 2. `cargo test -p pod-services -p pod-services-watch --all-features` passes, and `--service syncthing-relay` gives `holds` (24 endpoints). The tests: `each_syncthing_relay_is_an_ip_literal_on_443_with_its_device_id` and `the_watch_of_the_syncthing_relays_holds_each_endpoint_to_the_pool` (`crates/pod-services/tests/syncthing.rs`) and `a_proxy_node_link_is_a_form_that_podssh_reads` (`crates/pod-services/tests/public_carriers.rs`). The consumers: podssh (PodUtils/podssh@T-571, PodUtils/podssh#209) and podnet read the relay set, and podssh reads the nodes; they take the change at the next tag. Commit: see `git log` for T-1223.

# T-1415: `services.yml` 45603028501: the Trojan node `node-trojan-64-176-7-205` gone, and the mailbox of magic-wormhole with an HTTP 500

**Source:** the scheduled run 45603028501 of `services.yml` at `cd7cb5a` (2026-11-25, job `watch` 139166564269).
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The step "Each watch" gave `214 hold, 0 skipped, 0 first with no answer, 2 moved or failed`:

- `FAILED magic-wormhole-mailbox websocket-upgrade`: "wss://relay.magic-wormhole.io/v1 refused the upgrade: HTTP 500: Internal Server Error".
- `FAILED node-trojan-64-176-7-205 proxy-banner`: "64.176.7.205:443: connecting to 64.176.7.205:443 timed out after 29s".

## Premise

- Read: the log of the job (`gh run view 45603028501 --log-failed`); `crates/pod-services/services/magic-wormhole-mailbox.toml`; T-1223; calls C153 and C155.
- `node-trojan-64-176-7-205`: it held on 2026-11-23, gave a first no answer on 2026-11-24 (T-1223) and timed out again on 2026-11-25. The last scheduled run had a failure of the same class, so by call C153 the failure is a finding. T-1223 says that the node leaves in this case.
- `magic-wormhole-mailbox`: an HTTP 500 is an answer of the server, not a no answer (call C155), so call C153 does not give it a first failure: the run is red at the first one. The host and the path did not move: the watch `github-file` of the same service holds, and `public_relay.py` of upstream still names `relay.magic-wormhole.io`. The run by hand from this sandbox (`cargo run -q -p pod-services-watch -- --service magic-wormhole-mailbox`, 2026-11-25) gave the same text: `FAILED ... HTTP 500: Internal Server Error`, and the `github-file` watch held. A server on the host answers, so the service is not gone. The cause of the 500 on the upgrade is not known here: it can be a fault of the server of the host, or a change in the way that it takes an upgrade. One read from two hosts is not enough to call it a change of the service.

## Approach

- `node-trojan-64-176-7-205` leaves as `node-trojan-45-63-30-142` left (T-1223): its file, and the count of the row `node-*` in `crates/pod-services/README.md` (2 to 1). One file `node-*` of the kind `proxy-node` stays besides the node `node-xhttp-193-160-208-240`. The next hand-off tells podssh (PodUtils/podssh@T-574), the consumer of the nodes.
- `magic-wormhole-mailbox` keeps its file. The rule for the next scheduled run: when the `websocket-upgrade` watch gives a 5xx again on the next run, this entry reopens with step 2: read the upgrade with the mailbox client `pod-wormhole` against the host, and compare it with `leastauthority-mailbox`. When the watch holds, the 500 was a fault of the moment and this entry stays done. When the host gives no answer, or its upstream file names another host, the file changes by call C153.

## Prove

```sh
cargo test -p pod-services -p pod-services-watch --all-features
cargo run -q -p pod-services-watch -- --service magic-wormhole-mailbox
```

## Done

2026-11-25. The file of the node `node-trojan-64-176-7-205` is gone, and the row `node-*` of `crates/pod-services/README.md` counts 1. `magic-wormhole-mailbox` is not changed; its watch still gives HTTP 500 from this sandbox, and the next scheduled run decides by the rule in the Approach. `cargo test -p pod-services -p pod-services-watch --all-features` is the proof; the tests are `a_proxy_node_link_is_a_form_that_podssh_reads` (`crates/pod-services/tests/public_carriers.rs`) and the tests of the catalog. The consumer: podssh (PodUtils/podssh@T-574) reads the nodes and takes the change at the next tag. Commit: see `git log` for T-1415.
