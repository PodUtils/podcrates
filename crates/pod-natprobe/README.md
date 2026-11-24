# pod-natprobe

A probe of the NAT on the path of a host (T-802, GitHub #212). It gives
the class of each end that NAT traversal (T-499) needs. It is new code
from the RFCs: RFC 5780 (the mapping, the filtering, the lifetime of a
mapping), RFC 4787 (the port allocation, the IP pooling, hairpinning),
RFC 6052 and RFC 7050 (NAT64), RFC 6886 (NAT-PMP), RFC 6887 (PCP) and
UPnP IGD. The STUN codec is `pod_turn::stun`, with the attributes of
RFC 5780 (`crates/pod-turn/src/stun_nat.rs`).

The crate reads no variable of the environment, keeps no process state
and prints nothing. The sockets, the clock, the entropy, the resolver,
the events and the list of servers are the caller's.

## Design

* **No I/O in the core.** `engine::ProbeMachine` is a state machine:
  `start`, `poll_output` (a bind, a datagram, a DNS64 query, an HTTP
  request of UPnP), `handle_input`, `poll_timeout`, `handle_timeout` and
  `poll_event` (a decided question, the final report). Time is a tick in
  microseconds that the caller gives. With `default-features = false` the
  crate builds with no tokio (`examples/static-probe-lean`).
* **One socket for each question.** The mapping tests use the socket `M`;
  the filtering tests use a new socket `F`; the port allocation and the
  hairpin tests use new sockets. A test does not open a pinhole on the
  socket of another test.
* **A timeout is not a finding.** Each property is an `Obs<T>`:
  `Observed` with its evidence (the ids of the observations), `Unknown`
  with its reason and a partial constraint, or `NotApplicable`. A silence
  alone gives `Unknown`.
* **Untrusted replies.** A URL of UPnP is the gateway's IP alone
  (`wire::ssdp::gateway_url`), and the caller's HTTP client gets its
  canonical form; an OTHER-ADDRESS must have the class of the server's
  address; a hairpin datagram goes only to a public or shared address. The
  report keeps at most 256 observations of dropped datagrams and counts
  the rest (`Report::dropped_unrecorded`).
* **Positive controls.** A restrictive filtering class needs a control
  that proves that the server sends from the other address: the socket
  `F` opens a pinhole to that address on the same mapping, and the request
  goes again. A control on another mapping (a dependent mapping) is
  `Invalid`, and the class stays `Unknown`.

## The tests of RFC 5780

| Question | Requests | Result |
| --- | --- | --- |
| Mapping | `M` to PP; to AP; to AA (the alternate address of `OTHER-ADDRESS`); PP again | `M1` = the local address: no NAT. `M2` = `M1`: endpoint-independent. `M3` = `M2`: address-dependent, else address-and-port-dependent. A second `M1` that differs: unstable |
| Mapping, no RFC 5780 server | `M` to two servers or more | the same mapped address: endpoint-independent; two ports of one server IP decide the dependent class; else `Unknown` with the constraint "not endpoint-independent" |
| Filtering | `F` to PP; `CHANGE-REQUEST` 0x06 (other IP and port) and 0x02 (other port) | 0x06 answered: endpoint-independent. 0x02 answered and the control of 0x06 valid: address-dependent. 0x02 silent and the control of 0x02 valid: address-and-port-dependent |

A reply from the wrong source (a server that ignores `CHANGE-REQUEST`),
an error 420 or 400, and a server with no `OTHER-ADDRESS` give `Unknown`
with the reason, and the server gets a flag in the report.

## Use

```rust
use std::sync::Arc;
use pod_natprobe::{Prober, Profile, Seams, ServerEndpoint, Settings};

let servers = vec![ServerEndpoint::rfc5780("easybell", addr)];  // resolved by the caller
let settings = Settings::for_profile(Profile::Standard, servers);
let mut seams = Seams::system();          // system UDP, clock and entropy
seams.report_sink = report;               // the caller's events
let report = Prober::new().run(settings, seams).await?;
let v4 = report.v4.as_ref();              // connectivity, mapping, filtering, allocation, ...
let line = report.to_string();            // one line for the user
let json = report.to_json()?;             // feature `report`
let wire = report.compact_profile().encode(); // 20 bytes for a peer
```

`run::system::resolve_pod_services_stun(catalog, resolver)` gives the STUN
servers (`kind = "stun"`) of the caller's `pod_services::Catalog` through
the caller's `pod_core::Resolve`. `strategy::predict` gives the traversal strategy of
two peers from their classes; `strategy::plan_spray` and
`strategy::birthday_exact` plan a spray of ports.

## Profiles

| Profile | Limit | What |
| --- | --- | --- |
| `Fast` | 600 ms | the mapping and the positive findings; no control: a restrictive filtering stays `Unknown` (`Pending`) |
| `Standard` | 15 s | the controls, the port allocation, the IP pooling, hairpinning, IPv6 and NAT64, the gateway |
| `Deep` | 600 s | and the lifetime of a mapping (`RESPONSE-PORT`) and the idle gap of a port counter |

## Features

| Feature | Default | What |
| --- | --- | --- |
| `run` | yes | `Prober` on tokio, `SystemUdpFactory`, `SystemNetenv`, the STUN servers of `pod-services` |
| `report` | yes | the JSON of `Report` (serde) |
| `sim` | no | the simulator: NAT layers (each mapping, filtering, allocation, pooling, hairpin and lifetime class), STUN servers (RFC 5780, plain, and broken ones), a gateway, loss and delay, in virtual time |

## Settings

`Settings::for_profile` fills each field for a profile. The fields: the
servers of each family, `addr_policy` (`Lab` accepts the private and
loopback addresses of a test), the retransmission ladder (`Interactive`
or the RFC 8489 ladder), the token bucket (RFC 5780, section 5: 10
requests each second), `max_duration_ms`, the allocation samples and
their pace, the switches of the hairpin punch, of the port mapping (read only:
the probe sends no `MAP`), of UPnP over HTTP, of NAT64 and of IPv6, the lifetime settings, `software`, `jitter_seed` and
`corroborated_online`.

## Failures

`NatError` is a probe that cannot run (settings, a second run on one
`Prober`, no entropy, no socket); it maps to `pod_core::Failure`
(`Stage::Host` or `Stage::Connect`, `Class::Usage` or
`Class::Unavailable`). A server that does not answer is a finding of the
report.

## Parity and adoption

The crate is new code: it has no source and no parity list. podssh's
doctor has its own probe (`nat.rs` of `podssh-cli`); its session replaces
it with `Prober::run` on its sockets and its resolver.

## Tests

* `tests/classes.rs`: each of the nine classes of RFC 5780 in the
  simulator, the plain servers, the broken servers, the `Fast` profile,
  loss, UDP blocked, an unstable mapping.
* `tests/advanced.rs`: the port allocation, CGNAT and double NAT,
  hairpinning, ALG, the lifetime, the gateway, NAT64, the strategy.
* `tests/regressions.rs`: the codecs, the classifiers and their edges.
* `tests/loopback.rs`: `Prober` on real sockets against plain stand-ins
  and a NAT-PMP gateway on the loopback.
* `tests/rfc5780.rs`: `Prober` against a stand-in RFC 5780 server with
  two addresses (127.0.0.1 and 127.0.0.2).
* `tests/guards.rs`: the guards against a hostile reply (a URL of UPnP
  with user information or a control byte, an OTHER-ADDRESS in the LAN, a
  hairpin to a private address) and against a flood of stray datagrams.
* `tests/live.rs`: the STUN servers of the kind `stun` of `pod-services`
  (`#[ignore]`; `live.yml`, on Thursdays and by hand).

```sh
cargo test -p pod-natprobe --all-features
cargo clippy -p pod-natprobe --no-default-features -- -D warnings
```
