# pod-testkit

The test kit of podcrates: a dev-dependency of each crate, and of a
consumer that wants it. It is never a dependency of a release binary.

## API

* `need_loopback!()` and `need_loopback!(value)`: a test that needs TCP or
  UDP on 127.0.0.1 returns at once, with a line `skipping, REASON` on
  stderr, on a host that refuses `bind(AF_INET)` or has no loopback.
* `need_localhost!()`: the same for a host that does not resolve
  `localhost`.
* `need_userns!()`: the same for a host that refuses a user and a mount
  namespace with a tmpfs mount in them (Linux only; elsewhere the test
  skips).
* `need_tool!(TOOL)`: the same for a host with no `TOOL` that can execute.
  `TOOL` is a path (`"/bin/sh"`) or a name in `PATH` (`"qemu-img"`).
* `need_root!()`: the same for a process whose effective user is not
  root (Linux only; elsewhere the test skips).
* `host::loopback()`, `host::localhost()`, `host::userns()`: the probes,
  each run once for each test binary. `host::tool(TOOL)` and
  `host::root()` look each time.

A stand-in of this kit speaks bytes and HTTP only, and takes no type of a
pod crate, so that each crate can dev-depend on the kit.

## Adoption map

| Consumer | Old path | New path |
| --- | --- | --- |
| podnet | `test_host::need_loopback!()` (`tests/test_host/mod.rs`, `#[path]`) | `pod_testkit::need_loopback!()` |
| podnet | `test_host::need_localhost!()` | `pod_testkit::need_localhost!()` |
| podvm | a test that forks, calls `unshare(CLONE_NEWUSER\|CLONE_NEWNS)` and skips on exit 2 | `pod_testkit::need_userns!()` first |

## The terminal oracle (feature `screen`)

`pod_testkit::screen::Screen`: a model of a VT terminal (vt100) that reads
the bytes of a program and tells what a person sees: each row, the
cursor, the colors and the attributes of a cell. `pod-term` checks the
model against tmux (`crates/pod-term/tests/calibration.rs`).

| Consumer | Old path | New path |
| --- | --- | --- |
| podssh | `#[path = "../../podssh-terminal/tests/screen/mod.rs"] mod screen;` (tests of podssh-ws, podssh-ssh, podssh-cli) | `use pod_testkit::screen;` with `pod-testkit = { git = ..., tag = ..., features = ["screen"] }` as a dev-dependency |

## The stand-ins (feature `stand`)

`pod_testkit::stand`: servers on the loopback for the offline tests of a
client or a watch. Each runs on threads, keeps the bytes that each
connection sent (`Stand::seen`), and lives until the end of the test
binary.

| Item | What |
| --- | --- |
| `stand::http(handler)` | an HTTP/1.1 server: each request (`Request`: `method`, `target`, `headers`, `path()`, `header(name)`) gets the `Answer` (`status`, `headers`, `body`) of the handler, with `Content-Length`, kept alive; a `101` writes the body after the head and keeps the connection (a WebSocket's frames) |
| `stand::raw(serve)` | a TCP server that gives each connection to `serve` |
| `stand::udp(answer)` | a UDP server that answers each datagram with what `answer` gives |
| `Stand::http_over(handler, wrap)`, `Stand::raw_over(serve, wrap)` | the same over the caller's stream: `wrap` turns each TCP connection into it (TLS with rustls's `StreamOwned`); the kit itself has no TLS |
| `Stand::addr()`, `Stand::port()`, `Stand::seen()` | the address; the bytes of each connection that ended |

`crates/pod-services-watch/tests/watches/` uses each of them.
