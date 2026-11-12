# Live calls

This page lists each call of the CI and of the tests of podcrates that
reaches a host of another party. It gives the class of each call and the
stand-in that replaces it. The rule is call C150 (`docs/decisions.md`): a
call stays live only when its purpose is to watch the service of another
party, or when no stand-in can prove it. Each other call goes to a stand-in
in the test, to a server of another party on the loopback of the job (call
C151), or to a cache (call C152).

Why: the runners of GitHub reach the network from more than one cloud, with
no IPv6, and some hosts do not answer them. A red run that is not ours
hides a red run that is ours.

## The classes

- **replace**: a stand-in or a server on the loopback proves the same
  thing. The call leaves CI.
- **stub + weekly**: the stand-in proves the code on each push. One live
  run each week (`live.yml`, Thursday, and by hand) proves the real
  service.
- **keep live**: the call is a watch of the service, or no stand-in can
  prove it. The reason is in the row.
- **infrastructure**: a download of a tool or of a dependency. A cache and
  a pinned checksum make it rare (T-355).

## The runners of GitHub

- Linux runners of 2 and 4 cores run on AWS or Azure (a multi-cloud pool).
  Their address is not in one cloud's ranges. Windows runners run on Azure.
  Inbound ICMP is blocked (S1).
- A runner has no IPv6 route (S2). The watches of `services.yml` met it:
  5 failures of IPv6 addresses in run 42803032733. The runner now probes a
  route first.
- A service can give no answer to the addresses of one cloud: the Eclipse
  MQTT sandbox timed out from a runner in `live.yml` run 43172235777, and
  left the catalog (call C94).
- A registry limits the shared addresses of the runners: ECR Public gave
  429 in `services.yml` run 42803032733; Docker Hub refuses pulls past a
  limit (`scripts/pull-image.sh`).
- Quad9's DoH at 9.9.9.9 and 149.112.112.112 gave no connection within
  3 s in `live.yml` runs 42786042141 and 42987300429 (each failed), and
  once more in run 43172235777 (a retry passed).
- Three Trojan nodes timed out after 29 s in `services.yml` runs
  42803032733, 42806121501, 42933757949 and 43145957804. A scheduled run
  holds the first "no answer" of a watch, and fails on the second in a
  row (T-351, calls C153 and C155).

## The inventory

The rows count 34 calls: 2 replace, 3 stub + weekly, 24 keep live, and
5 infrastructure. Of the 24 that stay live, 3 get a server on the loopback
that proves the code of the client (L10, L18, L19). "Each day" is the cron
of the workflow.

### Tests and live steps

| # | Call | Where | What it proves | How often | Blocked on a runner | Class | Replacement |
| --- | --- | --- | --- | --- | --- | --- | --- |
| L1 | DoH to 1.1.1.1, 1.0.0.1, 8.8.8.8, 8.8.4.4, 9.9.9.9, 149.112.112.112 on 443 | `crates/pod-dns/tests/live_doh.rs`, step "pod-dns, DoH with the public resolvers" of `live.yml` | each fallback resolver answers in both forms through TLS verified at its address | Thursday, by hand | yes: Quad9, no connection within 3 s (runs 42786042141, 42987300429) | replace | CoreDNS 1.15.0 with a DoH listener on the loopback in `ci.yml` (`scripts/ci/coredns-tests.sh`, `crates/pod-dns/tests/doh_coredns.rs`); the liveness of each resolver is the watch `doh-answer` of `services.yml`; the live test runs each week (T-350, call C156) |
| L2 | TLS 1.3 to tcp.ssh.relay.ajam.dev:443 | `crates/pod-tls/tests/live_handshake.rs`, step "pod-tls, the handshake with the live relay" | the pure provider completes a handshake with a public chain and a verified name | each day | no: each run read passed | stub + weekly | the servers of `crates/pod-tls/tests/common` on each push; the live test each week (T-352) |
| L3 | MPTCP to tcp.ssh.relay.ajam.dev:443 | `crates/pod-proxy/tests/mptcp.rs`, step "pod-proxy, Multipath TCP to the relay's edge" | a measurement: does the edge take MPTCP | each day | no; the edge answered plain TCP (run 43172235777) | replace | the loopback tests of the same file prove the code; the measurement runs by hand only (T-352) |
| L4 | OCI pull from public.ecr.aws | `crates/pod-oci/tests/live.rs`, step "pod-oci, a pull of the pinned guest userland" | the pinned index, manifests and configs resolve and check | each day | yes: 429 of ECR Public (services run 42803032733; T-142) | stub + weekly | distribution 3.1.2 (`registry:3`) on the loopback in `ci.yml` (T-353); the live test each week |
| L5 | the modloop from dl-cdn.alpinelinux.org | step "pod-guest, the modloop of Alpine's virt kernel" of `live.yml`, `crates/pod-guest/tests/modloop_live.rs` | the newest modloop of Alpine 3.22 decodes and gives the module set | each day | no | stub + weekly | each week (T-352); the watch `http-sha256` of `alpine-netboot` holds the pinned file each day |
| L6 | pip from PyPI: magic-wormhole-mailbox-server 0.8.0, magic-wormhole 0.24.0 | step "magic-wormhole's mailbox server and library, in a virtualenv" of `live.yml` | no service: it installs the servers of the loopback tests | each day | no | infrastructure | a requirements file with hashes and the cache of pip; the tests move to `ci.yml` (T-354) |
| L7 | the public mailboxes relay.magic-wormhole.io and mailbox.mw.leastauthority.com | `crates/pod-wormhole/tests/live.rs`, `crates/pod-relay/tests/pair_code_live.rs` | two ends meet at each public mailbox | Thursday, by hand | no | keep live | a watch of the mailboxes that the consumers use with no setup (podssh Q65); the code is proven against magic-wormhole's own server on the loopback |
| L8 | the public MQTT broker public.cloud.shiftr.io | the same two files | two ends pair at each public broker | Thursday, by hand | yes: the Eclipse sandbox gave no answer (run 43172235777) | keep live | a watch of the brokers; the code is proven against Mosquitto in `ci.yml` (C95) |
| L9 | the owner's relay and its pool (tcp.ssh.relay.ajam.dev) | `crates/pod-relay/tests/live.rs` and the other `*_live.rs` of `pod-relay`, job `relay` of `live.yml` | the contract of the live relay: mint, pair, reverse, idle, drops, grants | by hand | unknown: T-327 | keep live | the stand-ins of `crates/pod-relay/tests/stand_in` and `crates/pod-relay/tests/e2e_harness` prove the client; the self-hosted relay of T-036 later (T-361) |
| L10 | the GSRN relay gs.thc.org | `crates/pod-gsr/tests/live.rs` | 1 MiB crosses the real relay | by hand, in no workflow | unknown | keep live | the stand-in of `crates/pod-gsr/tests/loopback.rs`; gs-netcat 1.4.43 as the other end (T-357) |
| L11 | GitHub's evaluator of expressions | T-280 (open) | the conformance of `pod-actions-expr` and `pod-actions-workflow` | planned | no | keep live | only GitHub's runner evaluates as GitHub does |

### `services.yml`

`crates/pod-services-watch` runs 138 watches of 71 service files each day
and by hand. A watch is a check of a fact of another party: each one is
**keep live** by its purpose. Its offline tests run each watch kind against
the stand-ins of `crates/pod-testkit/src/stand.rs`
(`crates/pod-services-watch/tests/watches`).

| # | Kinds | Hosts | Blocked on a runner | Class | Note |
| --- | --- | --- | --- | --- | --- |
| L12 | `http-json`, `http-text`, `http-status`, `http-sha256`, `crate-file` | the relay, GitHub, tailscale, syncthing, tox, npm, PyPI, FreeDOS, Microsoft, crates.io and more | no | keep live | |
| L13 | `tls-name`, `tls-names`, `tls-roots` | the relay, Cloudflare, localtunnel, trycloudflare, WARP | no (IPv6 addresses: yes, a route probe skips them) | keep live | |
| L14 | `websocket-upgrade`, `forward-banner` | croc, the mailboxes, shiftr, the forwarders, Yggdrasil peers | no | keep live | |
| L15 | `rdap` | rdap.org and the servers of IANA's bootstrap | no | keep live | |
| L16 | `oci-ping`, `oci-digest` | 8 registries | yes: 429 of ECR Public (run 42803032733) | keep live | one more try after `Retry-After` |
| L17 | `github-file`, `github-files`, `github-release`, `github-ssh-keys` | api.github.com, raw files | no | keep live | `GITHUB_TOKEN` of the job |
| L18 | `ssh-banner`, `ssh-host-key`, `forward-url` | serveo, pinggy, localhost.run, sandhole, ssh-j on 22 and 443 | no | keep live | sish on the loopback proves the code of `forward-url` (T-356) |
| L19 | `stun-binding` | stun.cloudflare.com:3478, stun.l.google.com:19302 (UDP) | no: each run read holds them | keep live | coturn on the loopback proves the STUN client (T-358) |
| L20 | `doh`, `doh-answer`, `dns-addresses`, `dns-srv` | Cloudflare, Google, Quad9, argotunnel.com | IPv6: yes, skipped by the route probe | keep live | these watches take the liveness of L1 |
| L21 | `proxy-banner`, `proxy-list` | 8 Trojan and VLESS nodes; 3 lists on raw.githubusercontent.com | unknown: timeouts in 4 runs | keep live | a "no answer" is a finding on the second run in a row (T-351, call C153) |
| L22 | `scripts/services/edge-facts.sh` (job `measure`) | region1 and region2.v2.argotunnel.com on 7844, 443, 8443, 80 | no | keep live | the facts of the edge that `pod-cloudflare` needs |

### Other workflows

| # | Call | Where | How often | Blocked on a runner | Class | Replacement |
| --- | --- | --- | --- | --- | --- | --- |
| L23 | clones of the source repositories on github.com | job `sources` of `upstream.yml` | each day | no | keep live | its purpose is the watch of the sources |
| L24 | ziglang.org's list of mirrors | job `mirrors` of `upstream.yml` | each day | no | keep live | a watch of the list |
| L25 | api.github.com: the numbers of the issues | `record-issues.yml` | each day | no | keep live | a watch of the inbox |
| L26 | api.github.com, crates.io: the tag, the release, the publish | `release.yml`, `publish.yml` | by hand, on a tag | no | keep live | the release itself |
| L27 | the advisory database of RustSec | `deny.yml` | each push, each day | no | keep live | the advisories must be current |
| L28 | clones of podssh, podnet and podvm at their pinned commits | `adoption.yml` (`scripts/adoption.sh`) | each push that changes a crate | no | keep live | the code of the consumers; their crates come from the cache (T-355) |
| L29 | crates.io's API: the reserved names | `scripts/check-crates-io-names.py` | by hand | no | keep live | a watch of the names |
| L30 | rustup: static.rust-lang.org | each job (`rustup toolchain install`) | each run | no | infrastructure | a download through `scripts/retry.sh`, not cached (call C185) |
| L31 | crates.io: the index and the crates | each cargo step | each run | no | infrastructure | the cache of the registry of each job, keyed by the lock files (T-355) |
| L32 | zig from the mirrors; `cargo install` of cargo-zigbuild and cargo-fuzz | `ci.yml` (static), `nightly.yml` (cross), `adoption.yml`, `fuzz.yml` | each run | the mirrors move (`upstream.yml`, job `mirrors`) | infrastructure | the cache by SHA-256; the release binaries of the tools by SHA-256 (T-355) |
| L33 | the images of Docker Hub (Mosquitto, TruffleHog, binfmt) | `scripts/pull-image.sh` in `ci.yml`, `secrets.yml`, `nightly.yml` | each run | yes: the limit of Docker Hub | infrastructure | the mirrors of the script hold the same digest |
| L34 | the Ubuntu archive (qemu) | `guest.yml`, `plants.yml` | each week; on a push to the checks | no | keep live | the packages of the runner's own image; a failure is rare, and `scripts/retry.sh` holds it |

### The crates of the import and of the plan

| Crate | Live hosts | Plan |
| --- | --- | --- |
| `pod-warp` (T-028) | api.cloudflareclient.com, consumer-masque.cloudflareclient.com:443, the WireGuard door on UDP 2408 | stand-ins of the API, of the MASQUE door and of `pod-wg` on each push; the kernel's WireGuard as an independent peer; a live run each week (T-359) |
| `pod-cloudflare` (T-029) | region1 and region2.v2.argotunnel.com:7844 (QUIC and HTTP/2), api.trycloudflare.com | a stand-in edge on the loopback on each push; one quick tunnel each week (T-360) |
| `pod-devtunnel` (T-378) | global.rel.tunnels.api.visualstudio.com and the host of a cluster (HTTPS), the relay URI of a tunnel (`wss://`) | the stand-in of the crate (feature `standin`: the management API, the relay and the host) on each push; `tests/live.rs` is ignored and takes the token of the user who runs it, never a shared account; its run in `live.yml` waits for the owner's token (T-386); the facts are watched by `services.yml` |
| `pod-transit` (T-027) | the transit relays of magic-wormhole | magic-wormhole-transit-relay 0.5.0 on the loopback (T-354) |
| Tor, Syncthing's relay protocol, bore, localtunnel | no client in podcrates | the watches of `services.yml` only |

## The chosen servers

Each server runs on 127.0.0.1 only, from a pinned artifact, started by a
script of `scripts/ci/` that fails on a skip, as
`scripts/ci/mosquitto-tests.sh` does (calls C95, C151).

| Server | Licence | Pin | Use | Source |
| --- | --- | --- | --- | --- |
| Eclipse Mosquitto 2.1.2 | EPL-2.0 or EDL-1.0 | `images/mosquitto/Dockerfile` (index `sha256:75d6662d...`, the same on Docker Hub) | MQTT over a WebSocket (in place) | S3 |
| CoreDNS 1.15.0 | Apache-2.0 | image `coredns/coredns:1.15.0`, index `sha256:0a9d8c8ddf34347eb59aad9a8784f0710de88d6d112726d11d271f9bc6daca26`; release `coredns_1.15.0_linux_amd64.tgz`, SHA-256 `2495c5b8f3fab6d0a336c397f1227b75406d86e6ec7125744a29df9e936ab8cd` | DoH (the server block `https://`, plugin `tls`): the release archive, which the test starts with its own zone and certificate (call C156) | S4, S5 |
| distribution 3.1.2 | Apache-2.0 | image `registry:3.1.2`, index `sha256:82f6ff4a7f982960c8aa51617fea3aabc485df6e55cce80d22deb1de36f1bc3c` | an OCI registry | S6 |
| magic-wormhole-mailbox-server 0.8.0 | MIT | wheel SHA-256 `e03f6a60284c6174baf4d94ee880f90071c1d01afe7f65380fa5f20ff0f01ad4` | the mailbox (in place, daily) | S7 |
| magic-wormhole-transit-relay 0.5.0 | MIT | wheel SHA-256 `e2a378a84e6bdb3e317d1e0dfaf02e9f4a92e30aa5686c5e4ef27209c3ac0b57` | the transit relay | S7 |
| magic-wormhole 0.24.0 | MIT | wheel SHA-256 `312d7d55cba76fe443fd207b1e006d12b7c56699961ed6dce46bed765945e712` | the peer of the library (in place) | S7 |
| sish 4.0.0 | MIT | image `antoniomika/sish:v4.0.0`, index `sha256:475a4943def8fdb3f28b5c58fa26ba74009266981e049cd125fdac9916cd8f44`; `sish-4.0.0.linux-amd64.tar.gz`, SHA-256 `3adb57cc008cb1d95d178a0ec0ceb67cf0d3645c0004d0d789011ce7dc01956f` | an SSH forward relay | S8 |
| sandhole 0.10.5 | AGPL-3.0 (run, not linked) | `sandhole-linux-amd64` of its release | a second SSH forward relay | S9 |
| coturn 4.18.0 | BSD-3-Clause | image `coturn/coturn:4.18.0`, index `sha256:bbefd3e1fdfdc0d58770fe01b581fd8b00d9f3a5580d00acb77cf719a6bc78e3` | STUN Binding on UDP | S10 |
| gs-netcat 1.4.43 | BSD-2-Clause | `gs-netcat_linux-x86_64`, SHA-256 `55d141bb2f62c527c3fea3413b8429fed7d52633d0b123b5e611bfa42165229f` (measured through the fetch proxy; check it at the first download) | the other end of `pod-gsr` | S11 |
| workerd v1.20261112.1 | Apache-2.0 | its release `workerd-linux-64.gz` | the Worker of the relay, when one exists | S12 |
| actions/cache 6.1.0 | MIT | commit `55cc8345863c7cc4c66a329aec7e433d2d1c52a9` | the cache of the downloads | S13 |
| cargo-zigbuild 0.23.4 | MIT | `cargo-zigbuild-x86_64-unknown-linux-musl.tar.xz`, SHA-256 `9e3cf73485edbd45905c8aadbc0fdf869c7ddc3848f0c898229f2680db52e44b` | in place of `cargo install` | S14 |

Not chosen:

- The relay of gsocket (hackerschoice/gsocket-relay, `gsrnd`): its LICENSE
  gives no permission to use the code (S11). The stand-in of `pod-gsr`
  stays the relay.
- A self-hosted Tor network (chutney): no crate of podcrates speaks Tor.
- Syncthing's `strelaysrv` and `stdiscosrv`: no crate speaks their
  protocol; `services.yml` reads their pool only.
- A MASQUE server of another party: none speaks Cloudflare's form of
  connect-ip; the stand-in comes from podssh's `podssh-warp` (T-028).
- A service container of GitHub: it starts before the checkout, so it
  cannot read a configuration of the repository (call C95).

## Sources

Each source was read on 2026-11-12, through the services of
web.pkgforge.dev (forge, read, search, fetch).

- S1: https://docs.github.com/en/actions/concepts/runners/github-hosted-runners
  ("Cloud hosts used by GitHub-hosted runners": AWS or Azure; "Inbound
  ICMP packets are blocked"), and
  https://docs.github.com/en/actions/reference/runners/github-hosted-runners
  ("IP addresses": not limited to Azure ranges).
- S2: https://github.com/actions/runner-images/issues/668 ("IPv6 on
  GitHub-hosted runners"; the last comments, 2026-09-18, ask for it).
- S3: https://hub.docker.com/v2/repositories/library/eclipse-mosquitto/tags/2.1.2-alpine
  (the index digest of `images/mosquitto/Dockerfile`);
  https://github.com/eclipse-mosquitto/mosquitto (the files `epl-v20` and
  `edl-v10`).
- S4: https://coredns.io/plugins/tls (the DoH server block `https://.`
  with `tls cert key`), https://coredns.io/plugins/https;
  https://github.com/coredns/coredns/releases/tag/v1.15.0 (2026-10-21,
  Apache-2.0).
- S5: https://hub.docker.com/v2/repositories/coredns/coredns/tags/1.15.0
  (pushed 2026-10-21).
- S6: https://github.com/distribution/distribution/releases (v3.1.2,
  2026-09-24, Apache-2.0);
  https://hub.docker.com/v2/repositories/library/registry/tags/3.1.2.
- S7: https://pypi.org/pypi/magic-wormhole-mailbox-server/json,
  .../magic-wormhole-transit-relay/json, .../magic-wormhole/json (the
  versions, MIT, the SHA-256 of each wheel). No official image exists.
- S8: https://github.com/antoniomika/sish/releases/tag/v4.0.0 (2026-10-27,
  MIT, its checksums file); https://docs.ssi.sh/getting-started (the flags
  `--ssh-address`, `--http-address`, `--domain`, `--authentication`);
  https://hub.docker.com/v2/repositories/antoniomika/sish/tags/v4.0.0.
- S9: https://github.com/EpicEric/sandhole/releases/tag/v0.10.5
  (2026-10-31, AGPL-3.0).
- S10: https://github.com/coturn/coturn (LICENSE: the three clauses of BSD;
  release 4.18.0, 2026-09-08);
  https://hub.docker.com/v2/repositories/coturn/coturn/tags/4.18.0.
- S11: https://github.com/hackerschoice/gsocket (BSD-2-Clause; release
  v1.4.43; `lib/gsocket-engine.c` reads `GSOCKET_IP` and `GSOCKET_PORT`);
  https://github.com/hackerschoice/gsocket-relay (LICENSE: no permission).
- S12: https://github.com/cloudflare/workerd/releases (v1.20261112.1,
  Apache-2.0).
- S13: https://github.com/actions/cache/releases (v6.1.0, 2026-06-26; the
  tag points at the commit above).
- S14: https://github.com/rust-cross/cargo-zigbuild/releases/tag/v0.23.4
  (2026-09-02, MIT, the file `.sha256` of the asset).
- The runs: the jobs and logs of `live.yml` and `services.yml` runs named
  above, read through forge.web.pkgforge.dev.
