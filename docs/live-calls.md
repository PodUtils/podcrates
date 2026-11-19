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

The rows count 54 calls: 2 replace, 12 stub + weekly, 29 keep live, and
11 infrastructure. Of the 29 that stay live, 4 get a server on the loopback
that proves the code of the client (L10, L18, L19, L35). "Each day" is the cron
of the workflow.

### Tests and live steps

| # | Call | Where | What it proves | How often | Blocked on a runner | Class | Replacement |
| --- | --- | --- | --- | --- | --- | --- | --- |
| L1 | DoH to 1.1.1.1, 1.0.0.1, 8.8.8.8, 8.8.4.4, 9.9.9.9, 149.112.112.112 on 443 | `crates/pod-dns/tests/live_doh.rs`, step "pod-dns, DoH with the public resolvers" of `live.yml` | each fallback resolver answers in both forms through TLS verified at its address | Thursday, by hand | yes: Quad9, no connection within 3 s (runs 42786042141, 42987300429) | replace | CoreDNS 1.15.0 with a DoH listener on the loopback in `ci.yml` (`scripts/ci/coredns-tests.sh`, `crates/pod-dns/tests/doh_coredns.rs`); the liveness of each resolver is the watch `doh-answer` of `services.yml`; the live test runs each week (T-350, call C156) |
| L2 | TLS 1.3 to tcp.ssh.relay.ajam.dev:443 | `crates/pod-tls/tests/live_handshake.rs`, step "pod-tls, the handshake with the live relay" | the pure provider completes a handshake with a public chain and a verified name | each day | no: each run read passed | stub + weekly | the servers of `crates/pod-tls/tests/common` on each push; the live test each week (T-352) |
| L3 | MPTCP to tcp.ssh.relay.ajam.dev:443 | `crates/pod-proxy/tests/mptcp.rs`, step "pod-proxy, Multipath TCP to the relay's edge" | a measurement: does the edge take MPTCP | each day | no; the edge answered plain TCP (run 43172235777) | replace | the loopback tests of the same file prove the code; the measurement runs by hand only (T-352) |
| L4 | OCI pull from public.ecr.aws | `crates/pod-oci/tests/live.rs`, step "pod-oci, a pull of the pinned guest userland from ECR Public" of `live.yml` | the pinned index, manifests and configs resolve and check | Thursday, by hand | yes: 429 of ECR Public (services run 42803032733; T-142) | stub + weekly | the release of distribution 3.1.2 on the loopback in `ci.yml` (`scripts/ci/registry-tests.sh`, `crates/pod-oci/tests/distribution.rs`): an index of two platforms that the test pushes, htpasswd and token auth, 401 with no login, the refusal of a blob and a manifest that do not match their digests; the watch `oci-digest` of `ecr-public-registry` holds the pinned index each day; the live test runs each week (T-353, call C186) |
| L5 | the modloop from dl-cdn.alpinelinux.org | step "pod-guest, the modloop of Alpine's virt kernel" of `live.yml`, `crates/pod-guest/tests/modloop_live.rs` | the newest modloop of Alpine 3.22 decodes and gives the module set | each day | no | stub + weekly | each week (T-352); the watch `http-sha256` of `alpine-netboot` holds the pinned file each day |
| L6 | pip from PyPI: magic-wormhole-mailbox-server 0.8.0, magic-wormhole-transit-relay 0.5.0, magic-wormhole 0.24.0 | step "pod-wormhole and pod-relay against magic-wormhole's mailbox server" of `ci.yml` (`scripts/ci/mailbox-tests.sh`) | no service: it installs the servers of the loopback tests | each push | no | infrastructure | `scripts/ci/wormhole-servers.txt` with the SHA-256 of each package, and the cache of pip (T-354, T-355) |
| L41 | the release archive `linux_amd64.tar.gz` of SeaweedFS 4.53 from github.com | step "pod-link against SeaweedFS" of `ci.yml` (`scripts/ci/s3-tests.sh`) | no service: it installs the S3-compatible server of the loopback tests of the s3 carrier (`crates/pod-link/tests/s3_seaweedfs.rs`) | each push | no | infrastructure | the SHA-256 of the archive and the tries of `scripts/retry.sh` (`scripts/ci/fetch-tool.sh`; T-623, call C540) |
| L47 | npm from registry.npmjs.org: bittorrent-tracker 11.2.3 and the packages of its lock file | step "pod-wormhole against webtorrent/bittorrent-tracker" of `ci.yml` (`scripts/ci/tracker-tests.sh`) | no service: it installs the WebTorrent tracker of the loopback tests of the meeting point (`crates/pod-wormhole/tests/tracker.rs`) | each push | no | infrastructure | `scripts/ci/bittorrent-tracker/package-lock.json` with the integrity hash (SHA-512) of each package, `npm ci --ignore-scripts --omit=optional`, and the tries of `scripts/retry.sh` (T-551, call C560) |
| L49 | the release archive `ntfy_2.37.2_linux_amd64.tar.gz` of ntfy from github.com | step "pod-wormhole against ntfy" of `ci.yml` (`scripts/ci/ntfy-tests.sh`) | no service: it installs the ntfy server of the loopback tests of the meeting point (`crates/pod-wormhole/tests/ntfy.rs`) | each push | no | infrastructure | the SHA-256 of the archive that its `checksums.txt` names, checked by `scripts/ci/fetch-tool.sh`, which keeps the archive in its directory (T-554, call C572) |
| L50 | the release archive `piping-server-x86_64-unknown-linux-musl.tar.gz` of piping-server-rust 0.18.0 from github.com | step "pod-piping against nwtgck's Piping Servers" of `ci.yml` (`scripts/ci/piping-tests.sh`) | no service: it installs the Rust Piping Server of the loopback tests of the carrier (`crates/pod-piping/tests/piping.rs`) | each push | no | infrastructure | the SHA-256 of the archive that its `sha256sums.txt` names, checked by `scripts/ci/fetch-tool.sh`, which keeps the archive in its directory (T-032) |
| L51 | npm from registry.npmjs.org: piping-server 1.12.9 and the packages of its lock file | step "pod-piping against nwtgck's Piping Servers" of `ci.yml` (`scripts/ci/piping-tests.sh`) | no service: it installs the Piping Server of ppng.io for the loopback tests of the carrier (`crates/pod-piping/tests/piping.rs`) | each push | no | infrastructure | the integrity hash of each package in `scripts/ci/piping-server/package-lock.json`, `npm ci` with no install scripts (T-032) |
| L54 | a shallow `git clone` of anomalyco/opentunnel at the tag v0.6.1 from github.com, and the crates of its lock file from crates.io; squid from the runner's apt | workflow `opentunnel-interop.yml` | no service: it builds OpenTunnel's own server, which runs on the loopback with the MySQL 8 of the runner, for `crates/pod-opentunnel/tests/opentunnel_interop.rs` | each push that changes the carrier, by hand | no | infrastructure | the tag and the `--locked` build of the server's own lock file (T-033) |
| L55 | git of chutney from gitlab.torproject.org at a pinned commit, its Python packages from PyPI, and tor from Ubuntu's archive | `scripts/tor-net.sh`, step "The private network" of `tor.yml` | no service: it installs the private Tor network of the tests of `pod-tor` (`tests/private_network.rs`, `tests/onion_service.rs`); its relays know no other network | each push of `crates/pod-tor`, each Tuesday, by hand | no | infrastructure | the pinned commit of chutney; the tests that need no network on each push in `ci.yml` (T-038) |
| L56 | Tor's own network: one bootstrap of Arti at each end, one onion service of a throwaway key, 1 MiB each way | `crates/pod-tor/tests/onion_service.rs` (`live_a_throwaway_service_on_tor_s_own_network_carries_a_mebibyte_each_way`), step "One onion service of a throwaway key on Tor's own network" of `tor.yml` | a node's onion service and its client on the real network: the publication of a descriptor, the circuits of a rendezvous | by hand (the input `onion`) | unknown: new here (podssh ran it in a workflow of its own) | stub + by hand | the private network of chutney on each push of `crates/pod-tor` (L55; T-038) |
| L7 | the public mailboxes relay.magic-wormhole.io and mailbox.mw.leastauthority.com | `crates/pod-wormhole/tests/live.rs`, `crates/pod-relay/tests/pair_code_live.rs` | two ends meet at each public mailbox | Thursday, by hand | no | keep live | a watch of the mailboxes that the consumers use with no setup (podssh Q65); the code is proven against magic-wormhole's own server on the loopback |
| L8 | the public MQTT broker public.cloud.shiftr.io | the same two files | two ends pair at each public broker | Thursday, by hand | yes: the Eclipse sandbox gave no answer (run 43172235777) | keep live | a watch of the brokers; the code is proven against Mosquitto in `ci.yml` (C95) |
| L44 | the public Nostr relays relay.primal.net and nos.lol (`wss://`, 443) | `crates/pod-wormhole/tests/live.rs` (`two_ends_meet_at_each_public_nostr_relay`), step "pod-wormhole and pod-relay, one short pairing through each public mailbox, ntfy server, Nostr relay, WebTorrent tracker and MQTT broker" of `live.yml` | two ends pair through the tag of a code at each relay of the kind `nostr-relay`: the key, the versions and a message each way (T-550) | Thursday, by hand | unknown: new; from a cloud session relay.damus.io closed each connection and nos.lol gave no answer, so neither is in the catalog (C548) | stub + weekly | the stand-in relay of `crates/pod-wormhole/tests/harness/nostr.rs` proves the client on each push (`crates/pod-wormhole/tests/nostr.rs`, `crates/pod-relay/tests/pair_code.rs`); no relay of another party publishes a static release (C549) |
| L46 | the public WebTorrent trackers tracker.openwebtorrent.com and tracker.webtorrent.dev (`wss://`, 443) | `crates/pod-wormhole/tests/live.rs` (`two_ends_meet_at_each_public_webtorrent_tracker`), step "pod-wormhole and pod-relay, one short pairing through each public mailbox, ntfy server, Nostr relay, WebTorrent tracker and MQTT broker" of `live.yml` | two ends pair through the swarm of a code at each tracker of the kind `webtorrent-tracker`: the key, the versions, messages each way and the giver's last word at its close (T-551) | Thursday, by hand | unknown: new; from a cloud session both paired; tracker.files.fm:7073 (not on 443) gave no tunnel, tracker.btorrent.xyz answered 502, so neither is in the catalog (C557) | stub + weekly | the stand-in tracker of `crates/pod-wormhole/tests/harness/tracker.rs` proves the client on each push (`crates/pod-wormhole/tests/tracker.rs`, `crates/pod-relay/tests/pair_code.rs`), and webtorrent/bittorrent-tracker 11.2.3 on the loopback in `ci.yml` (`scripts/ci/tracker-tests.sh`, L47) |
| L48 | the public ntfy servers ntfy.sh, ntfy.envs.net and ntfy.adminforge.de (`https://`, 443) | `crates/pod-wormhole/tests/live.rs` (`two_ends_meet_at_each_public_ntfy_server`), step "pod-wormhole and pod-relay, one short pairing through each public mailbox, ntfy server, Nostr relay, WebTorrent tracker and MQTT broker" of `live.yml` | two ends pair through the topic of a code at each server of the kind `ntfy-server`, on HTTP/2 and on HTTP/1.1: the key, the versions and a message each way; 14 of the 250 messages a day that ntfy.sh takes from one address (T-554) | Thursday, by hand | unknown: new; from a cloud session each paired in both forms; ntfy.jae.fi got no tunnel from the proxy (502), so it is not in the catalog (C569) | stub + weekly | the stand-in server of `crates/pod-wormhole/tests/harness/ntfy.rs` (HTTP/1.1, and HTTP/2 behind TLS in `ntfy_tls.rs`) proves the client on each push (`crates/pod-wormhole/tests/ntfy.rs`, `crates/pod-relay/tests/pair_code.rs`), and ntfy 2.37.2 on the loopback in `ci.yml` (`scripts/ci/ntfy-tests.sh`, L49) |
| L52 | the public Piping Server ppng.io (`https://`, 443); by hand, each public Piping Server of pod-services | `crates/pod-piping/tests/piping_live.rs` (`a_session_through_ppng_io_carries_64_kib_each_way`; `the_rates_of_both_forms_through_the_public_servers` with the input `measure` = `piping-rates`), step "pod-piping, a session through ppng.io" of `live.yml` | a session of the resumable layer carries 64 KiB each way through a node and a client of the job; the rates of the streamed form and of the numbered pieces (T-032) | Thursday, by hand | unknown: new | stub + weekly | nwtgck's two servers on the loopback prove the carrier on each push (`crates/pod-piping/tests/piping.rs`, `scripts/ci/piping-tests.sh`, L50, L51), with a stand-in proxy that holds each body to its end |
| L53 | OpenTunnel's service opentunnel.xyz (`https://`, 443: the API, a bridge, a tunnel's name) | `crates/pod-opentunnel/tests/opentunnel_live.rs`, step "pod-opentunnel, a tunnel of opentunnel.xyz" of `live.yml` | a node makes a tunnel with a throwaway key, a client reaches it by TLS by name and echoes 256 KiB, a second start keeps the tunnel, the tunnel is deleted; a planted route must fail (T-033) | Thursday, by hand | unknown: new | stub + weekly | the stand-in server of `crates/pod-opentunnel/tests/opentunnel_stand_in/mod.rs` on each push (`crates/pod-opentunnel/tests/opentunnel_link.rs`), and OpenTunnel's own server on the loopback (L54) |
| L9 | the owner's relay and its pool (tcp.ssh.relay.ajam.dev) | `crates/pod-relay/tests/live.rs` and the other `*_live.rs` of `pod-relay`, job `relay` of `live.yml` | the contract of the live relay: mint, pair, reverse, idle, drops, grants | by hand | no: runs 43221128869 and 43221537246 passed from a direct egress (T-327) | keep live | the stand-ins of `crates/pod-relay/tests/stand_in` and `crates/pod-relay/tests/e2e_harness` prove the client; the self-hosted relay of T-036 later (T-361) |
| L10 | the GSRN relay gs.thc.org | `crates/pod-gsr/tests/live.rs` | 1 MiB crosses the real relay | by hand, in no workflow | unknown | keep live | the stand-in of `crates/pod-gsr/tests/loopback.rs`; gs-netcat 1.4.43 as the other end (T-357) |
| L11 | GitHub's evaluator of expressions | T-280 (open) | the conformance of `pod-actions-expr` and `pod-actions-workflow` | planned | no | keep live | only GitHub's runner evaluates as GitHub does |
| L42 | about 15 HTTP Range requests (38 KB) to the Windows Server 2019 evaluation ISO at software-download.microsoft.com | `crates/pod-udf/tests/windows_live.rs`, step "pod-udf, boot.wim of the Windows evaluation ISO by Range requests" of `live.yml` | `Udf::find` of `/sources/boot.wim` in a UDF 1.02 volume of a real mastering tool: the size, the WIM magic and the SHA-256 of the first and the last block | Thursday, by hand | unknown: new | stub + weekly | the images of genisoimage and mkudffs in `crates/pod-udf/tests/data` (`tests/tools.rs`) and the images of `src/tests` on each push; the watch `http-sha256` of `microsoft-windows-eval` holds the primary volume descriptor each day (T-620, call C541) |
| L43 | a download of the whole Windows Server 2019 evaluation ISO (5 GB) at software-download.microsoft.com, checked by its size and SHA-256; the Ubuntu archive (ovmf, xorriso, qemu) | `crates/pod-guest/tests/windows_keep_live.rs`, `guest-windows.yml` | an unattended install of Server Core with the agent of `pod-guest`, then the next commands of a guest in keep-alive mode on new mailbox media through QMP (T-602, call C542), and a warm start with a command on a new medium where the QEMU of the runner saves the plan (T-628, call C586) | by hand | unknown: new | keep live | the walk of the agent and the tests of `swap` against a QMP stand-in and the QEMU of the host on each push and each week (`guest.yml`) |
| L45 | the management API of Dev Tunnels (global.rel.tunnels.api.visualstudio.com, the host of a cluster) and the relay URI of a tunnel (`wss://`) | `crates/pod-devtunnel/tests/live.rs`, step "pod-devtunnel, a stream through the relay to a tunnel of the owner" of `live.yml` (input `measure`: `devtunnel`; the secrets `POD_DEVTUNNEL_TUNNEL`, `POD_DEVTUNNEL_PORT` and, for a tunnel with no anonymous access, `POD_DEVTUNNEL_ACCESS_TOKEN`) | the connect information, the relay upgrade, the host key of a real host, a `direct-tcpip` stream to an SSH server and a reconnect, with no account on the client side (T-386, T-489) | by hand, while the owner hosts the tunnel | unknown: no run yet | keep live | the stand-in of the crate (feature `standin`: the management API, the relay and the host) proves each form of credential on each push; only the live service proves the values that the vendor's client gives |
| L35 | a TURN server of the caller (UDP or TLS on TCP) | `crates/pod-turn/tests/live.rs` (ignored; `POD_TURN_LIVE_SERVER`) | an allocation, a Refresh, a permission and a channel on a server of the caller | by hand, in no workflow | unknown | keep live | coturn 4.18.0 on the loopback in `ci.yml` (`scripts/ci/turn-tests.sh`, `crates/pod-turn/tests/coturn.rs`) proves the client over UDP and over TLS on each push: the 401, the 438, the 437 and the 441, the maximum lifetime, a Send and a ChannelData echo with its padding (T-380, call C197) |
| L36 | the WARP API (api.cloudflareclient.com) and its MASQUE front door on TCP 443, then the trace of 1.1.1.1 over HTTPS through the session | `crates/pod-warp/tests/live.rs`, step "pod-warp, a session at the WARP front door" of `live.yml` | a new device registers, a session opens and `Live` holds it, the trace says `warp=on` or `warp=plus`, and the device is removed | Thursday, by hand | unknown | stub + weekly | the stand-in API of `pod_testkit::stand::warp` and the stand-in edge of `crates/pod-warp/tests/common/edge.rs` on the loopback prove the registration, the session, `Live`, the probe and the trace on each push (`tests/register.rs`, `tests/net.rs`, `tests/live_session.rs`; T-339) |
| L38 | the public MQTT broker public.cloud.shiftr.io: a board of retained messages | `crates/pod-wormhole/tests/board_live.rs`, step "pod-wormhole, a board of retained messages at each public MQTT broker" of `live.yml` | a retained note is put, taken and cleared at each public broker (PodUtils/podssh@T-738) | Thursday, by hand | unknown: new | stub + weekly | the stand-in of `crates/pod-wormhole/tests/harness/broker.rs` (it keeps retained messages) and Eclipse Mosquitto in `ci.yml` (`scripts/ci/mosquitto-tests.sh`, `crates/pod-wormhole/tests/board.rs`) prove the board on each push (T-417) |
| L39 | an upload, a download and a delete of 32 KiB at the public file hosts tmpfiles.org, litterbox.catbox.moe and temp.sh (0x0.st only when the input `file-hosts` names it) | `crates/pod-services-watch/tests/file_host_live.rs`, step "pod-services, a round trip through each public file host" of `live.yml` | the forms of the facts of the kind `file-host`: the upload form, the URL of the answer, the download (tmpfiles.org: the signed link of the page), the delete by token | Thursday, by hand | yes: litterbox 412 and 0x0.st 429 or 503 to podssh's run (PodUtils/podssh#230); run 43448958418 of this step: tmpfiles.org and temp.sh hold, litterbox 412 "Invalid uploader" (not on by default since, C285) | stub + weekly | the stand-ins of each form in `crates/pod-services-watch/tests/watches/file_host.rs` prove the code on each push |

### `services.yml`

`crates/pod-services-watch` runs 215 watches of 93 service files each day
and by hand. A watch is a check of a fact of another party: each one is
**keep live** by its purpose. Its offline tests run each watch kind against
the stand-ins of `crates/pod-testkit/src/stand.rs`
(`crates/pod-services-watch/tests/watches`).

| # | Kinds | Hosts | Blocked on a runner | Class | Note |
| --- | --- | --- | --- | --- | --- |
| L12 | `http-json`, `http-text`, `http-status`, `http-sha256`, `crate-file` | the relay, GitHub, tailscale, syncthing, tox, npm, PyPI, FreeDOS, Microsoft, crates.io, the pages of the WebTorrent trackers, the health, the limits and the `config.js` of the ntfy servers, the categories of domain.web.pkgforge.dev and more | no | keep live | |
| L13 | `tls-name`, `tls-names`, `tls-roots` | the relay, Cloudflare, localtunnel, trycloudflare, WARP | no (IPv6 addresses: yes, a route probe skips them) | keep live | |
| L14 | `websocket-upgrade`, `forward-banner` | croc, the mailboxes, the Nostr relays, the WebTorrent trackers, shiftr, the forwarders, Yggdrasil peers | no | keep live | |
| L15 | `rdap` | rdap.org and the servers of IANA's bootstrap | no | keep live | |
| L16 | `oci-ping`, `oci-digest` | 8 registries | yes: 429 of ECR Public (run 42803032733) | keep live | one more try after `Retry-After` |
| L17 | `github-file`, `github-files`, `github-release`, `github-ssh-keys` | api.github.com, raw files | no | keep live | `GITHUB_TOKEN` of the job |
| L18 | `ssh-banner`, `ssh-host-key`, `forward-url` | serveo, pinggy, localhost.run, sandhole, ssh-j on 22 and 443 | no | keep live | sish on the loopback proves the code of `forward-url` (T-356) |
| L19 | `stun-binding` | stun.cloudflare.com:3478, stun.l.google.com:19302 (UDP) | no: each run read holds them | keep live | coturn 4.18.0 on the loopback in `ci.yml` proves the STUN codec of `pod_proxy::stun` (`scripts/ci/turn-tests.sh`, `crates/pod-proxy/tests/stun_coturn.rs`: the mapped address, FINGERPRINT, a 420; T-358) |
| L20 | `doh`, `doh-answer`, `dns-addresses`, `dns-srv` | Cloudflare, Google, Quad9, argotunnel.com | IPv6: yes, skipped by the route probe | keep live | these watches take the liveness of L1 |
| L21 | `proxy-banner`, `proxy-list` | 8 Trojan and VLESS nodes; 3 lists on raw.githubusercontent.com | unknown: timeouts in 4 runs | keep live | a "no answer" is a finding on the second run in a row (T-351, call C153) |
| L37 | `http-text`, `http-status`, `tls-name` of the kind `file-host` | tmpfiles.org, litterbox.catbox.moe, temp.sh, 0x0.st | 0x0.st: yes, its upload answered 429 or 503 at once to a GitHub runner (PodUtils/podssh#230); litterbox's upload answered 412; the reads: no; 0x0.st answers 500 to automated reads, so its watch is `tls-name` alone | keep live | the pages of the facts (with the fields of litterbox's upload form) and the status of a GET of each upload form; no upload: the round trip is L39 (T-416, T-528) |
| L40 | `transit-relay` | transit.magic-wormhole.io:4001 (TCP) | unknown: the first scheduled run measures it | keep live | magic-wormhole-transit-relay 0.5.0 on the loopback proves the code of the watch (`crates/pod-services-watch/tests/watches/transit.rs`, in `scripts/ci/mailbox-tests.sh`; T-548) |
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
| L31 | crates.io: the index and the crates | each cargo step | each run | no | infrastructure | the cache of the registry of each job, keyed by the lock files (T-355, done) |
| L32 | zig from the mirrors; `cargo install` of cargo-zigbuild and cargo-fuzz | `ci.yml` (static), `nightly.yml` (cross), `adoption.yml`, `fuzz.yml` | each run | the mirrors move (`upstream.yml`, job `mirrors`) | infrastructure | the cache by SHA-256; the release binaries of the tools by SHA-256 (T-355, done) |
| L33 | the images of Docker Hub (Mosquitto, coturn, TruffleHog, binfmt) | `scripts/pull-image.sh` in `ci.yml`, `secrets.yml`, `nightly.yml` | each run | yes: the limit of Docker Hub | infrastructure | the mirrors of the script hold the same digest |
| L34 | the Ubuntu archive (qemu) | `guest.yml`, `plants.yml` | each week; on a push to the checks | no | keep live | the packages of the runner's own image; a failure is rare, and `scripts/retry.sh` holds it |

### The crates of the import and of the plan

| Crate | Live hosts | Plan |
| --- | --- | --- |
| `pod-warp` (T-028) | api.cloudflareclient.com, consumer-masque.cloudflareclient.com:443, the WireGuard door on UDP 2408 | stand-ins of the API, of the MASQUE door and of `pod-wg` on each push; the kernel's WireGuard as an independent peer; a live run each week (T-359) |
| `pod-cloudflare` (T-029) | region1 and region2.v2.argotunnel.com:7844 (QUIC and HTTP/2), api.trycloudflare.com | a stand-in edge on the loopback on each push; one quick tunnel each week (T-360) |
| `pod-devtunnel` (T-378) | global.rel.tunnels.api.visualstudio.com and the host of a cluster (HTTPS), the relay URI of a tunnel (`wss://`) | the stand-in of the crate (feature `standin`: the management API, the relay and the host, with each form of credential) on each push; `tests/live.rs` is ignored and takes the user's token, a tunnel access token or none (a tunnel with anonymous access, T-489), never a shared account; its step in `live.yml` runs by hand while the owner hosts the tunnel (L45, T-386); the facts are watched by `services.yml` |
| `pod-actions-store` (T-564) | github.com over HTTPS (`git ls-remote`, a shallow `git fetch`) | a local bare repository made in each test (`tests/git.rs`) and a forge in memory on each push; `tests/store.rs` is ignored and runs in the job `live` of `live.yml` (step "pod-actions-store, two actions by tag from github.com") |
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
| coturn 4.18.0 | BSD-3-Clause | `images/coturn/Dockerfile` (image `coturn/coturn:4.18.0`, index `sha256:bbefd3e1fdfdc0d58770fe01b581fd8b00d9f3a5580d00acb77cf719a6bc78e3`, the same on Docker Hub's API and in its registry) | TURN over UDP and over TLS on TCP, and STUN Binding on UDP: `scripts/ci/turn-tests.sh` runs the image with the host's network and the options of call C197, and a second container that redirects to the first (call C567) | S10 |
| gs-netcat 1.4.43 | BSD-2-Clause | `gs-netcat_linux-x86_64`, SHA-256 `55d141bb2f62c527c3fea3413b8429fed7d52633d0b123b5e611bfa42165229f` (measured through the fetch proxy; check it at the first download) | the other end of `pod-gsr` | S11 |
| workerd v1.20261112.1 | Apache-2.0 | its release `workerd-linux-64.gz` | the Worker of the relay, when one exists | S12 |
| actions/cache 6.1.0 | MIT | commit `55cc8345863c7cc4c66a329aec7e433d2d1c52a9` | the cache of the downloads | S13 |
| SeaweedFS 4.53 | Apache-2.0 | `linux_amd64.tar.gz` of its release, SHA-256 `d1a7dec9f48d8b33895a0ea8951971defc2dc5312906dcc76fa5a5f74a6ee90d` (the digest of GitHub's API, checked at the download) | an S3-compatible server: `weed mini` with a bucket and a made-up key, for the s3 carrier of `pod-link` (`scripts/ci/s3-tests.sh`, call C540) | S15 |
| bittorrent-tracker 11.2.3 | MIT | `scripts/ci/bittorrent-tracker/package-lock.json` (the package `sha512-i9pWOf1Y...`, and the integrity hash of each of its dependencies) | a WebTorrent tracker over a WebSocket: `scripts/ci/bittorrent-tracker/serve.mjs` starts its `Server` with the WebSocket alone (call C560) | S16 |
| ntfy 2.37.2 | Apache-2.0 or GPL-2.0 | `ntfy_2.37.2_linux_amd64.tar.gz` of its release, SHA-256 `e67f53d7fbed529b83faf3993b584110921ef3567b0ac0b033c6910879a4ffd1` (the sum of its `checksums.txt`, checked at the download); a static binary | an ntfy server with its cache in a file: `scripts/ci/ntfy-tests.sh` starts `ntfy serve` on 127.0.0.1 (call C572) | S17 |
| piping-server 1.12.9 | MIT | `scripts/ci/piping-server/package-lock.json` (the package `sha512-mHfePSyu...`, and the integrity hash of each of its dependencies) | the Piping Server of ppng.io (Node.js): `crates/pod-piping/tests/piping_harness/mod.rs` starts it on a free port of 127.0.0.1 | S18 |
| piping-server-rust 0.18.0 | MIT | `piping-server-x86_64-unknown-linux-musl.tar.gz` of its release, SHA-256 `4a17e197829c3f56e47316cc9e02d72113f266671561f285c853e36fbbc60863` (the sum of its `sha256sums.txt`, checked at the download); a static binary | a Piping Server that speaks HTTP/1.1 and HTTP/2 by prior knowledge: `crates/pod-piping/tests/piping_harness/mod.rs` starts it on a free port of 127.0.0.1 | S18 |
| opentunnel-server 0.6.1 | MIT | the tag `v0.6.1` of anomalyco/opentunnel, built with its own lock file | OpenTunnel's server with MySQL 8 and its local test CA (`ISSUER=local`): `opentunnel-interop.yml` runs it on 127.0.0.1 | S19 |
| cargo-zigbuild 0.23.4 | MIT | `cargo-zigbuild-x86_64-unknown-linux-musl.tar.xz`, SHA-256 `9e3cf73485edbd45905c8aadbc0fdf869c7ddc3848f0c898229f2680db52e44b` | in place of `cargo install` | S14 |

Not chosen:

- The relay of gsocket (hackerschoice/gsocket-relay, `gsrnd`): its LICENSE
  gives no permission to use the code (S11). The stand-in of `pod-gsr`
  stays the relay.
- MinIO: its repository is archived (AGPL-3.0), and dl.min.io answers 410
  "archived and no longer maintained" in place of a release (S15).
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
  https://hub.docker.com/v2/repositories/coturn/coturn/tags/4.18.0 and the
  manifest list of `registry-1.docker.io` (the same index digest); the
  image's `turnserver -h`, and `src/server/ns_turn_server.c` at
  `1986df21e4b4152b5e0216989f649e20ff76aced` (the revision label of the
  image: the answers are keyed with SHA-1 alone, 437 and 441 for another
  user, the padding of ChannelData on a stream).
- S11: https://github.com/hackerschoice/gsocket (BSD-2-Clause; release
  v1.4.43; `lib/gsocket-engine.c` reads `GSOCKET_IP` and `GSOCKET_PORT`);
  https://github.com/hackerschoice/gsocket-relay (LICENSE: no permission).
- S12: https://github.com/cloudflare/workerd/releases (v1.20261112.1,
  Apache-2.0).
- S13: https://github.com/actions/cache/releases (v6.1.0, 2026-06-26; the
  tag points at the commit above).
- S14: https://github.com/rust-cross/cargo-zigbuild/releases/tag/v0.23.4
  (2026-09-02, MIT, the file `.sha256` of the asset).
- S15 (read on 2026-11-14): https://github.com/seaweedfs/seaweedfs/releases/tag/4.53
  (2026-11-08, Apache-2.0, the `digest` of each asset in GitHub's API);
  `weed help mini` of the release (the flags of the ports, `-bucket`,
  `-s3.config`); https://github.com/minio/minio (archived, AGPL-3.0) and
  https://dl.min.io/server/minio/release/linux-amd64/ (410).
- S16 (read on 2026-11-14): https://github.com/webtorrent/bittorrent-tracker
  at `v11.2.3` (`server.js`, `lib/server/parse-websocket.js`,
  `lib/server/swarm.js`, `package.json`: MIT, WebTorrent LLC);
  https://registry.npmjs.org/bittorrent-tracker/-/bittorrent-tracker-11.2.3.tgz
  (the integrity hash of the lock file).
- S17 (read on 2026-11-14): https://github.com/binwiederhier/ntfy/releases/tag/v2.37.2
  (2026-11-04; `checksums.txt` names the SHA-256 of each archive; the
  binary of `linux_amd64` is a static ELF; LICENSE and LICENSE.GPLv2: the
  server is Apache-2.0 or GPLv2). v2.38.0 came on 2026-11-14 and is not
  taken: a pin is at least some days old.
- S18 (read on 2026-11-15): https://github.com/nwtgck/piping-server-rust/releases/tag/v0.18.0
  (2024-03-01; `sha256sums.txt` names the SHA-256 of each archive; MIT,
  Ryo Ota); https://registry.npmjs.org/piping-server/1.12.9 (MIT; the
  integrity hash of the lock file); both are the servers that podssh's
  workflow of the Piping Server road ran.
- S19 (read on 2026-11-15): https://github.com/anomalyco/opentunnel at
  `v0.6.1` (docs/protocol.md, docs/server.md, crates/opentunnel-server;
  MIT), the server that podssh's interop workflow of the OpenTunnel road
  built.
- The runs: the jobs and logs of `live.yml` and `services.yml` runs named
  above, read through forge.web.pkgforge.dev.
