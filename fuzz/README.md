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
* The dev profile of this workspace optimizes the crates under test
  (`opt-level = 2`): the stable test runs the crypto of `pod-wg` and
  `pod-e2e` on 2000 mutations of each seed, which takes minutes with no
  optimization.
* A crash input becomes a unit test of its crate first, then the repair.

## Layout

| Path | What |
| --- | --- |
| `src/targets.rs`, `src/targets/` | the body of each target: the parser that it runs and what it checks (`targets/actions.rs`: the expressions and the workflow files of `pod-actions-expr` and `pod-actions-workflow`; `targets/devtunnel.rs`: the connect information of `pod-devtunnel`; `targets/job.rs`: the records of `pod-job`; `targets/link.rs`: the frames of `pod-link`; `targets/netlab.rs`: the counters of `pod-netlab`; `targets/podvm.rs`: the parsers of podvm; `targets/session.rs`: the records, the handover state and the UDP frames of `pod-session`; `targets/wormhole.rs`: the packets of an MQTT broker) |
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
| `broker_target` | `pod-broker` | `target::{parse_authority, parse_absolute, normal_path}` | new in `pod-broker` (T-099); its seeds are small targets written here, and `raw-utf8-in-path`, a crash that `fuzz.yml` found (T-183) |
| `gsr_message` | `pod-gsr` | `wire::Reader::next` over `wire::fuzz_messages` (feature `fuzzing`): fragments of 1 to 32 bytes, cancelled reads | new in `pod-gsr` (T-086, GitHub #24); its seeds are messages of the relay written here |
| `ws_frame` | `pod-ws` | `frame::{decode, encode}` | podssh `ws_frame` (at `8cd0f2b`); its seeds are the frames of RFC 6455 section 5.7 |
| `ws_upgrade` | `pod-ws` | `handshake::check_response` | podssh `ws_upgrade` (at `8cd0f2b`) |
| `close_reason` | `pod-ws` | `close::close_code_and_reason` | podssh `close_reason` (at `8cd0f2b`) |
| `quic_probe_answer` | `pod-quic` | `probe::answers` | new in `pod-quic` (T-019); its seeds are the Version Negotiation of 27 bytes of podssh's probe test, one of another ID, one that names the reserved version, and a short header |
| `hold_sockaddr` | `pod-hold` | `sockaddr::read`, `sockaddr::parse` | podnet `crates/podnet-netns/src/seccomp/notif.rs` `parse_sockaddr` (T-166); the memory of the task is a sealed memfd; its seeds are an `AF_INET` and an `AF_INET6` address, an address one byte short, and a length past the memory |
| `hold_syn` | `pod-hold` | `packet::parse_tcp_syn`, `packet::rst_reply_for_syn`, `packet::ipv4_checksum` | podnet `crates/podnet-netns/src/netns/packet.rs` (T-170); its seeds are a SYN, a SYN with IP options, an ACK, and a header of IHL 4 that podnet's parse took |
| `job_offer` | `pod-job` | `serde_json::from_slice::<JobOffer>`, `JobOffer::check`, the encode and the read again | new in `pod-job` (T-163); its seeds are the 4 valid and 8 invalid offers of `crates/pod-job/tests/examples/job-offer/` (podrunner's draft) |
| `oci_manifest` | `pod-oci` | `spec::Fetched::parse` with the `Content-Type` that the first byte picks; the document's own `mediaType` decides over the header; `Descriptor::parsed_digest` of each descriptor | podvm `podvm-oci` (T-173, PodUtils/podvm#241); its seeds are an OCI index and manifest, a docker list and manifest named by the header only, an index whose header says manifest, and a schema 1 document, written here |
| `oci_challenge` | `pod-oci` | `registry::fuzz_parse_challenge` (the feature `fuzzing`): each key trimmed and not empty, no parameter with no space, at most one parameter more than the commas | podvm `podvm-oci` (T-173); its seeds are the challenge of Docker Hub, a scope with a comma in quotes, a Basic challenge, and `Bearer` alone |
| `extract_layer` | `pod-extract` | `remove::collect`, `remove::apply`, `apply::apply_layer` of a layer that is tar, gzip or zstd (the first byte picks) in a fresh directory four levels deep: no entry escapes the root | podvm `podvm-extract` (T-173); its seeds are a layer with a directory, a file, a symbolic and a hard link and two whiteouts, in tar, gzip and zstd (raw blocks), and three that climb out (`../`, a link, an absolute path), written here |
| `elf_header` | `pod-enter` | `abi::Elf::parse`: the same answer twice, and no NUL in a name that it gives | podvm `podvm-enter` (T-173); its seeds are a static and a dynamic program of two lines each, built here with `cc -Os -s`, and a shell script |
| `squashfs_image` | `pod-guest` | `squashfs::Image` (the superblock, the xz options, the inode, directory, fragment, id and export tables, the blocks: a walk of up to 256 inodes, twice, with the same answer; a file that reads has its size) and `modloop::modules` (each module once, one line of `modules.dep` for each) | new in `pod-guest` (T-206, PodUtils/podvm#260); its seeds are images of the test writer of `pod-guest` (`write_the_fuzz_seeds`): a modloop with an x86 BCJ block, each kind of inode with metadata blocks of 64 bytes and an xz fragment, and an image with compressor options |
| `mailbox_message` | `pod-wormhole` | `message::{object, message_of, pake_of, words, hex, unhex}`, `Code::parse` | new in `pod-wormhole` (T-022); its seeds are the messages of a mailbox and the codes of the tests written here, and `cut-on-a-space`, a crash that `fuzz.yml` found (T-184) |
| `mqtt_packet` | `pod-wormhole` | `mqtt::packet::{decode, encode}`, `mqtt::topic` | new in `pod-wormhole` (T-261); its seeds are the packets of MQTT 3.1.1 that a meeting point reads, written here |
| `session_record` | `pod-session` | `decode::Decoder`, `Record::encode`: the bytes of a link in pieces of 1 to 64 bytes; each record encodes again to itself; an error stays | new in `pod-session` (T-020); its seeds are records of podssh's `tests/session_codec.rs` (a `GREETING`, an `OPEN`, a `REFUSE`, a `DATA` and an `ACK`) and an unknown type |
| `session_handover` | `pod-session` | `handover::LayerState::{decode, encode}` | new in `pod-session` (T-020); its seeds are a closed session, a carried session with a replay buffer, and a state of another version, written here |
| `e2e_frame` | `pod-e2e` | `fuzzing::{take_frames, verdict, net_part}` (feature `fuzzing`): each frame has its length, and the frames and the rest are the input; an empty frame is refused; a reason is safe on one line; a seed comes only with a verdict that lets in; a verdict and a net's part write back to the same answer and bytes | new in `pod-e2e` (T-021); its seeds are two frames, an empty frame, verdicts with a seed, with a refusal and with an item of a later kind, and the net's part of a proof, written here |
| `e2e_proof` | `pod-e2e` | `fuzzing::proof` (feature `fuzzing`), after a byte of flags: only the key of the seed's identity signs its Noise key; the word reads back to its ask; a grant has 1 to `MAX_GRANT` bytes; the net's form only where this end reads it | new in `pod-e2e` (T-021); its seeds are the proofs of the identity of the seed `[1; 32]`: plain, with a route, with an exit's target, in the net's form, with a grant, and one of another key, written here |
| `e2e_handover` | `pod-e2e` | `hold::ChannelState::{decode, encode}` | new in `pod-e2e` (T-021); its seeds are a held channel with a seed of its tokens, a fresh one, and a state of another version, written here |
| `udp_frame` | `pod-session` | `udp::{Decoder, framed}`: the stream in pieces of 1 to 64 bytes; each datagram frames back to its bytes | new in `pod-session` (T-020); its seeds are two datagrams, a DNS query, and a length over the limit |
| `wg_response` | `pod-wg` | `fuzzing::finish` (feature `fuzzing`; `Initiation::finish` on fixed keys): a response is taken only when it is the responder's, but for the reserved bytes and mac2 | new in `pod-wg` (T-024); its seeds are the response of the fixed keys, the same with other reserved bytes and another mac2, one with a changed mac1, and one byte short, written here |
| `wg_cookie` | `pod-wg` | `fuzzing::cookie` (`Cookie::open` on fixed keys): a cookie reply opens only when it is the responder's reply to the initiation, but for the reserved bytes | new in `pod-wg` (T-024); its seeds are the reply, the same with other reserved bytes, one with a changed cookie, and one to another initiation, written here |
| `warp_capsule` | `pod-warp` | `capsule::{CapsuleDecoder, parse_address_assign}`: the body in pieces of 1 to 64 bytes; each packet is a DATA capsule of 1 to `MAX_CAPSULE_LEN` bytes that encodes back to itself; after an error nothing is kept | podnet `src/warp/capsule.rs` (T-028); its seeds are two packets, an ADDRESS_ASSIGN before a packet, a capsule split in two, and a length over the limit, written here |
| `relay_list` | `pod-relay` | `relay::{parse_relay_list, forward_path}`: a path is always `/connect/HOST/PORT`, three segments, with nothing that adds a segment, a query or a line | podssh `relay_list`; its seeds are a host, a list with a port, an IPv6 literal and a list that is no list, written here |
| `relay_pool` | `pod-relay` | `pool::parse_pool`: each host is kept once, in lower case, of the primary's deployment and not the primary | podssh `relay_pool`; its seeds are a pool with a host of another deployment and one twice, a pool with limits and an empty pool, written here |
| `actions_expr` | `pod-actions-expr` | `Expr::parse` and `evaluate` (twice: the same value), `Template`, `Condition` (an implicit condition starts with `success()`), `Value::from_json` (its `toJSON` reads back to the same value) | new in `pod-actions-expr` (T-251); its seeds are expressions of the GitHub documents (a filter, a format with escapes, the ternary, the coercion, `fromJSON` with an index), a template, a JSON document, nested groups and an open string, written here |
| `actions_workflow` | `pod-actions-workflow` | `parse`, `Run::new` and `plan`: the order holds each job once, and the dry run gives the jobs in that order | new in `pod-actions-workflow` (T-251); its seeds are a matrix with `include` and `exclude`, needs with a condition and a reusable workflow, an anchor, and a cycle, written here |
| `devtunnel_connect_info` | `pod-devtunnel` | `ConnectInfo::parse` of an answer of the management API: an answer that parses has a connect token and one relay endpoint or more (at most 64, each with at most 16 host keys), each `wss://` or `ws://` to the loopback; its `Debug` and the text of a failure hold no token and no line break | new in `pod-devtunnel` (T-378); its seeds are a relay endpoint, plain relays to the loopback and to another host, a token that is no text and a relay URI with a user, written here |
| `link_frame` | `pod-link` | `Decoder` (with or without the preface) in pieces of 1 to 64 bytes: each frame keeps the rules of the protocol (a stream frame has an id, a body has at most 1 MiB, a credit is not 0) and encodes to bytes that decode to the same frame; an error stays | new in `pod-link` (T-300); its seeds are the life of a stream, the link frames, a body over 1 MiB, a credit of 0, a wrong preface and a `PING` with a stream id, written here |
| `netlab_counters` | `pod-netlab` | `parse_counters` of the table `lab` (twice: the same answer) and `judge` of the counters against themselves: each counter that must move fails, as after a flushed chain | new in `pod-netlab` (T-207); its seeds are the output of nftables 1.0.9 for the nine counters of a lab, counters of two tables and of a family `ip`, a counter two times, and a counter with no packets, written here |
| `wg_transport` | `pod-wg` | `fuzzing::open` (`Session::open`, twice): a message that opens has the type, the index, the header and the tag around its packet, and its second read is a replay | new in `pod-wg` (T-024); its seeds are an IPv4 and an IPv6 packet, a keepalive, an IPv4 length past the packet, a message to another session, and a header with no tag, written here |

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

| `vnet_frame` | podnet | `vnet::framing::read_frame` | `pod-netstack` (T-023) |
| `warp_capsule` | podssh | `podssh_warp::capsule` | `pod-warp` (T-028), one target with podnet's `capsule` |
| `capsule` | podnet | `warp::capsule::CapsuleDecoder` | `pod-warp` (T-028) |
| `capnp_message`, `rpc_message`, `register_response`, `connect_request`, `h2_header_block`, `srv_record`, `request_head` | podnet | `cloudflare::{capnp, rpc, schema, http2}`, `edge_discovery::parse_srv`, `proxy::build_request_head` (the head to the origin, not a parser of `pod-proxy`) | `pod-cloudflare` (T-029) |
| `capnp_message`, `quick_connect_request`, `quick_control` | podssh | `podssh_relay::quick` | `pod-cloudflare` (T-030), `capnp_message` one target with podnet's |
| `ingress_rule`, `config_yaml` | podnet | `ingress::{Service, ForwardTarget}`, `ws::LocalSpec`, `yaml::parse` | `pod-cloudflare` (T-029), with `src/config.rs`, which reads them; podnet keeps them if those files stay in podnet |
| `acl_rule` | podnet | `vnet::policy::AclRule` | none: `src/vnet/` stays in podnet |
| `escape_filter`, `known_hosts_line` | podssh | `podssh_ssh::escape` (the `~` escapes of SSH), `podssh_ssh::known_hosts` (OpenSSH's file) | none: podssh-ssh stays in podssh. `pod-term`'s `escape` reads the escapes of a terminal, and `pod-identity`'s pins are the file `known-nodes`: other parsers |
| `irc_message`, `irc_framing`, `cli_args` | podssh | `podssh_core::irc`, `podssh_cli::tree::parse` | none: they stay in podssh |
