# pod-core

The bottom crate of podcrates. Each other crate shares these parts, and
this crate depends on no other pod crate.

## API

| Module | What |
| --- | --- |
| `dial` | `Target` (a name and a port, or an address), `Dial` (a way out: `dial`, `dial_addrs`, `names_inside`, `datagrams`), `Resolve` (`resolve_from` with the order of its sources, `pinned`, `note_dialed`), `Connected`, `BoxedIo`, `BoxFuture` |
| `datagrams` | `Datagrams` in the shape of the UDP socket of quinn and noq, `SystemDatagrams`, `send_to`, `recv_from` |
| `failure` | `Failure` with its `Stage` (where the path broke) and its `Class` (what a caller can do) |
| `event` | `Event` (a line for the user), `Events`, `Observer` (the count of decisions), `Report` |
| `metered` | `Metered`: a stream that counts its bytes and signals at a limit |
| `backoff` | `backoff(retry)`, `CONNECT_TIMEOUT` |
| `clock` | the hint of a refused certificate, HTTP dates, `utc_minutes` |
| `text` | `one_line`, `multi_line`: text of a peer, safe to print |
| `names` | the checks of a host, a target and a node name |

A crate of podcrates prints nothing and reads no variable: it takes a
`Dial`, a `Resolve` and a `Report` from its caller, and returns a
`Failure`.

## Adoption map

| Consumer | Old path | New path |
| --- | --- | --- |
| podssh | `podssh_ws::text` | `pod_core::text` |
| podssh | `podssh_ws::clock` | `pod_core::clock` |
| podssh | `podssh_ws::names` | `pod_core::names` |
| podssh | `podssh_relay::open::{backoff, CONNECT_TIMEOUT, BACKOFF_FIRST, BACKOFF_MAX}` | `pod_core::backoff::*` |
| podnet | `podnet::edge_h2::Metered` | `pod_core::Metered` |
| podnet | `podnet::warp::register::utc_minutes` | `pod_core::clock::utc_minutes` |
| podnet | `podnet::hop::Hop` | `pod_core::Stage` (`Hop::EdgeRegister` is `Stage::EdgeRegister`, and so on); podnet keeps its hints and maps a `Failure` to its `AtHop` |
| podnet | `podnet::exit::Fault` | `pod_core::Class`; podnet keeps its exit codes and maps each class to one |
| podnet | `podnet::decisions::count` | `pod_core::Observer::decision`; podnet's observer keeps the counters |

## Sources

`SOURCES` lists the imported files: podssh `crates/podssh-ws/src/text.rs`,
`clock.rs`, `names.rs`; the `backoff` of `crates/podssh-relay/src/open.rs`;
the `Metered` of podnet `src/edge_h2.rs`; the `utc_minutes` of podnet
`src/warp/register.rs`.
