# pod-proxy

The way out of a TCP connection for the crates of podcrates and their
binaries: directly, with the race of the addresses of a name (RFC 8305)
and Multipath TCP on request, or through an HTTP CONNECT proxy or a
SOCKS5 proxy; the proxy and the `NO_PROXY` list of an environment; how a
connection ended; the HTTP/1.1 heads; a GET over HTTPS; the unsent bytes
of a socket, a limit of a write that makes no progress, and the codec of
STUN.

The crate reads no variable unless the caller asks, keeps no process
state and prints nothing. The consumer makes one `Dialer` from its
`Settings` and its resolver, and gives it as an `Arc<dyn pod_core::Dial>`
to each crate that opens a connection.

## API

| Item | What |
| --- | --- |
| `Dialer::new(settings, resolve)` | the dial of a consumer: its `Settings` and its `Arc<dyn pod_core::Resolve>`; it implements `pod_core::Dial` |
| `Dial::dial(&target, deadline)` | a `Connected` (the stream, its peer and local addresses); the stream has the write limit of the settings; a failure is a `pod_core::Failure` whose cause is the `DialError` |
| `Dial::dial_addrs(host, &addrs, deadline)` | a direct dial races the given addresses as a name's; through a proxy, each address in turn |
| `Dial::label()`, `Dial::names_inside()` | `direct`, `socks5h://proxy.example:1080`, `http://proxy.example:3128 of HTTPS_PROXY`; true when a proxy resolves the names |
| `Dialer::tcp(host, port, timeout)`, `Dialer::tcp_multipath(..)` | the `TcpStream` (and how Multipath TCP went), with `DialError` |
| `Dialer::proxy_for(host)` | the proxy of the settings for a host, or none: the loopback, a name that the resolver pins to loopback alone, and what `NO_PROXY` names go directly |
| `Settings::new(app, proxy)`, `ProxyChoice` (`FromEnvironment(Environment)`, `Direct`, `Via(Proxy)`), `ATTEMPT_DELAY` | the settings (below) |
| `Environment::read(&reader)`, `Environment::proxy_for(host, &pins)`, `Environment::no_proxy_entry(host)`, `Given`, `Origin` (`Variable`, `User`), `PROXY_VARS`, `NO_PROXY_VARS` | the proxy and the `NO_PROXY` list of the variables that the caller's reader gives, each with its variable |
| `env::{proxy_from_vars, is_loopback, no_proxy_matches, no_proxy_entry}` | the rules of a `NO_PROXY` list (names, suffixes, `*`, blocks `ADDR/BITS`), and the loopback |
| `Proxy` (`parse`, `url`, `url_at`, `host`, `port`, `kind`), `ProxyKind` (`scheme`, `name`, `request`), `HttpProxy`, `proxy::percent_decode` | the proxy that a URL names; its credentials never show in `Display` or `Debug` |
| `DialError` (`stage_and_class`, `cut`), `From<DialError> for Failure` | why a connection could not be opened, and its stage and class |
| `Cut` (`of`, `of_failure`, `words`), `TlsFailure::of(&err, &words)`, `Missed` (`words`, `tls`) | how a connection ended, by the kind of its error of I/O |
| `Client::new(context, dial)`, `with_user_agent`, `with_records` (feature `ech`), `https_get`, `https_get_accepting` | a GET over HTTPS through the caller's dial and TLS context; a `Failure` at the stage of the step that failed |
| `http::{exchange, read_response, parse_head, parse_status_line, decode_chunked, Response, Head}` | one HTTP/1.1 request and its response over an open stream, bounded (a head of 16 KiB, the body of the caller) |
| `http::request::{read_request, parse_request, connect_target, reason, refusal, answer, established, Request, RequestError, MAX_HEAD}` | a server's reading of a request head, and its answers |
| `host::{authority, check_name, split_host_port}`, `interleave`, `MAX_PROXY_RESPONSE` | `host:port`, the check of a name, the order of the families |
| `mptcp::{probe, enabled, connect, connect_with, socket, Multipath, ENABLED_FILE}` | Multipath TCP (Linux) |
| `lowat::{set, LOWAT}`, `write_limit::WriteLimit` | `TCP_NOTSENT_LOWAT` (Linux, macOS); a stream whose write fails when it makes no progress |
| `stun` | STUN's messages (RFC 8489): a Binding Request, a Binding response, the reading of any message |
| `resolve::System` (`new`, `with_pins`), `resolve::{LITERAL, PIN, SYSTEM}` | the system resolver after the IP literals and the caller's pins, for a consumer with no DNS over HTTPS and for the tests |

## Features

| Feature | What |
| --- | --- |
| none (default) | everything above; the GET client's hello is plain |
| `ech` | the GET client opens through `pod_tls::ech::open`: sealed where the TLS context asks for ECH and the name publishes a list, with the reader of HTTPS records of `Client::with_records` (`pod-dns` gives one) |

## Settings

| Field | What | podssh |
| --- | --- | --- |
| `Settings::app` | the program in the words of a 407 and of a SOCKS5 method | `podssh` |
| `Settings::user_agent` | the `User-Agent` of a CONNECT; none with `None` | `Some(format!("podssh/{}", env!("CARGO_PKG_VERSION")))` |
| `Settings::proxy` | `ProxyChoice` | `FromEnvironment(Environment::read(&\|n\| std::env::var_os(n)))` by default; `Direct` for `--direct`; `Via(proxy)` |
| `Settings::multipath` | ask for Multipath TCP with no proxy | `true` for the direct road with MPTCP |
| `Settings::lowat` | the unsent bytes of each socket | `Some(LOWAT)` (16 KiB, the default) |
| `Settings::write_limit` | the limit of a write of a stream of `Dial::dial` | `None` in the dial; podssh wraps its SSH transport in `WriteLimit::new(tcp, WRITE_TIMEOUT)` itself |
| `Settings::attempt_delay` | the attempt delay of the race | `ATTEMPT_DELAY` (250 ms, the default) |
| the resolver of `Dialer::new` | the addresses and the pins | `pod-dns` (T-015); until then an implementation of `pod_core::Resolve` over podssh's `resolve` module (`resolve_from`, `pinned`, `note_dialed`) |
| `Client::with_user_agent` | the `User-Agent` of a GET | `podssh/VERSION` |
| the `user_agent` argument of `http::exchange` | the `User-Agent` of a request; none when empty | `podssh/VERSION` |

The variables of the environment that `Environment::read` reads through
the caller's reader, in this order: `https_proxy`, `HTTPS_PROXY`,
`all_proxy`, `ALL_PROXY` (the first that is set and not empty), then
`no_proxy`, `NO_PROXY`. A value that is not UTF-8 is unset. The consumer
keeps the variables: it reads them once and passes the `Environment`.

The bypass of a name pinned to loopback addresses alone is the caller's:
`Environment::proxy_for(host, &pins)` takes the pins of the host, and
`Dialer::proxy_for` gives the pins of its resolver (`Resolve::pinned`).

## Parity

| Capability of podssh `crates/podssh-ws/src/{dial.rs,proxy.rs,proxy_auth.rs,socks5.rs,mptcp.rs,race.rs,cut.rs,lowat.rs,write_limit.rs,stun.rs,stun_tests.rs,http.rs,http/,client.rs}` at `6ece2ff` | Here |
| --- | --- |
| The direct dial, the HTTP CONNECT dial and the SOCKS5 dial, the whole within one bound | [x] `Dialer` |
| CONNECT by name, never by address: no DNS behind a proxy | [x] `connect` |
| Basic credentials from the proxy URL; the 407 that names each scheme of `Proxy-Authenticate` and what the program can give | [x] the program is `Settings::app` |
| The head of the answer read to its end and no further (16 KiB) | [x] `MAX_PROXY_RESPONSE` |
| A proxy's reason phrase made one safe line | [x] `http::parse_status_line` with `pod_core::text::one_line` |
| `https_proxy`, `HTTPS_PROXY`, `all_proxy`, `ALL_PROXY`, lower case first, an empty value unset | [x] `Environment::read`, through the caller's reader, with the variable of each value |
| `no_proxy`, `NO_PROXY`: names, suffixes, `.name`, `*.name`, `*`, a port ignored, blocks `ADDR/BITS` of IPv4 and IPv6, `::ffff:` addresses; the entry that matched | [x] `env` |
| The loopback goes directly; a name pinned to loopback alone goes directly | [x] `Environment::proxy_for` with the pins of the caller; `Dialer::proxy_for` gives the resolver's |
| A proxy URL: `http://`, a bare `host:port` (80), `socks5://` and `socks5h://` (1080), credentials percent-decoded, SOCKS4 refused by name, a host that no name holds refused, RFC 1929 lengths, `url`, `url_at` | [x] `Proxy` (the words of a refused scheme name no program) |
| SOCKS5: no authentication or RFC 1929, the name as address type 3 for `socks5://` and `socks5h://` alike, one more try with an address of the resolver after reply 8, each reply as the status of an HTTP proxy, a bound address read to its end, an answer to the password of version 1 or 5 | [x] `socks5` (the name for both schemes: see the differences below) |
| The race of addresses: the families in turns, 250 ms between attempts, the first that connects wins | [x] `race`, `interleave`, `Settings::attempt_delay` |
| A source of the resolver whose addresses do not connect hands the dial to the next (4 s each), and the resolver hears which source carried it | [x] `pod_core::Found::more`, `pod_core::SOURCE_WAIT`, `Resolve::note_dialed` |
| Multipath TCP on request on the direct road, plain TCP where the kernel refuses it, whether the server took it | [x] `mptcp`, `Settings::multipath` |
| `TCP_NODELAY` and `TCP_NOTSENT_LOWAT` on each dialed socket | [x] `Settings::lowat` |
| A write that makes no progress fails at its limit | [x] `WriteLimit`; `Settings::write_limit` for the streams of `Dial::dial` |
| How a dial or a handshake ended, by the kind of its error | [x] `Cut`, `Cut::of_failure` (in place of `ConnectError::cut`), `TlsFailure`, `Missed` |
| The errors and their words | [x] `DialError`, the same texts; a timeout has its stage |
| STUN's codec | [x] `stun` |
| HTTP/1.1: a request and its response, `Content-Length`, chunks, the close, the bounds, a body and a header made safe text, a server's request head, its refusals and answers | [x] `http`, `http::request` |
| The GET client: the configuration and the name checked first, the dial, the handshake (sealed with ECH where asked), the exchange, each bounded | [x] `Client` (ECH with the feature `ech`) |
| The rest of `client.rs`: the WebSocket client, `https_post_json`, `https_with_token`, `doctor`, the frames | `pod-ws` (T-016) |
| The tests: unit (27) and `tests/{dial,dial_pinned,dial_race,socks5,mptcp,http,limits}.rs`, with their names | [x] 85 passed, 1 ignored (the live test of MPTCP), with the new tests below |
| `tests/limits.rs`: the tests of the upgrade and of the frames | `pod-ws` (T-016) |
| `doh_wire.rs` `the_dialer_hands_a_cached_address_that_does_not_connect_to_the_next_source` (the cache of podssh's resolver) | `pod-dns` (T-015); the race's part is `dial_race` `a_source_whose_addresses_do_not_connect_hands_the_dial_to_the_next` |
| The fuzz targets `http_head`, `http_chunked`, `socks5_answer`, `proxy_url`, `no_proxy`, `connect_status` | T-017 (`socks5::fuzz_exchange` stays behind `cfg(fuzzing)`) |
| The plants `t172-no-fallback`, `t639-reason-of-429`, `t641-length-trusted` | [x] `scripts/plants/proxy-mptcp-no-fallback`, `proxy-reason-of-429`, `proxy-stun-length-trusted`: the same defects, caught by the same tests |

New tests: `env` (3), `error` (1), `resolve` (2), `dial_race`
`a_source_whose_addresses_do_not_connect_hands_the_dial_to_the_next`,
`http` `the_user_agent_goes_in_its_header_and_an_empty_one_sends_none`,
`tests/dialer.rs` (6), `tests/get.rs` (3); new plants
`proxy-pins-not-given` and `proxy-source-not-handed`.

The known differences with podnet `src/egress.rs`, `src/proxy_auth.rs`
and `src/fetch.rs`, for T-014:

* `socks5://`: podssh sends the name, as for `socks5h://`; podnet (and
  curl) resolves the name locally for `socks5://`.
* Digest: podnet answers a 407 of Digest (MD5, MD5-sess, SHA-256,
  SHA-256-sess, qop auth); podssh sends Basic only.
* `unix:` and `+unix:`: podnet reaches a proxy on a Unix socket.
* `+exec:`: podnet reaches a proxy at the stdin and the stdout of a
  command, only from a value that the user gave on purpose (`--proxy`,
  `PODNET_PROXY`: `Origin::User`), never from `HTTPS_PROXY`.
* The detection of the proxy of a tool's configuration when no variable
  is set (podnet `proxy_auth::detect`).
* The HTTP client under DoH, the mint of the relay and the API of the
  quick tunnels (podnet `fetch.rs`).
* The variables: podnet reads `HTTPS_PROXY` before `https_proxy` and
  `NO_PROXY` before `no_proxy`, skips a proxy URL that does not parse
  with a warning, applies `NO_PROXY` to `--proxy` too, and takes the
  port of a `NO_PROXY` entry; it does not pass the loopback by itself.
* The port of `http://` with no port: 8080 in podnet (curl), 80 in podssh.
* The counters of the decisions (`direct`, `none`, `no_proxy`, the kind
  of the proxy) go to a `pod_core::Observer`.

## Adoption map

| Consumer | Old path | New path |
| --- | --- | --- |
| podssh | `podssh_ws::dial::dial(host, port, &proxy, timeout)` | `dialer.tcp(host, port, timeout)` with `Dialer::new(Settings::new("podssh", proxy), resolver)`; or `Dial::dial(&Target::new(host, port), deadline)` |
| podssh | `podssh_ws::dial::dial_multipath(..)` | `Dialer::new(Settings { multipath: true, .. }, resolver).tcp_multipath(host, port, timeout)` |
| podssh | `podssh_ws::dial::ProxyChoice::FromEnvironment` | `ProxyChoice::FromEnvironment(Environment::read(&\|n\| std::env::var_os(n)))`, read once |
| podssh | `podssh_ws::dial::{ProxyChoice::Direct, ProxyChoice::Via}` | `pod_proxy::ProxyChoice::{Direct, Via}` |
| podssh | `podssh_ws::dial::proxy_from_env(host)` | `dialer.proxy_for(host)` (the pins of its resolver), or `environment.proxy_for(host, &resolver.pinned(host))` |
| podssh | `podssh_ws::dial::proxy_from_vars(host, var)` | `pod_proxy::env::proxy_from_vars(host, &var)` with `var` giving an `OsString`; no pins |
| podssh | `podssh_ws::dial::{PROXY_VARS, NO_PROXY_VARS, is_loopback, no_proxy_matches, no_proxy_entry}` | `pod_proxy::env::*` (and `PROXY_VARS`, `NO_PROXY_VARS` at the root); the variable of each value is `environment.proxy.origin`, `environment.no_proxy.origin` |
| podssh | `podssh_ws::dial::{parse_status_line}` | `pod_proxy::http::parse_status_line` |
| podssh | `podssh_ws::dial::{authority, split_host_port, check_name}`, `ATTEMPT_DELAY`, `interleave` | `pod_proxy::host::*`, `pod_proxy::{ATTEMPT_DELAY, interleave}` |
| podssh | `podssh_ws::{DialError, dial::DialError}` | `pod_proxy::DialError`; `Timeout` has a `stage`; `Failure::from(e)`; from a `Failure`, `e.source().downcast_ref::<DialError>()` |
| podssh | `podssh_ws::{Proxy, HttpProxy, ProxyKind}`, `podssh_ws::proxy::*` | `pod_proxy::{Proxy, HttpProxy, ProxyKind}`; `proxy::percent_decode` is public |
| podssh | `podssh_ws::resolve::{set_pins, pinned}` as the dial reads them | the resolver of the dialer: `Resolve::pinned`; `resolve::System::with_pins(pins)` until `pod-dns` |
| podssh | `podssh_ws::resolve::{resolve, resolve_from, Found, SOURCE_WAIT}` as the race reads them | `pod_core::{Resolve, Found, SOURCE_WAIT}`; `Found` has `more` |
| podssh | `podssh_ws::{Cut, Missed, TlsFailure}`, `podssh_ws::cut::*` | `pod_proxy::{Cut, Missed, TlsFailure}`; `TlsFailure::of(&e, context.words())`, `Missed::tls(&e, context.words())` |
| podssh | `DialError::cut()`, `ConnectError::cut()` | `DialError::cut()`; `Cut::of_failure(&failure)` |
| podssh | `podssh_ws::client::{https_get, https_get_accepting}(host, port, path, .., &trust, &proxy, timeout)` | `Client::new(context, dial).with_user_agent(..)` once, then `client.https_get(host, port, path, max_body, &trust, timeout)`, `client.https_get_accepting(..)`; `Err(Failure)` with the same words (`the HTTPS request failed: ..`, `the HTTPS request timed out after Ns`) |
| podssh | `podssh_ws::http::exchange(stream, method, ..)` | `pod_proxy::http::exchange(stream, user_agent, method, ..)` |
| podssh | `podssh_ws::http::*`, `podssh_ws::http::request::*` | `pod_proxy::http::*`, `pod_proxy::http::request::*` |
| podssh | `podssh_ws::mptcp::*`, `podssh_ws::write_limit::WriteLimit`, `podssh_ws::stun::*` | `pod_proxy::mptcp::*`, `pod_proxy::write_limit::WriteLimit`, `pod_proxy::stun::*` |
| podssh | `podssh_ws::lowat::set(&tcp)` | `pod_proxy::lowat::set(&tcp, pod_proxy::lowat::LOWAT)` |
| podssh | `podssh_ws::fuzz_exchange` (`cfg(fuzzing)`) | `pod_proxy::fuzz_exchange` |
| podssh | the variables `https_proxy`, `HTTPS_PROXY`, `all_proxy`, `ALL_PROXY`, `no_proxy`, `NO_PROXY` | podssh keeps them and reads them once: `Environment::read(&\|n\| std::env::var_os(n))` |
| podssh | `User-Agent: podssh/VERSION` (CONNECT, `http::exchange`) | `Settings::user_agent`, `Client::with_user_agent`, the argument of `http::exchange` |
| podssh | the words "podssh sends Basic only", "podssh sent none", "which podssh did not offer" | `Settings::app = "podssh"` |
| podssh | the plants `t172-no-fallback`, `t639-reason-of-429`, `t641-length-trusted` | they leave podssh's list with the files |

## Sources

`SOURCES` lists the imported files: podssh `crates/podssh-ws/src/`
`dial.rs` (now `dial.rs`, `env.rs`, `connect.rs`, `error.rs`, `host.rs`),
`proxy.rs`, `proxy_auth.rs`, `socks5.rs`, `mptcp.rs`, `race.rs`, `cut.rs`
(and `cut_tests.rs`), `lowat.rs`, `write_limit.rs`, `stun.rs`,
`stun_tests.rs`, `http.rs`, `http/`, the GET client of `client.rs`, and
the tests `tests/{dial,dial_pinned,dial_race,socks5,mptcp,http,limits}.rs`
at `6ece2ff`.
