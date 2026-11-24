# pod-core

The bottom crate of podcrates. Each other crate shares these parts, and
this crate depends on no other pod crate.

## API

| Module | What |
| --- | --- |
| `dial` | `Target` (a name and a port, or an address), `Dial` (a way out: `label`, `proxy_label` (the proxy that a dial of a target goes through, for a line that blames a proxy; the default is none), `dial`, `dial_http` (a request of `http://`; the default is `dial`), `dial_addrs`, `names_inside`, `datagrams`), `Resolve` (`resolve_from` with the order of its sources, `pinned`, `note_dialed`), `Found` (the addresses, their source, and `more` when a source after it may give more), `SOURCE_WAIT` (the time of a source with more after it in the race of addresses), `Connected`, `BoxedIo`, `BoxFuture` |
| `datagrams` | `Datagrams` in the shape of the UDP socket of quinn and noq, `SystemDatagrams`, `send_to`, `recv_from` |
| `failure` | `Failure` with its `Stage` (where the path broke) and its `Class` (what a caller can do) |
| `event` | `Event` (a line for the user), `Events`, `Observer` (the count of decisions), `Report` |
| `metered` | `Metered`: a stream that counts its bytes and signals at a limit |
| `files` | the private files of a program and of a user: `App` (its name and its variable), `places_from`, `candidate_dirs_from`, `chosen_from`, `Places` with the places passed over in another user's tree, `store_file_in_first`, `load_file_from`, `read_private`, `read_own`, `create_private`, `create_new_private`, `private_dir`, `safe_name`, the owners and the access lists of Unix and Windows; a Unix socket with its mode from the first instant at a path of any length, also past `sun_path`: `bind_staged`, `peer_ids`, and `socket_answers` (3 connects 50 ms apart, so that a listener that a forked child holds until its exec is stale); the atomic replace of a file at any path: `replace`, `Replace`, `Mode`, `Commit`, `Failed`, `Step`, `replace_private` ("The replace of a file", below) |
| `entropy` | `Entropy`, `OsEntropy`, `NoRandom`: the random source, which a test can plant |
| `http1` | the head of an HTTP/1.1 message, with no I/O and no tokio ("The head of HTTP/1.1", below): `Bounds` (`bytes`, `lines`; `DEFAULT`: `MAX_HEAD`, 16 KiB), `find_request`, `head_bounds`, `crlf_head_end`, `find`; `parse_request`, `Request` (`header`, `has`, `has_token`), `RequestError` (`status`); `parse_head`, `parse_status_line`, `Head`, `Framing::of`, `keeps`; `request_head`, `response_head`, `reason`, `refusal`, `answer`, `established`; `Exact` and `read_head_exact`: a head read one byte at a time, and not a byte further; `parse_content_range`, `Span`: a `Content-Range` (RFC 9110, section 14.4), the one parse of `pod-guest` and `pod-incus` |
| `lock` | `FileLock` (`try_lock`, `lock_within`, `path`, `mode`, `file`) and `Mode`: an advisory lock of a file for the life of a value, `flock(2)` on Unix and `LockFileEx` on Windows, with no link at the last component, close-on-exec, and a bounded wait whose refusal names the file |
| `backoff` | `backoff(retry)`, `CONNECT_TIMEOUT` |
| `clock` | `Clock` (the monotonic time and the wall clock, which a test can plant), `SystemClock`, `ManualClock`; the hint of a refused certificate, HTTP dates, `utc_minutes`, `rfc3339` |
| `text` | `one_line`, `multi_line`: text of a peer, safe to print |
| `names` | the checks of a host, a target and a node name |

A crate of podcrates prints nothing and reads no variable: it takes a
`Dial`, a `Resolve` and a `Report` from its caller, and returns a
`Failure`.

## The head of HTTP/1.1

`http1` is the one parser and writer of a head of HTTP/1.1 in podcrates. A reader of a crate reads bytes into a buffer and gives them to it; `pod_proxy::http` holds the readers on tokio (`read_request`, `read_request_within`, `read_head`, `read_head_in`, `read_head_exact`).

* A request head: a line ends with CRLF or a bare LF, and empty lines before the request line are passed over. A control byte in a line (but a tab), a bare CR, a space before a colon, a folded line and a request-target that is not visible ASCII are refused with 400; another version of HTTP with 505; a head past its `Bounds` with 431.
* A header name is in lower case. A value loses only the spaces and tabs around it: `Authorization` keeps its credential as the client sent it.
* A response head: lines end with CRLF; a line with no colon is passed over; the reason phrase is made one line of safe text.
* A written head refuses a part with CR, LF or NUL.

## The replace of a file

`pod_core::files::replace` is the one atomic replace of a file at a path
that the caller names (T-192, T-935, T-1066). Do not write a second copy:
`scripts/check-private-replace.py` (CI) refuses each `fs::rename` of
product code outside it and its listed exceptions.

```rust
use pod_core::files::{Mode, Replace, replace};
replace(path, body, Mode::Private)?;
Replace::new(Mode::Keep).follow_link(true).make_dir(true).write(path, body)?;
// A check of the temporary file before the rename (T-1109).
Replace::new(Mode::Private).write_checked(path, body, |tmp| same_key(tmp))?;
```

* The temporary file is in the directory of the target, with a name of
  its own (`.NAME.PID.N.tmp`). It is opened with `create_new` and
  `O_NOFOLLOW`, and gets its mode before the first byte. A file or a link
  at that name is never followed and never removed.
* The file is synced (`sync_all`), renamed over the target, and the
  directory is synced on Unix. On Windows the rename has POSIX semantics
  and writes through, with a bounded retry while another handle holds the
  name (call C829).
* `Mode::Private` gives 0600. `Mode::Keep` gives the permission bits of
  the old file (with no setuid, setgid or sticky bit), and 0600 when there
  is none; on Windows a private file, or a file that each may read when the
  old file is open to the other users. `Mode::Exact(bits)` gives the bits.
* With no option, a symbolic link at the path is replaced, not followed.
  `follow_link(true)` follows the links (40 at most) and replaces the
  target, so the link stays; a dangling link gives its target a new file.
* `make_dir(true)` makes each missing directory with mode 0700 and syncs
  its entry.
* `write_checked(path, body, check)` gives `check` the path of the
  temporary file after its write, its sync and its close, before the
  rename. An error of `check` removes the temporary file and leaves the
  target as it was; the error is of the step `Step::Check`, with the kind
  and the text of the error of `check`.
* `in_place_fallback(true)` writes the file in place when the rename
  fails (a bind-mounted file); the result is `Commit::InPlace`.
* `Ok(Commit::Durable)`: the file and its entry are on disk.
  `Ok(Commit::InstalledUnconfirmed(e))`: the file is in place, and a sync
  of a directory failed. `Err`: the old file stays, and no temporary file
  stays.
* The error is an `io::Error` of the kind of its cause. Its text names the
  step and the path: "replace PATH: STEP: CAUSE". `Failed::of(&e)` gives
  the `Step` and the path.
* `replace_private(path, body, bits, in_place_fallback)` is
  `Replace::new(Mode::Exact(bits))` with the error of the system as it
  came, for the callers that add their own text.

The copies of the consumers, with the call that replaces each:

| Consumer | Copy | Call |
| --- | --- | --- |
| podnet | `PodUtils/podnet:crates/podnet-netns/src/report.rs` `write_atomic` (`--report FILE`) | `replace(path, text.as_bytes(), Mode::Private)` |
| podnet | `PodUtils/podnet:src/warp/ssh_config.rs` `save` (`warp ssh-config`) | `Replace::new(Mode::Keep).follow_link(true).make_dir(true).write(path, text.as_bytes())` |
| podssh | `PodUtils/podssh:crates/podssh-cli/src/keygen_cert.rs` `replace` (a certificate) | `replace(path, bytes, Mode::Keep)` |
| podssh | `PodUtils/podssh:crates/podssh-ssh/src/known_hosts/edit.rs` (the new `known_hosts` under its lock) | `replace(path, text, Mode::Keep)` |
| podssh | `PodUtils/podssh:crates/podssh-cli/src/keygen_change.rs` `replace` (a private key, read back and compared before the rename) | `Replace::new(Mode::Private).write_checked(path, text.as_bytes(), CHECK)`, with `same_key` in `CHECK` |
| podvm | `PodUtils/podvm:crates/podvm/src/run/report.rs` `write`, `PodUtils/podvm:crates/podvm-supervise/src/report.rs` (a report); `PodUtils/podvm:crates/podvm/src/initns/hold.rs` `write`, `PodUtils/podvm:crates/podvm-windows/src/machines.rs` `write`, `PodUtils/podvm:crates/podvm-image/src/probe_cache.rs` `write`, `PodUtils/podvm:crates/podvm-complete/src/reach.rs` (a state or a cache file) | `replace(path, body, Mode::Exact(0o644))`: the mode that `std::fs::write` gives now with the umask 022 |
| podvm | `PodUtils/podvm:crates/podvm/src/windows/pull.rs` `write_new` (a base, with its directory) | `Replace::new(Mode::Exact(0o644)).make_dir(true).write(path, bytes)` |
| podrunner | `PodUtils/podrunner:crates/podrunner-slots/src/journal.rs` `compact` (its file has the mode of `File::create`) | `replace(path, lines, Mode::Exact(0o644))`, which also syncs the state directory |
| podrunner | `PodUtils/podrunner:crates/podrunner-remote/src/ledger.rs` (then opened to append); `PodUtils/podrunner:crates/podrunner-slots/src/spool.rs` `spool` (a spooled report) | `replace(path, body, Mode::Private)` |

Not a copy: a rename that gives the new file to another user (`PodUtils/podssh:crates/podssh-cli/src/node_install/system_install.rs`),
streams a large file (the `cp`, `zmodem` and `chat` of podssh;
`PodUtils/podvm:crates/podvm/src/machine/guest.rs` and the kernels of
podvm), moves a directory, or keeps a binary
(`PodUtils/podnet:src/update.rs`).

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
| `permission` | 71 | a file or a directory has the wrong permissions or the wrong owner |
| `suite` | 78 | the far end and this end have no protocol or suite in common |
| `cantcreate` | 73 | an output (a file or a directory) cannot be made or written |
| `internal` | 70 | a fault inside the tool, or a session that ended before its end with no known fault of the far end |

A unit of a supervisor stops a restart loop with
`RestartPreventExitStatus=64 78 79 80`. The codes 70 and 73 are not in
the list: a later try can work after a freed disk or a one-time fault.

One JSON log line of a tool has the keys of `schemas/log.v1.schema.json`:
`schema` (the constant `LOG_SCHEMA`, 1, the first key), `time` (RFC 3339, UTC), `level` (`LOG_LEVELS`: `error`, `warn`, `info`, `debug`, `trace`),
`event` (a dotted name, `relay.session.open`), `fields` (an object), and,
for a failure, `stage` and `class` (the names of `Stage` and `Class`). The
schema comes from the audit line of `pod-broker`, the one writer of these
lines in podcrates.

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
