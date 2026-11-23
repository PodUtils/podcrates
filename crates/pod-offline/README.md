# pod-offline

The carriers and the stores of a host with no network, at library level.
The rules and the gate come from PodUtils/podnet#499. No part of this crate
uses a network.

A host with no path has a serial port, a radio and a disk. The crate gives
the session layer the parts above them.

## The rules of the issue

| Rule | Where |
| --- | --- |
| SERIAL-1: AX.25 frames in KISS framing, 256-byte frames, a window of 1 to 7 with acknowledgements, one serial implementation | `serial::kiss`, `serial::arq`, `serial::port` |
| SERIAL-2: the retransmission timeout comes from the baud rate | `serial::arq::retransmit_after` is two frame times: about 4.3 s at 1200 baud, with no 200 ms floor |
| SERIAL-3: a frame above the maximum is a named error | `Error::FrameTooLarge`, name `frame-too-large` |
| HF-1: an ARDOP or VARA modem as a serial device with an ASCII command set, 300 to 3000 bit/s | `hf::Modem`, `hf::Session` |
| HF-2: the session ARQ defers to the ARQ of the modem | `hf::DeferArq` gives `Arq::deferring` |
| HF-3: a busy signal and a turnaround delay, half-duplex | `hf::Modem::transmit_at` refuses a reception and the turnaround |
| HF-4: the degraded-link profile | `hf::DeferArq::DEGRADED`: a window of 1, a deferring ARQ and the modem's own retransmission |
| SPOOL-1: a checksum, a sequence and the sender, consumed after the receiver takes it | `spool::Spool` |
| SPOOL-2: a directory of files, repairable by hand | the text header and `spool::Spool::check` |
| DISC-1: a signed announcement | `discovery::Announcement`, a DNS-SD TXT record |
| DISC-2: one radio interface | `discovery::Radio`, one implementation `KissRadio` |
| BUNDLE-1: a self-describing bundle with a signed manifest and a delta | `bundle` |
| SINK-1: one record format, merged by timestamp, redaction at write time | `capture` |
| PAIR-1: an eight-word single-use code bound to a `pod-identity` key | `pairing` |
| PAIR-2: a reused code or an expired offer is refused | `pairing::UsedCodes::claim`, `pairing::Giver::accept`, `Error::CodeUsed`, `Error::CodeExpired` |
| REPL-1: state the model | the section below |
| REPL-2: the unit is the session state and the peer registry | `Replica::set_session`, `Replica::set_peer` |

## The model of replication

The log is append-only. A merge copies each record that the replica does not
have. It never removes or rewrites a record.

Each record carries a vector clock. A clock `a` dominates a clock `b` when
each count of `b` is in `a` and `a` has one count more. The state of a key is
the record that no other record of that key dominates.

Two records are concurrent when neither clock dominates the other. The rule
is last-writer-wins for each key:

1. The record with the greater count of its own actor wins.
2. At an equal count, the greater hex of the actor's Ed25519 key wins.

The rule gives the same winner on each replica, whatever the order of the
merges. The losing records are written to the conflict log with the hash of
each value. The log is derived from the records, so a loaded log gives the
same conflicts.

The unit of replication is the state of a session and the registry of peers.
The namespaces are `session:` and `peer:`.

## The modules

- `serial`: the address `serial:PATH[,BAUD]`, the raw 8N1 open with
  `O_NOCTTY` and `TIOCEXCL` and the settings back at each end, the KISS
  codec, and the ARQ window.
- `hf`: the modem driver and the deferring session ARQ.
- `spool`: a directory of entries, each with a checksum, a sequence and the
  sender's key.
- `discovery`: the signed TXT announcement, the peers that verified, and the
  one radio interface.
- `bundle`: the signed manifest, the files, and a delta from a named base.
- `capture`: the record format, redaction at write time, and the merge.
- `pairing`: the eight words, the offer and its window, the single-use
  stores of the taker and the giver, and the paired record that binds the
  two keys.
- `replication`: the append-only log, the vector clock and the merge rule.

## Named limits

- The serial link is point-to-point. The frames carry the control byte and
  the sequence of AX.25, and no address field, no digipeater path and no
  frame check sequence. The crate does not implement SABM, DISC, RNR or the
  AX.25 UI frames.
- The ARQ acknowledges the frames up to a sequence; it has no selective
  repeat and no selective acknowledgement. A full window of 7 keeps the
  sequence modulo 8 unambiguous.
- The retransmission timeout uses the maximum frame, not the frame that was
  sent. It is conservative on a short frame.
- The open of a serial line is POSIX and Unix only. The feature `serial` is
  on by default.
- The HF command set is the subset that this driver needs. It is not the
  full ARDOP or VARA command set. There is no FEC, no channel scan, no
  bandwidth negotiation and no busy-channel probe. The driver blocks on the
  line while it waits for the answer of the modem.
- The turnaround is 500 ms of the driver. The crate does not measure the
  real turnaround of the channel.
- The 300 to 3000 bit/s range is checked, and the speed of the carrier is
  the caller's setting.
- The spool stores cleartext. Its checksum finds damage, not an attacker.
  The sender key is a claim, and no signature covers the body. File
  permissions are the operator's.
- The sequence of the spool is per sender. A duplicate is refused by the
  name of the file, and an entry that an interrupted `put` left is reported
  by `check` and never received.
- `check` reports the first byte of the body for a checksum fault. It cannot
  name the byte that changed.
- Discovery is not a multicast DNS resolver. It reads and writes the signed
  TXT record, and the caller carries it. There is no TTL, no cache flush, no
  probing and no name conflict resolution.
- The bundle does not compress, and a delta of a changed file carries the
  whole file. One key signs the manifest and the delta. The apply writes the
  files and the manifest last, so an interrupted apply keeps the old
  manifest and can run again. There is no rollback protection beyond the
  base id.
- A bundle path is relative and holds no `..`, no backslash and no empty
  part.
- Redaction is a literal byte match. It does not find an encoded or
  transformed secret. An empty secret is ignored. The timestamp of a record
  is the caller's, because the crate reads no clock.
- A code is eight words of 64, so 48 bits. The caller draws the entropy;
  the crate has no entropy source. The broadcast hash is
  PBKDF2-HMAC-SHA256 with the salt `KDF_SALT` and 100,000 iterations, so a
  stranger cannot search the code cheaply from an offer. An offer expires
  after its window (ten minutes by default), and each side keeps a
  single-use record: the taker's `UsedCodes` and the giver's `Giver`. Two
  hosts that share neither store can use one code. The paired record is the
  taker's signature, so the binding is in one direction; a mutual record
  needs both sides.
- Replication has no Byzantine protection. A record is trusted as it
  stands: the actor key names the writer and does not sign the record. The
  vector clock grows with the number of actors. A value is opaque bytes. A
  key is overwritten, never removed: the crate has no tombstone.
- The crate has no async I/O. A serial read and a modem read block the
  calling thread.

## Tests

`cargo test -p pod-offline --all-features` runs the tests of each module and
the tests in `tests/`. The tests of the serial line and of the modem use a
pseudo-terminal pair from `openpty`. A host with no `/dev/ptmx` skips each
of them with that reason on stderr; a skip is not a failure. The test of the
spool starts a child process that aborts between `receive` and `consume`.

## Build

```sh
cargo fmt --all
cargo clippy -p pod-offline --all-targets --all-features -- -D warnings
cargo test -p pod-offline --all-features
```
