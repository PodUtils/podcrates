# pod-link

A link between two peers: many streams over one carrier byte stream, with
a credit window for each stream and for the link, a keepalive, and the
carriers behind one trait. podnet asked for it for its link workstream
(PodUtils/podnet#466, row E0-2; PodUtils/podnet#467, rows E1-7 and E1-8;
the start signal PodUtils/podnet#480). T-300 made it, as new code, by
calls C116, C117 and C120 to C124. The crate reads no environment, keeps
no global state and prints nothing.

## Use

```rust
use pod_link::{Carrier, Link, Role, Settings, unix::Unix};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// The listener.
let carrier = Unix::default();
carrier.available()?;                         // a reason when the host refuses AF_UNIX
let mut door = carrier.listen("/run/user/1000/podnet/link.sock").await?; // mode 0600
let listener = Link::new(door.accept().await?, Role::Listener, Settings::default())?;
let mut stream = listener.accept().await?;    // even ids are the listener's

// The dialer.
let io = Unix::default().dial("/run/user/1000/podnet/link.sock").await?;
let dialer = Link::new(io, Role::Dialer, Settings::default())?;
let mut s = dialer.open()?;                   // odd ids; no round trip
s.write_all(b"hello").await?;
s.shutdown().await?;                          // CLOSE: a half close
dialer.close().await;                         // GOAWAY after the queued frames
```

## API

| Item | What |
| --- | --- |
| `Link` | `new(carrier, role, settings)` starts the task of the link on the current tokio runtime; `open`, `accept`, `close` (a `GOAWAY` after the queued frames), `closed` (wait for the end and its reason), `ended`, `streams`, `rtt` (of the last `PING`), `role`. A drop ends the link at once |
| `Stream` | `AsyncRead` and `AsyncWrite`; `id`, `reset(code)`, `buffered`. A shutdown sends `CLOSE`; a drop sends `CLOSE` and, when the peer did not close its side, `RESET` with `CANCEL` |
| `Role` | `Dialer` (odd ids) or `Listener` (even ids) |
| `Settings` | the windows, the stream limit, the frame size, the keepalive (below); `check` |
| `Error` | each failure by name; `class` (for the exit code), `io_kind`; into `std::io::Error` (a stream's error) and into `pod_core::Failure` (the stage `Session`) |
| `ResetCode`, `GoAwayCode` | `CANCEL`, `REFUSED`, `PROTOCOL`; `NORMAL`, `PROTOCOL`. A caller's own reset codes start at 16 |
| `Carrier`, `Listener`, `Facts` | the trait of a carrier: `name`, `facts` (ordered, reliable, duplex, unit), `available`, `dial`, `listen`; the accept side; `BoxIo` is the byte stream that they give |
| `CarrierError`, `CarrierErrorKind` | `Usage` (64), `InUse` and `Unavailable` (69), `NotFound` (66), `Refused` (77), `Io`; into `pod_core::Failure` (the stage `Connect`) |
| `memory::Memory`, `memory::pair` | the memory carrier: listeners by name in the value (a clone shares them), a ring of 64 KiB to 64 MiB in each direction (1 MiB by default); a full ring makes the writer wait |
| `unix::Unix`, `unix::pair` | the unix socket carrier (`cfg(unix)`): a path of 1 to 107 bytes, a socket of mode 0600, the claim of the path, the peer's uid; a socket pair for one process |
| `frame` | `Frame`, `Decoder` (`new`, `with_preface`, `push`, `next`, `buffered`), `FrameError`, `PREFACE`, `HEADER`, `MAX_RECORD`, `kind` |

## The wire

Each end first writes the preface `PODLINK` and the byte 1. Then each
frame is a header of 8 bytes (the type, a stream id of 32 bits, a body
length of 24 bits; big endian) and its body:

| Type | Frame | Id | Body |
| --- | --- | --- | --- |
| 1 | `OPEN` | the new stream | none |
| 2 | `DATA` | a stream | 1 byte to 1 MiB |
| 3 | `WINDOW` | a stream, or 0 for the link | the credit, 4 bytes, not 0 |
| 4 | `CLOSE` | a stream | none: the sender writes no more |
| 5 | `RESET` | a stream | the code, 4 bytes |
| 6 | `PING` | 0 | 8 bytes |
| 7 | `PONG` | 0 | the 8 bytes of the `PING` |
| 8 | `GOAWAY` | 0 | the code, 4 bytes |

A frame is the record that the link gives its carrier: at most 1 MiB of
body. A carrier with a smaller unit splits a record itself. A peer that
breaks a rule (an unknown type, a length that does not fit its type, a
body over 1 MiB, bytes past a window, an id of the wrong parity or used
again, a frame of a stream that never existed) ends the link: it gets a
`GOAWAY` with the code `PROTOCOL`. A frame of a stream that existed and is
gone is dropped, and its bytes go back to the link's credit.

## Settings

| Field | Default | Limits | What |
| --- | --- | --- | --- |
| `stream_window` | 1 MiB | 16 KiB to 8 MiB | the largest receive window of a stream. A stream starts at 16 KiB (`INITIAL_WINDOW`, part of the protocol); its window doubles each time its reader empties its buffer when credit goes back |
| `link_window` | 8 MiB | 16 KiB to 8 MiB, not less than `stream_window` | the most bytes of all streams that this end holds unread: the memory of a link. Credit goes back when the reader takes the bytes |
| `max_streams` | 256 | 1 to 4096 | the streams of the link at this end, from both ends; an `OPEN` over it gets `RESET` with `REFUSED` |
| `frame_size` | 16 KiB | 1 KiB to 1 MiB | the largest `DATA` body that this end sends |
| `keepalive` | 10 s | more than 0 | the silence before this end sends a `PING` |
| `dead_after` | 30 s | more than `keepalive` | the silence after which the link ends with `Error::Dead` |

A stream that stops reading holds at most its own window of the link's
window, and the other streams use the rest: with the defaults, seven
stalled streams that grew to 1 MiB leave 1 MiB to the others, and a
stream that stalls before it grows holds 16 KiB. The writer of a link
queues at most 4 frames (at least 64 KiB) of `DATA`, so that a credit and
a `PONG` do not wait behind a long run of bytes. A peer that sends more
than 8192 frames that wait for an answer, and does not read, ends the
link.

## The parity list

The rules of PodUtils/podnet#466 (E0-2) and #467 (E1-7, E1-8), and the
tests that hold them:

- [x] Dialer odd ids, listener even ids: `tests/streams.rs`
  (`ids_by_role_*`), `tests/limits.rs` (`an_open_of_the_wrong_parity_*`).
- [x] A window of 16 KiB at the start and 8 MiB at most, for each stream
  and for the link: `tests/windows.rs`
  (`a_writer_waits_for_credit_and_splits_its_frames`,
  `a_reader_that_keeps_up_grows_its_window`), `tests/limits.rs`
  (`bytes_past_the_window_*`, `each_setting_out_of_its_limits_is_refused`).
- [x] 256 streams by default and 4096 at most: `tests/limits.rs`
  (`an_open_over_the_own_limit_fails`,
  `the_peer_refuses_an_open_over_its_limit`).
- [x] A record of 1 MiB at most, writes split into frames:
  `tests/frames.rs`, `tests/limits.rs`
  (`a_frame_over_one_mebibyte_ends_the_link`), `tests/windows.rs`
  (`four_mebibytes_*`).
- [x] A stream that stops reading cannot starve another: `tests/windows.rs`
  (`a_stalled_reader_starves_no_other_*`).
- [x] 300 echoes: `tests/streams.rs` (`three_hundred_echoes_*`).
- [x] A keepalive and a dead link: `tests/keepalive.rs`.
- [x] Open, data, credit, close (a half close), reset, the end of a link:
  `tests/streams.rs`.
- [x] The unix carrier: a path of 1 to 107 bytes, mode 0600, a regular
  file refused (64), a live socket in use (69), a stale socket replaced,
  the peer's uid unless allowed, a socket pair: `tests/carriers.rs`.
- [x] The memory carrier: a ring of 64 KiB to 64 MiB (1 MiB by default), a
  full ring makes the writer wait and drops nothing, the end of one side
  is the end of stream of the other: `tests/carriers.rs`.
- [ ] The resume of a link over `pod-session` (podnet T-263): T-311.
- [ ] The ARQ for a carrier that loses frames, with 3 % of the frames
  lost (podnet T-277): T-310.

## The adoption map

| podnet | pod-link |
| --- | --- |
| PodUtils/podnet@T-262: the session of streams, credits and keepalive | `Link`, `Stream`, `Settings`, `frame` |
| PodUtils/podnet@T-264: the registry of carriers with an `available()` reason | stays in podnet (call C117); each row holds a `Box<dyn Carrier>` and adds podnet's facts (cost, privilege, build feature) |
| PodUtils/podnet@T-274: the unix socket carrier; `bind_private` and `claim_unix_path` of `PodUtils/podnet:crates/podnet-netns/src/listen.rs`; `--allow-other-uid` | `unix::Unix` (`allow_other_uid`), `unix::pair` |
| PodUtils/podnet@T-275: the memory carrier | `memory::Memory`, `memory::pair` |
| exit 64, 69, 77 of a carrier | `CarrierError::class().exit_code()` |
| exit of a link | `Error::class().exit_code()`: a dead link 75, a broken protocol 76 |
