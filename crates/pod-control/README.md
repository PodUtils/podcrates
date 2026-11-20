# pod-control

One local control API for each long-running program of PodUtils: JSON lines
on a Unix socket or a Windows named pipe, with the version of the protocol
in each line (T-103; cfb's A-X-15 and A-N-03, githome's G-08). A program
implements the trait `Host`; the crate gives the catalogue of the methods,
the tiers, the server, the client, the JSON Schema `control.v1` and a
conformance check that the program runs against its own server. New code,
from the JSON of podnet `src/status.rs` and podssh `status` (an object with
`schema`) and podrunner's control module (`crates/podrunner/src/control.rs`:
the mode 0600, the check of the peer uid, the deadline of a line, the
snapshot of `status`).

The server is std threads: a synchronous program takes it with no runtime.
The crate prints nothing and does not exit.

## Use

```rust,ignore
use pod_control::{Catalogue, Extra, Settings, Tier, unix};

let mut settings = Settings::new("podrunner", env!("CARGO_PKG_VERSION"));
settings.same_user = Tier::Operate;
let catalogue = Catalogue::new(vec![Extra::new("capacity", Tier::Read, "The free slots of each unit kind.")])?;
let (server, incoming) = unix::serve(&state.join("ctl.sock"), settings, catalogue)?;
loop {
    // The loop of the program: each request waits for the host here.
    incoming.answer_waiting(&mut supervisor, Duration::from_millis(50));
    server.snapshot().set(supervisor.status_value());
}
```

A client:

```rust,ignore
let mut c = pod_control::Client::connect_unix(&path, pod_control::ClientSettings::default())?;
let status = c.status()?;
c.drain(pod_control::Drain { exit: true })?;
```

## The wire (protocol 1)

Each line is one JSON object with `"schema": 1`, at most `Settings::line_max`
bytes (64 KiB) without its line end, and UTF-8.

```text
{"schema":1,"id":7,"method":"drain","params":{"exit":true}}            a request of the client
{"schema":1,"id":7,"ok":true,"result":{"draining":true}}               its answer
{"schema":1,"id":7,"ok":false,"error":{"code":"busy","detail":"..."}}  an error
{"schema":1,"id":1,"for":7,"method":"confirm","params":{}}             a call back of the server for request 7
{"schema":1,"id":1,"ok":true,"result":true}                            the answer of the client
```

* A request has `method`; an answer has `ok`, and `result` or `error`.
  Each side numbers its own requests. An answer to a line with no id that
  can be read has `"id": null`.
* The codes: `bad-request`, `not-utf8`, `too-long`, `unknown-method`,
  `bad-params`, `refused`, `busy`, `budget`, `timeout`, `unsupported`,
  `failed`, `internal`. `Code::class` gives the `pod_core::Class` of each.
* The `detail` of an error holds no byte of the request: a request can
  hold a token.
* A line over the cap is `too-long`, and the connection closes. A line
  that is not UTF-8 is `not-utf8`, and the connection goes on.
* Each whole line has `Settings::read_timeout` (5 s): a peer that sends one
  byte at a time meets the deadline of the line. A connection with no line
  in that time closes.
* `schemas/control.v1.schema.json` is the schema of one line, made from
  the catalogue (`pod_control::schema()`); `$defs/params/NAME` and
  `$defs/results/NAME` give the params and the result of each method, and
  `$defs/catalogue` the rows.

## The catalogue

| Method | Route | Tier | What |
| --- | --- | --- | --- |
| `version` | local | read | The program, its version and the protocol. |
| `methods` | local | read | The methods that this caller can call, with the tier of the caller. |
| `auth` | local | read | `{"token": "..."}`: the master token; the connection then has the master tier. One try for each connection; a wrong token closes it. |
| `status` | delegated | read | The state of the program: a JSON object of its own schema (podrunner's `status.v1`). |
| `metrics` | delegated | read | `{"counters": {...}, "gauges": {...}}`; `Metrics::prometheus` writes the text of Prometheus. |
| `reload` | delegated | master | Read the configuration again; its effect outlives the token. |
| `drain` | delegated | operate | `{"exit": true}` (the default): no new work, each work in flight ends, then the program exits. `{"exit": false}`: the program keeps running and takes no new work. |
| `stop` | delegated | operate | Stop now: the program ends the work in flight by its own rule, and exits. |

A local method is answered by the server, also while the host is busy. A
delegated method goes to the host through `Incoming`. A host adds methods of
its own as `Extra` rows; `Host::extra` answers them. `Catalogue::new`
refuses a name of the catalogue, a name twice, a name that is not a
lower-case word with `-`, and an extra whose effect outlives its token
(`Extra::outlives`) below the master tier.

## The tiers

`read` < `operate` < `master`. A peer of the user of the server gets
`Settings::same_user` (default `operate`); another uid gets the tier of its
row in `Settings::uids` (Unix), or is refused at the accept (`refused`).
`auth` with `Settings::master` gives `master`. A method above the tier of
the caller gets the answer of a name that is not there, word for word
(`unknown-method`, "no such method"), before its params are read; `methods`
does not list it.

## Calls back

A delegated request can call the client back on its connection:
`Context::call(method, params)`. The client answers with the `back` of
`Client::call_with`; `Client::call` answers `unsupported`. Each request has
`Settings::callbacks_per_request` (16) calls back and each connection
`Settings::callbacks_per_connection` (256): past the budget of the request,
each call is `CallFault::Budget` and the request answers `budget`; past the
total, the connection also closes. The client holds the same budgets
(`ClientSettings`): past them it drops the connection (`Error::Budget`).

## The host is busy

Each delegated request waits `Settings::answer_wait` (5 s) for the host.
Past it, `status` gets the last value of `Server::snapshot`, and another
method gets `busy`. `Settings::clients` (8) connections are served at one
time; one more gets `busy`.

## Transports

* Unix (`unix`): `bind` with `Settings::mode` (0600) from the first instant
  (`pod_core::files::bind_staged`); a stale socket is replaced, a live socket
  is `AddrInUse`, another file is `AlreadyExists`. The peer ids come from
  `SO_PEERCRED` or `getpeereid` (`pod_core::files::peer_ids`). The drop of
  `Server` removes its socket while the path holds it.
* Windows (`pipe`): `\\.\pipe\NAME`, an access list of the user, SYSTEM and
  the Administrators (`pod_core::files::PrivateAttributes`), no remote client,
  and the first instance only (a second server is `AddrInUse`). The SID of
  the peer process is the check of the peer user. The client refuses a
  server of another user (SYSTEM is taken). A watch thread of each stream
  cancels a read or a write at its deadline (`CancelIoEx`).

## Conformance

`conformance::check(connect, &Options)` runs the checks of the wire against
a running server and gives each finding: `version`, `methods`, the answer of
an unknown method and of each method above the tier, `bad-params`, the frame
(not JSON, no `"schema": 1`, another version, no id, an unknown field, not UTF-8,
too long), and the forms of `status` and `metrics`. It calls no method that
changes the program. A program runs it from a test against its own server.

## The take of podrunner

podrunner's `crates/podrunner/src/control.rs` maps to this crate:

| podrunner | pod-control |
| --- | --- |
| `control::bind(path)` | `unix::bind(path, 0o600)` |
| `control::spawn_with(listener, snapshot)` | `unix::serve_listener(listener, path, settings, catalogue)`; the snapshot is `server.snapshot()` |
| `control::answer_waiting(&calls, &mut sup, wait)` | `incoming.answer_waiting(&mut sup, wait)` |
| `Handler` | `Host` |
| `Request::Version` | local `version` (`Settings::program`, `Settings::version`) |
| `Request::Status` | `status` |
| `Request::Drain` | `drain {"exit": false}` |
| `Request::Stop` | `drain {"exit": true}` (no new claim, each running job ends, then exit) |
| `Capacity`, `ClearDegraded`, `Withdraw`, `DrainSource`, `ResumeSource`, `StopSource` | `Extra` rows (`capacity` read; the others operate) |
| `RestartSource` | an `Extra` row of the master tier (a key file read again is a new credential), or `same_user: Tier::Master` |
| `control::call(path, &req)` | `Client::connect_unix(path, ..)?.call(name, params)` |
| `{"v":1,"op":"status"}`, `{"v":1,"reply":"ok","value":...}` | `{"schema":1,"id":1,"method":"status"}`, `{"schema":1,"id":1,"ok":true,"result":...}` |
| `LINE_MAX`, `CLIENTS`, `EXCHANGE` | `Settings::line_max`, `Settings::clients`, `Settings::read_timeout` and `Settings::answer_wait` |

The conformance check goes into a test of podrunner against `serve`.

## Tests

```sh
cargo test -p pod-control --all-features
```

* The catalogue is the dispatch: `each_method_of_the_catalogue_is_served`,
  `each_served_method_is_in_the_catalogue`,
  `the_local_and_the_delegated_methods_do_not_overlap`,
  `the_names_of_the_catalogue_are_unique_and_of_the_wire_form`.
* The tiers: `a_method_that_outlives_its_token_takes_the_master_tier`,
  `a_method_above_the_tier_is_found_as_a_name_that_is_not_there`,
  `a_method_above_the_tier_answers_as_an_unknown_one`,
  `a_peer_of_another_user_is_refused_unless_granted`,
  `auth_gives_the_master_tier_and_a_wrong_token_closes`.
* The frame: `tests::bad_lines_are_bad_requests_with_no_byte_of_the_line`,
  the tests of `frame`, `a_line_over_the_cap_closes_and_a_line_not_utf8_does_not`,
  `a_slow_client_is_cut_at_the_read_timeout`.
* The budgets: `a_request_past_its_callback_budget_is_a_fault`,
  `the_calls_back_of_a_connection_have_a_total`,
  `a_client_drops_a_server_past_its_budget`.
* The schema: `the_schema_file_is_generated_from_the_catalogue` (write it
  again with `POD_CONTROL_WRITE_SCHEMAS=1`),
  `each_line_of_the_wire_is_valid_for_the_schema_and_a_broken_one_is_not`.
* Conformance: `the_server_of_the_crate_conforms_at_each_tier`,
  `a_server_that_breaks_the_protocol_has_findings`.
* Windows (`tests/pipe.rs`, in the Windows jobs of `nightly.yml`).
* Plants: `control-served-not-in-catalogue`, `control-master-method-lower-tier`,
  `control-budget-raised`, `control-tier-refusal-names-the-method`.
* The fuzz target `control_line` (`fuzz/`): `wire::parse` and the lines
  written again.
