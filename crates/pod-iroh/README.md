# pod-iroh

The iroh road between two ends: QUIC between Ed25519 keys (iroh 1.4),
through an iroh relay (HTTPS on port 443, upgraded to a WebSocket) when UDP
is refused, and directly when a probe shows that a datagram comes back. A
session is one QUIC stream that carries the records of the resumable layer
(`pod-session`), so that it goes on across a lost relay connection, a new
address of either end, or another road. A node lets in only the client keys
of its allowlist (`pod-identity`). A node keeps a record at a server of
pkarr that names its home relay and its other roads (DERP, the quick
tunnel, Pinggy, the tunnel of `ssh -R`, an onion service, OpenTunnel), so
that a client with an old ticket, or with the node's key alone, finds it.

The base is podssh's `crates/podssh-iroh` (PodUtils/podssh@T-162,
PodUtils/podssh@T-163, PodUtils/podssh@T-350, PodUtils/podssh@T-445,
PodUtils/podssh@T-665 and others), with a copy of `iroh-relay` 1.4.2 and
podssh's patch in `vendor/` (see "The fork of iroh-relay"). iroh builds with `tls-ring`, so no aws-lc is in the tree
(`cargo tree -i aws-lc-sys` finds none); the endpoint takes the caller's
provider all the same. The static probe links it and makes an endpoint with
the pure-Rust provider of `pod-tls`.

## API

| Module | What |
| --- | --- |
| `endpoint` | `Options { relays, tls, resolve, proxy, trust, udp, secret, accepts, relay_tls, qad_port, quic }`, `Options::new(tls, resolve)`; `bind(&options)`, `open(&options)` (the endpoint, the answer of the probe of UDP, the arm of QUIC: `Opened { endpoint, udp, quic, kept }`, `quic_lines`); `Udp` (`Probe`, `On`, `Off`, `from_value`); `BindError` (`Proxy`, `Trust`, `Iroh`); `default_relays()`; `UDP_ENV` |
| `client` | `Dialer::new(endpoint, addr)`, `finding(finder)`, `link`, `link_as(lane)`, `moved`, `path`, `connection_id`, `refused`, `close`; `carry`, `carry_heard`; `Failure` (`Refused`, `Failed`); `CONNECT_LIMIT`, `FIND_AFTER`, `RELAY_ONLY`, `WENT_DIRECT` |
| `far` | `serve(endpoint, settings, budget, open, admit)`, `serve_kept(.., keeper)` (a keeper that another road shares, and that says its budget's lines) |
| `stream`, `share` | `open_session`, `open_lane`, `accept_session`, `Lane`, `Stream`; `share::lease`, `Lease`: one connection to a node for the sessions of a process |
| `ticket` | `parse` (`iroh:TICKET` or iroh's ticket alone), `format`, `of`, `home_relay`, `follow` |
| `keys` | `load(places, place, entropy)`, `load_in`, `Key`, `identity_key`, `fingerprint`, `name`; the places of `pod_identity::file` |
| `allow` | `Allowlist` (`parse`, `read`, `admits`, `bad_lines`); `REFUSED`, `REFUSED_REASON` |
| `home_relay` | `say_changes(endpoint, say)`: a line when the connection to a home relay ends and iroh dials it again, one for each new cause of a failed dial, and one when it is back, with the time of the gap (PodUtils/podssh#283, ask 4); `Relays::look`, `Status` |
| `relays` | `default_urls`, `defaults`, `parse_url`, `parse_list`, `select(flag, variable)`, `Source`; `ping`, `ping_kept`, `home(relays, trust, way)`; `ENV` |
| `pkarr` | `Server`, `Choice`, `choice(value)`, `server_for`, `n0_relays`; `publish`, `lookup`, `lookup_with`, `read`; `keep`, `keep_roads` (a node's record, put again as its roads change); `Finder::new(server, trust, way)`; `record::{Roads, Record, packet, quick_name, pinggy_address, tunnel_name, onion_address, opentunnel_name}`; `ENV` |
| `pinggy` | `Address` (`NAME#SHA256:PIN`), `Pin` (`of_spki`, `parse`), `is_name`: the form of a Pinggy address of a record (call C767) |
| `probe` | `udp()`, `round_trip`, `Answer`, `REPORT_LIMIT` |
| `resolve` | `Chain::new(resolve)`: the caller's `pod_core::Resolve` as iroh's resolver |
| `tuning` | `Tuning` (`IROH`, `TUNED`, `from_value`, `config`, `words`; `Default` is the arm of `DEFAULT`), `Congestion`, `WORDS`, `QUIC_ENV` |
| `version` | `ask`, `ask_with`, `notice(relay, words, remedy)`, `refusal(relays, trust, way, remedy)`, `online(endpoint, limit, trust, way, remedy)`, `is_online`, `Answer`, `Offline`, `IROH` |
| `test_relay`, `test_pkarr` | feature `test-relay`: iroh's relay server and a server of pkarr on the loopback; `tls()`, `resolver()`, `way(choice)`, `direct()`, `options()`: the way out of a test |

`ALPN` (`podssh/1`), `ALPN_LANES` (`podssh/2`) and `FEATURES` are at the
root.

## Features

| Feature | What |
| --- | --- |
| `test-relay` | iroh's relay server (`iroh/test-utils`, `iroh-relay/server`), a server of pkarr and the way out of a test, for the tests of this crate and of a consumer. A consumer turns it on in `[dev-dependencies]` only; no binary carries it |

## Settings

The crate reads no variable of the environment. The consumer reads its own
and fills these values.

| Value | What |
| --- | --- |
| `Options::tls` | the TLS context of the binary (`pod_tls::Context`): its provider, for QUIC and for the relay's TLS, and the roots of `trust` |
| `Options::resolve` | the resolver of the relay's name and of the proxy's name (`pod-dns`, or `pod_proxy::resolve::System`) |
| `Options::proxy` | the proxy of the relay's dial (`pod_proxy::ProxyChoice`): iroh dials the relay itself and takes a proxy's URL; a choice of the environment is asked for the first relay. HTTP CONNECT or SOCKS5 on TCP; a proxy on a Unix socket or at a command is refused |
| `Options::trust`, `relay_tls` | the roots of the relay's certificate; a verifier of an embedder or a test |
| `Options::udp` | `Udp::from_value` of the consumer's `PODSSH_IROH_UDP` (`UDP_ENV`) |
| `Options::quic` | `Tuning::from_value` of the consumer's `PODSSH_IROH_QUIC` (`QUIC_ENV`); `None` is the arm `tuned` |
| `Options::secret`, `accepts`, `qad_port`, `relays` | the key, whether the end takes sessions, a port of QUIC address discovery of a test, the relays |
| `way: &pod_ws::Way` | the way out of each request of the crate: `/ping`, pkarr, the question of the version; its user agent goes in each request |
| `relays::select(flag, variable)` | the consumer's `--iroh-relay` and `PODSSH_IROH_RELAY` (`relays::ENV`) |
| `pkarr::choice(value)` | the consumer's `PODSSH_IROH_PKARR` (`pkarr::ENV`) |
| `remedy` of `version` | the consumer's words of what a user does when a relay refuses this iroh |

The TXT name `_podssh` of the record and the ALPNs `podssh/1` and
`podssh/2` are names of the wire of podssh's nodes, which this crate keeps:
podssh is its consumer (call C767).

## Parity

Each part of the source, as it is here:

- [x] the endpoint: no address lookup, the relays given, the proxy of the
  relay's dial (HTTP CONNECT, with the refusal of credentials that iroh
  would send wrong; SOCKS5 through the patched iroh-relay), the resolver
  and the trust of the caller, UDP after a round trip of QUIC address
  discovery, or after a bind, or never;
- [x] the client: a stream of the connection for each link, a new
  connection after a lost one, the node's record asked when the ticket's
  relay does not reach the node, the refusal of a key, the path that
  carried the session;
- [x] the far end: the sessions of each admitted connection, kept across
  links by one keeper, a refused key closed before any stream;
- [x] the lanes: one connection for the sessions of a process, a copy's
  stream below a terminal's, an end of before;
- [x] the tickets, the keys and the allowlist;
- [x] the relays: the table, the flag before the variable, `/ping` and the
  first relay that answers;
- [x] pkarr: the record of iroh with the other roads beside it, its
  signature checked, the server of n0 for n0's relays, a node that keeps
  its record as its roads change;
- [x] QUIC's settings and the arms of the A/B run;
- [x] the question of a refused version, and its notice.

The differences: the way out of each request is a `pod_ws::Way`, and the
proxy of the relay's dial a `pod_proxy::ProxyChoice` (podssh: a
`ProxyChoice` of podssh-ws that read the environment); the resolver is the
caller's (podssh: podssh-ws's chain); the endpoint takes the provider of
the caller's context (podssh: `presets::Minimal`, aws-lc-rs); iroh builds
with `tls-ring` (podssh: `tls-aws-lc-rs`); `Udp::from_environment`,
`Tuning::from_environment`, `relays::from_environment`,
`pkarr::choice_from_environment` and `pkarr::choose` are gone, and
`BindError::Quic` with them (the consumer parses its variable); `far::serve`
says nothing of the budget (podssh: its `told()`); the notice of a refused
version takes the consumer's words; a request of the version question names
the user agent of the way (podssh: `podssh/VERSION`); the Pinggy address of
a record is `pod_iroh::pinggy::Address` (podssh: `podssh_relay::pinggy`);
`tests/proxy_env.rs` reads an environment of its own, and its planted
defect is an endpoint with no proxy, which iroh's own selection gives of
that environment (podssh: it set the process's variables).

The relay dial follows `pod-proxy`'s rule of the schemes: a `socks5://`
proxy gets an address of the relay, which the caller's resolver gives, and
a `socks5h://` proxy gets the name (T-808; podssh's patch sent the name for
both, PodUtils/podssh@T-280).

## Adoption map

| Consumer | Old path | New path |
| --- | --- | --- |
| podssh | `podssh_iroh::{..}` | `pod_iroh::{..}`, the same names |
| podssh | `Options { proxy: ProxyChoice::FromEnvironment, trust, .. }`, `Options::default()` | `Options::new(podssh_ws::tls::context().clone(), podssh_ws::resolve::resolver())` with `proxy = podssh_ws::dial::settings(&ProxyChoice::FromEnvironment, false).proxy` (a `pod_proxy::ProxyChoice::FromEnvironment` of the process's variables), `udp`, `quic`, then `relays`, `trust`, `secret`, `accepts`: one function of podssh-cli (`iroh_way::options`) |
| podssh | `relays::home(relays, trust, &proxy)`, `relays::ping(.., &proxy, ..)`, `relays::ping_kept(.., &proxy, ..)` | `relays::home(relays, trust, &way)`, `relays::ping(.., &way, ..)`, `relays::ping_kept(.., &way, ..)`; the way is `podssh_ws::client::way(&ProxyChoice::FromEnvironment)` |
| podssh | `pkarr::{publish, lookup, keep, keep_roads}(.., proxy, ..)`, `Finder::new(server, trust, proxy)` | the same with `way` (`pod_ws::Way`) |
| podssh | `version::{refusal, online}(.., &proxy)`, `version::notice(relay, words)` | `(.., &way, REMEDY)`, `notice(relay, words, REMEDY)`: podssh keeps its words ("podssh update brings a newer podssh, or --iroh-relay URL names a relay that serves this one") |
| podssh | `keys::load(place, entropy)` | `keys::load(places, place, entropy)`: `podssh_relay::cache::pod_places()`, also in the tests of podssh-cli |
| podssh | `Udp::from_environment()` | `Udp::from_value(var("PODSSH_IROH_UDP").as_deref())` |
| podssh | `Tuning::from_environment()`, `BindError::Quic` | `Tuning::from_value(var("PODSSH_IROH_QUIC").as_deref())` before `Options::quic`, podssh's own error ("the QUIC settings of the iroh road: ..."); `None` is the default arm, not the variable |
| podssh | `relays::from_environment(flag)` | `relays::select(flag, var("PODSSH_IROH_RELAY"))` |
| podssh | `pkarr::choose(relays)`, `pkarr::choice_from_environment()` | `pkarr::server_for(&pkarr::choice(var("PODSSH_IROH_PKARR").as_deref())?, relays)` |
| podssh | the variables, read in podssh-iroh | read by name in podssh-cli, as the test of the manual's facts finds a variable by its quoted name; a unit test compares each name with `relays::ENV`, `pkarr::ENV`, `endpoint::UDP_ENV`, `tuning::QUIC_ENV`. The scan of the facts reads that file only in a build with the feature `iroh` |
| podssh | `far::serve(..)` with podssh's `budget::told()` | `far::serve_kept(.., Arc::new(Keeper::new(deadline).with_tell(told())))` |
| podssh | `record.pinggy: podssh_relay::pinggy::Address` | `pod_iroh::pinggy::Address`: podssh's `Address::parse(&address.to_string())` |
| podssh | `resolve::Podssh` | `resolve::Chain::new(resolver)` |
| podssh | `iroh` with `tls-aws-lc-rs` in podssh-cli's dependencies (the types) | no TLS feature: this crate takes `tls-ring`, and the endpoint takes podssh's provider. podssh-cli takes `pod-ws` too, for the type of the way |
| podssh | `crates/podssh-iroh/tests/`, `vendor/iroh-relay`, `vendor/patches/iroh-relay`, the `[patch.crates-io]` of its root | the tests of this crate; the fork and the `[patch.crates-io]` of podcrates' root. podssh keeps a `[patch.crates-io] iroh-relay` of its own (a `[patch]` of a git dependency does not reach its consumer): `{ git = "https://github.com/PodUtils/podcrates", tag = "vX.Y.Z" }`. Cargo finds the package in `vendor/iroh-relay`, which the workspace of podcrates excludes (measured, T-810). The lock then names that git source: podssh's scan of the patches (`scripts/repo_scans.py`) takes it |
| podssh | its plants `t445-record-no-derp`, `t611-name-not-read`, `t665-quic-unset`, `t666-share-while-made` | the plants of this crate (`scripts/plants/iroh-*`): the files that they change are here |

## The fork of iroh-relay

- **Upstream**: the crate `iroh-relay` 1.4.2 as crates.io publishes it
  (SHA-256
  `ffa03fd9a865148bd5d2f4a617232bcb77cf23956da50ef4f8204d6772f72c79`, the
  `cksum` of the crates.io index), from n0-computer/iroh at
  `2131f266da11d37700e7afb6b6b59925cdaa7037` (its `.cargo_vcs_info.json`). License: MIT OR Apache-2.0, with the
  BSD-3-Clause notice of the code that it took from Tailscale
  (`LICENSE-BSD3`).
- **The tree**: `vendor/iroh-relay` is that crate byte for byte (but
  cargo's marker `.cargo-ok`), with the patches of
  `vendor/patches/iroh-relay/` applied in order: `0001-socks5-proxy.patch`,
  as podssh carried it at `0370b0b` (`SOURCES`), and
  `0002-socks5-resolves-here.patch` of podcrates (T-808).
- **The patch**: the relay dial (`dial_url_proxy`) spoke HTTP `CONNECT`
  only. A `socks5://` or `socks5h://` proxy gets the exchange of RFC 1928,
  with no TLS of its own: for `socks5h://` the relay's name with the
  address type 3, for `socks5://` an address of the relay from the
  client's resolver (as curl and `pod-proxy` take the schemes), the
  user and password of RFC 1929 from the URL, the reply read to its end,
  and on reply 8 one more connection with an address; 10 s for the
  exchange; port 1080 when the URL names none. reqwest's feature `socks`
  is on, so that iroh's HTTPS probe of the relays goes through the same
  proxy. `tests/proxy_env.rs` proves it.
- **How the workspace uses it**: `[patch.crates-io]` of the root
  `Cargo.toml`; `exclude` keeps it out of the workspace, so its tests and
  their dependencies are not the workspace's. Only `pod-iroh` builds it.
- **To carry it to another version**: download
  `https://static.crates.io/crates/iroh-relay/iroh-relay-VERSION.crate`,
  compare its SHA-256 with the `cksum` of that version in
  `https://index.crates.io/ir/oh/iroh-relay`, and unpack it into
  `vendor/iroh-relay` (remove `.cargo-ok`). Run
  `git apply --directory=vendor/iroh-relay vendor/patches/iroh-relay/*.patch`.
  When a hunk applies with an offset, make that patch again from the
  unpacked crate, so that each patch applies with no offset. Move the
  ranges of `iroh` and `iroh-dns` in this crate, and run
  `cargo update -p iroh -p iroh-relay`. An iroh that asks for another
  iroh-relay leaves the patch unused: cargo says so, and the SOCKS5 test of
  `tests/proxy_env.rs` fails.

## Tests

`cargo test -p pod-iroh --all-features`: 72 tests, each with its name of
the source. The unit tests: the credentials of a proxy, the probe of UDP
against iroh's relay server, the arms of QUIC, the record and its checks,
the choice of a server of pkarr, the notice of a refused version.
`tests/road.rs` (a session through a stand-in CONNECT proxy and the relay,
20 MiB each way), `tests/new_address.rs` (the client's tunnels go silent
in an echo of 8 MiB and its new address has no network for 45 s: the
session resumes whole, and the lines of the home relay come),
`tests/share.rs` (the lanes), `tests/tickets.rs` (iroh's
own ticket and the allowlist), `tests/keys.rs`, `tests/relays.rs` (the
order of the relays, `/ping`, SOCKS5 with a password),
`tests/proxy_env.rs` (`ALL_PROXY` alone, SOCKS5 by name for `socks5h://`, by address for `socks5://`),
`tests/pkarr_lookup.rs` (a node that moved, a forged record, iroh's own
client of pkarr), `tests/record_roads.rs` (a record with each road, a
client with the node's key alone over the stand-in DERP server of
`pod-derp`), `tests/version.rs`. Each runs on the loopback.

## Sources

`SOURCES` lists each file of podssh at `0370b0b` and the file that it
became.
