# pod-core

The bottom crate of podcrates. Each other crate shares these parts, and
this crate depends on no other pod crate.

## API

| Module | What |
| --- | --- |
| `dial` | `Target` (a name and a port, or an address), `Dial` (a way out: `dial`, `dial_http` (a request of `http://`; the default is `dial`), `dial_addrs`, `names_inside`, `datagrams`), `Resolve` (`resolve_from` with the order of its sources, `pinned`, `note_dialed`), `Found` (the addresses, their source, and `more` when a source after it may give more), `SOURCE_WAIT` (the time of a source with more after it in the race of addresses), `Connected`, `BoxedIo`, `BoxFuture` |
| `datagrams` | `Datagrams` in the shape of the UDP socket of quinn and noq, `SystemDatagrams`, `send_to`, `recv_from` |
| `failure` | `Failure` with its `Stage` (where the path broke) and its `Class` (what a caller can do) |
| `event` | `Event` (a line for the user), `Events`, `Observer` (the count of decisions), `Report` |
| `metered` | `Metered`: a stream that counts its bytes and signals at a limit |
| `files` | the private files of a program and of a user: `App` (its name and its variable), `places_from`, `candidate_dirs_from`, `chosen_from`, `Places` with the places passed over in another user's tree, `store_file_in_first`, `load_file_from`, `read_private`, `read_own`, `create_private`, `create_new_private`, `private_dir`, `safe_name`, the owners and the access lists of Unix and Windows |
| `entropy` | `Entropy`, `OsEntropy`, `NoRandom`: the random source, which a test can plant |
| `backoff` | `backoff(retry)`, `CONNECT_TIMEOUT` |
| `clock` | the hint of a refused certificate, HTTP dates, `utc_minutes` |
| `text` | `one_line`, `multi_line`: text of a peer, safe to print |
| `names` | the checks of a host, a target and a node name |

A crate of podcrates prints nothing and reads no variable: it takes a
`Dial`, a `Resolve` and a `Report` from its caller, and returns a
`Failure`.

## Exit codes and the log line

Each class of a `Failure` has one exit code, `Class::exit_code()`, on the
codes of sysexits(3). A tool of PodUtils that ends on a failure exits with
the code of its class, and names the class in its JSON. A test of this
crate holds this table to the code.

| Class | Code | When |
| --- | --- | --- |
| `usage` | 64 | a value that the caller gave cannot be used |
| `notfound` | 66 | a path or a name does not exist |
| `unavailable` | 69 | a service or a path is not available |
| `tempfail` | 75 | a service refused for now; a later try can work |
| `timeout` | 75 | a wait ended at its time limit; a later try can work |
| `protocol` | 76 | the far end broke the protocol |
| `refused` | 77 | the far end refused the credentials or the identity |
| `config` | 78 | a configuration cannot be used; do not restart |
| `revoked` | 78 | a credential that worked is gone; do not restart |
| `policy` | 79 | a local policy refused the action |
| `unsupported` | 80 | the host, the build or the far end does not support the action |

A unit of a supervisor stops a restart loop with
`RestartPreventExitStatus=64 78 79 80`.

One JSON log line of a tool has the keys of `schemas/log.v1.schema.json`:
`schema` (the constant `LOG_SCHEMA`, 1, the first key), `time` (RFC 3339, UTC), `level` (`error`, `warn`, `info`, `debug`, `trace`),
`event` (a dotted name, `relay.session.open`), `fields` (an object), and,
for a failure, `stage` and `class` (the names of `Stage` and `Class`).

## Adoption map

| Consumer | Old path | New path |
| --- | --- | --- |
| podssh | `podssh_ws::text` | `pod_core::text` |
| podssh | `podssh_ws::clock` | `pod_core::clock` |
| podssh | `podssh_ws::names` | `pod_core::names` |
| podssh | `podssh_relay::open::{backoff, CONNECT_TIMEOUT, BACKOFF_FIRST, BACKOFF_MAX}` | `pod_core::backoff::*` |
| podssh | `podssh_relay::cache::*` (the generic part; the token cache goes to `pod-relay`) | `pod_core::files::*`; each function of the process's environment takes an `App` and a lookup `var` |
| podssh | `podssh_relay::session::{Entropy, OsEntropy, NoRandom}` | `pod_core::entropy::*` |
| podnet | `podnet::edge_h2::Metered` | `pod_core::Metered` |
| podnet | `podnet::warp::register::utc_minutes` | `pod_core::clock::utc_minutes` |
| podnet | `podnet::hop::Hop` | `pod_core::Stage` (`Hop::EdgeRegister` is `Stage::EdgeRegister`, and so on); podnet keeps its hints and maps a `Failure` to its `AtHop` |
| podnet | `podnet::exit::Fault` | `pod_core::Class`; podnet keeps its exit codes and maps each class to one |
| podnet | `podnet::decisions::count` | `pod_core::Observer::decision`; podnet's observer keeps the counters |

## Sources

`SOURCES` lists the imported files: podssh `crates/podssh-ws/src/text.rs`,
`clock.rs`, `names.rs`; `crates/podssh-relay/src/cache.rs` and `cache/`; the
entropy of `crates/podssh-relay/src/session/secret.rs`; the `backoff` of
`crates/podssh-relay/src/open.rs`;
the `Metered` of podnet `src/edge_h2.rs`; the `utc_minutes` of podnet
`src/warp/register.rs`.
