# The services of other parties in podssh: the inventory

The inventory of the podssh session for T-048, at podssh `9d0623d` and the
commit after it (2026-11-05). Each line names the file and lines in
`PodUtils/podssh`, what the place holds or does, the source of its values,
and the code that reads it. "T-047" marks a place of the relay contract,
which goes to `pod-relay` (T-047), not to `pod-services`.

No hardcoded hosts in podssh for: smee.io, hostc, Tunnelmole, Sandhole,
ntfy, NTP, osewa servers, Tor bridges or directory authorities
(`crates/podssh-tor/src/bridges.rs` only parses the user's lines; Arti
brings its own directory).

## A. Lists of hosts

- `crates/podssh-relay/src/relay.rs:12-13` DEFAULT_RELAY_HOST
  `tcp.ssh.relay.ajam.dev` (operator decision 2026-10-08); `:18-34`
  RELAY_IDLE_SECS 180, SESSION_BYTE_CAP 64 MiB, SESSION_TIME_CAP 12 h from
  the relay's `/relays.json` 2026-10-03-r2; the test at `:250` ties them to
  `relay-limits.toml`.
- `crates/podssh-relay/src/pool.rs:25-34` SEED: six
  `tcp-<region>.ssh.relay.ajam.dev` hosts, read from `/relays.json`
  2026-10-08; read by `pool::*`, `relay::select_relays`.
- `crates/podssh-relay/src/pool/signed.rs:45-49` BUILT_IN keys of the signed
  deployment list, empty now; read by `pool::kept`, `pool::members`,
  podssh-cli `relay_deployments.rs`.
- `crates/podssh-ts/src/config.rs:26-29` and `crates/podssh-cli/src/ts.rs:109`:
  the default TS relay `tcp.ts.relay.ajam.dev:443`.
- `crates/podssh-cli/src/doctor/relay_checks.rs:22,27-37` MEASURED_VERSION
  "2026-10-03-r2", FORWARD_HOST `github.com`, GITHUB_KEYS (3 SHA-256, as
  `api.github.com/meta` 2026-10-08); also read by doctor's forwarder, via,
  vless, shadowsocks, exit and `relay_rank`.
- `crates/podssh-ws/src/resolve.rs:61-73` DOH_RESOLVERS: 1.1.1.1 and 1.0.0.1
  `cloudflare-dns.com`, 8.8.8.8 `dns.google`, 8.8.4.4 `dns.google.com`,
  9.9.9.9 `dns.quad9.net`, 149.112.112.112 `dns9.quad9.net` (measured
  2026-10-31, names 2026-11-01); read by `resolve`, `resolve/https.rs`,
  doctor `names.rs`, `address.rs`.
- `crates/podssh-cli/src/doctor/nat.rs:34-39` STUN DEFAULT
  `stun.cloudflare.com:3478`, `stun.l.google.com:19302`; `PODSSH_STUN`
  overrides.
- `crates/podssh-relay/src/derp/servers.rs:1-17` TAILCAT
  `tc301a`..`tc304a.ipn.dev` (`tailcat.dev/derpmap.json` 2026-10-31),
  PROBE_PATH `/derp/probe` (derper v1.104.4); read by `fleet()`,
  `ssh/derp.rs`, `node_derp.rs`, doctor `derp.rs`.
- `crates/podssh-cli/src/ts/probe.rs:28-31` DERP_MAP_HOST
  `login.tailscale.com` `/derpmap/default`; read by `ts/control.rs` and the
  ts probe.
- `crates/podssh-iroh/src/relays.rs:13-22` DEFAULT n0 relays `use1-1`,
  `usw1-1`, `euc1-1`, `aps1-1.relay.n0.iroh.link` (iroh 1.3.0 and 1.4.1
  `defaults::prod`); `crates/podssh-iroh/src/pkarr.rs:39-41` N0
  `https://dns.iroh.link/pkarr`.
- `crates/podssh-relay/src/piping/server.rs:1-10` DEFAULT_HOST `ppng.io`.
- `crates/podssh-relay/src/code/mod.rs:37-40` MAILBOXES
  `wss://relay.magic-wormhole.io/v1`, `wss://mailbox.mw.leastauthority.com/v1`.
- `crates/podssh-relay/src/transit/mod.rs:35-37` DEFAULT_URL
  `wss://relay.mw.leastauthority.com`; read by `node_transit.rs`, doctor
  `carriers.rs`.
- `crates/podssh-relay/src/opentunnel/mod.rs:43-48` DEFAULT_API
  `https://opentunnel.xyz` :443, the host built at `:102`;
  `crates/podssh-cli/src/ssh/opentunnel.rs:23` HOSTED `.opentunnel.xyz`.
- `crates/podssh-relay/src/quick/edge.rs:17-33` SRV
  `_v2-origintunneld._tcp.argotunnel.com`, REGIONS `region1` and
  `region2.v2.argotunnel.com`, PORT 7844, SERVER_NAME `h2.cftunnel.com`,
  ROOTS_FIRST_END 2029-08-15 (from cloudflared's source); `quick/api.rs:28-29`
  `api.trycloudflare.com` `/tunnel`; `quick/quic.rs:27-29`
  `quic.cftunnel.com`, ALPN `argotunnel`;
  `crates/podssh-iroh/src/pkarr_record.rs:38-39` QUICK_ZONE `trycloudflare.com`.
- `crates/podssh-warp/src/lib.rs:66-81` ENDPOINT_H2 162.159.198.2:443 (usque
  DefaultEndpointH2V4, 2026-10-31), `consumer-masque.cloudflareclient.com`,
  `cloudflareaccess.com:443`, TERMS_URL; `register.rs:27-36`
  `api.cloudflareclient.com`, `api.devices.cloudflare.com` (2026-11-02),
  API_VERSION, USER_AGENT, CLIENT_VERSION from usque; `probe.rs:19-20`
  1.1.1.1:443 (and v6) inside the tunnel.
- `crates/podssh-ws/src/forwarder/builtin.rs:1-46` BUILTIN forwarders:
  `wss://web-proxy.mudlet.org/`, `wss://ssheasy.com/p`,
  `wss://mudix.delwing.workers.dev/`, `wisp+wss://wisp.mercurywork.shop/`;
  read by podssh-cli `forwarder/mod.rs`, doctor `forwarder.rs`.
- `crates/podssh-ws/src/iap/mod.rs:34-38` `tunnel.cloudproxy.app` (GCP IAP);
  `crates/podssh-cli/src/ssh/cloud/google.rs:36-40` `oauth2.googleapis.com`;
  `cloud/iap.rs:163` 35.235.240.0/20 (text); `crates/podssh-ws/src/eice/mod.rs:35,72`
  the AWS EICE name pattern.
- `crates/podssh-cli/src/acme/mod.rs:29` LETS_ENCRYPT directory (staging only
  in the test of `acme/store.rs:157`).
- Manual text that repeats these defaults (it follows each move):
  `crates/podssh-cli/src/man/notes_public.rs:7-20`, `man/facts/roads.rs:17,205-206`,
  `man/facts/cloud.rs:41-70`, `man/notes_ssh.rs:141-155`,
  `man/notes_pair.rs:171-172,235-260`, `man/notes_doctor.rs:41`,
  `man/notes_deployments.rs:5`, `flags/derp.rs:15-25`,
  `flags/opentunnel.rs:9-14`, `flags/relay.rs:61`.

## B. Code that measures or watches

- `crates/podssh-probe` (`src/facts.rs:32,128`, `src/relay_facts.rs`): embeds
  `relay-facts.toml` and checks it against the relay's document;
  `crates/podssh-cli/src/relay_spec.rs` (`podssh relay spec`) is its front
  end. T-047.
- `crates/podssh-core/src/irc/limits.rs:49` reads `relay-facts.toml` at
  compile time (the frame cap); `crates/podssh-relay/src/serve/docs.rs:19`
  includes `podssh-probe/tests/spec/relay-spec-2026-10-03-r2.txt` (served
  by podssh's own relay role).
- The rank: `crates/podssh-relay/src/rank.rs`, `rank_auto.rs`,
  `rank_list.rs`; podssh-cli `relay_rank.rs`, `relay_rank_auto.rs`. The
  health and the pool: podssh-relay `health.rs` (`/health`), `pool.rs`
  (`/relays.json`), `pool/members.rs` (the daily read of the net's relay
  members); podssh-cli `relay_diag.rs`.
- The probes of each service: `derp/servers.rs:123` `probe()`; podssh-iroh
  `relays.rs` `/ping` failover; podssh-relay `quick/probe.rs`,
  `piping/probe.rs`; podssh-cli `ts/probe.rs`.
- The doctor (`crates/podssh-cli/src/doctor/`): `relay_checks.rs`
  (`/health`, `github.com:22` with its key check, `github_keys()` reads
  `api.github.com/meta`, the clock from `Date`), `nat.rs`, `names.rs`,
  `address.rs`, `derp.rs`, `iroh.rs`, `opentunnel.rs`, `quick.rs`,
  `carriers.rs`, `forwarder.rs`, `tor.rs`, `ipv6.rs`, `http2.rs`, `ech.rs`,
  `members.rs`, `via.rs`, `vless.rs`, `shadowsocks.rs`, `exit.rs`;
  `stream/prints.rs` holds the defaults of products of other parties
  (timeouts and caps, not hosts).
- The live tests (`#[ignore]`): podssh-relay `tests/live.rs`,
  `live_backpressure.rs`, `pair_live.rs`, `grant_live.rs`, `blocking_live.rs`,
  `reverse_live.rs`, `reverse_idle_live.rs`, `reverse_drops_live.rs`,
  `derp_live.rs`, `piping_live.rs`, `pair_code_live.rs`, `transit.rs`,
  `opentunnel_live.rs`; podssh-cli `tests/node_live.rs`, `proxy_live.rs`,
  `relay_live.rs`, `throughput_live.rs`, `tor_live.rs`; podssh-ws
  `tests/live_doh.rs`, `live_forwarder.rs`; podssh-iroh `tests/relays.rs:59,88`
  (an offline check that DEFAULT is iroh's list).

## C. Workflows

- `relay-watch.yml` (daily): `check-relay-spec.py` (T-047), `check-domain.py`
  (RDAP), `check-relay-limits.py`, `relay_watch.py` opens issues.
- `build.yml:198-245` and `:87` (each push and pull request): the live
  `check-relay-spec.py` and its plants (T-047), `check-relay-limits.py`,
  `relay-spec-agree.py`.
- `upstream.yml` (weekly): `check-upstream.py` (the cloudflared release),
  `check-vendored.py` (crates.io, GitHub), `check-release-tools.py`
  (ziglang.org, PyPI).
- `forwarder-live.yml` (daily), `pair-code.yml` (weekly), `transit-live.yml`
  (weekly), `opentunnel-live.yml` (weekly).
- By dispatch only: `doh-live.yml`, `piping.yml`, `quick-tunnel-edge.yml`,
  `quick-tunnel-live.yml`, `warp-live.yml`, `tor-live.yml`,
  `relay-measure.yml` (`relay-measure.py`), `relay-live.yml`,
  `reverse-drops.yml`, `service-live.yml`, `exit-live.yml`,
  `ts-headscale.yml`, `hop-live.yml`, `update-live.yml` (podssh's own
  releases), `iroh-ab.yml` (the operator's own server), `feel-netem.yml`
  (the osewa binary only).

## D. Scripts

- `scripts/check-relay-spec.py` (T-047), `check-relay-limits.py:36` (ORIGIN
  hardcoded), `relay-spec-agree.py:26`, `relay_sections.py`, `relay_watch.py`,
  `check-domain.py:40-42` (the RDAP bootstrap of `data.iana.org`, `rdap.org`).
- `scripts/relay-measure.py`, `relay_measure_lib.py`,
  `relay_measure_selftest.py`; `capture-close.py:38`, `capture-reverse.py:33`
  RELAY `tcp.ssh.relay.ajam.dev`.
- `scripts/check-upstream.py:42-43`, `check-vendored.py:54-55`,
  `check-release-tools.py:21-22`, `fetch-zig.sh`.
- `scripts/quick-tunnel-edge.py:39-42,137-144` (the DoH of
  `cloudflare-dns.com`, the argotunnel SRV and regions, the trycloudflare
  API, 7844, `h2` and `probe.cftunnel.com`), `quick-tunnel-probe.py`.
- `scripts/release-check-here.sh:191` IRC `irc.oftc.net:6697`,
  `irc.libera.chat:6697`, `irc.undernet.org:6667`; `:220` n0 `use1-1`;
  `release-check-catalog.sh:36`; `release-check-api.py:36`.
- `scripts/ts-derp-prove.sh:121,126`, `ts-headscale.sh:332-389`
  `tcp.ts.relay.ajam.dev`; `scripts/box/probe.sh:96` 1.1.1.1:443;
  `tor-net.sh:30-33` the sources at `gitlab.torproject.org` (a private
  network); `interop-faults.sh:19` the stand-in names
  `tcp-test-{a,b}.ssh.relay.ajam.dev`.

## E. Data files

- `crates/podssh-probe/facts/relay-facts.toml` (the origin; `[pin]` version
  2026-10-03-r2 and the SHA-256 measured 2026-10-04;
  `forward-max-frame-bytes` measured again 2026-10-31; the facts of the
  structure) (T-047); read by podssh-probe, podssh-core `irc/limits.rs`,
  `check-relay-spec.py`, `check-domain.py`, `relay-spec-agree.py`.
- `crates/podssh-probe/facts/relay-limits.toml` (the limits of
  `/relays.json`, 2026-10-31); read by `check-relay-limits.py`, the test of
  `relay.rs:250`.
- `crates/podssh-probe/tests/spec/relay-spec-2026-10-03-r2.txt` (T-047); read
  by the tests of the probe and of the CLI, `relay-spec-agree.py`,
  podssh-relay `serve/docs.rs`.
- `crates/podssh-relay/src/quick/cloudflared-pin.txt` (the tag 2026.10.6 and
  the SHA-256 of each file; read by the test of `quick/mod.rs:88-99` and by
  `check-upstream.py`), `quick/cloudflare-roots.pem` (cloudflared's
  tlsconfig at 2026.10.6).
- `crates/podssh-cli/tests/fixtures/derpmap-default-2026-10-10.json` (read by
  `tests/ts_probe.rs:17`).
- `scripts/zig-mirrors.txt` (ziglang.org's community mirrors of 2026-10-31;
  read by `fetch-zig.sh`, `check-release-tools.py`).
- `scripts/interop-baseline.txt:826-828` (`tcp.ts.relay.ajam.dev:443`),
  `:840,856` (`github.com:22`); read by `crates/podssh-todo/src/interop/mod.rs:20`.

## F. What podssh says should stay in podssh (for the owner)

- The feed and the verifier of `podssh update` (`update/mod.rs`,
  `update/sigstore.rs` ISSUER, `trust/fulcio.pem`, `rekor.pem`,
  `update-live.yml`, `release-proofs.yml`, `release-check-*`): they verify
  podssh's own releases.
- The copy of the relay contract `relay-spec-*.txt`: podssh's own relay role
  serves it (`serve/docs.rs`); keep a copy or take it from `pod-relay`.
- `forward-max-frame-bytes` of `relay-facts.toml` is read when podssh-core
  compiles: a dependency edge, or it stays.
- The rank, the pool and the health measure any relay (the user's own, the
  net's members): core logic; only the lists of hosts are of other parties.
- The upkeep of the supply chain is no list of services:
  `check-vendored*.py`, `zig-mirrors.txt`, `fetch-zig.sh`,
  `record-issues.yml`, `deny.yml`.
- The cloud APIs (GCP IAP and OAuth, AWS EICE) are the endpoints of the
  user's own accounts: they go with those features.
- On the line, for the owner: the default relay (the `ajam.dev` hosts, SEED,
  the ts relay: the operator's own deployment and the default with no
  setup), `github.com:22` with GITHUB_KEYS as doctor's fixed target of a
  probe, Let's Encrypt as the default ACME directory of the role, and the
  pins of cloudflared of `quick/` (they move only with `quick/`).
