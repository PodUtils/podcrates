# podcrates

The Rust crates that the PodUtils projects share: TLS, DNS, proxies,
WebSockets, the relay client and its resumable layer, the end-to-end
channel, WARP, the Cloudflare tunnel, file transfer and more.

The consumers are [podssh](https://github.com/PodUtils/podssh),
[podnet](https://github.com/PodUtils/podnet) and podvm. Each crate links
into a static binary on each target of the consumers.

## State

The work is in the record [TODO/](TODO/PROGRESS.md) (milestone M0, the
foundation, runs now). `pod-todo`, the checker of the record, exists. The
first release follows M0.

## The crates

| Wave | Crates |
| --- | --- |
| 0 | `pod-todo` (exists), `pod-core`, `pod-testkit` |
| 1 | `pod-identity`, `pod-term`, `pod-tls`, `pod-proxy`, `pod-dns`, `pod-ws` |
| 2 | `pod-quic`, `pod-session`, `pod-e2e`, `pod-wormhole`, `pod-netstack`, `pod-wg`, `pod-relay` |
| 3 | `pod-transit`, `pod-warp`, `pod-cloudflare` |
| 4 | `pod-derp`, `pod-piping`, `pod-opentunnel`, `pod-acme`, `pod-egress`, `pod-relay-serve`, `pod-iroh`, `pod-tor`, `pod-ts` |
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
