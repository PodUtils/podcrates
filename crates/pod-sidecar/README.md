# pod-sidecar

A packet sidecar at library level, for PodUtils/podnet#495. It holds the
desync primitives, a local middlebox simulator, a genetic search, a
disagreement probe with three values, split and tamper, fake TCP and a
per-flow bond.

The crate opens no socket by default, needs no privilege, and reads no
environment variable. A raw socket exists behind a runtime check, and a host
that refuses it gives a named skip.

## The feature gate

The whole crate is behind the cargo feature `sidecar`, and that feature is
off by default:

```toml
pod-sidecar = { workspace = true, features = ["sidecar"] }
```

A default build of the crate is empty. No default binary of a consumer has a
packet path, a raw socket or a TUN device (rules SYN-1 and LIM-2). A build
that wants the sidecar asks for the feature and takes the responsibility.

## The rules of the issue

* SYN-1: the sidecar needs raw sockets or a TUN device, and it is never a
  built-in path. Where raw sockets are not available, it reports a named
  skip, not a failure. `probe::raw_capability` checks the host before any
  socket opens. A refusal becomes `Verdict::Unknown` with the reason.
* SYN-2: five primitives, drop, tamper, duplicate, fragment and coalesce. A
  strategy is an ordered list of steps. Each step has a trigger:
  `FirstOutboundData`, `SynAck`, `FirstInboundRst` or `Always`.
* SYN-3: the genetic search runs against the local simulator only. It never
  touches a live path.
* SYN-4: a strategy file carries `date`, `network`, `found_by` and `steps`.
  A file with no date is refused at load with `StrategyError::NoDate`.
* PROBE-1: the probe sends legal but unusual segments: an out-of-window
  segment, a retransmission with a different payload, and a FIN before the
  data is acknowledged.
* PROBE-2: the verdict has three values, `Disagrees`, `Agrees` and
  `Unknown`. A probe that cannot run is `Unknown`, never `Agrees`.
* SPLIT-1: split at an offset, disorder, fake TTL, tamper a checksum, pad
  the first segment. One library and one configuration format.
* SPLIT-2: a behaviour has a stated defeat. Without a defeat it is a claim,
  not a technique.
* FAKE-1: prefer a real TCP carrier with message framing. Fake TCP exists
  for an application that needs datagram boundaries below our framing.
* FAKE-2: fake TCP has costs: an extra round trip, a changed TTL, and a
  checksum that a middlebox may repair.
* BOND-1: the bond assigns a flow to one link and moves it only when that
  link fails. It is never a per-packet round robin.
* BOND-2: two links at 10:1 carry a session within 20 percent of the sum.
* LIM-1: each limit names the condition that triggers it and the check that
  detects it. The `limits` module holds the page, and a test fails when a
  split behaviour has no limit.
* LIM-2: the sidecar is not in the default binary. The build feature is the
  only way that it ships.

## The five primitives

`Primitive` has five values. A `Strategy` is `Vec<Step>`, and each step
carries one primitive and one trigger.

| Primitive | What it does | The divergence that it produces |
| --- | --- | --- |
| drop | The segment never arrives | The byte stream is short, and the wire has one segment less |
| tamper | One payload byte changes, and the checksum is repaired | One byte of the stream differs |
| duplicate | The segment arrives twice | The wire has one segment more, and the stream is unchanged |
| fragment | The payload becomes two segments at an offset | The wire has one segment more, and TCP reassembles the stream |
| coalesce | Two contiguous segments become one | The wire has one segment less, and the stream is unchanged |

The simulator compares each run with a run that has no strategy. It reports
the state of each end, the byte stream of each end and the count of segments
in each direction. `Divergence::summary` gives one line.

## The simulator

`Simulator::run` plays a three-way handshake, two data sends with an echo,
and a close. Each end holds the state of RFC 793, a receive window of 4096
bytes, a reassembly queue and a checksum check. The simulator also models an
out-of-window segment, a retransmission with a different payload, and a FIN
before the data is acknowledged. The same seed and the same strategy give
the same bytes, the same states and the same divergence.

The module `endpoint` holds one end. A test reads the state, the stream and
the counts from outside the run.

## The search

`Search::find` maximizes the simulated disagreement and subtracts a cost for
each step. `Search::towards` scores a run against a hand-written strategy,
so a test can require convergence on it. The initial population holds every
one-step strategy, then random genomes. Each generation keeps the best half
and breeds the rest by crossover and mutation. A test with a fixed seed and
40 generations reproduces the divergence of a hand-written strategy.

## The probe

`Probe::run` takes a `Path`: the simulator, a silent path, an unavailable
device, or a raw socket. It sends the three unusual segments of PROBE-1.

* A path that answers with a RST, a wrong acknowledgement or other bytes
  gives `Disagrees`.
* A path that answers as a correct TCP does, or does not answer at all,
  gives `Agrees`. Silence is not evidence of interference.
* A path that cannot run gives `Unknown`, and the reason is on the finding.

`RawPath` sends a SYN, waits for a SYN-ACK, sends the ACK and then the
unusual segment on a raw IPv4 TCP socket. A host without `CAP_NET_RAW` or
uid 0 gives `Unknown` with the errno.

## Split and tamper

One file format holds the behaviours:

```toml
date = "2026-11-12"
network = "lab-a"
found_by = "hand"

[[behaviours]]
kind = "split-at"
offset = 3

[[behaviours]]
kind = "tamper-checksum"
mask = 258
```

The defeats, as rule SPLIT-2 requires:

| Behaviour | What it changes | Its defeat |
| --- | --- | --- |
| split at an offset | One segment becomes two at the offset | A receiver that reassembles the IP fragments, or a TCP stack that reassembles the byte stream |
| disorder | The first two segments swap their order | A receiver that reassembles the stream and tolerates a gap, as every TCP stack does |
| fake TTL | The first segment carries a low TTL | A path that honours the TTL and drops the segment at zero |
| tamper a checksum | One bit of the first checksum changes | A receiver that requires a valid checksum, or a middlebox that repairs it |
| pad the first segment | The first segment carries extra bytes | A receiver that reassembles the stream and compares it with the original |

`Simulator::split` returns `SplitError::Reassembled`,
`SplitError::ChecksumRejected` or `SplitError::TimeExceeded` when the path
defeats the behaviour. Each error names the behaviour.

## Fake TCP

`FakeTcp` carries one message in frames over a datagram channel. A frame
carries a magic, a version, a message id, a fragment index and count, the
total length, a checksum and the payload. The receiver holds fragments that
arrive out of order in a window, and it returns a message only when each
fragment is present. A frame with a bad checksum gives
`FakeTcpError::BadChecksum`, and a frame beyond the window gives
`FakeTcpError::OutOfWindow`.

The costs, as rule FAKE-2 requires:

* an extra round trip, because the far end answers after a whole message;
* the TTL of the carrier, which a path sees and can act on;
* a checksum that a middlebox may repair, so a changed frame can arrive with
  a valid checksum and the framing cannot tell.

A path that refuses UDP by protocol number refuses these frames too. The
test uses a simulator that refuses a plain datagram and carries the framed
echo.

## The bond

`Bond` assigns a flow to one link and records the move only when that link
fails. A new flow goes to the live link with the least load for its
capacity. On two links at 10:1, one flow gets 10 of a sum of 11, which is 91
percent of the sum. The cost is stated: one flow cannot use two links, so a
single session gets the capacity of its best link and not the sum.

## The limits page

Each limit has the condition that triggers it and the check that detects it.
The `limits` module holds this table as data, and `tests/limits.rs` fails
when a split behaviour has no entry.

| Limit | Condition | Detection |
| --- | --- | --- |
| split at an offset | A receiver that reassembles the IP fragments, or a TCP stack that reassembles the byte stream in order | `Simulator::split` with `Policy::strict` returns `SplitError::Reassembled`, and the appended bytes arrive whole |
| disorder | A receiver that reassembles the byte stream and tolerates a gap | `Simulator::split` with `Policy::strict` returns `SplitError::Reassembled`, and the delivered stream is unchanged |
| fake TTL | A path that honours the TTL, decrements it and drops the segment at zero | `Simulator::split` with `Policy::strict` returns `SplitError::TimeExceeded` |
| tamper a checksum | A receiver that requires a valid TCP checksum, or a middlebox that repairs the checksum | `Simulator::split` with `Policy::strict` returns `SplitError::ChecksumRejected`; the endpoint also counts a bad checksum |
| pad the first segment | A receiver that reassembles the stream and compares it with the original | `Simulator::split` with `Policy::strict` returns `SplitError::Reassembled`; the padding is visible in `SplitRun::padding` |
| fake TCP | A path that refuses UDP by protocol number, or repairs the checksum of a frame | `Simulator::udp_echo` returns a refusal, and `FakeTcp::recv` returns `FakeTcpError::BadChecksum` when a frame is changed |
| raw socket probe | A host with no `CAP_NET_RAW` and no uid 0 | `probe::raw_capability` returns a `Refusal` that names the error, and the probe reports `Verdict::Unknown` |
| silent probe | A path that drops every unusual segment and answers nothing | The verdict is `Verdict::Agrees` with an empty response, so silence is not evidence of a disagreement |
| bond, one flow | A session that is one flow, so the per-flow scheduler puts it on one link | `Bond::estimate` is 10 of a sum of 11 on links at 10:1, which is 91 percent of the sum |
| bond, move | A link that fails, which moves each of its flows once | `Bond::moves` counts each move, and a flow on a live link does not move |

## The checks

```sh
cargo fmt --all --check
cargo clippy -p pod-sidecar --all-targets --all-features -- -D warnings
cargo clippy -p pod-sidecar --all-targets -- -D warnings
cargo test -p pod-sidecar --all-features
```

The crate is 0BSD. The tests use no network and no privilege, except the
capability check of the raw socket, which passes on a host that refuses it
with a printed skip line.
