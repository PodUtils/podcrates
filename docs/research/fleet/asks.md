# The asks of GitHub #35 and #36: the route of each ask

This page routes the 96 asks of cfb (GitHub #35, 84 asks) and githome (GitHub #36, 12 asks), after the rulings of
2026-11-09 (`docs/decisions.md`). `TODO/upkeep.md`, entry T-098, is the triage. Priority is the priority of the ask.

Route words:

* `T-NNN`: an entry of podcrates.
* `podssh`, `podnet`, `podvm`: the product of that repository does the work in its own record.
* `new project NAME`: a repository of its own (the owner creates it).
* `deferred: RULING`: the work waits for the ruling that the words name. The ruling and its date are in
  `docs/decisions.md`.

Rulings that decide a route:

* New projects: each has its own repository (2026-11-09).
* Tunnel set and connector of the fleet: after the first release on crates.io (2026-11-09). Written as "deferred:
  connector".
* Test accounts: not now (2026-11-09). Written as "deferred: test accounts".
* No npm package: the schemas live in podcrates (2026-11-09).
* podnet is Linux only. podssh serves the operator commands on Windows and macOS (2026-11-09).
* KVM is lifted for podvm (2026-11-09).
* The relay Worker is Rust on wasm32, after T-036 and the wire document T-120 (2026-11-09).
* Betas for each repository are allowed. No crates.io release now (2026-11-09).

## A-N: podnet

| Ask | P | Ask | Route |
| --- | --- | --- | --- |
| A-N-01 | P0 | Live test of a named tunnel on four legs with two replicas, from a CI secret | deferred: test accounts (T-116); podnet live step |
| A-N-02 | P0 | Apply a remote config (HTTP/2 push, QUIC `UpdateConfiguration`); refuse it with a local config | deferred: connector (T-113) |
| A-N-03 | P0 | `/ready` and `/metrics` of the connector | deferred: connector (T-113, T-103) |
| A-N-04 | P1 | Freeze `doctor --json` schema 1, with `--quick` | podnet |
| A-N-05 | P0 | A signed podnet prerelease with a stable connector profile | podnet (a beta is allowed; T-104 gives the index) |
| A-N-06 | P1 | Honour `originRequest.access`, or refuse with exit 78 | deferred: connector (T-113) |
| A-N-07 | P1 | Access client for TCP: a local listener with a service token file | T-117; podssh (the command) |
| A-N-08 | P1 | Zero Trust enrolment with a service token; Mesh node mode | T-115; live part deferred: test accounts |
| A-N-09 | P1 | Prove a Workers VPC service binding to a podnet tunnel | deferred: test accounts (T-116) |
| A-N-10 | P1 | Raise the rate of `quic+warp` to 50 % of direct `quic` | T-115 |
| A-N-11 | P1 | A hardened systemd system unit for the connector | T-102; podnet (the command) |
| A-N-12 | P1 | `relay node` as a system service with pair renewal and resume | podnet (`PodUtils/podnet@T-087`); T-102 |
| A-N-13 | P0 | The job egress proxy of cfb (per-job CA, placeholders, scrub, deny file) | T-099, T-100 |
| A-N-14 | P2 | A pool service in the ingress | deferred: connector (T-114) |
| A-N-15 | P2 | `podnet bench URL --json` and a live step | podnet (the command); live step deferred: test accounts (T-116) |
| A-N-16 | P2 | Post-quantum key exchange on the edge legs | deferred: connector (T-114) |

## A-S: podssh

| Ask | P | Ask | Route |
| --- | --- | --- | --- |
| A-S-01 | P0 | `each`, `run`, `job` take `node://`, net names, `iroh:`, `derp:` | podssh |
| A-S-02 | P0 | A node as a system service of a named user, hardened, with no token in the unit | podssh; T-102 |
| A-S-03 | P0 | A node reachable for months: pair renewal and legs by key, in a release | podssh (a beta is allowed; `PodUtils/podssh@T-446`); T-025 |
| A-S-04 | P0 | One switch `PODSSH_PUBLIC` over each third-party service; `status --json` gives the set | podssh; T-112 |
| A-S-05 | P1 | Liveness that lets a Durable Object relay hibernate | podssh; T-111 |
| A-S-06 | P1 | `podssh relay conform --relay-host HOST --json` | podssh |
| A-S-07 | P1 | Node route to a Unix socket (`unix-connect:PATH`) | podssh |
| A-S-08 | P1 | Node door from a signed net document at an HTTPS URL | podssh |
| A-S-09 | P1 | Public parts of each key as JSON, offline | podssh |
| A-S-10 | P1 | Probe each road to a list of nodes as JSON | podssh |
| A-S-11 | P1 | Local egress daemon for the agent: HTTP CONNECT on a Unix socket | T-099 (the mode with no TLS end); podssh hosts the `main` |
| A-S-12 | P2 | `relay serve` serves the iroh relay protocol and DERP on one listener | podssh |
| A-S-13 | P2 | An exit on the reverse road that a Worker calls, with a signed allowlist | podssh (the fixed denies use the policy of T-099) |
| A-S-14 | P2 | A lease (`X-Relay-Lease`) in place of a token on the forward and reverse roads | podssh; T-111 |
| A-S-15 | P2 | A versioned wire document with test vectors | T-120 |
| A-S-16 | P2 | `-J node://NAME` and `mcp` destinations that name a node | podssh |
| A-S-17 | P3 | One-way carrier through an S3 bucket | podssh |
| A-S-18 | P2 | A local forward as a service; the measured rate of the relay role | podssh; T-102 |

## A-V: podvm

| Ask | P | Ask | Route |
| --- | --- | --- | --- |
| A-V-01 | P0 | A confine layer from one profile: Landlock, seccomp deny filter, `no_new_privs`, rlimits | podvm (`PodUtils/podvm@T-1658`); T-108 |
| A-V-02 | P0 | Hold a payload to one egress: each `connect` goes to a named upstream | podvm (`PodUtils/podvm@T-1663`, `PodUtils/podvm@T-1665`); T-109 |
| A-V-03 | P0 | The policy of the egress proxy of cfb | T-099, T-100; podvm hosts it as `--net broker` |
| A-V-04 | P1 | Node CI job from an OCI image with no root, no userns, no mount | podvm (`PodUtils/podvm@T-1643`); T-059 later |
| A-V-05 | P2 | Headless Chromium of Playwright; a `workerd` profile with no network | podvm |
| A-V-06 | P1 | `-v HOST:GUEST` through the path map, `:ro` with `EROFS` | podvm |
| A-V-07 | P1 | `--ulimit`, a run timeout that kills the tree, peak RSS and CPU in the report | podvm; T-060 |
| A-V-08 | P1 | A microVM from an OCI image through `run`, with KVM | podvm (`PodUtils/podvm@T-1656`); T-062 |
| A-V-09 | P1 | A JSON report of each run, and real labels | podvm |
| A-V-10 | P2 | A published JSON schema of `podvm probe --json` | T-107; podvm |
| A-V-11 | P2 | `--secret src=,target=,mode=0400` with no argv and no env | podrunner (podvm dropped its T-1696, PodUtils/podvm#241: podrunner stages the credential files) |

## A-C: the Cloudflare One side

| Ask | P | Ask | Route |
| --- | --- | --- | --- |
| A-C-01 | P0 | Named tunnel on TCP 443 only as a production lane | deferred: connector (T-113, T-115) |
| A-C-02 | P0 | Drop-in contract for the `cloudflared` unit: drain, `--metrics`, no privilege | deferred: connector (T-113, T-102, T-103) |
| A-C-03 | P1 | Headless enrolment in a Zero Trust team with a service token | T-115 |
| A-C-04 | P1 | Access service-token headers from files on every WebSocket road | T-111; podssh and podnet (the commands) |
| A-C-05 | P1 | A Mesh node in user space | T-115 |
| A-C-06 | P1 | Serve Workers VPC flows over the 443 leg with an allow list | deferred: connector (T-114) |
| A-C-07 | P1 | A stable answer to a revoked credential: exit code 78, no restart loop | T-101; T-102; each product |
| A-C-08 | P0 | A held link for the node agent | T-110, T-120; podssh (`podssh link`) |
| A-C-09 | P2 | SSH login with a short-lived Access certificate over `cf://` | podssh |
| A-C-10 | P1 | Fixed JSON schema of the egress lanes | podnet |
| A-C-11 | P3 | Device posture results for a team device | T-115 |
| A-C-12 | P2 | A weekly throughput bench of each leg | deferred: test accounts (T-116) |

## A-X: new projects and cross-cutting asks

| Ask | P | Ask | Route |
| --- | --- | --- | --- |
| A-X-01 | P0 | `podrunner`: ephemeral CI runners | new project podrunner |
| A-X-02 | P1 | `podnode`: the one node agent | new project podnode |
| A-X-03 | P1 | `podinit`: one command that turns a fresh server into a node | new project podinit |
| A-X-04 | P1 | `podrelay-worker`: the podssh relay as a Cloudflare Worker | new project podrelay-worker (after T-036 and T-120) |
| A-X-05 | P1 | One egress proxy with credential injection | T-099, T-100 |
| A-X-06 | P1 | Ingress for a self-hosted Workers runtime | deferred: connector (T-114) |
| A-X-07 | P2 | `podsnap`: encrypted append-only backup shipper | new project podsnap |
| A-X-08 | P2 | `podkey`: in-memory secret agent with a time window | new project podkey |
| A-X-09 | P2 | `podstat` (the ask named it `podwatch`): outside prober and heartbeat | new project podstat |
| A-X-10 | P2 | `podlog`: ship logs and metrics over 443 | new project podlog |
| A-X-11 | P3 | `podrepl`: SQLite replication between nodes | new project podrepl |
| A-X-12 | P0 | Windows and macOS builds of each operator command | podssh (podnet stays Linux only) |
| A-X-13 | P0 | Signed releases and a JSON release index in each repository | T-104 |
| A-X-14 | P1 | Acceptance registry: each ask id maps to a named CI test | T-105 |
| A-X-15 | P1 | One local control API on a Unix socket or a named pipe | T-103 |
| A-X-16 | P1 | TypeScript packages for bun and Workers | deferred: no npm package (schemas in T-101, T-103, T-104, T-107) |
| A-X-17 | P1 | One convention for exit codes and one JSON log schema | T-101 |
| A-X-18 | P1 | Test doubles on loopback: fake edge, relay, GitHub, control plane | T-106; the new projects own their fakes |
| A-X-19 | P2 | `llms.txt`, `llms-full.txt` and `--help --json` for each tool | podssh, podnet, podvm |
| A-X-20 | P2 | MCP servers for fleet work with read-only mode | podssh |
| A-X-21 | P2 | Fuzz targets, cargo-deny, cargo-audit, an SBOM | T-118; each repository |
| A-X-22 | P2 | One CI matrix of restricted hosts | T-119 |
| A-X-23 | P2 | Performance targets with a bench in CI | deferred: test accounts (T-116) |
| A-X-24 | P2 | Umbrella installer: install, pin, verify, update, roll back | new project podutils (after T-104) |
| A-X-25 | P1 | Host qualification `--role origin\|compute --json` | new project podnode (`podnode qualify`); T-107 |
| A-X-26 | P2 | Rate limit and byte meter for each hostname or route | deferred: connector (T-114) |
| A-X-27 | P1 | Many tunnels in one process; upgrade with no downtime | deferred: connector (T-114) |

## G: githome (#36)

Each G ask is for the project podrunner.

| Ask | P | Ask | Route |
| --- | --- | --- | --- |
| G-01 | P0 | A job-source contract: GitHub and a forge at once, with a slot quota for each | new project podrunner |
| G-02 | P0 | A unit kind for each label (`container`, `microvm`, a podvm rung); no fallback | new project podrunner; podvm (A-V-08) |
| G-03 | P0 | The one-job runner credential reaches only the runner process | podrunner (A-V-11; restated in PodUtils/podrunner#6) |
| G-04 | P0 | A policy for each job: open by default, fixed denies, audit line, placeholder swap | T-099, T-100; new project podrunner delivers the policy |
| G-05 | P1 | Placement by architecture; no emulation unless a label asks | new project podrunner |
| G-06 | P1 | One JSON report for each job to its source | new project podrunner; podvm (A-V-07, A-V-09) |
| G-07 | P1 | Local caches kept apart for each trust level | new project podrunner; podvm (A-V-06) |
| G-08 | P1 | A capacity signal, a withdraw call and `podrunner drain` | new project podrunner; T-103 |
| G-09 | P1 | A container engine inside a unit for `services:` and Docker actions | new project podrunner; podvm (A-V-04) |
| G-10 | P1 | A fake forge job source in memory | new project podrunner; T-106 |
| G-11 | P2 | Warm units from an image pinned by digest, with SBOM and signature | new project podrunner; T-104 |
| G-12 | P2 | Limits from the source; end and destroy in 60 s | new project podrunner; podvm (A-V-07) |
