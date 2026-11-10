# fuzz

The fuzz targets of the parsers of podcrates (cargo-fuzz and libFuzzer;
`docs/design.md`, section 8, step 4; T-017). This directory is a cargo
workspace of its own: the root workspace excludes it, so the library
crates do not build libFuzzer's C++.

## Run

```sh
sh scripts/fuzz.sh 60                 # each target for 60 s
sh scripts/fuzz.sh 300 proxy_url      # one target for 300 s
cargo check --manifest-path fuzz/Cargo.toml --bins   # the build, on stable Rust
cargo test --manifest-path fuzz/Cargo.toml --lib     # the stable test
```

* `scripts/fuzz.sh` pins the nightly Rust and the version of cargo-fuzz.
  It needs Linux with glibc and a C++ compiler. A crash fails the run,
  and its input stays in `fuzz/artifacts/TARGET/`.
* `.github/workflows/fuzz.yml` runs each target each week and by hand,
  first with the plant `fuzz-one-line-escape`, which must crash.
  `ci.yml` runs the build and the stable test on each push.
* A crash input becomes a unit test of its crate first, then the repair.

## Layout

| Path | What |
| --- | --- |
| `src/targets.rs` | the body of each target: the parser that it runs and what it checks |
| `src/lib.rs` | `TARGETS`, and the checks that more than one target uses |
| `src/tests.rs` | each target on its seeds and on 2000 mutations of them, on stable Rust; each target has its binary and its seeds |
| `fuzz_targets/NAME.rs` | the binary of a target: it calls `targets::NAME` |
| `seeds/NAME/` | the seeds of a target, small files; libFuzzer reads them, and adds what it finds to `corpus/NAME/` (not in git) |

## The targets

Each target keeps the name of its source: podssh at `d3745ab`
(`fuzz/fuzz_targets/`) and podnet at `a8071b1` (`src/fuzzing.rs`).

| Target | Crate | Parser | Source |
| --- | --- | --- | --- |
| `one_line` | `pod-core` | `text::one_line` | podssh `one_line` |
| `pem_bundle` | `pod-tls` | `bundle::pem_certificates` | podssh `pem_bundle` |
| `http_head` | `pod-proxy` | `http::parse_head` | podssh `http_head` |
| `http_chunked` | `pod-proxy` | `http::decode_chunked` | podssh `http_chunked` |
| `socks5_answer` | `pod-proxy` | `fuzz_exchange` (feature `fuzzing`) | podssh `socks5_answer` |
| `proxy_url` | `pod-proxy` | `Proxy::parse` | podssh `proxy_url` and podnet `proxy_url`, one target |
| `no_proxy` | `pod-proxy` | `env::no_proxy_matches`, `NoProxy` | podssh `no_proxy` and podnet `no_proxy`, one target |
| `connect_status` | `pod-proxy` | `http::parse_status_line`, `digest::challenge_in`, `proxy_auth::schemes` | podssh `connect_status` and podnet `connect_response_head`, one target |
| `dns_wire` | `pod-dns` | `wire::{addrs, srv, read_name}`, `https::{from_wire, from_rdata, check_list}` | podssh `dns_wire` (at `a791553`) |
| `doh_json` | `pod-dns` | `json::{parse, srv_of, parse_srv}`, `json::Answer::of`, `https::from_json` | podnet `doh_json` (at `380d9a4`); its SRV part reads `pod_dns::json::srv_of`, and podnet's `edge_discovery::parse_srv` goes with `pod-cloudflare` |
| `broker_target` | `pod-broker` | `target::{parse_authority, parse_absolute, normal_path}` | new in `pod-broker` (T-099); its seeds are small targets written here |
| `gsr_message` | `pod-gsr` | `wire::Reader::next` over `wire::fuzz_messages` (feature `fuzzing`): fragments of 1 to 32 bytes, cancelled reads | new in `pod-gsr` (T-086, GitHub #24); its seeds are messages of the relay written here |
| `ws_frame` | `pod-ws` | `frame::{decode, encode}` | podssh `ws_frame` (at `8cd0f2b`); its seeds are the frames of RFC 6455 section 5.7 |
| `ws_upgrade` | `pod-ws` | `handshake::check_response` | podssh `ws_upgrade` (at `8cd0f2b`) |
| `close_reason` | `pod-ws` | `close::close_code_and_reason` | podssh `close_reason` (at `8cd0f2b`) |

Where both sources test the same parser, one target holds the checks and
the seeds of both:

* `proxy_url`: podssh's checks (a proxy on TCP shows only its host and
  port; its URL parses back to the same proxy) and podnet's (the label
  has no `@`); podnet's seeds. A proxy on a Unix socket is new in
  `pod-proxy`: it has no credentials.
* `no_proxy`: podssh's input (the list, a line break, the host; `*`
  matches each host) and podnet's hosts with a port (`NoProxy`); podnet's
  seeds.
* `connect_status`: podnet's input (any bytes, as lossy UTF-8, as the dial
  reads the head) and podssh's check (the reason is safe on one line);
  podnet's seeds. The readers of a 407 run on the same head.

The sources have no other seed for these targets (podssh's
`scripts/fuzz-seeds.py` writes seeds for the targets that wait); the
other seeds are small inputs written here. The seeds of `dns_wire` are
answers of `crates/pod-dns/tests/fixtures/doh/` and the HTTPS answer of
`crates/pod-dns/src/https_tests.rs`, as resolvers sent them; those of
`doh_json` are podnet's two seeds, an NXDOMAIN and the HTTPS answer of
Cloudflare's JSON form.

## The targets that wait for their crate

| Target | Source | Parser | Crate |
| --- | --- | --- | --- |
| `relay_list`, `relay_pool` | podssh | `podssh_relay::relay::parse_relay_list`, `pool::parse_pool` | `pod-relay` (T-025) |
| `vnet_frame` | podnet | `vnet::framing::read_frame` | `pod-netstack` (T-023) |
| `warp_capsule` | podssh | `podssh_warp::capsule` | `pod-warp` (T-028), one target with podnet's `capsule` |
| `capsule` | podnet | `warp::capsule::CapsuleDecoder` | `pod-warp` (T-028) |
| `capnp_message`, `rpc_message`, `register_response`, `connect_request`, `h2_header_block`, `srv_record`, `request_head` | podnet | `cloudflare::{capnp, rpc, schema, http2}`, `edge_discovery::parse_srv`, `proxy::build_request_head` (the head to the origin, not a parser of `pod-proxy`) | `pod-cloudflare` (T-029) |
| `capnp_message`, `quick_connect_request`, `quick_control` | podssh | `podssh_relay::quick` | `pod-cloudflare` (T-030), `capnp_message` one target with podnet's |
| `ingress_rule`, `config_yaml` | podnet | `ingress::{Service, ForwardTarget}`, `ws::LocalSpec`, `yaml::parse` | `pod-cloudflare` (T-029), with `src/config.rs`, which reads them; podnet keeps them if those files stay in podnet |
| `acl_rule` | podnet | `vnet::policy::AclRule` | none: `src/vnet/` stays in podnet |
| `escape_filter`, `known_hosts_line` | podssh | `podssh_ssh::escape` (the `~` escapes of SSH), `podssh_ssh::known_hosts` (OpenSSH's file) | none: podssh-ssh stays in podssh. `pod-term`'s `escape` reads the escapes of a terminal, and `pod-identity`'s pins are the file `known-nodes`: other parsers |
| `irc_message`, `irc_framing`, `cli_args` | podssh | `podssh_core::irc`, `podssh_cli::tree::parse` | none: they stay in podssh |
