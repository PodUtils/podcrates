# pod-qga

A blocking client of the QEMU Guest Agent (`qemu-ga` on Linux, the agent
of the virtio-win drivers on Windows) over the Unix socket of its
virtio-serial channel `org.qemu.guest_agent.0`. It gives a host `exec`,
the copy of files and a clean stop of a full guest. podvm asked for it
(PodUtils/podvm#350, item 5) for `podvm exec`, `cp` and `stop` of a
Windows guest (PodUtils/podvm@T-1868). T-594 made it.

The protocol is read in the schema `qga/qapi-schema.json` of QEMU and in
`docs/interop/qemu-ga-ref`: one JSON request on one line, one JSON answer
on one line, in order, with no id. The crate is written from the schema,
with no code of QEMU.

## Use

```rust
let mut settings = pod_qga::Settings::new(run_dir.join("qga.sock"));
settings.connect_wait = std::time::Duration::from_secs(30);
let mut agent = pod_qga::Client::connect(settings)?; // connect, then sync
agent.ping()?;

let mut cmd = pod_qga::Exec::new("cmd.exe");
cmd.args = vec!["/c".into(), "ver".into()];
let pid = agent.exec(&cmd)?;
let done = agent.wait_exec(pid, Duration::from_millis(100), Duration::from_secs(60))?;
// done.exit_code, done.stdout, done.stderr

agent.write_file("C:\\podvm\\in.txt", b"data")?;
let back = agent.read_file("C:\\podvm\\in.txt", 64 << 20)?;
agent.shutdown(pod_qga::Shutdown::Powerdown)?;
```

The emulator side: `-chardev socket,path=qga.sock,server=on,wait=off,id=qga0
-device virtio-serial -device virtserialport,chardev=qga0,name=org.qemu.guest_agent.0`.

## API

| Call | Agent command | Return |
| --- | --- | --- |
| `Client::connect(Settings)` | the connect, then `Client::sync` | `Client` |
| `Client::sync` | the byte 0xFF, then `guest-sync-delimited` with a new id; each byte up to the 0xFF of the answer is dropped, and an answer with another id is old | `()` |
| `Client::ping` | `guest-ping` | `()` |
| `Client::exec(&Exec)` | `guest-exec` (`path`, `arg`, `env`, `input-data` in base64, `capture-output`) | the pid in the guest |
| `Client::exec_status(pid)` | `guest-exec-status` | `ExecStatus` (`exited`, `exit_code`, `signal`, `stdout`, `stderr`, the two cut flags) |
| `Client::wait_exec(pid, poll, limit)` | `guest-exec-status` each `poll`, at most `limit` | `ExecStatus` of the end, else `ErrorKind::NotEnded` |
| `Client::file_open(path, OpenMode)` | `guest-file-open` (`r`, `w`, `a`, `r+`) | the handle |
| `Client::file_read(handle, count)` | `guest-file-read` | `Chunk` (`data`, `eof`); its `count` must be its length |
| `Client::file_write(handle, data)` | `guest-file-write` (`buf-b64`, `count`) | the count written |
| `Client::file_close(handle)` | `guest-file-close` | `()` |
| `Client::read_file(path, limit)` | open `r`, reads of `Settings::chunk` to the end, close | the bytes, at most `limit` |
| `Client::write_file(path, data)` | open `w`, writes of `Settings::chunk`, close | `()` |
| `Client::shutdown(Shutdown)` | `guest-shutdown` (`powerdown`, `halt`, `reboot`) | `()` |
| `wire::{request, answer, sync_id, exec_pid, file_handle, written, base64}`, `Answer`, `Exec`, `ExecStatus`, `Chunk`, `OpenMode`, `Shutdown`, `SENTINEL` | the JSON, as pure functions on each target | |
| `Error` (`call`, `kind`, `class`, `agent_class`, `stage`), `ErrorKind`, `Result` | | |

`Client` is on Unix only. `wire` and `Error` build on each target.

## The answers and the errors

| `ErrorKind` | When | `Class` |
| --- | --- | --- |
| `NoAgent { socket, cause }` | the connect failed after `connect_wait` | `Refused` for a denied socket, else `Unavailable` |
| `Agent { class, desc }` | the `"error"` member of the answer; the text is one line, cut to 512 characters | `CommandNotFound`: `Unsupported`; `CommandDisabled`: `Policy`; `DeviceNotFound`: `NotFound`; else `Unavailable` |
| `Timeout { after }` | no whole answer within `call_timeout` | `Timeout` |
| `NotEnded { pid, after }` | the command of `wait_exec` still runs at `limit` | `Timeout` |
| `Closed` | the agent closed the connection before the answer | `Unavailable` |
| `Protocol(why)` | not JSON, no `"return"`, a field of the wrong type, an answer over `max_answer` | `Protocol` |
| `Limit { what, limit }` | a file over the limit of `read_file` | `Policy` |
| `Usage(why)` | an empty path, a read of no byte | `Usage` |
| `Io(e)` | a read or a write of the socket failed | of the `io::ErrorKind` |

A call that did not get its answer (the bound, a closed connection, a
broken answer) leaves old bytes in the socket: the next call syncs first.
A refusal of the agent is a whole answer, and the client stays in step.
`read_file` and `write_file` close their handle also when a read or a
write fails.

`guest-shutdown` gets no answer when the agent obeys: the guest stops. A
refusal within `shutdown_wait` is an error; no answer, or the end of the
connection, is success. The emulator (QMP) tells if the guest stopped.

## Settings

| Field | Default | What |
| --- | --- | --- |
| `socket` | | the Unix socket of the agent |
| `connect_wait` | 0 | how long the connect waits for the socket; the emulator makes it when it starts |
| `call_timeout` | 10 s | the bound of one call, from the request to the end of its answer; also of a sync |
| `max_answer` | 64 MiB | the longest answer line |
| `chunk` | 1 MiB | the bytes of one read or write of `read_file` and `write_file`; its base64 must fit in `max_answer` |
| `shutdown_wait` | 1 s | how long `shutdown` waits for a refusal |

The crate reads no environment variable, keeps no global, prints nothing,
and starts no thread.

## What stays in podvm

The chardev and the virtio-serial port of the guest, the install of the
agent in the guest, the `podvm exec`, `cp` and `stop` commands, the
choice of the poll and of the limits, and the check through QMP that the
guest stopped.

## Tests

`cargo test -p pod-qga`. A stand-in agent (`tests/standin/`) answers on
a Unix socket in the temporary directory, as `qemu-ga` does, with a model
of a guest (files, handles, commands); a 0xFF gets the parse error of the
agent. `tests/agent.rs`: the connect skips old answers and an old delimited
answer; exec, its status, its output and the wait; a file in and out in
chunks of 3 bytes, an empty file, each handle closed; a file over the
limit refused with its handle closed; a refusal with its class, with no
new sync; a command that the agent does not have; a shutdown with no
answer, and a refused one; no agent at the path; values refused before a
request. `tests/faults.rs`: a call with no answer ends at its bound and the
next call syncs; a late answer is not the answer of the next call; a
closed connection; a broken answer; a return of the wrong shape; an answer
over the limit; a sync that never gets its id; a wait that ends at its
limit. The unit tests check the JSON of each request and the reader of
each return. The fuzz target `qga_answer` (`fuzz/`) reads the answers.
The plants: `qga-no-sync-after-a-timeout`, `qga-sync-takes-an-old-answer`,
`qga-handle-left-open`, `qga-shutdown-with-no-answer-fails`.
