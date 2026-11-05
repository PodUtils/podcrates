# podcrates

The Rust crates that the PodUtils projects share: TLS, DNS, proxies,
WebSockets, the relay client and its resumable layer, the end-to-end
channel, WARP, the Cloudflare tunnel, file transfer and more.

The consumers are [podssh](https://github.com/PodUtils/podssh),
[podnet](https://github.com/PodUtils/podnet) and podvm. Each crate links
into a static binary on each target of the consumers.

## State

The plan is in [docs/plan.md](docs/plan.md). No crate exists now. The
work starts when the owner gives the go-ahead.

## The crates

| Wave | Crates |
| --- | --- |
| 1 | `pod-todo`, `pod-testkit`, `pod-identity`, `pod-term`, `pod-proxy`, `pod-tls`, `pod-dns`, `pod-ws` |
| 2 | `pod-session`, `pod-e2e`, `pod-netstack`, `pod-wg`, `pod-relay` |
| 3 | `pod-wormhole`, `pod-transit`, `pod-warp`, `pod-cloudflare` |
| 4 | `pod-derp`, `pod-piping`, `pod-opentunnel`, `pod-acme`, `pod-egress`, `pod-relay-serve`, `pod-iroh`, `pod-tor`, `pod-ts` |

## Documents

| Document | What |
| --- | --- |
| [AGENTS.md](AGENTS.md) | the rules for each session |
| [docs/plan.md](docs/plan.md) | the plan: the crates, their sources, the rules, the order |
| [docs/decisions.md](docs/decisions.md) | the rulings of the owner, the calls and the questions |
| [SECURITY.md](SECURITY.md) | how to report a vulnerability |

## License

0BSD. See [LICENSE](LICENSE).
