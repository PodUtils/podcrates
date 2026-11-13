# pod-warp

Cloudflare WARP, spoken directly: anonymous registration, the P-256
identity of the device and its file, MASQUE CONNECT-IP over HTTP/2 on TCP
443 through the caller's dial or over HTTP/3 on the caller's UDP, with the
key of the front door pinned, the WireGuard tunnel of WARP and a device
carried inside a session, and a userspace stack over the session, given as a network of the caller: a
`pod_core::Dial` (TCP streams, names resolved inside the tunnel) and
`pod_core::Datagrams` (UDP).

The base is podnet's `src/warp/` (`PodUtils/podnet:src/warp`); podssh's
`crates/podssh-warp` retires onto it (`docs/design.md`, section 4). This
is the first part of T-028 (partial): the registration, the identity, the
doors, the session over HTTP/2, the network and the rules; and T-335: the
settings of podnet's stack of `src/warp/ip.rs` (the congestion controller,
the TCP burst, the counters of the link, the budget of a relay) and the
check of the data path; and T-336: the relay leg of the caller, the race
of the direct path and the relay, the leg file and the budget of a relay
session; T-338: the local SOCKS5 and HTTP proxy with its rules, its
names, its UDP and its DNS forwarder, the resolvers inside the tunnel (on
UDP or over TLS), the local forwards and the pipe of `warp nc`; and T-337:
MASQUE over HTTP/3, the WireGuard tunnel and the carried device. The rest
is T-339 (partial: the adoption trials T-412, which passes in podnet, and T-413).
T-406: the L4 door and its proxy. T-339 adds `Live` (a session replaced
when it closes), the exit of a session, the socket of a running proxy, and
the TUN with its routes and its resolver (Linux).

The crate reads no environment variable and no file that the caller does
not name, prints nothing and keeps no global state. Each failure is a
`pod_core::Failure` (stage `Warp`, or the stage of the dial); a line for
the user is a `pod_core::Event` of the caller's `Report`; a decision goes
to its `Observer`. The TLS provider and the way out are the caller's.

**Build setting.** The stack is `pod-netstack`'s: each binary sets
`SMOLTCP_IFACE_MAX_ADDR_COUNT = "8"` in the `[env]` table of its
`.cargo/config.toml`.

## API

| Item | Contents |
| --- | --- |
| `Settings` | `api: Api`, `way: Way`, `relay: Option<Relay>`, `wireguard: Option<wg::Peer>`, `carry: Option<wg::Peer>`, `door: Door`, `routes: Routes`, `hints: DeviceHints`, `report: Report`, `net: NetSettings`; `Settings::new(api, way)` (no relay). |
| `Warp` | `register(settings)`, `register_or_load(settings, &IdentityFile)`, `from_parts`, `from_stored`, `save(path)`; `device`, `identity`, `settings`; `refresh`, `account`, `set_license`, `rotate_key`, `delete`; `open_session() -> MasqueSession` (with a relay: the race of the two legs), `connect() -> WarpNet` (with the check of the data path; a session that carries nothing marks its door bad; with `carry`, the stack of the carried device inside the session, `CARRY_MTU` 1200, IPv4 only, the budget of the outer session); `won_door`, `with_won_door`, `mark_door_bad`, `endpoint_in_use`; `relay_first`, `with_relay_first`, `leg`, `take_leg_change`. |
| `WarpNet` | `Dial` (`label` `WARP`, `dial`, `dial_addrs`, `names_inside` true, `datagrams`); `start(session, v4, v6, &NetSettings, report)`, `addresses`, `is_closed`, `close_reason`, `close(why)` (podnet's `close_session`), `stack`, `with_resolvers`, `link_stats`, `renew_due` (the budget of the session, else `NetSettings::renew`), `check_data_path(&DataPathCheck) -> Duration`, `checked`. When the session closes (the edge ends it, a pump ends, the reactor of the stack stops, also by a panic, or `close`), each open stream and UDP socket fails at once with `ConnectionReset` and the first cause, and a new dial and a new UDP socket fail with it. A UDP datagram to a family that the device lacks is `InvalidInput` ("the tunnel has no IPv6 address for ..."), and no packet leaves. |
| `net` | `NetSettings { mtu, tcp_buffer, congestion_control, tcp_burst, tcp_timeout, tcp_keepalive, tcp_linger, nodelay, link_stats, check, renew }` (default: 1280, `TCP_BUFFER` 512 KiB, none, no limit, `TCP_TIMEOUT` 60 s, `TCP_KEEPALIVE` 20 s, `TCP_LINGER` 30 s, no Nagle, the counters of the session, `DataPathCheck::default()`, no relay), `stack_config(v4, v6)`; `parse_cc` (`none`, `reno`, `cubic`), `parse_tcp_burst` (1 to `TCP_BURST_MAX`, 430, or `none`), `tcp_burst_max(buffer, mtu)`; `Renew` (a function of the caller); `CongestionControl`, `DeviceStats`, `DeviceStatsSnapshot`. A bad value is `Config` (exit 78). |
| `check` | `DataPathCheck { servers, tls, name, limit, resend }` (default: the resolver of the tunnel of the first family of the device on UDP, no TLS client, `cloudflare.com`, 5 s, 1.2 s), `of(&Resolvers)` (the resolvers of a proxy, on UDP or over TLS), `servers_for`, `CHECK_LIMIT`, `CHECK_RESEND`. A session with no answer is `Unavailable` (stage `Warp`); a server over TLS with no TLS client of the caller is no answer. |
| `resolver` | `Resolver` (`Udp(addr)`, `Tls { addr, name }`; `parse`: `IP[:PORT]`, `udp://`, `tls://IP[:PORT][#NAME]`; `addr`; `Display`), `parse_resolvers(specs)` (comma lists), `Resolvers { list, tls, limit, resend, connect_timeout }` (`new`, `in_use`: the list, else 1.1.1.1:53 on UDP; `exchange(net, query)`: each in turn, the first answer with the id; `exchange_with`; `query(net, name, kind)`), `question(name, kind)` (a random id), `udp_exchange` (resent each 1.2 s), `dot_exchange` (RFC 7858), `DNS_TIMEOUT`, `DNS_RESEND`, `CONNECT_TIMEOUT`, `DNS_PORT`, `DOT_PORT`, `DNS_HEADER`. Each takes the session as a `Dial`. |
| `proxy` | `Proxy { tunnel, direct, rules, names, credentials, max_connections, stats, report, connect_timeout, request_limit }` (`new(tunnel, direct)`: the rules route each destination through the tunnel, no credentials, no limit), `run_tcp(listener)`, `run_unix(listener)`, `client(stream, control) -> bool` (false at the limit), `route(net, target)`, `serve_dns(socket)` (the DNS forwarder, at most `MAX_DNS_TASKS` in flight), `admit`, `health() -> Health`; `Tunnel` (`session()`: the session of now; `Fixed(dial)`, `WarpNet`), `Stats`, `Health`, `Admitted`; `serve_client(stream, dial, udp, &Opening)` (one client with the dial of the caller: SOCKS5 CONNECT, `UDP ASSOCIATE`, 0x83, HTTP `CONNECT`, an `http://` URL; a SOCKS4 client, a TLS client and another first byte are refused at once), `Opening { credentials, limit, report, control }`, `Udp { net, names }`, `relay_bind_ip`, `DnsGate`; `names` (`Names { resolvers, pins }`: `resolve_all`, `resolve`, `connect`; `Pins`, `order`); `CONNECT_TIMEOUT`, `REQUEST_LIMIT` (30 s), `HEALTH_EVERY` (600 s), `MAX_DNS_TASKS` (256). The decisions `warp.dispatch` and `warp.rule`; the events `warp.proxy.open` and `warp.proxy.limit`. |
| `forward` | `Listen` (`Tcp`, `Unix`; `parse`, `Display`), `Rule { listen, target }` (`parse("LISTEN=HOST:PORT")`; a loopback target is `Usage`), `Forward { rule, tunnel, names, connect_timeout, report }` (`new(rule, tunnel, names, max)`, `run_tcp`, `run_unix`, `connection(stream) -> bool`); the event `warp.forward.open`. |
| `nc` | `target(host, port)`, `PipeEnd { shutdown, idle }` (default: no FIN at the end of the input, `TCP_IDLE` 5 s), `pipe_with(io, input, output, end)`, `udp_pipe_with(out, back, input, output, idle)`, `udp_via_net(net, &names, host, port)`, `udp_via_stream(stream, host, port)` (an association in the stream of a running proxy), `UdpEnds`. |
| `api` | `Api::new(pod_proxy::Client)`, `with_base(url)` (podnet's `PODNET_WARP_API`, podssh's `PODSSH_WARP_API`), `with_bases`, `bases`, `request`, `send` (each base in turn; a `POST` goes on only after no answer); `ApiError`, `api_status`, `bases(value)`, `API_TIMEOUT`. |
| `register` | `create_device`, `create_device_with_key`, `enrol_masque`, `get_device`, `get_account` (license hidden), `set_license`, `delete_device`, `registration_error` (error 1015). |
| `device` | `Device` (`ipv4`, `ipv6`, `local_address`, `masque_endpoint`, `masque_endpoints`, `peer_public_key_pem`, `reserved`; `Debug` hides the token and the license), `Account`, `DeviceConfig`, `Peer`, `Endpoint`, `Interface`, `Addresses`, `Services`, `Policy`, `redact_account`. |
| `hints` | `DeviceHints { name, model, locale }`, `check`. |
| `identity` | `Identity` (`generate`, `of`, `key`, `spki_b64`, `spki_der`, `certified(now)`, `to_pem`, `from_pem`), `spki_from_pem`, `spki_from_cert`, `warp_plus_label`, `license_class`. |
| `key`, `der` | `DeviceKey` (P-256 in pure Rust: `generate`, `from_scalar`, `from_pkcs8`, `pkcs8`, `point`, `spki`, `sign`, `certificate`, `certified`, `signer`); the DER of a certificate, an SPKI, PEM and PKCS #8 (`der::pkcs8`). |
| `store` | `IdentityFile { path, foreign, lock_wait }`, `Stored`, `register_or_load`, `load`, `save`, `LOCK_WAIT`. |
| `files` | `Foreign` (`Refuse`, `Allow`), `check_private`, `write_private`, `lock`, `lock_path`, `Held`. |
| `door` | `Door` (`Record`, `Pinned(Target)`), `Routes`, `endpoint_order`, `doors`, `door_from`, `door_text`, `DOOR_TTL`. |
| `leg` | `Relay { dial, name, meter, delay, delay_known }` (`Relay::new(dial, name)`: no meter, `RELAY_DELAY` 3 s, `RELAY_DELAY_KNOWN` 0.5 s; `delay_for`), `Leg` (`Direct`, `Relay`; `as_str`), `relay_first_from(text, now, relay)`, `leg_text(leg, now, relay)`, `LEG_TTL` (6 h). The direct path starts first; the relay starts after its delay, or at once when the direct path failed; the first session that opens wins and the other try is dropped; a failure of both names both causes. A relay reaches both families (`Routes::default()`). The decision `warp.leg`: `direct`, `relay`, `relay-first`, `no-relay`. |
| `tls` | `client_config(provider, identity, pinned_spki, alpn)`, `handshake_failure`, `is_pin_mismatch`, `PIN_MISMATCH`, `ALPN_H2`. |
| `session` | `Way { dial, udp, h3, meter, provider, sni, authority, timeout }`, `Meter` (a function of the caller: a stream in, the metered stream and its `Renew` out), `MasqueSession` (`connect`: HTTP/3 with `Way::h3`; `connect_wg(peer, way)`, `connect_wg_inside(peer, outer)`, `is_h3`, `is_wg`, `split`, `relay_budget`), `MasqueTx` (`send_packet`, `send_packets`, `waits`, `keepalive`, `close`), `MasqueRx` (`recv_packet`, `take_assigned`, `progress`, `is_closed`), `Keepalive` (`h2`, `h3`, `wg`, `run`), `h2_builder`; the windows, the budget of DATA frames, `SEND_RESERVE`, `SEND_STALL`, `CONNECT_TIMEOUT`, `MAX_MISSED_PONGS`. |
| `h3` | MASQUE over HTTP/3 on `pod-quic` over `Datagrams`: `connect`, `open`, `request`, `H3Tx`, `H3Rx`, `Incoming`, `closed`, `quic_settings`, `ALPN`, `SETTINGS`, `IDLE_TIMEOUT` 30 s, `KEEP_ALIVE` 10 s, `INITIAL_MTU` 1242, `DATAGRAM_BUFFER`; `h3::wire`: `Frames`, `frame`, `connect_ip_headers`, `response_status` (QPACK with the static table only). A packet larger than a datagram goes as a DATA capsule on the stream. |
| `l4` | The L4 door (podnet issue 140): `L4` (`new(door, udp, tls)`, `with_sni`, `with_form`, `with_wait`, `door`, `connection`, `open(target)`; a `Dial` with `names_inside` true and no UDP), `Warp::l4()`, `Form` (`Plain`, `Extended`, `Header`; `parse`), `connect_headers_form`, `authority`, `OPEN_WAIT` 15 s, `PIPE_BUFFER`; `h3::DataFrames` (DATA frames of any size), `Frames::into_rest`; `service::l4_sni`. `Proxy::over_l4(door, direct)` and `Proxy::l4`: the rules with `decide_static`, the decision `warp.rule` `l4`, no UDP. |
| `wg` | The WireGuard tunnel on `pod-wg`: `Peer { keys, endpoint, v4, v6 }`, `connect(peer, datagrams)`, `connect_inside(peer, &WarpNet)`, `WgTx`, `WgRx`, `WgKeepalive`, `HANDSHAKE_TRIES`. Only a datagram from the door is read. |
| `udp` | `UdpWay` (`label`, `datagrams(door)`), `SystemUdp` (a socket of the system of the family of the door); `WarpNet` is one. A way with no UDP refuses HTTP/3 and WireGuard (`Usage`). |
| `capsule` | The capsules of RFC 9297 and the types of RFC 9484: `put_varint`, `get_varint`, `put_capsule`, `CapsuleDecoder`, `parse_address_assign`, `MAX_CAPSULE_LEN`. |
| `dns` | `lookup(net, resolvers, name, limit)` (DNS over TCP inside the tunnel), `RESOLVERS`. |
| `rules` | `Action`, `Rule` (`parse(spec, place)`, `matcher_text`), `Table` (`build(specs, file, default, lint, allow)`, `for_name`, `for_ip`, `for_addresses`, `decide(&lookup, host)`, `decide_static`), `Host` (`of(target)`, `Display`), `Lookup` (the names of a proxy, or the DNS over TCP of a `WarpNet`), `Lint`, `LintItem`, `NoLint`, `Rules` (`set`, `current`, `counters`), `Counters`, `Refused`, `load_file`, `matcher` (`RuleMatch`, `Net`, `load_nets`, `domain_matches`). |
| `live` | `Live<T: Closable>` (`start(first, connect, &LiveSettings)`, `start_with`, `start_aged`, `probed`, `get` (waits `WAIT_FOR_NEW` 20 s for a new session, else `Unavailable`), `get_within`, `reconnects`, `data_path` (`ok`, `tcp-dead`), `report`), `LiveSettings { base, max, max_age, probe, report }` (default: `BACKOFF_BASE` 1 s, `BACKOFF_MAX` 30 s, no age, `DEFAULT_PROBE`), `Closable` (`WarpNet` is one), `Probe`, `DEFAULT_PROBE` (TCP to 1.1.1.1:443 each 30 s, 5 s, 3 misses), `parse_probe` (`IP:PORT` or `off`; else `Usage`), `Connect`, `Report { data_path, last_probe }`, `publish(live, sink)`, `RENEW_DRAIN` (90 s); `Warp::connect_live(&settings)`; a `Live` of a `Dial` is a `proxy::Tunnel`. A spent relay budget or a dead TCP path gives a new session for the new flows with no gap; the old one closes after the drain. The events `warp.live.closed`, `warp.live.open`, `warp.live.failed`, `warp.live.renew`, `warp.live.probe`; the decision `warp.live` (`closed`, `max-age`, `relay-budget`, `tcp-probe`). |
| `exit` | `Exit { colo, loc, ip, warp, rtt, ipv6, ipv6_error }` (`to_json`), `parse(body, rtt)`, `trace(net, tls)`, `trace_at`, `egress_v6(net, tls)`, `whoami_query`, `first_txt`, `trace_address`, `trace_address_v6`, `trace_name`, `TRACE_TIMEOUT`: the trace of 1.1.1.1 over HTTPS through the session, with the caller's TLS client. |
| `find` (Unix) | `Pointer { socket, state_dir }`, `pointer_dir_of(runtime, temp, name)`, `write_pointer`, `read_pointer_at` (mode 0600 and this user only), `remove_pointer`, `candidates_of(given, variable, run_dir, pointer)`, `answers`, `running(tried, pointer)` (none answers: `Unavailable`), `POINTER_FILE`, `SOCKET_FILE`. |
| `tun` (Linux) | `Tun` (`create(name, address, mtu)`, `from_fd`, `add_ipv6`, `read`, `write`), `run(tun, session, &TunSettings)`, `TunSettings { probe, max_age, rp_filter, keepalive, mtu, report }`, `KEEPALIVE`; `tun::probe::TunProbe` (a SYN from `PROBE_PORT` 61999, its answer taken out of the TUN), `tun::rp_filter::RpFilter` (`system(keep)`, `apply`), `effective`, `SYSTEM_ROOT`; the event `warp.tun.rp-filter`. |
| `route` | `parse_route_table`, `parse_entries`, `parse_entries6`, `plan_exempt`, `plan_exempt6` (`Exempt`, `Exempt6`), `Gateway`, `Entry`, `Entry6`, `nameservers(&[Resolver])`, `nameservers_of_service`, `resolv_text(marker, servers)`, `mask`, `install_all`; on Linux `Routes` (`setup(tun, required, optional, ipv6, &Tables)`, `set_dns`, `exempted`, `up`, `down`; the drop puts each change back), `Tables` (`system()`), `set_dns`; `route::resolv` (Unix): `Paths { resolv, record, lock, marker }` (`in_dir`), `set_file`, `reconcile`, `undo`, `replace_file`, `Dns`, `MARKER`, `RESOLV`. |
| `service` | The facts of `cloudflare-warp` of `pod-services`: `api_base`, `api_second`, `api_version`, `client_version`, `user_agent`, `masque_sni`, `l4_sni`, `masque_port`, `masque_doors`, `connect_uri`, `terms_url`; of `cloudflare-dns`: `tunnel_resolver` (`udp-1`), `check_name`. |

## Settings

Each variable of a consumer is a field (`docs/design.md`, section 1, rule
6):

| podnet | podssh | `pod-warp` |
| --- | --- | --- |
| `PODNET_WARP_API` | `PODSSH_WARP_API` | `Api::with_base` |
| `PODNET_WARP_DEVICE_NAME`, `_MODEL`, `_LOCALE` | | `Settings::hints` |
| `PODNET_FOREIGN_IDENTITY=allow` (with `--identity`) | | `IdentityFile::foreign = Foreign::Allow` |
| `--masque-endpoint` | | `Settings::door = Door::Pinned(target)` |
| `--masque-sni` | | `Way::sni` |
| `has_default_route` | | `Settings::routes` |
| `HTTPS_PROXY`, `--transport` | `HTTPS_PROXY` | `Way::dial`, and the dial of the `pod_proxy::Client` of `Api` |
| the WebSocket relay (`PODNET_RELAY`, `ws_relay::Relay`) | | `Settings::relay = Some(Relay::new(dial, url))`, the dial of the relay of `pod-relay` |
| `--no-relay` | | `Settings::relay = None` |
| HTTP/3 (`MasqueOptions::h3`; no relay, no proxy) | | `Way::h3 = true` with `Way::udp = Some(Arc::new(SystemUdp))`; `Warp::open_session` skips `Settings::relay` |
| WireGuard (`with_wireguard`, the peer of `warp export`) | | `Settings::wireguard = Some(wg::Peer { .. })` with `Way::udp` |
| the carried device (`with_carry`) | | `Settings::carry = Some(wg::Peer { .. })` |
| `warp-leg.json` (`read_leg`, `write_leg`) | | `leg::relay_first_from` into `Warp::with_relay_first`; `Warp::take_leg_change` and `leg::leg_text` for the caller's file |
| `--allow-shadowed-rules` | | the `allow` of `Table::build` |
| the state directory (`warp.json`, `warp-door.json`) | the device's private file | `IdentityFile::path`; `door_from` and `door_text` for the caller's door file |
| `--cc`, `PODNET_WARP_CC` (`ip::set_cc`, `check_cc`) | | `NetSettings::congestion_control`, from `net::parse_cc` |
| `PODNET_WARP_TCP_BURST` (`ip::check_tcp_burst`) | | `NetSettings::tcp_burst`, from `net::parse_tcp_burst` |
| the counters of the links of a process (`ip::link_stats`) | | `NetSettings::link_stats` (one `Arc<DeviceStats>` for each session), `WarpNet::link_stats` |
| `PODNET_WARP_NO_CHECK` | | `NetSettings::check = None` |
| `--resolver`, `PODNET_WARP_RESOLVERS` (`set_resolvers`) | | `resolver::parse_resolvers` into `Proxy::names.resolvers.list`, and `DataPathCheck::of(&resolvers)` for the check; the TLS client of DNS over TLS (`trust::context()`) is `Resolvers::tls` |
| `--resolve`, `PODNET_RESOLVE` (`resolve::pinned`) | | `Names::pins` |
| `--max-connections` (`set_max_connections`) | | `Proxy::max_connections`; `Forward::new(.., max)` |
| `--proxy-user`, `--proxy-password-file` (`set_credentials`) | | `Proxy::credentials` |
| `warp proxy --dns` (`serve_dns`) | | `Proxy::serve_dns(socket)` |
| `warp forward -L` | | `forward::Rule::parse`, `Forward` |
| `warp proxy --l4`, `--l4-sni` (`WarpClient::l4`, `proxy::run_l4`) | | `Proxy::over_l4(Arc::new(warp.l4()?.with_sni(name)), direct)` |
| `PODNET_WARP_L4_FORM` | | `L4::with_form(Form::parse(value)?)` (another value is `Config`) |
| the bind of a listener (`proxy::bind`, `bind_all`, the private Unix socket) | | the caller's; `Proxy::run_tcp`, `run_unix` take the listener |
| `warp nc` (`via_running`, `via_relay`, `pipe`, `ssh_config`) | | the caller's; `nc::pipe_with` and the UDP pipes take the stream |
| the budget of a relay session (`ws_relay::meter`, `StackConfig::renew`) | | `Relay::meter` (or `Way::meter`); `NetSettings::renew` for a session with no meter |
| `--probe` (`live::set_probe`, `parse_probe`) | | `LiveSettings::probe`, `TunSettings::probe`, from `live::parse_probe` |
| `--session-max-age` (`live::set_max_age`) | | `LiveSettings::max_age`, `TunSettings::max_age` |
| `--keep-rp-filter` (`rp_filter::set_keep`) | | `TunSettings::rp_filter = Some(RpFilter::system(keep))` |
| `PODNET_WARP_SOCKET`, `--socket`, the run directory, `XDG_RUNTIME_DIR` | | `find::candidates_of(given, variable, run_dir, pointer)`, `find::pointer_dir_of(runtime, temp, "podnet")` |
| `/etc/resolv.conf` and the state directory of `warp tun --default-route` | | `route::resolv::Paths::in_dir(RESOLV, dir)` with podnet's `marker` |
| `CARRY_MTU` (the carried device) | | `CARRY_MTU`; the stack of the carry takes it in place of `NetSettings::mtu` |

## The tests

Loopback only: the stand-in API (`pod_testkit::stand::warp`), the HTTP/2
and capsule part of the stand-in edge (`pod_testkit::masque`), and its TLS
and far end in `tests/common/edge.rs` (call C135).

| Approach of T-028 | Test |
| --- | --- |
| The registration on the loopback | `tests/register.rs`: `the_second_api_host_answers_after_a_failure`, `a_503_on_the_post_is_not_sent_to_the_second_host`, `a_device_registers_enrols_and_is_resumed_from_its_file`, `a_429_on_registration_names_the_remedy`, `the_account_calls_hide_the_license_and_rotate_the_key`, `the_hints_of_a_device_go_into_the_bodies` |
| The identity of another uid | `files::tests`: `a_foreign_owner_is_a_config_error`, `a_foreign_owner_reads_with_the_consent_of_the_caller`; `tests/register.rs`: `a_loose_identity_leads_to_no_registration`, `a_refused_identity_leads_to_no_registration` (as root) |
| The pinned door | `door::tests::a_pinned_door_skips_the_order`; `tests/net.rs`: `a_pinned_door_is_the_only_door`, `the_doors_of_the_record_go_in_order` |
| A session as a `Dial` and `Datagrams` | `tests/net.rs`: `a_session_carries_tcp_to_an_address_and_to_a_name`, `a_session_carries_udp_as_datagrams`, `a_new_key_of_the_front_door_reads_the_record_again`, `a_key_that_the_edge_did_not_enroll_is_refused` |
| The lint as a function of the caller | `rules::tests::the_lint_of_the_caller_judges_the_table` |
| The live check | `tests/live.rs`: `live_register_connect_and_trace` (ignored; the step "pod-warp, a session at the WARP front door" of `live.yml`, row L36 of `docs/live-calls.md`) |
| The proxy, the forward, nc | T-338 (below) |

| Approach of T-338 | Test |
| --- | --- |
| The dispatch of a client: SOCKS5, HTTP `CONNECT`, an `http://` URL, SOCKS4, a silent client, the credentials, the refusal of a rule, the limit | `proxy::tests`: `a_socks4_client_is_refused_at_once`, `a_socks5_client_reaches_the_dialed_target`, `a_refused_destination_gets_its_own_reply`, `a_socks5_client_gives_its_credentials`, `an_http_client_gives_its_credentials`, `a_silent_client_is_closed`, `an_http_connect_client_reaches_the_dialed_target`, `an_http_url_request_reaches_the_origin_in_origin_form`, `the_connection_limit`; in `pod-proxy`: `tests/serve.rs` |
| The client of a UDP association | `proxy::tests`: `the_udp_client_is_fixed_by_the_first_sender`, `the_udp_relay_binds_the_address_of_the_control_connection`; `tests/proxy.rs`: `another_local_sender_can_not_take_the_udp_association`, `a_udp_association_survives_a_datagram_to_ipv6`, `a_udp_association_in_the_stream_reaches_the_echo` |
| The rules of the proxy over the session | `tests/proxy.rs`: `the_rules_of_the_proxy_route_each_destination`, `an_http_connect_goes_through_the_tunnel` |
| The names and the pins | `proxy::names::tests`: `the_dial_order`, `warp_resolve_takes_the_pin_first`; `net::settings::tests::the_stack_takes_the_ipv6_address_of_an_ipv4_tunnel` |
| The resolvers on UDP and over TLS; the check of the data path over TLS | `resolver::tests`: `the_resolver_specs`, `dns_over_tls_frames_each_message`; `tests/proxy.rs`: `the_first_resolver_that_answers_wins`, `a_lost_dns_query_is_sent_again`, `the_check_of_the_data_path_asks_a_resolver_over_tls` |
| The DNS forwarder and its bound | `proxy::dns_bound::tests`: `a_burst_gives_at_most_the_bound_in_flight`, `a_short_datagram_starts_nothing`; `tests/proxy.rs`: `the_dns_forwarder_gives_the_answer` |
| The forward | `forward::tests::the_rules_of_a_local_forward`; `tests/proxy.rs`: `a_forward_carries_each_connection_through_the_tunnel` |
| nc | `nc::tests`: `targets`, `a_udp_pipe_sends_each_read_and_prints_each_answer`, `the_pipe_ends_when_the_far_end_closes`, `the_reply_survives_an_input_that_ends_at_once`, `with_n_the_far_end_sees_the_eof`, `the_pipe_ends_after_idle_with_no_reply`, `a_udp_pipe_through_a_stream_frames_each_datagram`; `tests/proxy.rs`: `a_udp_pipe_of_nc_reaches_the_echo` |
| The L4 door (`l4.rs`, `proxy::run_l4`) | T-406 (below) |

The tests of the bind of a listener (`a_unix_socket_that_answers_is_not_taken`,
`a_refused_tcp_listener_does_not_stop_the_unix_socket`) and of the SSH
configuration of nc (`the_ssh_config_uses_warp_nc`,
`the_printed_block_quotes_the_path_as_the_written_block`) stay in podnet
with that code (call C201, call C203).

| Approach of T-406 | Test |
| --- | --- |
| A flow is one CONNECT stream on one QUIC connection; a large DATA frame; a refusal | `tests/l4.rs`: `a_flow_through_an_l4_door_on_the_loopback`; `l4::tests`: `the_connect_request`, `the_form_of_the_connect_is_checked`, `the_data_of_a_stream_crosses_each_cut` |
| A closed connection opens again | `tests/l4.rs`: `the_l4_door_opens_a_new_connection_after_a_close` |
| The rules of a proxy over the door, with no lookup; no UDP | `tests/l4.rs`: `a_proxy_over_the_l4_door_decides_with_no_lookup` |
| The door, the UDP and the key of a device | `tests/l4.rs`: `the_l4_door_of_a_device_takes_its_door_and_its_key` |

| Approach of T-335 | Test |
| --- | --- |
| The congestion controller (`--cc`) | `net::settings::tests::the_congestion_controller`; `tests/stack.rs`: `a_session_with_each_controller_carries_a_large_echo`; in `pod-netstack`: `each_tcp_socket_takes_the_congestion_controller_of_the_config`, `a_download_with_each_controller_crosses_a_bounded_link` |
| The TCP burst | `net::settings::tests::the_tcp_burst_variable_is_checked`, `the_stack_takes_the_burst_the_buffer_and_the_counters`; `tests/stack.rs`: `the_tcp_burst_clamps_the_window_that_the_edge_sees`; in `pod-netstack`: `the_burst_limit_clamps_the_window_that_the_stack_advertises` |
| The counters of the link | `tests/stack.rs`: `the_counters_of_the_link_count_each_session`; in `pod-netstack`: `stacks_with_shared_counters_add_to_one_count`, `shared_counters_take_the_inbound_drops_of_each_link` |
| The check of the data path, and the door that carried nothing | `check::tests::the_servers_of_the_check_follow_the_families_of_the_device`; `tests/stack.rs`: `the_check_of_the_data_path_needs_an_answer`, `a_door_that_carries_nothing_goes_last`, `a_lost_check_question_is_sent_again`, `no_check_takes_a_session_that_carries_nothing` |
| The budget of a relay | `tests/stack.rs`: `the_budget_of_a_relay_says_when_to_renew` |
| The TCP timeout, the keepalive, the linger and no Nagle (T-393) | `net::settings::tests::the_stack_has_the_tcp_timeout_the_keepalive_and_no_nagle_of_podnet`; in `pod-netstack`: the tests of `stack/tests/timeout.rs` (`a_tcp_socket_has_a_timeout_and_a_keepalive`, `the_tcp_sockets_have_no_nagle`, a connect and a stream to a peer that is gone) |
| UDP to a family that the device lacks is refused (T-395) | `net::datagrams::tests::a_family_that_the_device_lacks_is_refused`; `tests/net.rs`: `udp_to_ipv6_on_an_ipv4_tunnel_is_refused` (the check with a server of each family asks no IPv6 server), `udp_on_a_dual_stack_tunnel_takes_the_source_of_each_family`, `udp_to_ipv4_on_an_ipv6_tunnel_is_refused` |
| The flows of a closed session fail at once (T-394) | `tests/net.rs`: `the_flows_of_a_session_that_the_edge_ends_fail_at_once` (`Edge::end_sessions`), `a_close_by_the_caller_fails_the_flows_of_the_session`; in `pod-netstack`: the tests of `reset.rs` (a panic of the reactor) |

| Approach of T-336 | Test |
| --- | --- |
| The direct path wins when it is fast | `tests/leg.rs`: `the_direct_path_wins_when_it_is_fast` |
| The relay wins when the direct path hangs; the loser is dropped; the budget goes to `WarpNet` | `tests/leg.rs`: `the_relay_wins_when_the_direct_path_hangs` |
| The relay goes first when the leg says so; a direct path that works takes the first place back | `tests/leg.rs`: `the_relay_goes_first_when_the_leg_says_so` |
| A failure of both legs names both causes; the relay starts at once after a failure of the direct path | `tests/leg.rs`: `a_failure_of_both_legs_names_both_causes` |
| The text of the leg file | `leg::tests`: `the_leg_file_puts_a_recent_relay_first`, `the_leg_text_reads_back`, `a_change_of_the_order_is_kept_for_the_leg_file` |

| Approach of T-339 | Test |
| --- | --- |
| `Live`: a closed session is replaced, the backoff, the age, the relay budget and the TCP probe with no gap, the reports of the data path | `live::tests`: `a_closed_session_is_replaced`, `a_spent_relay_budget_renews_with_no_gap`, `a_session_is_replaced_at_max_age`, `a_caller_gets_an_error_when_no_session_opens`, `warp_probe_renews_a_session_with_no_tcp`, `the_probe_option`, `warp_status_names_the_data_path`, `the_tunnel_of_a_live_session_is_the_session_of_now`, `the_watcher_tells_each_step_to_the_caller`; `tests/live_session.rs`: `a_live_session_comes_back_after_the_edge_ends_it`, `the_tcp_probe_of_a_session_reaches_its_target` |
| The exit | `exit::tests`: `the_whoami_query_and_its_answer`, `the_json_keeps_ip_and_adds_ipv6`, `the_trace_body_gives_the_exit`; `tests/live_session.rs`: `the_exit_of_a_session_is_read_from_the_trace` |
| The socket of a running proxy | `find::tests`: `the_lookup_order_of_the_warp_socket`, `the_pointer_dir_does_not_depend_on_the_state_directory`, `a_pointer_file_that_others_can_read_is_refused`, `warp_exec_finds_warp_up_of_another_state_directory` |
| The TUN, its probe and its rp_filter | `tun::tests::creates_an_interface_with_the_address` (a skip with no `CAP_NET_ADMIN`); `tun::probe::tests`: `the_syn_of_the_probe_is_a_valid_packet`, `warp_tun_probe_takes_its_answer_out`, `warp_tun_probe_ends_a_dead_session`; `tun::rp_filter::tests`: `the_effective_rp_filter_is_the_larger_value`, `warp_tun_writes_0_unless_it_keeps_the_value` |
| The routes and the resolver | `route::tests`: `a_failed_route_removes_the_routes_before_it`, `the_plan_of_each_ipv6_masque_address`, `the_plan_of_each_masque_address`, `the_default_gateway_of_the_route_table`, `masks`, `a_required_address_with_no_route_is_refused`; `route::resolv::tests`: `the_resolv_conf_is_replaced_and_never_removed_first`, `replace_file_sets_the_mode`, `resolv_backup_survives_a_kill`, `resolv_backup_restores_a_symlink_and_an_absent_file`, `resolv_marker_guards_the_restore`, `resolv_record_of_another_version_is_not_applied`, `a_live_tun_blocks_a_second_one` |

The tests of podnet's record of an instance (`the_instance_line`,
`warp_status_names_the_tunnel_instance`,
`the_ready_document_names_the_data_path`), of `pod.rs` and of `up_summary`
stay in podnet with that code (call C215).

| Approach of T-337 | Test |
| --- | --- |
| A session over HTTP/3 carries datagrams and capsules | `tests/h3.rs`: `a_session_over_http3_carries_datagrams_and_capsules`; `h3::tests` (the frames, QPACK) |
| HTTP/3 refuses a proxy and a wrong key | `tests/h3.rs`: `http3_refuses_a_proxy_and_a_wrong_key` |
| HTTP/3 takes no relay | `tests/h3.rs`: `a_session_over_http3_takes_no_relay` |
| A loopback tunnel of WireGuard against the stand-in of `pod-wg` | `tests/wg.rs`: `a_wireguard_tunnel_carries_packets_both_ways`, `wireguard_refuses_a_way_with_no_udp`, `no_answer_is_a_failure_that_names_the_door`, `warp_connect_over_wireguard_gives_a_network` |
| The carried device over a `WarpNet` of the stand-in edge | `tests/wg.rs`: `a_carried_device_gives_a_network_inside_the_session` (a UDP route of `Edge::start_with_udp`) |

The stand-in edge answers a DNS question on UDP port 53 too, notes the
largest TCP window of the client, the ends of each UDP datagram and the
packets of each family, and drops the first questions that a test names
(`Watch` of `tests/common/edge.rs`); `Seen::silent` of
`pod_testkit::masque` makes a front door that takes the handshake and
carries nothing. `tests/proxy.rs` adds a route of DNS over TLS (port 853)
to a server of RFC 7858 on the loopback with a key that the TLS client of
the test pins. Plants: `warp-check-without-answer`,
`warp-check-door-kept`, `warp-burst-not-passed`,
`netstack-controller-not-set`, `netstack-burst-not-set`,
`netstack-shared-counters-lose-the-drops`, `warp-close-resets-nothing`,
`netstack-tcp-timeout-not-set`, `netstack-peer-reset-read-as-timeout`,
`warp-udp-to-a-family-the-device-lacks`,
`warp-race-relay-waits-for-the-direct-path`,
`warp-proxy-rule-not-applied`, `warp-dns-bound-not-held`,
`warp-h3-udp-past-the-way`, `warp-h3-takes-the-relay`,
`warp-l4-rule-not-applied`, `warp-l4-closed-connection-kept`,
`warp-live-drain-closes-at-once`, `warp-tun-probe-answer-not-taken`.

## The parity list

| Capability | podnet | podssh | `pod-warp` |
| --- | --- | --- | --- |
| Registration (`POST`, `PATCH`), the version pair of `pod-services` | yes | yes | yes |
| A second base of the API (PodUtils/podnet@T-241: a `POST` is final) | yes | routes (PodUtils/podssh@T-562) | yes (`Api::with_bases` takes podssh's routes) |
| Error 1015 names the remedy | yes | | yes |
| Account, license, rotation, removal | yes | | yes |
| Device hints | yes | | yes |
| The identity file, its lock, the record `created` | yes | a private file of its own | yes (podnet's form) |
| A foreign identity with consent | ask 9 | | yes |
| P-256 key and certificate in pure Rust | rcgen | yes | yes |
| Key of the front door pinned; the record read again on a new key | yes | yes (PodUtils/podssh@T-399) | yes |
| The order of the doors; the door that worked first; a door with no packet last | yes | the endpoints in turn (PodUtils/podssh@T-561) | yes |
| A pinned door | address | | `HOST:PORT` |
| MASQUE over HTTP/2, the windows, the batch, the keepalive | yes | yes | yes |
| MASQUE over HTTP/3; WireGuard; the carried device | yes | | yes (on the UDP of the caller, `UdpWay`) |
| The relay leg and the race, the leg file | yes | | yes (the relay is a dial of the caller) |
| The stack (congestion control, burst, counters, the check of the data path) | yes | smoltcp of its own | yes (`NetSettings` over the stack of `pod-netstack`; the check in `Warp::connect`) |
| The TCP timeout and keepalive, no Nagle (podnet issue 125, podssh) | yes | no Nagle | yes (`NetSettings`; a timeout reads as `TimedOut`) |
| The streams of a closed session fail at once; a close by the caller (podnet issues 125, 185) | yes | | T-394 |
| UDP to a family that the device lacks is refused (podnet issue 112) | yes | | yes |
| The budget of a relay session | yes | | yes (`Relay::meter`, `MasqueSession::relay_budget`, `WarpNet::renew_due`) |
| Names inside the tunnel | through the proxy's resolver | yes | yes |
| UDP | yes | | yes (`Datagrams`) |
| The rules with the lint of shadowed rules | yes | | yes (the lint is the caller's) |
| The proxy (SOCKS5, HTTP, UDP, the rules, the DNS forwarder), the forward, nc | yes | | yes (over a `Dial`; the listeners and the SSH configuration are the caller's) |
| Resolvers inside the tunnel over TLS (podnet issue 144), also for the check | yes | | yes (the TLS client is the caller's) |
| The L4 door (`--l4`) | yes | | yes (`L4` on the UDP of the caller; `Proxy::over_l4`) |
| `Live`, TUN, routes | yes | | yes (`live`, `tun` and `route`; the files, the paths and the probe are the caller's) |
| A tunnel that takes TLS and relays nothing (the probe of podssh, PodUtils/podssh@T-560) | the check of the data path | yes | yes (the check of the data path) |

## The adoption map

| podnet | `pod-warp` |
| --- | --- |
| `warp::WarpClient` | `Warp` (with `Settings`) |
| `WarpClient::register_or_load(path)` | `Warp::register_or_load(settings, &IdentityFile::new(path))` |
| `WarpClient::connect()` (a `WarpStack`) | `Warp::connect()` (a `WarpNet`; podnet implements its `podnet_core::network` traits over it, PodUtils/podnet#452) |
| `warp::register::*` | `register::*` with `&Api`; `api::api_status`, `device::redact_account` |
| `warp::identity::Identity` | `identity::Identity` |
| `warp::door::endpoint_order(all, won, bad, ipv6, v4_route)` | `door::endpoint_order(all, won, bad, Routes { v4, v6 })` |
| `door_from`, the door file | `door::door_from`, `door::door_text`; the file stays podnet's |
| `warp::session::{MasqueSession, MasqueTx, MasqueRx, Keepalive}` | `session::*`, with `Way` in place of `session::Dial` |
| `warp::capsule` | `capsule` |
| `warp::rules` (global `TABLE`, `COUNTERS`, `watch`) | `rules::Rules` (a value), `Table::build(.., &lint, allow)`; podnet passes `shadow::check` and keeps the reload of the file |
| `warp::ip::{parse_cc, set_cc, check_cc}`, `CC_VARIABLE` | `net::parse_cc` into `NetSettings::congestion_control`; podnet keeps the flag and the variable |
| `warp::ip::check_tcp_burst`, `TCP_BURST_VARIABLE` | `net::parse_tcp_burst` into `NetSettings::tcp_burst` |
| `warp::ip::{TCP_TIMEOUT, TCP_KEEPALIVE, TCP_LINGER}`, no Nagle in `Driver::tcp_socket` | `net::{TCP_TIMEOUT, TCP_KEEPALIVE, TCP_LINGER}`, the defaults of `NetSettings::{tcp_timeout, tcp_keepalive, tcp_linger, nodelay}` |
| `Command::UdpSend` refused when `!has_family(to)` | the `Datagrams` of `WarpNet::datagrams` (`InvalidInput`, the same words) |
| `warp::ip::link_stats()` (a global) | one `Arc<DeviceStats>` of podnet in `NetSettings::link_stats`; its `snapshot()` |
| `WarpStack::renew_due`, `StackConfig::renew` | `WarpNet::renew_due`, `NetSettings::renew = Some(Arc::new(move \|\| budget.spent()))` |
| `WarpClient::relay()`, `open_session` (the race), `RELAY_DELAY`, `RELAY_DELAY_KNOWN` | `Settings::relay`, `Warp::open_session`, `Relay::delay`, `Relay::delay_known` |
| `ws_relay::meter(io)`, `MasqueSession::relay_budget` | `Relay::meter = Some(Arc::new(\|io\| { let (io, b) = RelayBudget::meter_for(&ctx, io); (Box::new(io), Arc::new(move \|\| b.spent())) }))`; `MasqueSession::relay_budget` |
| `read_leg`, `write_leg`, `relay_first_from`, `LEG_TTL` | `leg::relay_first_from`, `Warp::with_relay_first`, `Warp::take_leg_change`, `leg::leg_text`, `leg::LEG_TTL`; the file stays podnet's |
| `decisions::count("masque", "direct" \| "relay" \| "relay_first" \| "no_relay")` | `Observer::decision("warp.leg", "direct" \| "relay" \| "relay-first" \| "no-relay")` |
| `WarpStack::close_session(why)`, `Driver::fail_all`, `supervise` | `WarpNet::close(why)`; `NetStack::reset_all` at the first cause; the watch of `NetStack::stopped()` |
| `proxy::check_data_path(&stack, ipv4)` | `WarpNet::check_data_path(&DataPathCheck)`; `Warp::connect` runs it and marks the door bad |
| `proxy::NO_CHECK_VARIABLE` | `NetSettings::check = None` |
| `proxy::run(live, bound)`, `serve` | `Arc<Proxy>::run_tcp(listener)` or `run_unix(listener)`, with `Proxy::tunnel` (the `Live` of T-339 as a `Tunnel`) |
| `proxy::handle_client_auth(stream, dial, udp, auth)` | `proxy::serve_client(stream, dial, udp, &Opening)`; the dial gives a `BoxedIo`, a refusal is a failure of the class `Policy` |
| `proxy::route`, `proxy::dial`, `proxy::resolve`, `resolve_all`, `order` | `Proxy::route`, `Names::connect`, `Names::resolve`, `Names::resolve_all`, `proxy::order` |
| `proxy::STATS`, `set_max_connections`, `set_credentials`, `health_log` | `Proxy::stats`, `max_connections`, `credentials`; `Proxy::health()` each `HEALTH_EVERY` for the caller's line |
| `proxy::Resolver`, `parse_resolvers`, `set_resolvers`, `resolvers()`, `forward_dns` | `resolver::Resolver`, `parse_resolvers`, `Proxy::names.resolvers`, `Resolvers::in_use`, `Resolvers::exchange` |
| `rules::decide(&stack, &table, &host)`, `rules::is_refused(&err)` | `Table::decide(&lookup, &host)`; `failure.class == Class::Policy` |
| `podnet_netns::socks` (`socks5_read_command_auth`, `socks5_reply`, `socks5_udp_unwrap`, `http_read_request_auth`) | `pod_proxy::serve` (`read_socks5`, `socks5_reply`, `reply_for`, `read_http_proxy`), `pod_proxy::udp::unwrap` |
| `decisions::count("dispatch", ..)`, `count("rule", ..)` | `Observer::decision("warp.dispatch", ..)`, `("warp.rule", ..)` |
| `forward::{Rule, run}`, `ProxyListen` | `forward::{Rule, Forward}`, `forward::Listen` |
| `nc::{target, PipeEnd, TCP_IDLE, pipe_with, udp_pipe_with, udp_via_stack, udp_via_running}` | `nc::{target, PipeEnd, TCP_IDLE, pipe_with, udp_pipe_with, udp_via_net, udp_via_stream}` (the caller finds its running proxy and opens the stream with `pod_proxy::udp::open_in_stream`) |
| `MasqueSession::connect` with `Dial { h3: true, .. }`, `h3::*` | `MasqueSession::connect` with `Way { h3: true, udp, .. }`, `h3::*` |
| `wg::Peer` (`from_exported` stays with `export.rs`), `MasqueSession::connect_wg(peer)` | `wg::Peer`, `MasqueSession::connect_wg(peer, way)` |
| `MasqueSession::connect_wg_inside(peer, &stack)`, `connect_carried`, `CARRY_MTU` | `MasqueSession::connect_wg_inside(peer, &net)`, `Settings::carry`, `CARRY_MTU` |
| `decisions::count("masque", "wireguard" \| "carry")` | `Observer::decision("warp.leg", "wireguard")`, `("warp.carry", "carried")` |
| `warp::l4::{L4, l4_sni, connect_headers_form, authority}`, `WarpClient::l4(sni)`, `L4::dial(host, port)` | `l4::{L4, connect_headers_form, authority}`, `service::l4_sni`, `Warp::l4()` with `L4::with_sni`, `L4::open(&target)` or `Dial::dial` |
| `proxy::run_l4(l4, bound)` | `Arc::new(Proxy::over_l4(l4, direct)).run_tcp(listener)` |
| `warp::live::{Live, Closable, Probe, DEFAULT_PROBE}`, `WarpClient::connect_live()` | `live::{Live, Closable, Probe, DEFAULT_PROBE}`, `Warp::connect_live(&LiveSettings)`; the closure of `Connect` gives a `Failure` |
| `live::instance::publish_own(&live)` (the record of the instance) | `live::publish(&live, sink)`; the record of the instance stays podnet's |
| `warp::exit::{trace, egress_v6}(&stack)` | `exit::{trace, egress_v6}(&net, &tls)` with podnet's TLS client (`trust::context()`) |
| `warp::find::{running, candidates, write_pointer, read_pointer, remove_pointer}` | `find::running(&find::candidates_of(..), pointer)`, `find::{write_pointer, read_pointer_at, remove_pointer}` with podnet's paths; podnet adds its hint (`podnet warp up`) to the text |
| `warp::tun::{Tun, run}`, `tun::probe`, `tun::rp_filter` | `tun::{Tun, run}` with `TunSettings`, `tun::probe`, `tun::rp_filter::RpFilter` |
| `warp::route::{Routes, nameservers, resolv}` | `route::Routes::setup(.., &Tables::system())` and `Routes::set_dns(&route::nameservers(&list), &paths)`, `route::resolv` |
| `exit::fault(Fault::X, e)` | `Failure { stage, class }`; `Class::exit_code` |
| `decisions::count("masque", ..)` | `Observer::decision("warp.door", ..)` |

| podssh (`crates/podssh-warp`) | `pod-warp` |
| --- | --- |
| `key::DeviceKey`, `der` | `key::DeviceKey`, `der` |
| `register::Api` with `routes::Route` | `Api::with_bases` |
| `road::Warp` (open the tunnel, a stream to `HOST:PORT`) | `Warp::connect` and `WarpNet::dial` |
| `dns::lookup` | `dns::lookup` |
| `stand_in::StandIn` | `pod_testkit::masque` and the edge of `tests/common/edge.rs` |
| `device::Device` (its private file) | `store` (podnet's file form) |
