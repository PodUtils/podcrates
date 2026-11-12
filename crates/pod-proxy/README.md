# pod-proxy

The way out of a TCP connection for the crates of podcrates and their
binaries: directly, with the race of the addresses of a name (RFC 8305)
and Multipath TCP on request, or through an HTTP CONNECT proxy (Basic or
Digest) or a SOCKS5 proxy (`socks5://` resolves here, `socks5h://` sends
the name), on TCP, on a Unix socket or at a command; the proxy and the
`NO_PROXY` list of an environment, by the rules of the consumer, and the
proxy of a tool's configuration; how a connection ended; the HTTP/1.1
heads; an HTTP client (a GET over HTTPS, and a request of any method to
an `http://` or `https://` URL); the credentials that a client of a
served proxy gives; the unsent bytes of a socket, a limit of a write that
makes no progress, and the codec of STUN.

The crate reads no variable and no file unless the caller asks, keeps no
process state and prints nothing. The consumer makes one `Dialer` from its
`Settings` and its resolver, and gives it as an `Arc<dyn pod_core::Dial>`
to each crate that opens a connection.

## API

| Item | What |
| --- | --- |
| `Dialer::new(settings, resolve)` | the dial of a consumer: its `Settings` and its `Arc<dyn pod_core::Resolve>`; it implements `pod_core::Dial` |
| `Dial::dial(&target, deadline)` | a `Connected` (the stream, its peer and local addresses); the stream has the write limit of the settings; a failure is a `pod_core::Failure` whose cause is the `DialError` |
| `Dial::dial_addrs(host, &addrs, deadline)` | a direct dial races the given addresses as a name's; through a proxy, each address in turn |
| `Dial::dial_http(&target, deadline)` | the dial of a request of plain `http://`: through the proxy of `Scheme::Http` (`http_proxy`); `Client::send` and `send_streamed` use it for an `http://` URL |
| `Dial::label()`, `Dial::names_inside()` | `direct`, `socks5h://proxy.example:1080`, `http://proxy.example:3128 of HTTPS_PROXY`; true when the far end resolves the names (HTTP, `socks5h://`), false for `socks5://` |
| `Dialer::with_proxy_resolver(resolve)`, `Dialer::with_observer(observer)` | the resolver of a proxy's own host; the counter of the decision `egress` of each connection (`direct`, `via`, `none`, `loopback`, `no_proxy`, `unusable`, `http`, `socks5`, `unix`, `exec`) |
| `Dialer::tcp(host, port, timeout)`, `Dialer::tcp_multipath(..)`, `Dialer::stream(..)` | the `TcpStream` (and how Multipath TCP went), with `DialError`; `stream` gives a `Stream` of each kind (TCP, through a proxy on a Unix socket or at a command) |
| `Dialer::proxy_for(host)`, `Dialer::proxy_at(host, port)`, `Dialer::proxy_for_scheme(scheme, host, port)` | the proxy of the settings for a host, or none: the loopback and a name that the resolver pins to loopback alone (by the rules), and what `NO_PROXY` names go directly; `proxy_for` and `proxy_at` are for `Scheme::Https` |
| `Settings::new(app, proxy)`, `ProxyChoice` (`FromEnvironment(Environment)`, `Direct`, `Via(Proxy)`), `ATTEMPT_DELAY` | the settings (below) |
| `Environment::read(&reader)`, `Environment::read_with(&reader, rules)`, `with_proxy(spec, origin)`, `or_detected(&reader, &read_file)`, `proxy_for(host, &pins)`, `proxy_at(host, port, &pins)`, `proxy_for_scheme(scheme, host, port, &pins)`, `proxy_of(scheme)`, `no_proxy_entry(host)`, `no_proxy_entry_at(host, port)`, `proxy`, `http_proxy`, `skipped`, `Given`, `Origin` (`Variable`, `User`, `File`), `Scheme` (`Http`, `Https`), `Rules`, `PROXY_VARS`, `HTTP_PROXY_VARS`, `HTTP_PROXY_VARS_LOWER`, `NO_PROXY_VARS`, `PROXY_VARS_UPPER_FIRST`, `HTTP_PROXY_VARS_UPPER_FIRST`, `NO_PROXY_VARS_UPPER_FIRST` | the proxy and the `NO_PROXY` list of the variables that the caller's reader gives, by the rules of the consumer, each with where it came from; the proxy that the user gave; the proxy of a tool's file |
| `env::{proxy_from_vars, is_loopback, no_proxy_matches, no_proxy_entry, no_proxy_entry_at}`, `NoProxy` (`parse`, `matches(host, port)`) | the rules of a `NO_PROXY` list (names, suffixes, `*`, addresses in any form, blocks `ADDR/BITS`, ports), and the loopback |
| `detect::{detect, sources, read_file, proxy_in_gitconfig, proxy_in_npmrc, proxy_in_pipconf, proxy_in_environment_file}` | the proxy of git, npm, pip and `/etc/environment`, through the caller's reader of files |
| `Proxy` (`parse`, `parse_trusted`, `parse_with(spec, commands, http_port)`, `secret_parts`, `url`, `url_at`, `label`, `kind()`, `carrier()`, `address()`, `has_credentials()`, `scheme()`, `kind_text()`, `outcome()`, `host`, `port`, `kind`), `ProxyKind` (`Http`, `Socks5 { remote_dns }`; `scheme`, `name`, `request`), `Carrier` (`Tcp`, `Unix`, `Exec`), `HttpProxy`, `proxy::{percent_decode, HTTP_PORT, SOCKS_PORT}` | the proxy that a URL names; its credentials never show in `Display`, `label` or `Debug`, nor more than the first word of a command |
| `Stream` (`Tcp`, `Unix`, `Exec`; `tcp`, `into_tcp`, `peer_addr`, `local_addr`), `ChildIo` (`spawn`, `exited`) | the stream of a dial; a command's stdin and stdout as one stream |
| `digest::{parse_challenge, challenge_in, authorization, cnonce, md5, Challenge, ALGORITHMS}` | HTTP Digest (RFC 7616) for a `CONNECT` |
| `proxy_auth::schemes(head)` | the schemes of each `Proxy-Authenticate` of a `407`, in order, once each: for a caller that speaks to a proxy of its own |
| `Credentials` (`new`, `load`, `user`, `basic_header`, `check`, `check_basic`) | the user and the password that a client of a served proxy gives |
| `dial_addrs(&addrs, timeout)`, `group_errors(&errors)`, `hint::open_files(&err)` | a race of given addresses with no proxy, and the index of the one that won; the errors of many addresses, one entry for each text; the words of too many open files |
| `DialError` (`stage_and_class`, `cut`), `From<DialError> for Failure` | why a connection could not be opened, and its stage and class; `DialError::Connect` keeps the cut and the kind of the error of I/O of the last address |
| `Cut` (`of`, `of_failure`, `words`), `TlsFailure::of(&err, &words)`, `Missed` (`words`, `tls`) | how a connection ended, by the kind of its error of I/O |
| `Client::new(context, dial)`, `with_user_agent`, `with_records` (feature `ech`), `https_get`, `https_get_accepting` | a GET over HTTPS through the caller's dial and TLS context; a `Failure` at the stage of the step that failed |
| `Client::send(request)`, `client::Request` (`new`, `header`, `body`, `json`, `timeout`, `body_idle`, `via`, `trust`, `max_body`, `redirects`), `client::{MAX_BODY, MAX_HEAD, TIMEOUT, BODY_IDLE}`, `Response::url` | one request of any method to an `http://` or `https://` URL, under one time limit; `https://` offers ALPN `http/1.1` over the roots of the context unless the request gives a trust; a handshake that the network ended is at `Stage::Network`. `url` is the URL of the answer: after redirects, the last one |
| `Request::redirects(n)` (0 by default: none) | `send` and `send_streamed` follow 301, 302, 303, 307 and 308 of a `GET` or a `HEAD`, at most `n`; another method gets the 3xx as it is, and so does a 3xx with no `Location`. The `Location` resolves against the URL that gave it (absolute, `//host/path`, `/path`, a relative path with `.` and `..`, `?query`); its fragment goes. Each hop takes the proxy of its own URL and the trust of the request; `Request::timeout` covers the heads of all hops, `body_idle` the body of the last. `Authorization`, `Proxy-Authorization` and `Cookie` go only to the scheme, the host and the port of the first URL. A step from `https://` to `http://` is an error (`a redirect from https:// to http:// is refused: FROM to TO`), and so is one more redirect than `n` (`more than N redirects; the last one to URL`); a failure of a hop says `redirected to URL`. The URLs in the words have no query |
| `Client::send_streamed(request) -> Result<client::Streamed, Failure>`, `Streamed` (`status`, `reason`, `headers`, `url`, `header`, `is_success`, `tokio::io::AsyncRead`) | the same request; the head of the answer within `Request::timeout`, then the body as a reader that owns the connection: by `Content-Length`, by chunks or to the close, with no bound of its size; each wait for bytes within `Request::body_idle` (30 s by default), else `TimedOut`; a body that ends before its length or its last chunk is `UnexpectedEof`; a `HEAD`, 1xx, 204 and 304 have no body. `send` reads its answer through it, with `max_body` and no idle limit. A drop after the end of the body gives the connection to the pool (below); a drop before it closes the connection |
| `Client::with_keep_alive(on)` (`true` by default), `client::pool::{IDLE, PER_KEY, TOTAL}` | the idle connections of `send` and `send_streamed` ("Keep-alive", below); `false`: `Connection: close` and a new connection for each request |
| `http::{exchange, read_response, read_answer, Reading, parse_head, parse_status_line, decode_chunked, Response, Head, MAX_HEAD, MAX_INTERIM}`, `Response::{is_success, text}` | one HTTP/1.1 request and its response over an open stream, bounded (a head of 16 KiB or of the caller's `Reading`, the body of the caller), with up to 5 interim answers passed over |
| `http::{read_head, Body, Framing, Idle}` | the head of a response, and its body as a reader (`Body::new(stream, rest, framing, limit)`, `to_end`, `is_done`, `into_reusable`: the stream after a body read to its end by its length or its chunks), with a limit for each wait (`Idle`, `into_inner`); `read_answer` reads through them |
| `http::request::{read_request, parse_request, connect_target, reason, refusal, answer, established, Request, RequestError, MAX_HEAD}` | a server's reading of a request head, and its answers |
| `host::{authority, check_name, split_host_port}`, `interleave`, `MAX_PROXY_RESPONSE` | `host:port`, the check of a name, the order of the families |
| `mptcp::{probe, enabled, connect, connect_with, socket, Multipath, ENABLED_FILE}` | Multipath TCP (Linux) |
| `lowat::{set, LOWAT}`, `write_limit::WriteLimit` | `TCP_NOTSENT_LOWAT` (Linux, macOS); a stream whose write fails when it makes no progress |
| `udp::{open_in_stream, associate, write_frame, read_frame, wrap, wrap_name}` | the SOCKS5 datagram path on a stream that the caller dials: the command 0x83 (UDP in the stream: a frame of a 2-byte length and a datagram of RFC 1928 each way), `UDP ASSOCIATE`, and the datagram header. The reply 0x07 to 0x83 is `DialError::UdpRefused` |
| `stun` | STUN's messages (RFC 8489): a Binding Request, a Binding response, the reading of any message; MESSAGE-INTEGRITY (`signed`, `integrity`), FINGERPRINT (`fingerprinted`, `fingerprint_ok`), the long-term key (`long_term_key`), the methods and attributes of TURN (RFC 8656, RFC 6062) and `message_type` (T-370; the vectors of RFC 5769, 2.1, 2.2 and 2.4) |
| `resolve::System` (`new`, `with_pins`), `resolve::{LITERAL, PIN, SYSTEM}` | the system resolver after the IP literals and the caller's pins, for a consumer with no DNS over HTTPS and for the tests |

## Keep-alive

`Client::send` and `send_streamed` (and the redirects that they follow)
keep a connection open after its answer, and send the next request with
the same key on it. The default is on: a pull of many blobs from one
registry pays one TCP and TLS handshake, not one for each blob.

* The key: the scheme, the host (in lower case), the port, the way out
  (the same `Arc<dyn Dial>`: the client's dial or the request's `via`,
  compared by its pointer) and the trust (`Trust::Default`, the same path
  of `Trust::File`, or the same `Arc` of a caller's `ClientConfig`). A
  connection never serves another trust or another way out. Two
  configurations with the same roots are two keys. Two clients made with
  `Client::new` have two pools; the clones of one client share its pool.
* A connection goes back to the pool when its answer is of `HTTP/1.1`,
  has no `Connection: close`, is not a 1xx, and its body was read to its
  end by its `Content-Length` or its chunks, with no byte after it. A
  body read to the close, a body dropped before its end, and a request
  with its own `Connection: close` close the connection.
* The limits: an idle connection stays at most `IDLE` (30 s); at most
  `PER_KEY` (4) for each key and `TOTAL` (16) for each client; the oldest
  goes first.
* An idle connection that the server closed, or that has bytes that no
  request asked for, is dropped before use. A `GET` or a `HEAD` on an idle
  connection that fails before a byte of the answer goes again, once, on
  a new connection. Another method does not: the server can have done it.
* `with_keep_alive(false)` sends `Connection: close` and opens a new
  connection for each request, as before.
* A pooled connection belongs to the tokio runtime that opened it. A
  client shared by two runtimes stays correct: the check before a reuse
  drops a connection whose runtime is gone, and the request opens a new
  one (`a_connection_stays_with_its_runtime`, with a POST, which is not
  tried again). Each runtime then keeps its own connections only when it
  has its own client. The blocking client runs each request on the runtime
  of its dialer.
* A connection through a proxy stays open in the pool too: the proxy sees
  the tunnel until the idle limit or the drop of the client.
* `Client::https_get` and `https_get_accepting` do not use the pool: they
  send `Connection: close` through `http::exchange`, which works on a
  stream that the caller opened.

## Features

| Feature | What |
| --- | --- |
| none (default) | everything above; the GET client's hello is plain |
| `ech` | the GET client opens through `pod_tls::ech::open`: sealed where the TLS context asks for ECH and the name publishes a list, with the reader of HTTPS records of `Client::with_records` (`pod-dns` gives one) |
| `blocking` | `blocking::Dialer` (`tcp`, `stream`) and `blocking::Client` (`https_get`, `send`, `send_streamed`, `with_keep_alive`): the same code on a private current-thread runtime, for a synchronous consumer. A TCP connection comes back as the standard `TcpStream`; a stream through a proxy at a command, and the body of `blocking::Streamed` (`status`, `reason`, `headers`, `url`, `header`, `is_success`, `std::io::Read`), are read on that runtime. Do not call it from a task of a tokio runtime |
| `fuzzing` | `fuzz_exchange`: the SOCKS5 exchange with a scripted proxy, for the fuzz target `socks5_answer` (`fuzz/`). The `--cfg fuzzing` of cargo-fuzz also exposes it. No consumer turns it on |

## Settings

| Field | What | podssh | podnet |
| --- | --- | --- | --- |
| `Settings::app` | the program in the words of a 407 and of a SOCKS5 method | `podssh` | `podnet` |
| `Settings::user_agent` | the `User-Agent` of a CONNECT; none with `None` | `Some(format!("podssh/{}", env!("CARGO_PKG_VERSION")))` | `None` (podnet sent none with CONNECT) |
| `Settings::proxy` | `ProxyChoice` | `FromEnvironment(Environment::read(&\|n\| std::env::var_os(n)))` by default; `Direct` for `--direct`; `Via(proxy)` | `Direct` for `--no-proxy`; else `FromEnvironment(env)`: `env = Environment::read_with(&var, rules)`, then `.with_proxy(spec, Origin::User("--proxy".into()))` for `--proxy` (`Origin::User("PODNET_PROXY".into())` for the variable), else `.or_detected(&var, &pod_proxy::detect::read_file)`; `Via(proxy)` for a dial that names its proxy (podnet's `via`) |
| `Settings::multipath` | ask for Multipath TCP with no proxy | `true` for the direct road with MPTCP | `false` |
| `Settings::lowat` | the unsent bytes of each socket | `Some(LOWAT)` (16 KiB, the default) | `None` (podnet set no `TCP_NOTSENT_LOWAT`) |
| `Settings::write_limit` | the limit of a write of a stream of `Dial::dial` | `None` in the dial; podssh wraps its SSH transport in `WriteLimit::new(tcp, WRITE_TIMEOUT)` itself | `None` |
| `Settings::attempt_delay` | the attempt delay of the race | `ATTEMPT_DELAY` (250 ms, the default) | `ATTEMPT_DELAY` (podnet's is 250 ms too) |
| `Rules::proxy_vars`, `Rules::no_proxy_vars` | the variables, in their order | `PROXY_VARS`, `NO_PROXY_VARS` (lower case first, the default) | `PROXY_VARS_UPPER_FIRST`, `NO_PROXY_VARS_UPPER_FIRST` |
| `Rules::http_proxy_vars` | the variables of a request of plain `http://`, in their order | `HTTP_PROXY_VARS` (the default): `http_proxy`, `HTTP_PROXY`, `all_proxy`, `ALL_PROXY`, `https_proxy`, `HTTPS_PROXY` | `HTTP_PROXY_VARS_UPPER_FIRST`: `HTTP_PROXY`, `http_proxy`, `ALL_PROXY`, `all_proxy`, `HTTPS_PROXY`, `https_proxy` |
| `Rules::skip_unusable` | a URL that cannot be used is passed over to the next variable, into `Environment::skipped` (with a warning to `tracing`) | `false` | `true` |
| `Rules::http_port` | the port of `http://` with no port | `HTTP_PORT` (80) | `8080` |
| `Rules::no_proxy_ports` | an entry `host:port` of `NO_PROXY` excludes that port only | `false` (the port is passed over) | `true` |
| `Rules::loopback_direct` | a loopback target, and a name pinned to loopback alone, goes directly | `true` | `false` (podnet sends it to the proxy unless `NO_PROXY` names it) |
| the resolver of `Dialer::new` | the addresses and the pins | `pod-dns` (T-015), or an implementation of `pod_core::Resolve` over podssh's `resolve` module (`resolve_from`, `pinned`, `note_dialed`) | `pod-dns` (T-015), or one over podnet's `resolve::resolve` (pins, the system, DoH) |
| `Dialer::with_proxy_resolver` | the resolver of the host of a proxy on TCP | none (the dialer's) | one over `resolve::resolve_no_doh` (the pins and the system): a DoH query can go through the proxy |
| `Dialer::with_observer` | the counter of the decision `egress` | none | an `Observer` that calls `decisions::count(decision, outcome)` |
| `Client::with_user_agent` | the `User-Agent` of a request that gives none | `podssh/VERSION` | `podnet/VERSION` |
| the `user_agent` argument of `http::exchange` | the `User-Agent` of a request; none when empty | `podssh/VERSION` | |

The variables of the environment that `Environment::read` reads through
the caller's reader, in this order: `https_proxy`, `HTTPS_PROXY`,
`all_proxy`, `ALL_PROXY` (the first that is set and not empty) into
`proxy`; `http_proxy`, `HTTP_PROXY`, `all_proxy`, `ALL_PROXY`, `https_proxy`,
`HTTPS_PROXY` into `http_proxy`; then `no_proxy`, `NO_PROXY`; `read_with` reads those of its
`Rules`.

The proxy of each scheme (`Environment::proxy_of`):

| Connection | `Scheme` | The proxy |
| --- | --- | --- |
| a request of plain `http://` (`Client::send`, `send_streamed`) | `Http` | the proxy of the user (`with_proxy`); else `http_proxy` (`Rules::http_proxy_vars`); else the proxy of a tool's file. Each list of six (`HTTP_PROXY_VARS` and `HTTP_PROXY_VARS_UPPER_FIRST`) ends with the two names of `https://`, in its own case order, so that a host that names only the proxy of `https://` keeps it for `http://`, as the dial did before; curl's list (`HTTP_PROXY_VARS_LOWER`) does not read them |
| `https://`, `wss://`, each TCP dial (`Dial::dial`, `Dialer::tcp`, `stream`) | `Https` | `proxy` (`Rules::proxy_vars`, the proxy of the user, or of a tool's file), as before |

The loopback rule and `NO_PROXY` hold for each scheme. A request of
`http://` reaches an HTTP proxy by `CONNECT host:port`, as `https://`
does: the client sends no absolute URI (`GET http://host/path`). A proxy
that refuses `CONNECT` to a port that is not 443 (the default of Squid)
refuses such a request. curl reads no `HTTP_PROXY`: in a CGI program, a
client of the server sets `HTTP_PROXY` with its header `Proxy:`. A
consumer that runs as a CGI program takes `HTTP_PROXY_VARS_LOWER`. The
default reads `HTTP_PROXY` after `http_proxy`, as `ureq` did for podvm.
`ureq` read `ALL_PROXY` before `HTTPS_PROXY`; pod-proxy reads
`HTTPS_PROXY` first, as curl. A value
that is not UTF-8 is unset. `or_detected` reads `HOME` through the same
reader, and the files of `detect::sources` (`~/.gitconfig`,
`~/.config/git/config`, `~/.npmrc`, `~/.config/pip/pip.conf`,
`~/.pip/pip.conf`, `/etc/gitconfig`, `/etc/pip.conf`, `/etc/environment`)
through the caller's reader of files. The consumer keeps the variables:
it reads them once and passes the `Environment`. A proxy command
(`+exec:`) comes only from `Origin::User`: `Rules::parse(spec, &origin)`
and `Proxy::parse_trusted` take it, `Proxy::parse` and a variable or a
file never.

The bypass of a name pinned to loopback addresses alone is the caller's:
`Environment::proxy_for(host, &pins)` takes the pins of the host, and
`Dialer::proxy_for` gives the pins of its resolver (`Resolve::pinned`).

## Parity

| Capability of podssh `crates/podssh-ws/src/{dial.rs,proxy.rs,proxy_auth.rs,socks5.rs,mptcp.rs,race.rs,cut.rs,lowat.rs,write_limit.rs,stun.rs,stun_tests.rs,http.rs,http/,client.rs}` at `6ece2ff` | Here |
| --- | --- |
| The direct dial, the HTTP CONNECT dial and the SOCKS5 dial, the whole within one bound | [x] `Dialer` |
| CONNECT by name, never by address: no DNS behind a proxy | [x] `connect` |
| Basic credentials from the proxy URL; the 407 that names each scheme of `Proxy-Authenticate` and what the program can give | [x] the program is `Settings::app`; the words now name Digest too (see the changes for podssh) |
| The head of the answer read to its end and no further (16 KiB) | [x] `MAX_PROXY_RESPONSE` |
| A proxy's reason phrase made one safe line | [x] `http::parse_status_line` with `pod_core::text::one_line` |
| `https_proxy`, `HTTPS_PROXY`, `all_proxy`, `ALL_PROXY`, lower case first, an empty value unset | [x] `Environment::read`, through the caller's reader, with the variable of each value |
| `no_proxy`, `NO_PROXY`: names, suffixes, `.name`, `*.name`, `*`, a port ignored, blocks `ADDR/BITS` of IPv4 and IPv6, `::ffff:` addresses; the entry that matched | [x] `env` |
| The loopback goes directly; a name pinned to loopback alone goes directly | [x] `Environment::proxy_for` with the pins of the caller; `Dialer::proxy_for` gives the resolver's |
| A proxy URL: `http://`, a bare `host:port` (80), `socks5://` and `socks5h://` (1080), credentials percent-decoded, SOCKS4 refused by name, a host that no name holds refused, RFC 1929 lengths, `url`, `url_at` | [x] `Proxy` (the words of a refused scheme name no program) |
| SOCKS5: no authentication or RFC 1929, the name as address type 3, one more try with an address of the resolver after reply 8, each reply as the status of an HTTP proxy, a bound address read to its end, an answer to the password of version 1 or 5 | [x] `socks5`; the name goes for `socks5h://` only (the rule of curl: see the changes for podssh) |
| The race of addresses: the families in turns, 250 ms between attempts, the first that connects wins | [x] `race`, `interleave`, `Settings::attempt_delay` |
| A source of the resolver whose addresses do not connect hands the dial to the next (4 s each), and the resolver hears which source carried it | [x] `pod_core::Found::more`, `pod_core::SOURCE_WAIT`, `Resolve::note_dialed` |
| Multipath TCP on request on the direct road, plain TCP where the kernel refuses it, whether the server took it | [x] `mptcp`, `Settings::multipath` |
| `TCP_NODELAY` and `TCP_NOTSENT_LOWAT` on each dialed socket | [x] `Settings::lowat` |
| A write that makes no progress fails at its limit | [x] `WriteLimit`; `Settings::write_limit` for the streams of `Dial::dial` |
| How a dial or a handshake ended, by the kind of its error | [x] `Cut`, `Cut::of_failure` (in place of `ConnectError::cut`), `TlsFailure`, `Missed` |
| The errors and their words | [x] `DialError`, the same texts; a timeout has its stage |
| STUN's codec | [x] `stun` |
| The integrity of TURN of podssh (PodUtils/podssh@T-621: `signed`, `integrity`, `fingerprint_ok`, `long_term_key` of `PodUtils/podssh:crates/podssh-ws/src/turn/msg.rs`) | [x] `stun` (T-370) |
| HTTP/1.1: a request and its response, `Content-Length`, chunks, the close, the bounds, a body and a header made safe text, a server's request head, its refusals and answers | [x] `http`, `http::request` |
| The GET client: the configuration and the name checked first, the dial, the handshake (sealed with ECH where asked), the exchange, each bounded | [x] `Client` (ECH with the feature `ech`) |
| The rest of `client.rs`: the WebSocket client, `https_post_json`, `https_with_token`, `doctor`, the frames | `pod-ws` (T-016) |
| The tests: unit (27) and `tests/{dial,dial_pinned,dial_race,socks5,mptcp,http,limits}.rs`, with their names | [x] with the tests of podnet and the new tests: 141 passed, 1 ignored (the live test of MPTCP); the tests changed for the merge are in the changes for podssh |
| `tests/limits.rs`: the tests of the upgrade and of the frames | `pod-ws` (T-016) |
| `doh_wire.rs` `the_dialer_hands_a_cached_address_that_does_not_connect_to_the_next_source` (the cache of podssh's resolver) | `pod-dns` (T-015); the race's part is `dial_race` `a_source_whose_addresses_do_not_connect_hands_the_dial_to_the_next` |
| The fuzz targets `http_head`, `http_chunked`, `socks5_answer`, `proxy_url`, `no_proxy`, `connect_status` | [x] `fuzz/` (T-017), with their names and their checks; `fuzz_exchange` behind the feature `fuzzing` or `cfg(fuzzing)` |
| The plants `t172-no-fallback`, `t639-reason-of-429`, `t641-length-trusted` | [x] `scripts/plants/proxy-mptcp-no-fallback`, `proxy-reason-of-429`, `proxy-stun-length-trusted`: the same defects, caught by the same tests |

New tests: `env` (3), `error` (1), `resolve` (2), `dial_race`
`a_source_whose_addresses_do_not_connect_hands_the_dial_to_the_next`,
`http` `the_user_agent_goes_in_its_header_and_an_empty_one_sends_none`,
`tests/dialer.rs` (6), `tests/get.rs` (3); new plants
`proxy-pins-not-given` and `proxy-source-not-handed`.

| Capability of podnet `src/egress.rs`, `src/proxy_auth.rs`, `src/fetch.rs` and `tests/egress.rs` at `a8071b1` | Here |
| --- | --- |
| The one dial of each connection: `--proxy`, `--no-proxy`, else the variables (`ProxyChoice`, the process's `set_choice`) | [x] `Dialer` with `Settings::proxy`; a value of the consumer, no process state |
| `HTTPS_PROXY`, `https_proxy`, `ALL_PROXY`, `all_proxy` in this order; `NO_PROXY` before `no_proxy`; an empty value unset | [x] `Rules::{proxy_vars, no_proxy_vars}` with `PROXY_VARS_UPPER_FIRST`, `NO_PROXY_VARS_UPPER_FIRST` |
| A proxy variable that does not parse is passed over with a warning, and the next one is read | [x] `Rules::skip_unusable`, `Environment::skipped`, a warning to `tracing` |
| `NO_PROXY` applies to `--proxy` too | [x] `Environment::with_proxy` keeps the list |
| `NO_PROXY`: `*`, names and `.name`, addresses, blocks, an entry with `:port` for that port only, `[v6]:port`; an entry that cannot be read is passed over with a warning | [x] `no_proxy_entry_at`, `NoProxy`, `Rules::no_proxy_ports`; an address matches the same address in another form; the warning in `Environment::read_with` |
| The loopback goes to the proxy unless `NO_PROXY` names it | [x] `Rules::loopback_direct = false` |
| The proxy of a tool's file when no variable is set (git `http.proxy`, npm `https-proxy`/`proxy`, pip `proxy`, `/etc/environment`), read one time; a value that does not parse is passed over with a warning | [x] `detect`, `Environment::or_detected` (once by the consumer, no `OnceLock`); `HOME` and the files through the caller's readers; the file is the origin (`Origin::File`) |
| A proxy URL: `http` (8080 with no port), `socks5`, `socks5h` (1080), any case; `https://` and SOCKS4 refused with their words; no path, query or fragment, with the words of percent-encoding; an escape of two hex digits; credentials in UTF-8; RFC 1929 lengths; the port 0 refused; a host in brackets is IPv6; a host with a character that no name holds refused | [x] `Proxy::parse_with(spec, commands, 8080)` (`Rules::http_port`); the forms of `proxy/parse.rs` |
| `socks5://` resolves the name here (the pins, the system, DoH) and sends the address; `socks5h://` sends the name; the words name `socks5h://` when the name does not resolve | [x] `socks5`, through the dialer's resolver; `Dial::names_inside` false for `socks5://` |
| A proxy on a Unix socket: `SCHEME+unix:/PATH`, `SCHEME://unix:/PATH`, an absolute path | [x] `Carrier::Unix`, `Stream::Unix` (Unix hosts) |
| A proxy at a command, `SCHEME+exec:COMMAND` (`sh -c`, one process for each connection, its stderr the process's, killed at the drop), only from `--proxy` and `PODNET_PROXY` | [x] `Carrier::Exec`, `ChildIo`, `Proxy::parse_trusted`, `Origin::User` |
| The Display of a proxy: `scheme://host:port`, `scheme+unix:/PATH`, `scheme+exec:WORD ...`; Debug and Display with no credentials | [x] `Proxy::label`; `Display` stays `host:port` (podssh) for a proxy on TCP; Debug shows `<hidden>` |
| `secret_parts`: the user and password text, the password as written and decoded, a command whole; none for a URL with no user | [x] `Proxy::secret_parts` |
| `host()`, `carrier()`, `kind()`, `kind_text()`, `scheme()`, `address()`, `has_credentials()` | [x] the same names; `host` is a field |
| HTTP CONNECT: `Host`, `Proxy-Connection: keep-alive`, Basic from the URL, the head read to its end and no further | [x] `connect`; the bound of the head is podssh's 16 KiB (podnet: 8 KiB) |
| Digest: a 407 with a Digest challenge after Basic gets the answer on a new connection; MD5, MD5-sess, SHA-256, SHA-256-sess, qop auth, opaque; the scheme in any case, among other challenges; a challenge with no nonce or another algorithm is not answered; the client nonce of 16 random bytes | [x] `digest`, `connect`; SHA-256 of `sha2` (podnet: ring); the nonce from `pod_core::entropy::OsEntropy`, an error when the system gives none (podnet: zeros) |
| The 407: each scheme of each `Proxy-Authenticate`, what was sent (none, Basic, a Digest answer), the advice for Basic and Digest only (cntlm), for a Digest challenge not answered, for a Digest answer refused | [x] `proxy_auth::explain`, with the program of `Settings::app` |
| A refusal: the status, the reason phrase and the first line of the body (512 bytes, by `Content-Length`, 2 s), made safe | [x] `DialError::ProxyRefused`, its `reason` |
| SOCKS5: the greeting, RFC 1929, the replies with their words, the bound address read to its end | [x] `socks5` (podssh's words of the replies) |
| `Refusal` and `refusal(err)`: a refusal of a proxy in the chain of an error | [x] `DialError::ProxyRefused` in the `Failure`'s causes; a SOCKS5 refusal has the status of its HTTP case (403, 407, 502) and a reason that starts with `SOCKS5`, where podnet gave the status 0 |
| The stage of a failure (`hop::Hop::{Proxy, Dns}`) | [x] `DialError::stage_and_class`, `pod_core::Stage` |
| The proxy host is resolved with no DoH (a DoH query can go through this proxy); a host that does not resolve names the proxy host | [x] `Dialer::with_proxy_resolver`; `the proxy host H does not resolve` |
| The counters of the decisions (`egress`: `direct`, `none`, `no_proxy`, `via`, `http`, `socks5`, `unix`, `exec`) | [x] `Dialer::with_observer`; also `loopback` and `unusable` |
| A direct dial: each address of the name, the attempts 250 ms apart, the families in turns, `TCP_NODELAY` | [x] `race`, `Settings::attempt_delay` |
| `dial_addrs(addrs, timeout)` with the index of the address | [x] `dial_addrs` |
| The errors of many addresses, one entry for each text (`group_errors`); the kind of the last error kept | [x] `group_errors` in `race`; the kind is `DialError::Connect::kind`, beside its `cut` |
| The hint of too many open files (`rlimit::hint`) | [x] `hint::open_files`, the same words |
| The client: one request of any method, `http://` and `https://`, its headers, a JSON body, a time limit (30 s), `Connection: close`, `Accept-Encoding: identity`, `Host` with the port when not the default, a user agent, `Content-Length`; a header with a line break refused; the words `METHOD URL` with no query | [x] `Client::send`, `client::Request` (`json` takes the text) |
| A proxy for one request only (`Request::via`) | [x] `Request::via` takes a dial (`Arc<dyn Dial>`), as a `Dialer` with `ProxyChoice::Via` |
| TLS of a request: the roots of the context (`--ca-file`, `SSL_CERT_FILE`, the file beside the binary, the system, the compiled-in roots), ALPN `http/1.1`, the stage of a handshake that the network ended | [x] `Context::roots` through `Client::send`; `Stage::Network` |
| The URL: the fragment not sent (also before a path), a query without a path, IPv6 in brackets, no user part, no space or control character | [x] `client/url.rs` |
| The answer: a head of 64 KiB, more than 64 headers, up to 5 interim answers, a body by length, by chunks (a size that overflows refused) or to the end, a `HEAD`, 204 and 304 with no body, the end of TLS with no close_notify, the bound of the body for each request (16 MiB by default); `is_success`, `text`, `header` | [x] `http::read_answer`, `Reading`, `client::{MAX_HEAD, MAX_BODY}`, `Response`; the body through `http::Body`, the reader of `Client::send_streamed` too |
| `Response::json` | the consumer: `serde_json::from_slice(&response.body)` (pod-proxy takes no serde) |
| The mint of the relay and the API of the quick tunnels: their paths and their JSON | they stay in podnet (`ws_relay.rs`, `quicktunnel.rs`), then `pod-relay` (M2) and `pod-cloudflare` (M3); they call `Client::send` |
| `Credentials` of a client of a served proxy: 1 to 255 bytes, no `:` in the user, a password file of this user that others cannot read, a check in constant time, Basic | [x] `Credentials`; the file through `pod_core::files::read_own` (also refuses a link and another user's file) |
| The unit tests of the three files and `a_silent_address_does_not_hold_the_dial` of `tests/egress.rs`, with their names | [x] 33: `src/proxy/tests.rs` (8), `src/env_tests.rs` (1), `src/race.rs` (2), `src/proxy_auth.rs` (2), `tests/http.rs` (`status_parsing`), `tests/egress.rs` (4), `src/digest_tests.rs` (4), `src/detect.rs` (2), `src/credentials.rs` (2), `src/client/url.rs` (2), `tests/fetch.rs` (5) |
| The other tests of `tests/egress.rs` (they run the podnet binary) | they stay in podnet (the adoption map) |
| The fuzz targets `proxy_url`, `no_proxy`, `connect_response_head` (podnet `src/fuzzing.rs`) | [x] `fuzz/` (T-017): one target each with podssh's `proxy_url`, `no_proxy`, `connect_status`, with the checks and the seeds of both (`fuzz/README.md`) |

New tests of the merge: `the_secret_parts_of_a_url`,
`a_carrier_word_after_the_scheme_is_no_carrier`,
`the_rules_of_a_consumer_choose_the_order_the_ports_and_the_loopback`,
`a_proxy_of_the_user_can_be_a_command_and_keeps_no_proxy`,
`the_proxy_of_a_tool_comes_through_the_readers_of_the_caller`,
`the_entries_that_cannot_be_read`,
`the_challenge_of_a_head_and_the_client_nonce`,
`the_hint_of_open_files_names_the_limit`,
`the_basic_header_and_a_missing_file`,
`a_socks5_url_sends_the_address_that_the_resolver_of_the_dialer_gives`,
`a_socks5_url_with_a_name_that_does_not_resolve_names_socks5h`,
`a_refused_digest_answer_names_the_user_and_the_password`,
`a_refusal_keeps_the_first_line_of_its_body`,
`each_dial_is_one_decision_of_the_observer`,
`the_proxy_host_takes_the_resolver_of_the_proxy`,
`a_head_has_no_body_and_101_is_the_answer`,
`a_request_sends_its_head_and_its_body`,
`a_request_takes_its_own_way_out_and_its_own_user_agent`,
`a_silent_server_ends_at_the_time_limit`,
`a_close_in_the_tls_handshake_is_the_network`,
`a_request_over_https_takes_its_trust_or_the_roots_of_the_context`.

New tests of the adoption trials: `tests/http.rs`
`the_schemes_of_a_407_are_read_for_a_caller_that_speaks_to_its_proxy`
(podssh's second proxy reads the schemes of its `407`), `tests/egress.rs`
`a_failed_dial_keeps_the_kind_of_its_last_error` (podnet's doctor reads a
refusal of this host by the kind of its error).

## Changes for podssh

The merge changes these behaviours of podssh's code:

* **`socks5://` resolves the name here** with the dialer's resolver (its
  pins too) and sends the address, as curl does; `socks5h://` sends the
  name. A user of podssh who needs the name sent (a host with no DNS)
  writes `socks5h://`. `Dial::names_inside` is false and the label is
  `socks5://...` for a `socks5://` proxy. `ProxyKind::Socks5` is
  `ProxyKind::Socks5 { remote_dns }`. The tests of `tests/socks5.rs` that
  send a name take `socks5h://`.
* **Digest.** A 407 with a Digest challenge, after Basic with the
  credentials of the URL, gets the Digest answer on a new connection.
  The words of a 407: `REASON; it asks for NTLM or Negotiate; podssh sent
  Basic with the user and the password of the proxy URL; podssh supports
  Basic and Digest only: ...` (podssh: `it asks for NTLM and Negotiate,
  and podssh sends Basic only`); with no credentials, `podssh sent no
  credentials (put user:password@ in the proxy URL)`.
* A refusal's reason has the first line of the body after the reason
  phrase; a CONNECT has `Proxy-Connection: keep-alive`; a SOCKS5
  refusal of the password says `the proxy refused the user and the
  password`.
* A proxy URL with a path, a query or a fragment is refused (podssh
  passed over the path), and so are the port 0, a host in brackets that
  is not IPv6, and a `%` or `\` in a name. The words of a refused scheme
  name no program: `the proxy scheme ftp:// is not supported; HTTP CONNECT
  goes to an http:// proxy, and SOCKS5 to a socks5:// or socks5h:// one`
  (podssh: `proxy scheme ftp:// is not supported; podssh speaks HTTP
  CONNECT ...`).
* `Proxy`'s Debug shows `<hidden>` for the credentials, and a carrier.
  `Display` stays `host:port`; `Proxy::label` gives the scheme form.
* `http::read_response` passes over up to 5 interim answers (1xx but
  101) and takes the end of a TLS stream with no close_notify as the
  end of the stream.
* `Dialer::tcp` refuses a proxy that is not on TCP (`Dialer::stream`
  takes each); `Environment` has the fields `skipped` and `rules`.
* The detail of `DialError::Connect` groups the errors of the addresses
  by their words.

## Changes for podnet

The merge changes these behaviours of podnet's code, as its trial shows
them (podnet's glue keeps its words of a refusal, its hops and its
counters):

* The words of a 407: `the proxy refused CONNECT with HTTP 407: Proxy
  Authentication Required; it asks for NTLM or Negotiate; podnet sent ...`
  (podnet: `the proxy requires authentication (HTTP 407): ...`).
* A SOCKS5 refusal has the reason `SOCKS5: ...` or `SOCKS5 reply N, ...`,
  and the status of its HTTP case in `DialError::ProxyRefused`; podnet's
  `Refusal` keeps the status 0 for it.
* The head of a proxy's answer is read up to 16 KiB (podnet: 8 KiB); the
  Digest client nonce comes from the system, and none is an error (podnet:
  zeros); the credentials file is refused when it is a link or a file of
  another user.
* A handshake of a request that the network ended has the stage
  `Network`; podnet says it in the words of its `hop::tls_text`.

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
| podssh | `podssh_ws::{DialError, dial::DialError}` | `pod_proxy::DialError`; `Timeout` has a `stage`, `Connect` a `kind` (the kind of the last error of I/O, or `None`); `Failure::from(e)`; from a `Failure`, `e.source().downcast_ref::<DialError>()` |
| podssh | `podssh_ws::{Proxy, HttpProxy, ProxyKind}`, `podssh_ws::proxy::*` | `pod_proxy::{Proxy, HttpProxy, ProxyKind}`; `proxy::percent_decode` is public; `ProxyKind::Socks5` is `ProxyKind::Socks5 { remote_dns }` (`{ .. }` in a pattern); `format!("{}://{p}", p.kind.scheme())` is `p.label()` |
| podssh | `podssh_ws::resolve::{set_pins, pinned}` as the dial reads them | the resolver of the dialer: `Resolve::pinned`; `resolve::System::with_pins(pins)`, or the pins of `pod-dns` |
| podssh | `podssh_ws::resolve::{resolve, resolve_from, Found, SOURCE_WAIT}` as the race reads them | `pod_core::{Resolve, Found, SOURCE_WAIT}`; `Found` has `more` |
| podssh | `podssh_ws::{Cut, Missed, TlsFailure}`, `podssh_ws::cut::*` | `pod_proxy::{Cut, Missed, TlsFailure}`; `TlsFailure::of(&e, context.words())`, `Missed::tls(&e, context.words())` |
| podssh | `DialError::cut()`, `ConnectError::cut()` | `DialError::cut()`; `Cut::of_failure(&failure)` |
| podssh | `podssh_ws::client::{https_get, https_get_accepting}(host, port, path, .., &trust, &proxy, timeout)` | `Client::new(context, dial).with_user_agent(..)` once, then `client.https_get(host, port, path, max_body, &trust, timeout)`, `client.https_get_accepting(..)`; `Err(Failure)` with the same words (`the HTTPS request failed: ..`, `the HTTPS request timed out after Ns`) |
| podssh | `podssh_ws::http::exchange(stream, method, ..)` | `pod_proxy::http::exchange(stream, user_agent, method, ..)` |
| podssh | `podssh_ws::http::*`, `podssh_ws::http::request::*` | `pod_proxy::http::*`, `pod_proxy::http::request::*` |
| podssh | `podssh_ws::proxy_auth::schemes` (the 407 of its second proxy, `via`) | `pod_proxy::proxy_auth::schemes` |
| podssh | `podssh_ws::mptcp::*`, `podssh_ws::write_limit::WriteLimit`, `podssh_ws::stun::*` | `pod_proxy::mptcp::*`, `pod_proxy::write_limit::WriteLimit`, `pod_proxy::stun::*` |
| podssh | `podssh_ws::lowat::set(&tcp)` | `pod_proxy::lowat::set(&tcp, pod_proxy::lowat::LOWAT)` |
| podssh | `podssh_ws::fuzz_exchange` (`cfg(fuzzing)`) | `pod_proxy::fuzz_exchange` (`cfg(fuzzing)`, or the feature `fuzzing`) |
| podssh | the fuzz targets `http_head`, `http_chunked`, `socks5_answer`, `proxy_url`, `no_proxy`, `connect_status` | the targets of the same names in `fuzz/` of podcrates; podssh removes its own |
| podssh | the variables `https_proxy`, `HTTPS_PROXY`, `all_proxy`, `ALL_PROXY`, `no_proxy`, `NO_PROXY` | podssh keeps them and reads them once: `Environment::read(&\|n\| std::env::var_os(n))`. `Environment::read` reads `http_proxy`, `HTTP_PROXY` too, for a request of `http://` alone: podssh's dials are TLS, WebSocket and TCP, so no behaviour changes |
| podssh | `User-Agent: podssh/VERSION` (CONNECT, `http::exchange`) | `Settings::user_agent`, `Client::with_user_agent`, the argument of `http::exchange` |
| podssh | the words "podssh sends Basic only", "podssh sent none", "which podssh did not offer" | `Settings::app = "podssh"`; the words of a 407 are those of the changes for podssh |
| podssh | a `socks5://` URL of a user who needs the name sent | `socks5h://` (the changes for podssh) |
| podssh | the plants `t172-no-fallback`, `t639-reason-of-429`, `t641-length-trusted` | they leave podssh's list with the files |
| podnet | `egress::{set_choice, choice}`, `ProxyChoice::{FromEnvironment, Direct, Via}` (`main.rs` `network_options`, `status.rs`) | one `Dialer::new(Settings { proxy, .. }, resolver)` made in `main` and shared (`Arc<dyn Dial>`); `--no-proxy` is `ProxyChoice::Direct`; `--proxy`/`PODNET_PROXY` is `ProxyChoice::FromEnvironment(env.with_proxy(spec, Origin::User(..)))`; else `FromEnvironment(env.or_detected(&var, &detect::read_file))`; podnet keeps its own copy of the choice for `status` |
| podnet | `egress::Proxy::from_env()`, `NoProxy::from_env()`, `PROXY_VARIABLES`, `NO_PROXY_VARIABLES` (`status.rs`, `report.rs`) | `Environment::read_with(&\|n\| std::env::var_os(n), rules)` with the rules of the settings table; `PROXY_VARS_UPPER_FIRST`, `NO_PROXY_VARS_UPPER_FIRST` |
| podnet | `egress::detected_proxy()` (`DETECTED`), `proxy_auth::{detect, proxy_in_gitconfig, proxy_in_npmrc, proxy_in_pipconf, proxy_in_environment_file}` | `Environment::or_detected(&var, &pod_proxy::detect::read_file)`, once; `pod_proxy::detect::*`; the line of the detection goes to `tracing` |
| podnet | `egress::proxy_for(host, port)` (`agent.rs`, `doctor.rs`, `edge_h2.rs`, `main.rs`, `preflight.rs`, `resolve.rs`, `tunnel.rs`) | `dialer.proxy_at(host, port)` (`Result<Option<Proxy>, String>`: `.ok().flatten()` where podnet took an `Option`) |
| podnet | `egress::dial(host, port, None, timeout)` (`doctor.rs`, `ws.rs`, `ws_relay.rs`, `warp/session.rs`) | `dialer.stream(host, port, timeout)` (a `Stream`), or `Dial::dial(&Target::new(host, port), deadline)` (a `BoxedIo`) |
| podnet | `egress::dial(host, port, Some(&proxy), timeout)`, `Proxy::connect(host, port, timeout)` | a `Dialer` with `ProxyChoice::Via(proxy)`: `.stream(host, port, timeout)`; its decision is `via` |
| podnet | `egress::dial_direct(host, port, timeout)` (`agent.rs`, `warp/proxy.rs`) | a `Dialer` with `ProxyChoice::Direct`: `.tcp(host, port, timeout)` |
| podnet | `egress::dial_addrs(addrs, timeout)` (`doctor.rs`, `edge_h2.rs`) | `pod_proxy::dial_addrs(addrs, timeout)` (`DialError`; the families take turns) |
| podnet | `egress::group_errors`, `egress::ATTEMPT_DELAY` | `pod_proxy::{group_errors, ATTEMPT_DELAY}` |
| podnet | `egress::Proxy::{parse, parse_trusted, secret_parts, host, carrier, kind, kind_text, scheme, address, has_credentials}`, `Carrier`, `ProxyKind` (`main.rs`, `status.rs`, `doctor.rs`, `warp/`, `tunnel.rs`, `quicktunnel.rs`, `lib.rs`) | `pod_proxy::{Proxy, Carrier, ProxyKind}` with the same methods; `Proxy::parse_with(spec, false, 8080)` (or `rules.parse(spec, &origin)`) where an `http://` URL with no port must be 8080; `proxy.host()` is the field `proxy.host`; `format!("{proxy}")` is `proxy.label()` |
| podnet | `egress::Refusal { status, text }`, `egress::refusal(err)` (`doctor.rs`, `report.rs`) | `DialError::ProxyRefused { proxy, target, status, reason }` found in the causes of the `Failure` (`std::error::Error::source` and `downcast_ref::<DialError>()`); podnet writes its words `the proxy refused CONNECT with HTTP {status}: {reason}`; a SOCKS5 refusal is the one whose `reason` starts with `SOCKS5` (podnet's status 0) |
| podnet | `hop::at(Hop::Proxy, e)`, `hop::at(Hop::Dns, e)` of the dial, `hop::tls_hop` of `fetch.rs` | `Failure::stage` (`Stage::Proxy`, `Stage::Dns`, `Stage::Network`); podnet maps a `Stage` to its `Hop` |
| podnet | `decisions::count("egress", outcome)` | `Dialer::with_observer(observer)`, whose `decision("egress", outcome)` calls `decisions::count` |
| podnet | the kind of the error of I/O of a failed direct dial (`podnet doctor`: a refusal of this host, issue 180) | `DialError::Connect::kind` |
| podnet | `rlimit::hint(&err)` in the dial | `pod_proxy::hint::open_files(&err)`, in the detail of `DialError::Connect` |
| podnet | `resolve::resolve(host)`, `resolve::resolve_no_doh(host)` of the dial | the resolver of `Dialer::new` and of `Dialer::with_proxy_resolver` (`pod-dns`, T-015) |
| podnet | `report::hide` of `Proxy::secret_parts(spec)`, `report::clean` of a refusal's words | podnet keeps `hide`; the crate's words are made safe with `pod_core::text::one_line` |
| podnet | `proxy_auth::{parse_challenge, authorization, cnonce, md5, Challenge}` | `pod_proxy::digest::*`; `cnonce(&mut OsEntropy)` gives `Result` |
| podnet | `proxy_auth::Credentials::{new, load}` (`main.rs`, `ingress.rs`, `proxy.rs`, `warp/proxy.rs`) | `pod_proxy::Credentials` (the errors are `String`: `.map_err(anyhow::Error::msg)`); `load` refuses also a link and a file of another user |
| podnet | `fetch::Request::new(method, url)...send()` (`doctor.rs`, `quicktunnel.rs`, `resolve.rs`, `tunnel.rs`, `update.rs`, `warp/register.rs`, `ws_relay.rs`) | `client.send(pod_proxy::client::Request::new(method, url)...)` with one `Client::new(context, dial).with_user_agent(format!("podnet/{}", env!("CARGO_PKG_VERSION")))` made in `main` |
| podnet | `Request::json(&value)` | `Request::json(value.to_string())` |
| podnet | `Request::via(Option<Proxy>)` (`quicktunnel.rs`, `tunnel.rs`) | `Request::via(Some(Arc::new(dialer_via_proxy)))`, a `Dialer` with `ProxyChoice::Via(proxy)` |
| podnet | `fetch::Response::{status, headers, body, is_success, text, header}` | `pod_proxy::http::Response` (also `reason`) |
| podnet | `fetch::Response::json::<T>()` (`warp/register.rs`, `ws_relay.rs`, `quicktunnel.rs`) | `serde_json::from_slice::<T>(&response.body)` |
| podnet | `fetch::MAX_BODY` (`update.rs`) | `pod_proxy::client::MAX_BODY` |
| podnet | `trust::context().client_config_with(&[], &[b"http/1.1"])` of `fetch.rs` | the `pod_tls::Context` of `Client::new`; `send` offers ALPN `http/1.1` over its roots |
| podnet | `net::BoxedIo` of the dial | `pod_core::BoxedIo` (`Connected::io`), or `pod_proxy::Stream` |
| podnet | `egress::ChildIo` | `pod_proxy::ChildIo` |
| podnet | `fuzzing.rs` `proxy_url`, `no_proxy`, `connect_response_head` (`Proxy::parse`, `NoProxy::parse`, `parse_status`) | the targets `proxy_url`, `no_proxy`, `connect_status` in `fuzz/` of podcrates; podnet removes its three, and `parse_status` calls `pod_proxy::http::parse_status_line` (a `&str`) |
| podnet | the variables `HTTPS_PROXY`, `https_proxy`, `ALL_PROXY`, `all_proxy`, `NO_PROXY`, `no_proxy`, `HOME`, `PODNET_PROXY` | podnet keeps them and reads them through its reader once; `PODNET_PROXY` is a value of the user (`Origin::User`). `HTTP_PROXY`, `http_proxy` serve a request of `http://` alone. `rules()` sets the new field `http_proxy_vars: &pod_proxy::HTTP_PROXY_VARS_UPPER_FIRST`. podnet's `egress::Egress` (its own `Dial` over the `Dialer`) keeps the default `Dial::dial_http`, which is `dial`: its requests of `http://` (a relay or an API base of `http://`) keep `HTTPS_PROXY`, and no behaviour changes. To read `HTTP_PROXY` for them, `Egress` forwards `dial_http` to the `Dialer` |
| podnet | `tests/egress.rs`: `socks5_carries_the_api_and_the_edge`, `no_proxy_bypasses_the_proxy`, `a_407_names_the_schemes`, `a_proxy_refusal_names_the_hop_and_keeps_the_text`, `a_proxy_host_that_does_not_resolve_is_an_error_not_a_crash`, `socks5_resolves_the_target_with_the_pins`, `a_proxy_url_with_no_password_is_shown` | they stay in podnet: they run the binary; the words that they read come from the crate (`NTLM or Negotiate`, `podnet supports Basic and Digest only`, `the proxy refused the user and the password`, `the proxy host podnet-proxy.invalid`) and from podnet's glue (`HTTP 403`, `through the proxy http://127.0.0.1:9`) |
| podnet | `tests/egress.rs` `a_silent_address_does_not_hold_the_dial` | it moved here (`tests/egress.rs`); podnet removes it |
| podvm | `podvm-ssh` `transport::Dialer::dial_tcp` of a relay leg (`mux/node.rs` `dial_ws`, the node and the operator): `resolve_bounded`, then `TcpStream::connect_timeout` for each address; no proxy | `Dialer::from_environment(timeout)`: `transport/egress.rs` makes a `blocking::Dialer::new(Settings::new("podvm", ProxyChoice::FromEnvironment(Environment::read(&\|n\| std::env::var_os(n)))), Arc::new(resolve::System::new()))` when a proxy variable is set; `proxy_at(host, port)` decides; `stream(host, port, timeout)` carries a dial through the proxy (`Stream::Tcp`, `Stream::Unix`); a direct target (the loopback, `NO_PROXY`) keeps podvm's direct dial |
| podvm | `podvm-ssh` `Dialer::new(timeout)` (the `proxy` binary, an SSH `ProxyCommand`) | it stays direct: SSH chooses the proxy of a `ProxyCommand` |
| podvm | `podvm-image` `registry::Client` (`ureq` 2, `try_proxy_from_env`), `transport::Policy::use_proxy` (its own loopback and `NO_PROXY` rules), the Windows guest-image fetch of `podvm` (`windows/setup.rs`, `ureq` 2 with no proxy) | `podvm-image/src/net.rs`: one `blocking::Client::new(context, &dialer).with_user_agent("podvm/VERSION")` over a `blocking::Dialer::new(Settings::new("podvm", ProxyChoice::FromEnvironment(..)), Arc::new(resolve::System::with_pins(..)))` for each registry client; `send_streamed(Request::new("GET", url).header(..).timeout(..).body_idle(..).trust(Trust::caller(config)))`, and podvm reads each blob through `blocking::Streamed` (`std::io::Read`) into its verifier and the staged file, with `status()` and `header(..)` for the token challenge, `Retry-After`, `Link` and the digest. podvm's redirect glue goes: `Request::redirects(5)` follows a redirect of a GET or a HEAD with podvm's rules (five at most; `Authorization` only to the first origin; no redirect from `https://` to `http://`), and `blocking::Streamed::url` names the last URL. The trust of each request is podvm's own configuration: the verified one, or the one that accepts any certificate for a registry named insecure. The environment reader joins `NO_PROXY` and `no_proxy`, and the resolver pins `ip6-localhost` to `::1`: podvm's own rules hold, and `Policy::use_proxy` goes. A request of `http://` (an insecure registry) takes `http_proxy`, then `HTTP_PROXY`, then `ALL_PROXY`, as `ureq` read them: podvm needs no glue for `HTTP_PROXY`, and `HTTPS_PROXY` alone still serves such a request (the last of the default list). podvm names the host of a transport failure once: it takes the repeats of the host out of the words of the crate |
| podvm | `podvm-complete` `reach.rs` (`ureq` 2, `try_proxy_from_env`, a HEAD with 2 s bounds) | the same `net.rs`: `send("HEAD", ..)`; a status is an answer. ureq leaves podvm |

## Sources

`SOURCES` lists the imported files: podssh `crates/podssh-ws/src/`
`dial.rs` (now `dial.rs`, `env.rs`, `connect.rs`, `error.rs`, `host.rs`),
`proxy.rs`, `proxy_auth.rs`, `socks5.rs`, `mptcp.rs`, `race.rs`, `cut.rs`
(and `cut_tests.rs`), `lowat.rs`, `write_limit.rs`, `stun.rs`,
`stun_tests.rs`, `http.rs`, `http/`, the GET client of `client.rs`, and
the tests `tests/{dial,dial_pinned,dial_race,socks5,mptcp,http,limits}.rs`
at `6ece2ff`; podnet `src/egress.rs` (now `proxy.rs`, `proxy/`, `env.rs`,
`env/`, `stream.rs`, `connect.rs`, `socks5.rs`, `proxy_auth.rs`,
`race.rs`, `hint.rs`, `dial.rs`), `src/proxy_auth.rs` (now `digest.rs`,
`detect.rs`, `credentials.rs`), `src/fetch.rs` (now `client.rs`,
`client/`, `http.rs`) and the test `a_silent_address_does_not_hold_the_dial`
of `tests/egress.rs` at `a8071b1`; podnet `crates/podnet-netns/src/upstream.rs`
(the datagram path: now `socks5/udp.rs`) at `29ba18a`.
