# pod-dns

The resolver of the crates of podcrates and their binaries: a name to
its addresses without trust in the resolver of the host. In order: an IP
literal, the pins of the user, the system resolver, the addresses that
carried a session (a cache that the consumer keeps), DNS over HTTPS, and
the old addresses of the cache last. DNS over HTTPS goes to resolvers by
their address (a resolver needs no resolver), or by their name through a
proxy that resolves names, in the JSON form or in RFC 8484's wire form,
over HTTP/1.1 or over HTTP/2, through the caller's dial and TLS. The same
resolvers give the SRV records of a name, its HTTPS record (the ECH list
for `pod-tls`) and an answer of the JSON form with its status.

The resolvers are data: the crate holds no host. The default list comes
from the services of the kind `doh` of `pod-services`; a consumer gives
its own. The crate reads no variable and no file unless the caller asks
(`route::has_default_route`), keeps no process state and prints nothing.
The consumer makes one `Resolver` from its `Settings`, gives it the way
to DNS over HTTPS, and gives it to its `pod_proxy::Dialer` as the
`Arc<dyn pod_core::Resolve>`.

## Use

```rust
use std::sync::Arc;
use pod_proxy::{Dialer, ProxyChoice};

let settings = pod_dns::Settings { pins, user_agent: "podssh/1.2.3".into(), ..Default::default() };
// The resolver of the dial of DNS over HTTPS, and of the host of a proxy:
// the pins and the system, no DNS over HTTPS.
let plain = Arc::new(pod_dns::Resolver::new(settings.clone()));
let doh_dial = Arc::new(Dialer::new(pod_proxy::Settings::new("podssh", ProxyChoice::Direct), plain.clone()));
let resolver = Arc::new(pod_dns::Resolver::new(settings).with_doh(context.clone(), doh_dial));
let dialer = Dialer::new(proxy_settings, resolver.clone()).with_proxy_resolver(plain);
```

The dial of DNS over HTTPS must not ask DNS over HTTPS through itself:
its resolver is one of `Resolver::new`. The same holds for the host of a
proxy (`Dialer::with_proxy_resolver`): a DoH query can go through that
proxy.

## API

| Item | What |
| --- | --- |
| `Resolver::new(settings)` | a resolver with no DNS over HTTPS: an IP literal, the pins, the system, the cache; a failure says `DNS over HTTPS: not asked for this name` |
| `Resolver::with_doh(context, dial)`, `with_observer(observer)` | DNS over HTTPS through `dial` with the TLS of `context`; one decision `dns` for each lookup, with the outcome `literal`, `pin`, `system`, `cache`, `doh`, `old-cache` or `fail` |
| `Resolve for Resolver` (`resolve_from`, `resolve`, `pinned`, `note_dialed`, `label`) | the resolver as `pod-proxy`'s dial asks it: a `pod_core::Found` with its source (the place in the order: `literal` 0, `pin` 1, `system` 2, `cache` 3, `doh` 4, `old-cache` 5) and `more` (the cache and DNS over HTTPS hand the dial to the next source when no address connects); a failure is a `pod_core::Failure` at `Stage::Dns` |
| `Resolver::find(host, port, deadline, after)`, `Found`, `Source` (`ORDER`, `words`, `name`, `falls_back`, `core`, `of`), `SOURCE_WAIT` | the order with the source of the crate; the failure names each source that was tried |
| `Resolver::ips(host, deadline)` | the addresses with no port |
| `Resolver::settings`, `memory`, `asks_doh`, `way` | |
| `Resolver::set_pins(pins)`, `pins()` | the pin table in force: `Settings::pins` at the start, then each table that the caller gives; a lookup reads one table, and a replacement counts from the next lookup |
| `Memory` (`new`, `is_on`, `set_remembered`, `remembered`, `note_dialed`, `dialed`, `last_dialed`), `Remembered`, `split`, `FRESH` | the cache of the addresses that carried a session (fresh for 7 days, then old and last), and the record of the source of each address that the dialer connected to (32 at most) |
| `Way { settings, context, dial, kept }` | a way to the resolvers: the resolver's own (`Resolver::way`), or one that a caller makes for the proxy of one road |
| `Kept` (`new`, `len`, `is_empty`, `clear`), `IDLE` | the connection that waits for the next question, one for each host and port of one way out (5 s idle; a `Resolver` owns one, a caller gives one to its `Way` for the dial of that way; `None`: a connection for each question); a kept connection that failed gets one reconnect |
| `Way::lookup(host, deadline)`, `lookup_with(resolvers, ..)` | the addresses of DNS over HTTPS from the resolvers in turn |
| `Way::ask(resolver, host, deadline)` | one resolver straight at its address: the addresses and the form, or a `pod_proxy::Missed` with how the connection ended (a doctor's question) |
| `Way::srv(name, deadline)`, `srv_with`, `Srv` | the SRV records of a name, in either form |
| `Way::https(name, deadline)`, `https_with`, `https::Published` | the ECH list of a name's HTTPS record, in either form; `None` when the first resolver that answers gives none |
| `Way::query(name, kind, deadline)`, `json::{Answer, Record, parse}`, `Answer::of(type)` | the first answer of the JSON form, with its status (an NXDOMAIN is an answer) |
| `pod_tls::ech::Records for Resolver` and `for Way` (feature `ech`) | the reader of a name's ECH list that `pod_tls::ech::open` takes |
| `Doh` (`operator`, `ip`, `name`, `port`, `json`, `wire`, `http2`; `new`, the builders, `of_service`, `forms`, `target`, `hosts`), `Form` (`Json`, `Wire`; `words`, `accept`) | one resolver as data |
| `doh::{defaults, of_services, turns, order}`, `Names` (`First`, `Last`) | the resolvers of the services of `pod-services`; podssh's order; the hosts to ask in order |
| `route::{has_default_route, default_route_in, by_family, ROUTES_V4, ROUTES_V6}` | the family of this host's default route (Linux), and the resolvers of that family first |
| `Settings`, `parse_pins`, `Nxdomain` (`End`, `NextOperator`), `EACH`, `CONNECT` | the settings (below) |
| `wire` (`query`, `query_param`, `addrs`, `srv`, `read_name`, `code`, `A`, `AAAA`, `SRV`, `HTTPS`) | RFC 1035's messages as RFC 8484 carries them; by hand, fuzzed |
| `txt` (`query(id, name, kind, edns)`, `read(msg, id, name, kind)`, `question(msg)`, `answer(id, name, kind, records, edns)`, `Answer` (`id`, `truncated`, `rcode`, `edns`, `records`; `txt`, `cnames`), `Record` (`Txt`, `Cname`), `TXT`, `CNAME`, `OPT`, `PLAIN_SIZE`, `EDNS_SIZE`) | a query with an ID and an EDNS0 size, and the TXT and CNAME records, the TC bit, the full RCODE and the server's size of its answer, for a carrier of bytes over DNS (PodUtils/podnet#479); the server halves: the ID, the name and the type of a question, and an answer of TXT or CNAME records with an OPT record (T-493, for the dns carrier of `pod-link`); the transport is the caller's; by hand, fuzzed |
| `json` (`parse_answer`, `parse_srv`, `srv_of`) | the JSON form |
| `https` (`from_json`, `from_wire`, `from_rdata`, `check_list`, `ECH_KEY`, `MAX_LIST`) | the HTTPS record (RFC 9460) and its ECH list |
| `h2` (`config_h2`, `get_over`) | one GET over HTTP/2 |
| `NXDOMAIN`, `MAX_ANSWER` | the words of NXDOMAIN; the largest answer (64 KiB) |

## Features

| Feature | What |
| --- | --- |
| none (default) | everything above but the reader of ECH lists for `pod-tls` |
| `ech` | `pod_tls::ech::Records` on `Resolver` and `Way`; it turns on `pod-tls/ech` (and its pure provider) |

## Settings

| Field | What | podssh | podnet |
| --- | --- | --- | --- |
| `resolvers` | the resolvers of DNS over HTTPS, in order | `doh::defaults()` (the default): the role `v4` of each service of the kind `doh`, in podssh's turns (`1.1.1.1`, `8.8.8.8`, `1.0.0.1`, `8.8.4.4`, `9.9.9.9`, `149.112.112.112`) | `route::by_family(&doh::of_services(pod_services::of_kind("doh").filter(\|s\| s.text("json-path").is_some()), &["v4", "v6"]), route::has_default_route(false))`: Cloudflare's and Google's addresses of both families, the family of the host first |
| `names` | through a dial whose far end resolves names (`Dial::names_inside`), each resolver by its name first, then its address (`First`); or the addresses first and each name after (`Last`) | `Names::First` | `Names::Last` |
| `pins` | the pins of the user | `parse_pins` of `--relay-addr`, `PODSSH_RELAY_ADDR` and the settings file's `RelayAddr` | `parse_pins` of `--resolve`, and of `PODNET_RESOLVE` added to them |
| `each` | the bound of one resolver's answers | `EACH` (5 s) | `EACH` (podnet: 5 s for each request) |
| `connect` | the bound of the connection to a resolver asked for addresses | `CONNECT` (3 s) | 5 s (podnet's step) |
| `system` | the bound of the system resolver | `None` (the deadline) | `Some(5 s)` |
| `both` | A and AAAA both, or AAAA only when A gave none | `false` | `true` |
| `nxdomain` | NXDOMAIN ends the lookup (`End`), or asks the next operator (`NextOperator`) | `Nxdomain::End` | `Nxdomain::NextOperator` |
| `memory` | the cache of addresses on or off | `true`, with `memory().set_remembered(..)` of podssh's file | `false` (podnet keeps no cache) |
| `trust` | the trust of the resolvers' certificates | `Trust::Default` | `Trust::caller(Arc::new(context.client_config_with(&[], &[])?))` (the roots of its context: `--ca-file`, `SSL_CERT_FILE`, the file beside the binary, the system, the compiled roots) |
| `user_agent` | the `User-Agent` of a question over HTTP/1.1 | `podssh/VERSION` | `podnet/VERSION` |
| the dial of `with_doh` | the way to the resolvers | a `Dialer` with `ProxyChoice::Direct` over a `Resolver::new` (podssh asked straight at each address); the dial of a road for `Way::srv` and the ECH list | the `Dialer` of podnet's proxy over a `Resolver::new` (podnet's DoH went through its proxy) |
| `with_observer` | the counter of the decision `dns` | none | an `Observer` that calls `decisions::count(decision, outcome)` (the word of an IP literal is `literal`, podnet's `ip`) |

The variables stay with the consumers: podssh reads `PODSSH_RELAY_ADDR`,
podnet reads `PODNET_RESOLVE`, and each gives the pins.

## Parity

| Capability of podssh `crates/podssh-ws/src/{resolve.rs,resolve/}` at `a791553` | Here |
| --- | --- |
| The order: an IP literal, the pins, the system, the fresh cache, DNS over HTTPS, the old cache; a literal and a pin end the order | [x] `Source::ORDER`, `Resolver::find` |
| A source whose addresses may be stale hands the dial to the next (`Found::more`); DNS over HTTPS leaves the old cache `SOURCE_WAIT` | [x] `pod_core::Found::more`, `pod_core::SOURCE_WAIT` |
| The failure names each source that was tried, `DNS over HTTPS: ...` | [x] |
| The pins of `--relay-addr` (`parse_pins`, `set_pins`, `pinned`): any case, a final dot, a bracketed IPv6 address, a host more than once | [x] `parse_pins`, `Settings::pins` (a value of the resolver, no process state) |
| The cache (`set_remembered`, `remembered`, `split`, `FRESH`), the dialer's record (`note_dialed`, `dialed`, `last_dialed`, 32 kept) | [x] `Memory`, a field of the resolver; off with `Settings::memory` |
| The resolvers by address with the names of their certificates; the JSON form first, the wire form its fallback; Quad9 over HTTP/2 alone | [x] `Doh`, data of `pod-services` (`doh::defaults`) |
| A resolver straight at its address with the cut of a failure (`doh_ask`, `doh_ask_kept`) | [x] `Way::ask` |
| The lookup over the list (`doh_lookup`, `doh_lookup_with`): A then AAAA, the first that answers, NXDOMAIN ends it, `out of time` | [x] `Way::lookup`, `lookup_with` |
| The bounds: 5 s for a resolver, 3 s for its connection | [x] `Settings::each`, `Settings::connect` |
| A miss of the form goes to the other form; a busy resolver (429, 5xx) and the resolver's own status do not | [x] `miss::read` |
| Through a proxy, a resolver by its name first, then its address; a name once | [x] `doh::order`, `Names::First`, by `Dial::names_inside` |
| SRV records in the JSON and the wire form (`doh_srv`, `doh_srv_with`, `parse_srv`, `Srv`) | [x] `Way::srv`, `srv_with`, `json::parse_srv` |
| The HTTPS record and its ECH list in either form; the generic form; the lowest priority; the checks of each length (`https`) | [x] `https`, `Way::https`, `https_with` |
| The reader of ECH lists for `pod-tls` (`ech::Records` of podssh-ws) | [x] `Records for Resolver` and `for Way` (feature `ech`) |
| RFC 8484's messages by hand (`wire`) | [x] `wire` |
| HTTP/2 (`h2::config_h2`, `get_over`), its own end with GOAWAY and close_notify | [x] `h2` (the words of the consumer are an argument) |
| `NXDOMAIN`, `Form`, `parse_answer` | [x] |
| The glue of podssh's dial (`dial::Resolver`, `source_of`, `source_from`) | [x] `Resolve for Resolver`, `Source::core`, `Source::of` |
| The unit tests (28) and `tests/{doh_h2,doh_names,doh_wire,live_doh}.rs`, with their names; `each_source_of_the_order_comes_back_as_it_went` of `dial.rs` | [x] 29 unit, doh_h2 3, doh_names 2, doh_wire 8, live_doh 4 (ignored) |
| The fixtures of `tests/fixtures/doh/` | [x] the same files |
| The fuzz target `dns_wire` | [x] `fuzz/`, with its name and its checks |
| The plant `t642-length-past-end` | [x] `scripts/plants/dns-length-past-end`, caught by the same test |

| Capability of podnet `src/resolve.rs` at `380d9a4` | Here |
| --- | --- |
| The chain: an IP address, the pins, the system resolver with a bound of 5 s, DNS over HTTPS | [x] the order of the crate (with no cache: `Settings::memory`), `Settings::system` |
| The words of the system's failure (`system resolver: no address`, `... no answer in 5 s`, `... ERROR`) and `DNS over HTTPS: ...`; the addresses once each | [x] for both consumers |
| The pins of `--resolve` and `PODNET_RESOLVE` (`set_pins`, `parse_pins`, `PINS_VARIABLE`): one host more than once, any case, a final dot, a bracketed IPv6 address; the merge of the flag and the variable | [x] `parse_pins`, `Settings::pins`; the consumer reads the flag and the variable, and merges them |
| DoH by IP address at Cloudflare and Google, IPv4 and IPv6 (`DOH_SERVERS`) | [x] the data of `pod-services` (`cloudflare-doh`, `google-doh`), `Doh::of_service` with the roles `v4` and `v6` |
| The family of the host first (`has_default_route`, `by_family`) | [x] `route::has_default_route`, `route::by_family` (the routes read when the caller asks) |
| The servers by name only through a proxy, after the servers by address (`DOH_SERVERS_BY_NAME`, `doh_servers`) | [x] `Names::Last`, by `Dial::names_inside` |
| DoH JSON: `?name=..&type=..`, `Accept: application/dns-json`, through the proxy of the dial, the roots of the context | [x] `Form::Json`, the way of the resolver, `Settings::trust` |
| A and AAAA both, the addresses merged and once each | [x] `Settings::both` |
| NXDOMAIN at one operator asks the next operator; NXDOMAIN at each is an error that names them (issue 119) | [x] `Nxdomain::NextOperator` |
| The chain with no DoH for the host of a proxy and in a DoH query (`resolve_no_doh`, `NO_DOH`): no endless recursion (issue 259) | [x] `Resolver::new` (no way), `Dialer::with_proxy_resolver`; the dial of DoH has a resolver with no way |
| `doh_query(name, rtype)`: the first server that answers, NXDOMAIN is an answer; `DohAnswer`, `DohRecord` | [x] `Way::query`, `json::{Answer, Record}` |
| `answering_step(host)`: `pin`, `system`, `doh` or `none` | [x] `Resolver::find(..).map(\|f\| f.source.name())`, `none` on a failure |
| `resolve(host)`: the addresses with no port | [x] `Resolver::ips` |
| The counters (`decisions::count("dns", ...)`) | [x] `Resolver::with_observer` |
| The debug lines of each step | [x] `tracing` |
| The unit tests, with their names | [x] `settings::tests::pins`, `doh::tests::the_servers_by_name_need_a_proxy`, `route::tests::the_family_of_the_host_goes_first`, `lookup::tests::{nxdomain_at_one_operator_asks_the_next_operator, nxdomain_at_each_operator_is_an_error_that_names_them, a_server_that_fails_is_skipped}`, `resolver::tests::{a_name_in_the_no_doh_scope_does_not_ask_doh, an_ip_address_is_used_as_it_is}` |
| `the_servers_are_ip_addresses_of_two_operators` | [x] in `pod-services` (`tests/moved.rs`): it tests the data |
| The fuzz target `doh_json` | [x] `fuzz/`, with its name; podnet's `edge_discovery::parse_srv` goes with `pod-cloudflare`, and `json::srv_of` reads the record here |

New tests: `memory::tests::a_cache_that_is_off_keeps_nothing`,
`doh::tests::the_default_resolvers_take_turns_and_the_wire_form_alone_comes_last`,
`tests::a_resolver_is_the_data_of_its_service`,
`route::tests::a_default_route_is_read_from_the_text_of_the_routes`,
`resolver::tests::each_lookup_is_one_decision_of_the_observer`, and
`tests/doh_json.rs` (podnet's chain and its query against a loopback
stand-in of the JSON form: 3, with
`an_nxdomain_of_aaaa_after_the_addresses_of_a_keeps_them`, which failed on
the first merge). New plants: `dns-nxdomain-ends-the-lookup`,
`dns-cache-not-off`.

`cargo test -p pod-dns --all-features`: 42 unit, doh_h2 3, doh_json 3,
doh_names 2, doh_wire 8; live_doh 4 ignored.

`tests/doh_coredns.rs` (T-350): the wire form against CoreDNS on
127.0.0.1, over HTTP/1.1 and over HTTP/2: the addresses of a name, AAAA
when A gave none, the SRV records, NXDOMAIN at the first answer, and a
certificate of another root refused. CoreDNS is the program
`POD_DNS_TEST_COREDNS_BIN`, else `coredns` on the path; a host with
neither skips, unless `POD_DNS_TEST_COREDNS=required`.
`scripts/ci/coredns-tests.sh` runs it in `ci.yml` with the release of
CoreDNS 1.15.0, checked by its SHA-256. `live_doh` runs on Thursdays and
by hand (`live.yml`).

The reader of TXT and CNAME answers (`txt`, T-305): `txt::tests` (the
query with its OPT record, the strings and the CNAME targets, the full
RCODE, the cut that needs the TC bit, the foreign answers, and the
question and the answer of a server: `a_server_reads_a_question_and_builds_an_answer`) and
`tests/txt_udp.rs` (a stand-in authoritative server on the loopback cuts
an answer at 512 bytes with no EDNS0 and sends it whole with a size of
1232; the same answer is whole over TCP). The fuzz target `dns_wire`
reads each input as a TXT and a CNAME answer, with the seeds `txt-edns`
and `txt-cut`. Plants: `dns-txt-cut-taken`, `dns-txt-opt-not-sent`.

## Changes for podssh

* The failure of the system resolver says `system resolver: ...` (podnet's
  words): `system resolver: no address`, `system resolver: no answer in N
  s`, `system resolver: ERROR` (podssh: `no addresses`, `timed out`,
  `ERROR`). The addresses of the system come once each.
* The pins and the cache are of the resolver, not of the process: podssh
  makes one `Resolver` with its pins, and gives its file's addresses to
  `memory().set_remembered`.
* DNS over HTTPS goes through the dial that podssh gives (a direct
  `Dialer`): its socket options and its words of a failed connection
  (`could not connect to ...`) are `pod-proxy`'s.
* A resolver is asked by its name through a dial whose far end resolves
  names (`Dial::names_inside`: an HTTP proxy, `socks5h://`), not by the
  proxy of its name: a `socks5://` proxy asks by the address alone, and a
  name that `NO_PROXY` names is asked by name directly (its system
  resolver answers, or the address follows).
* A JSON answer that is not an object says `not a DNS JSON answer`
  (podssh: `DNS status missing`); both go to the other form.
* `Doh` holds owned values (an `IpAddr`, `String`s), an operator and a
  port; `h2::get_over` takes the words of the consumer; the `Host` of a
  resolver at another port than 443 has its port; an IPv6 resolver is in
  brackets in `Host` and in the URI of HTTP/2.

## Changes for podnet

* A resolver whose JSON answer has no `Status` goes to its wire form
  (podnet took 0). `Way::query` gives `status: None` for it.
* After a failure of the way (a time limit, a reset) on the question A of
  a resolver, its question AAAA is not asked: the next resolver is (podnet
  asked both, 5 s each).
* The wire form is the fallback of each resolver that serves it (podnet:
  the JSON form alone); a consumer that keeps the JSON form alone gives
  resolvers with no wire path.
* The failure of DNS over HTTPS names the reason of each resolver that
  was asked (podnet: the last one), or the operators that said NXDOMAIN.
* The servers by name are each name of the services once
  (`cloudflare-dns.com`, `dns.google`, `dns.google.com`), where podnet had
  two.
* A pin's host is letters, digits, `.`, `-` and `_` (podnet took any
  text).
* The question over HTTP/1.1 offers no ALPN (podnet's `fetch` offered
  `http/1.1`).
* The word of an IP literal for the observer is `literal` (podnet: `ip`).

## Adoption map

| Consumer | Old path | New path |
| --- | --- | --- |
| podssh | `podssh_ws::dial::Resolver` (the glue of `pod-proxy`) | `pod_dns::Resolver`, made once (`Settings` with podssh's pins and user agent, `with_doh(context, direct_dialer)`), and given to each `Dialer` |
| podssh | `podssh_ws::resolve::{resolve, resolve_from}(host, port, deadline)` (`doctor/nat.rs`, `node_exit.rs`, `ssh/transport/after.rs`, `podssh-iroh` `endpoint.rs` and `resolve.rs`, `podssh-relay` `quic_road/dial.rs`, `quick/probe.rs`, `quick/quic.rs`, `udp/flow.rs`, `podssh-warp` `road.rs`) | `resolver.find(host, port, deadline, None).await.map(\|f\| f.addrs)` (`Result<_, String>`), or `Resolve::resolve` (`Failure`) |
| podssh | `resolve::{Found, Source, ORDER, SOURCE_WAIT}` | `pod_dns::{Found, Source, SOURCE_WAIT}`, `Source::ORDER` |
| podssh | `resolve::{parse_pins, set_pins}` (`pins.rs`, `settings.rs`; the tests `live_harness`, `relay_serve.rs`, `serve_list`, `serve_net`, `reverse_drops_live.rs`, `relay_serve_h2.rs`, `relay_serve_quic.rs`, podssh-ws `h2_road.rs`, `meek.rs`, `via.rs`, `vless_support`, `webtunnel.rs`, `wstunnel.rs`) | `pod_dns::parse_pins`; `Settings { pins, .. }` of the resolver of the dial (a test gives its dialer a resolver with its pins) |
| podssh | `resolve::pinned(host)` (`doctor/names.rs`, `podssh-relay` `addresses.rs`) | `resolver.settings().pinned(host)`, or `Resolve::pinned` |
| podssh | `resolve::{set_remembered, remembered, dialed, last_dialed, Remembered, split, FRESH}` (`doctor/names.rs`, `man/facts.rs`, `podssh-relay` `addresses.rs`, `health.rs`) | `resolver.memory().{set_remembered, remembered, dialed, last_dialed}`, `pod_dns::{Remembered, split, FRESH}` |
| podssh | `Source::Pin \| Source::Literal` (`addresses.rs`) | the same names: `pod_dns::Source` |
| podssh | `resolve::DOH_RESOLVERS` (`doctor/address.rs`, `doctor/names.rs`, `man/facts.rs`, `tests/doctor.rs`) | `resolver.settings().resolvers` (or `pod_dns::doh::defaults()`); `r.ip` is an `IpAddr` (`.to_string()` for a `&str`) |
| podssh | `resolve::{doh_ask, doh_ask_kept}(resolver, host, deadline)` (`doctor/names.rs`) | `way.ask(&resolver, host, deadline)` (`Missed`; `.why` for the words) |
| podssh | `resolve::{doh_lookup, doh_lookup_with}` | `way.lookup(host, deadline)`, `way.lookup_with(&resolvers, ..)` |
| podssh | `resolve::doh_srv(name, proxy, deadline)` (`podssh-relay` `quick/edge.rs`), `doh_srv_with`, `Srv` | `Way { settings, context, dial: &dialer_of(proxy), kept: None }.srv(name, deadline)`, `srv_with`; `pod_dns::Srv` |
| podssh | `resolve::https::doh_https(name, proxy, deadline)`, `ech::Records(proxy)` (`ech.rs`, `client/get.rs`) | `Way { .., dial: &dialer_of(proxy), kept: None }` as the `records` of `pod_tls::ech::Route`, or `Arc<Resolver>` for `Client::with_records` (feature `ech`) |
| podssh | `resolve::https::{from_json, from_wire, from_rdata, check_list, Published, ECH_KEY, MAX_LIST}` | `pod_dns::https::*` (`Published` of the crate; `.into()` gives `pod_tls::ech::Published`) |
| podssh | `resolve::wire` (`podssh-warp` `dns.rs`), `resolve::NXDOMAIN` (`podssh-warp` `dns.rs`, `road.rs`, `doctor/names.rs`) | `pod_dns::wire`, `pod_dns::NXDOMAIN` |
| podssh | `resolve::{Doh, Form, parse_answer, parse_srv, h2}` | `pod_dns::{Doh, Form}`, `pod_dns::json::{parse_answer, parse_srv}`, `pod_dns::h2` (`get_over(.., &words)`) |
| podssh | the fuzz target `dns_wire` | the target of the same name in `fuzz/` of podcrates; podssh removes its own |
| podssh | the plant `t642-length-past-end` | it leaves podssh's list with the file |
| podssh | `crates/podssh-ws/tests/{doh_h2,doh_names,doh_wire,live_doh}.rs` and `tests/fixtures/doh/` | they moved here; podssh removes them. Its `doh-live.yml` (with `scripts/box/proxy.py --take names`) moves to a live workflow of podcrates, which runs `cargo test -p pod-dns --test live_doh -- --ignored` with `POD_DNS_NAMES_PROXY` in place of `PODSSH_DOH_NAMES_PROXY` |
| podssh | the tests that stay: `podssh-cli` `tests/doctor.rs` (the address of the first resolver), the tests of the roads that pin a name to the loopback | the pins of the resolver of their dialer |
| podnet | `resolve::resolve(host)` (`doctor.rs`, `main.rs`, `warp/nc.rs`), and the glue `egress::Resolver { doh: true }` | `resolver.ips(host, deadline)`; the `Arc<pod_dns::Resolver>` is the resolver of the `Dialer` |
| podnet | `resolve::resolve_no_doh(host)`, the glue `egress::Resolver { doh: false }`, the task-local `NO_DOH` | a `pod_dns::Resolver::new(settings)` (no way): `Dialer::with_proxy_resolver`, and the resolver of the dial of `with_doh` |
| podnet | `resolve::{set_pins, parse_pins, PINS_VARIABLE}` (`main.rs`) | `pod_dns::parse_pins` of `--resolve` and of `PODNET_RESOLVE` (podnet keeps the variable and its exit code 78), joined in `Settings::pins`; errors are `String` |
| podnet | `resolve::has_default_route(ipv6)` (`doctor.rs`, `warp/mod.rs`) | `pod_dns::route::has_default_route(ipv6)` |
| podnet | `resolve::answering_step(host)` (`tunnel.rs`) | `resolver.find(host, 0, deadline, None).await.map_or("none", \|f\| f.source.name())` |
| podnet | `resolve::doh_query(name, rtype)` (`edge_discovery.rs`) | `resolver.way().expect("DoH").query(name, rtype, deadline)` (`json::Answer`: `status: Option<u64>`, `records`) |
| podnet | `resolve::{DohAnswer, DohRecord}` (`edge_discovery.rs`, `fuzzing.rs`) | `pod_dns::json::{Answer, Record}` (`kind` for `rtype`), `json::parse` for `serde_json::from_slice` |
| podnet | `resolve::{DOH_SERVERS, DOH_SERVERS_BY_NAME, doh_servers, DohServer}` | `Settings::resolvers` and `Names::Last` (the settings table) |
| podnet | `resolve::STEP_TIMEOUT` | `Settings::{system, each, connect}` |
| podnet | `decisions::count("dns", ..)` | `Resolver::with_observer` |
| podnet | `fuzzing.rs` `doh_json` | the target `doh_json` in `fuzz/` of podcrates; podnet removes its own |
| podnet | the tests that stay: `tests/egress.rs` `socks5_resolves_the_target_with_the_pins`, `tests/connect.rs` `connect_uses_resolve_pins`, `tests/cli.rs` `invalid_values_are_refused` (`PODNET_RESOLVE` and `--resolve`), `edge_discovery.rs` `decodes_a_cloudflare_doh_answer` (with `json::parse`, until `pod-cloudflare`) | they run the binary or the edge; the pins come from `parse_pins` |

## Sources

`SOURCES` lists the imported files: podssh `crates/podssh-ws/src/resolve.rs`
(now `lib.rs`, `doh.rs`, `settings.rs`, `json.rs`, `miss.rs`, `lookup.rs`,
`question.rs`, `way.rs`, `resolver.rs`), `resolve/direct.rs` (now
`ask.rs`, `get.rs`), `resolve/h2.rs`, `resolve/https.rs`,
`resolve/https_tests.rs`, `resolve/memory.rs`, `resolve/order.rs`,
`resolve/wire.rs`, `resolve/tests.rs`, `resolve/tests/cuts.rs`, and the
tests `tests/{doh_h2,doh_names,doh_wire,live_doh}.rs` with
`tests/fixtures/doh/` at `a791553`; podnet `src/resolve.rs` (now
`settings.rs`, `doh.rs`, `route.rs`, `lookup.rs`, `resolver.rs`,
`json.rs`, `way.rs`, `ask.rs`) at `380d9a4`.
