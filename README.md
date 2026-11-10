# podcrates

The Rust crates that the PodUtils projects share: TLS, DNS, proxies,
WebSockets, the relay client and its resumable layer, the end-to-end
channel, WARP, the Cloudflare tunnel, file transfer and more.

The consumers are [podssh](https://github.com/PodUtils/podssh),
[podnet](https://github.com/PodUtils/podnet) and podvm. Each crate links
into a static binary on each target of the consumers.

## State

Version 0.8.0 (`v0.8.0`) is the last release. The releases `v0.1.0` to
`v0.8.0` have notes in [docs/releases/](docs/releases/). A consumer takes a
crate by the git tag of this repository. The record
[TODO/](TODO/PROGRESS.md) has the state, the work order and the open
entries. M6 (the crates of podvm) is the current wave; M8 (the fleet)
waits.

## The crates

Each name in `crates/` is in the table. A crate that has no mark is
planned. [docs/design.md](docs/design.md), section 4, gives the source and
the dependencies of each crate.

| Wave | Crates |
| --- | --- |
| M0 | `pod-todo` (exists), `pod-core` (exists), `pod-testkit` (exists) |
| M1 | `pod-identity` (exists), `pod-term` (exists), `pod-tls` (exists), `pod-proxy` (exists), `pod-dns` (exists), `pod-services` (exists), `pod-services-watch` (exists: the watches and their runner, `publish = false`), `pod-ws` (exists) |
| M2 | `pod-quic` (exists), `pod-session`, `pod-e2e`, `pod-wormhole` (exists), `pod-netstack`, `pod-wg`, `pod-relay` |
| M3 | `pod-transit`, `pod-warp`, `pod-cloudflare` |
| M4 | `pod-derp`, `pod-piping`, `pod-opentunnel`, `pod-acme`, `pod-egress`, `pod-relay-serve`, `pod-iroh`, `pod-tor`, `pod-ts`, `pod-gsr` (exists) |
| M6 | `pod-hostprobe` (exists), `pod-supervise` (exists), `pod-guest` (exists), `pod-oci` (exists), `pod-extract` (exists), `pod-rootfs` (exists), `pod-enter` (exists) |
| M7 | `pod-microvm`, `pod-sud`, `pod-confine`, `pod-netlab`, `pod-pcap`, `pod-sandprobe` |
| M8 | `pod-broker` (exists), `pod-unit`, `pod-control`, `pod-release` |

## Documents

| Document | What |
| --- | --- |
| [AGENTS.md](AGENTS.md) | the rules for each session |
| [docs/design.md](docs/design.md) | the rules of each crate, the crates and their sources, the traits, the targets, the procedure for one crate |
| [TODO/PROGRESS.md](TODO/PROGRESS.md) | the state, the work order and the questions for the owner |
| [docs/maintenance.md](docs/maintenance.md), [docs/development.md](docs/development.md) | the procedures, the tools, the rules of the code and the tests |
| [docs/decisions.md](docs/decisions.md) | the rulings of the owner, the calls and the questions |
| [SECURITY.md](SECURITY.md) | how to report a vulnerability |

## License

0BSD. See [LICENSE](LICENSE).
