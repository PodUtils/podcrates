# pod-incus

A client of the REST API of incus on its Unix socket
(`INCUS_DIR/unix.socket`): HTTP/1.1 with no TLS, typed calls with serde,
the background operations of incus and their wait, and an error that names
the call and gives the words of incus. No `incus` or `lxc` binary. The
floor is incus 6.0 LTS. podvm asked for it (PodUtils/podvm#253) for its
incus back end (PodUtils/podvm@T-1718); podrunner can use it for a unit
kind. T-179 made it.

The API is read at lxc/incus `63377d9bacba35715edf1bcfb4fd155485a6a6fb`
(`main`, 2026-11-10): `doc/rest-api.md`, `doc/rest-api.yaml`,
`doc/api-extensions.md`, `shared/api/*.go`, the handlers of
`cmd/incusd/` (the wait, exec, the upload of an image, the files, the
console, the state) and `client/incus_*.go` (the behaviour of the client).
The 6.0 floor is checked at the tag `v6.0.0`: the API extensions, the
paths of exec's logs, the wait and the frames of `shared/ws` are the same.
The ranged read of a file is read at `845f3817bf0f70d0c12ed50ad0d2bd8239ee0459`
(`main`, 2026-11-18) and at `v6.0.0`: `internal/server/response/response.go`
(`fileResponse::Render`, `http.ServeContent`) and `cmd/incusd/instance_file.go`.
incus is Apache-2.0; the crate is written from the API documents and the
types, with no code of incus.

## Use

```rust
let mut settings = pod_incus::Settings::new(incus_dir.join("unix.socket"));
settings.user_agent = "podvm/1.2.3".into();
let incus = pod_incus::Client::new(settings);
incus.server().await?.check()?; // API 1.0 and the extensions of NEEDED

let tarball = tokio::fs::File::open(&path).await?;
let length = tarball.metadata().await?.len();
let mut import = pod_incus::ImageImport::new(pod_incus::ImageData::Unified(pod_incus::Payload::new(tarball, Some(length))));
import.aliases = vec!["podvm/alpine".into()];
let image = incus.import_image(import).await?;

let mut new = pod_incus::NewInstance::container("job-1", pod_incus::Source::Fingerprint(image.fingerprint));
new.config.insert("raw.lxc".into(), "lxc.init.cmd = /podvm/init".into());
incus.create_instance(&new).await?;
incus.start("job-1").await?;
let out = incus.exec_recorded("job-1", &pod_incus::Exec::new(["sh", "-c", "uname -a"])).await?;
// out.exit_code, out.stdout, out.stderr
incus.stop("job-1", true, None).await?;
incus.delete_instance("job-1").await?;
```

Each call opens its own connection (`Host: unix.socket`,
`Connection: close`), so one `Client` serves many tasks. A call that incus
runs in the background returns after the wait for its operation.

## API

| Call | incus | Types | incus version |
| --- | --- | --- | --- |
| `Client::server` | `GET /1.0` | `Server`, `Environment`; `Server::check` against `NEEDED` | 6.0 |
| `Client::import_image` | `POST /1.0/images`, the body a stream: `application/octet-stream` (unified) or `multipart/form-data` (`metadata`, then `rootfs` or `rootfs.img`); `X-Incus-public`, `X-Incus-filename`, `X-Incus-properties`, `X-Incus-fingerprint`; then the wait; then `POST /1.0/images/aliases` for each alias | `ImageImport`, `ImageData`, `Payload`, `ImportedImage` (`fingerprint`, `size` of the operation's metadata) | 6.0 |
| `Client::create_alias` | `POST /1.0/images/aliases` | name, fingerprint, description | 6.0 |
| `Client::alias` | `GET /1.0/images/aliases/NAME`; a 404 is `None` | `ImageAlias` (`name`, `target`, `description`, `kind`) | 6.0 |
| `Client::create_instance` | `POST /1.0/instances`, then the wait | `NewInstance` (`InstancesPost`: `source` of type `image` by alias or fingerprint, or `none`; `type`; `config`; `devices`; `profiles`; `ephemeral`; `start`) | 6.0, `instances` |
| `Client::set_state`, `start`, `stop` | `PUT /1.0/instances/NAME/state`, then the wait | `StateChange` (`action`; `timeout` in seconds, -1 for the default of incus; `force`; `stateful`), `Action` | 6.0 |
| `Client::instances` | `GET /1.0/instances?recursion=1` | `Vec<Instance>` | 6.0 |
| `Client::instance` | `GET /1.0/instances/NAME` | `Instance` | 6.0 |
| `Client::delete_instance` | `DELETE /1.0/instances/NAME`, then the wait | | 6.0 |
| `Client::console_log` | `GET /1.0/instances/NAME/console` | bytes | 6.0, `console` |
| `Client::instance_state` | `GET /1.0/instances/NAME/state` | `InstanceState` (`status`, `disk` of each device, `memory` with `usage_peak`, `cpu.usage` in nanoseconds, `processes`, `pid`) | 6.0 |
| `Client::profile` | `GET /1.0/profiles/NAME` | `Profile` (`config`, `devices`: the `pool` of the `root` disk) | 6.0 |
| `Client::oom_kills` | `GET /1.0/metrics`, the sample `incus_memory_OOM_kills_total` of the instance (the state has no OOM count) | `Option<u64>`; `None` where no sample names the instance | 6.0, `metrics` |
| `Client::exec_recorded` | `POST /1.0/instances/NAME/exec` with `record-output`, the wait, `metadata.return`; `GET`, then `DELETE`, of the two logs that `metadata.output` names (`logs/exec-output/`) | `Exec`, `ExecOutput` (`exit_code`, `stdout`, `stderr`) | 6.0, `container_exec_recording`; `user`, `group`, `cwd`: `container_exec_user_group_cwd` |
| `Client::exec_attached` | `POST /1.0/instances/NAME/exec` with `wait-for-websocket`; `GET /1.0/operations/ID/websocket?secret=` for `control`, `0`, `1`, `2` | `Attached` (`Stdin`, `Output`, `Control`, `wait`, `output`) | 6.0 |
| `Client::get_file` | `GET /1.0/instances/NAME/files?path=` | `File` (`FileMeta` from `X-Incus-uid`, `-gid`, `-mode`, `-type`; `FileContent`: bytes, or the entries of a directory) | 6.0 |
| `Client::get_file_to(name, path, range, limit, out)` | `GET /1.0/instances/NAME/files?path=` with `Range: bytes=START-` or `bytes=START-LAST` (`http.ServeContent` of incus: 206 with `Content-Range`; 416 with `bytes */SIZE` for a start at or past the end, an empty read); the body to a `std::io::Write` in pieces of `files::PIECE` (64 KiB), at most `limit` bytes, with no bound of `max_body`; a 200 to a range has the bytes before the start passed over | `ByteRange` (`start`, `end` not included), `FileRead` (`meta`, `start`, `written`, `size`, `ended`: false where the limit stopped the read first); a directory is `Usage`; a write of `out` that fails is `Output` | 6.0 |
| `Client::put_file` | `POST /1.0/instances/NAME/files?path=` | `FileWrite` (`X-Incus-type`, `-uid`, `-gid`, `-mode`, `X-Incus-write: append`) | 6.0; a directory `directory_manipulation`, a link `file_symlinks`, an append `file_append` |
| `Client::storage_pool` | `GET /1.0/storage-pools/NAME` | `StoragePool` (`driver`, `status`, `config`, `source()` of `config.source`, `used_by`) | 6.0 |
| `Client::storage_pool_resources` | `GET /1.0/storage-pools/NAME/resources`: the space and the inodes that incusd measures in its own mount namespace | `PoolResources` (`space`, `inodes`: `PoolSpace`, `PoolInodes`, each `used` and `total`; `free_bytes`, `free_inodes`: `None` with no count of inodes) | 6.0 |
| `Client::image` | `GET /1.0/images/FINGERPRINT`; a 404 is `None` | `Image` (`fingerprint`, `size`, `filename`, `architecture`, `kind` of `type`, `public`, `properties`, `aliases`: `ImageAliasName`) | 6.0 |
| `Client::operation`, `wait`, `wait_success`, `cancel` | `GET /1.0/operations/ID`, `GET /1.0/operations/ID/wait?timeout=N`, `DELETE /1.0/operations/ID` | `Operation`, `status` | 6.0 |

incus 6.0.0 has each extension of `NEEDED`. It has no
`image_import_alias` (the header `X-Incus-aliases`): the crate makes the
aliases with `POST /1.0/images/aliases`, which works on each version.

## The answers and the errors

* The type of the envelope decides, not the HTTP status: the wait of an
  operation sends 200 before its body, and an error after that comes as a
  body of type `error`.
* `Error` names the call (`POST /1.0/instances/web/exec`, the path with no
  query: a secret of a websocket never goes into an error or a log) and
  its `ErrorKind`: `NoServer` (no incus at the socket: the path does not
  exist, the connection was refused, or the system has no Unix socket),
  `Incus` (the HTTP status, `error_code` and `error` of incus), `Operation`
  (the `status_code` and `err` of an operation that failed or was
  canceled), `Timeout` (a wait at its limit), `Protocol`, `Unsupported`,
  `Usage`, `Io`, `WebSocket`, `Output` (the writer of the caller refused a write), `TooLarge` (an answer over
  `Settings::max_body`: the HTTP status, the size as `BodySize`, the bound, the first 8 KiB of the body,
  and the `error` of an `error` envelope in them, which `Error::incus_error` gives). `Error::class` gives the class of
  `pod_core`; `pod_core::Failure::from(error)` is at the stage `host`.
  `Error::operation` gives the id of the background operation of the call
  where incus started one (an error of its wait, `Operation`, `Timeout`):
  a caller can read the operation with `Client::operation`.
* A body of an answer is read whole within `Settings::max_body` (64 MiB),
  except the body of `get_file_to`, which goes to the writer in pieces.
  An envelope of type `error` over the bound is read on, to
  `wire::MAX_SKIMMED` (1 GiB), with each string cut to its first 64 KiB
  and "… (N bytes more)": incusd puts the whole stderr of `tar` into the
  error of a full storage pool (PodUtils/podvm#651). Another answer over
  the bound is `ErrorKind::TooLarge`.
* A text of incus in an error is made safe to print. A text over 512
  characters keeps its first 256 characters, the last 128 of its first
  line, and its last 128, with " … " between (call C920): the cause of a
  wrapped stderr ends its first line.

## exec

* `record-output`: incus writes stdout and stderr to two logs; the crate
  reads both after the end of the command and deletes them, as the client
  of incus does. Each log path that `metadata.output` names must be a
  file directly under `/1.0/instances/NAME/logs/exec-output/`, else the
  call fails with `Protocol`: the crate reads and deletes no other path. A
  stand-in incus for tests must give paths of that form.
* `wait-for-websocket`: incus starts the command when the sockets `0`,
  `1` and `2` are connected. Data goes in binary messages; an empty text
  message ends a stream (the barrier of `shared/ws`), as a Close does.
  `Stdin::close` sends the barrier. `Control::signal` sends
  `{"command":"signal","signal":N}`. `Attached::output` feeds stdin and
  reads stdout and stderr at once: a reader of one stream alone can stop
  a command whose other stream fills its buffer. `Attached::wait` closes
  the control socket only after the end of the command, as incus kills a
  command whose control socket closes.
* An exit of 127 or 126: incusd 6.0 ends such an exec as a failure
  ("Command not found", "Command not executable"), and its wait answers
  an error envelope (400, the same words), not the operation
  (PodUtils/podvm#653). `exec_recorded` and `Attached::wait` then read
  `GET /1.0/operations/ID` and give its `metadata.return` as the exit code
  (a missing or odd `return`: the code of the words); `exec_recorded` also
  reads and deletes the two logs. Where incusd no longer has the operation
  (404), the words give the code (127, 126) and `exec_recorded` gives empty
  output: the paths of the logs went with the operation. Another failure of
  that read is the error of the read. The limit: incusd gives the same
  answer for a program that is not there and for a command that ran and
  ended with 127 (or 126); the crate cannot tell them apart.
* incus pings each socket each 10 s and needs no answer. The crate reads
  the pings of stdout and stderr; those of stdin and control stay in the
  buffer of the socket (about 4 KiB an hour).
* Interactive exec (a PTY) and the console websocket are not in the crate.

## Settings

`Settings::new(socket)`: `project` (`?project=` on each call; a user's
socket of incus, `unix.socket.user`, needs `user-UID`), `user_agent`,
`connect_timeout` (5 s), `max_body` (64 MiB), `wait` (the limit of each
wait for an operation; `None`: no limit, `timeout=-1`), `write_timeout`
(30 s, a frame of a websocket). The crate reads no variable of the
environment: `INCUS_DIR` and `INCUS_SOCKET` stay with the consumer.

## What stays in podvm

The choice of the back end; the conversion of an OCI image to a unified
tarball (`metadata.yaml` and `rootfs/`); the `lxc.init.cmd` script and the
other `raw.lxc` keys; the start and the stop of a user's `incusd` (root
of the user's own namespace) and its `INCUS_DIR`.

## Adoption map

| Consumer | Old | New |
| --- | --- | --- |
| podvm | the minimal client of PodUtils/podvm@T-1718 (HTTP/1.1 on `INCUS_DIR/unix.socket`) | `pod-incus = { git = "https://github.com/PodUtils/podcrates", tag = "vX.Y.Z" }`; `pod_incus::Client` with `Settings::new(INCUS_DIR.join("unix.socket"))` |
| podvm | its `GET /1.0` | `Client::server`, then `Server::check` |
| podvm | its upload of a unified tarball and its wait | `Client::import_image` with `ImageData::Unified` |
| podvm | its read of an alias (404 where absent) and its `POST /1.0/images/aliases` | `Client::alias` (`None` where absent), `Client::create_alias` |
| podvm | its create, start, stop, delete and list | `create_instance`, `start`, `stop`, `delete_instance`, `instances` |
| podvm | its exec (`record-output` or the websockets) | `exec_recorded`, or `exec_attached` with `Attached::output` |
| podvm | its console log and its files (`X-Incus-uid`, `-gid`, `-mode`, `-type` of `file` and `directory`) | `console_log`, `get_file`, `put_file` with `FileWrite::file` or `FileWrite::directory` and its `uid`, `gid`, `mode` |
| podvm | `logs --podvm-tier=incus`: a whole `get_file` on each poll, and its bound of 64 MiB | `get_file_to(name, LOG, Some(ByteRange::from(seen)), left, out)` on each poll: the bytes past the offset alone, in pieces; a `size` below `seen` sets the offset back (PodUtils/podvm#631) |
| podrunner (later) | a unit kind on incus | the same calls |

## Tests

`cargo test -p pod-incus`: the unit tests (the envelope, the escape of a
path, the multipart body, the chunks of a stream, the bodies of the posts)
and a stand-in incus on a Unix socket in the temporary directory
(`tests/common/`): an HTTP/1.1 server that answers the documented JSON of
each call, with the operation model (202, then the wait, with its limit),
the websockets of exec with the barrier and the control socket, and an
error on demand. A `Range` of a file gets the answer of `http.ServeContent`
(206, or 416 at the end), or the whole file where a test sets
`ignore_range`. No `incusd` runs. A socket with no server is refused by
name.

## Sources

New code (T-179), written from the API of incus at the commit above. No
file is imported, so the crate has no `SOURCES` file.
