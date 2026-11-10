This file holds the entries of milestone M8: the shared crates for the asks of GitHub #35 (cfb) and #36
(githome). `docs/research/fleet/asks.md` routes each ask. Each crate follows the procedure of
`docs/design.md`, section 8.

# T-099: `pod-broker`: the egress broker: per-job policy, one resolve, the upstream chain, the audit and the drop (GitHub #35, #36)

**Source:** GitHub #35 (A-X-05, A-V-03, A-N-13, A-S-11) and GitHub #36 (G-04)
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** L
**Status:** done

## Problem

cfb (A-X-05, A-V-03, A-N-13, A-S-11) and githome (G-04) need one egress proxy for a job. cfb wants an
allow-list by default. githome wants open egress by default with fixed denies. `pod-egress` names the roads
of T-035, so the broker is a new crate (call C25).

## Premise

- Read: the vocabulary of PodUtils/podvm#184 (407, 403, 502, 504), the rules of `podnet pod` and `warp proxy`.
- Not read: a TLS end in any of them. T-100 adds it.
- Not measured: the broker.

## Approach

Add the crate. A policy schema with two modes: allow-list, and open with fixed denies that no policy lifts
(metadata, private, link-local, host addresses, `fd00:ec2::254`). Host and path prefix on whole segments.
One resolve with a deny file. The CONNECT target, the Host header and the SNI must agree (421). An ambiguous
target gets 400. A chain to the upstream proxy. One JSON audit line for each connection with job id, host,
port, decision and bytes, and no secret value. `load` and `drop` on a Unix socket with JSON lines. Drop at
the TTL. The crate is a library: each product hosts it in a thin `main` (call C27).

## Prove

```sh
cargo test -p pod-broker --all-features
sh scripts/agent/check.sh
```

## Done

2026-11-09, the commit of this entry. New crate `crates/pod-broker` (library only, C27): the policy of
version 1 in JSON and TOML (`schemas/broker-policy.v1.schema.json`), the fixed denies, the denies of the
host and its deny file, the strict reading of a target (400), 421 on a `Host` that differs, 407, 403, 502
and 504, one resolve with a dial of the checked addresses only (C28), the upstream chain through
`pod_proxy::Dialer`, the jobs with `load`, `drop`, `list` and the TTL on a control socket of mode 0600,
one JSON audit line for each connection with no secret value (C31), and the seam `TunnelEnd` for T-100
(C29). The fuzz target `broker_target` of the request target. The static probe calls the crate.
`cargo test -p pod-broker --all-features`: 55 passed (8 unit, 47 in `tests/`), 30 runs in a loop with no
failure. The planted path in an audit line fails `no_audit_line_holds_a_planted_secret`.
`cargo test --manifest-path fuzz/Cargo.toml --lib`: 2 passed. `sh scripts/agent/check.sh`: fmt 0,
clippy 0, doc 0, record 0, size 0, test 0; the static build waits for CI (no zig here), and the static probe
runs on the host with the line of `pod-broker`.

# T-100: `pod-broker`: TLS end with a CA for each job, placeholder credentials and the scrub of answers (GitHub #35, #36)

**Source:** GitHub #35 (A-X-05, A-V-03, A-N-13) and GitHub #36 (G-04)
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** L
**Status:** open

## Problem

The swap of a placeholder credential for the real one needs the TLS end of the broker. The job must trust
the CA of the broker and must never see the real value.

## Premise

- Read: the asks in `docs/research/fleet/asks.md`.
- Not measured: the swap.

## Approach

A CA for each job, written as a bundle file; the variables `SSL_CERT_FILE`, `GIT_SSL_CAINFO`,
`NODE_EXTRA_CA_CERTS` and `REQUESTS_CA_BUNDLE` name it. Swap the placeholder in a header, in Basic
authentication, in a query and in a body. Put the placeholder back in answers. A test plants a secret and
proves that no log line holds it. A fuzz target of the request target.

## Prove

```sh
cargo test -p pod-broker --all-features
sh scripts/fuzz.sh 60
```

## Start condition

T-099 is done.

# T-102: `pod-unit`: hardened service units for systemd, launchd and Windows, with no secret in a unit (GitHub #35)

**Source:** GitHub #35 (A-N-11, A-S-02, A-S-18, A-N-12, A-C-02, A-C-07)
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** M
**Status:** open

## Problem

podssh and podnet each write their own units (user units, crontab, LaunchAgent, Windows). cfb needs system
units with a named user and hardening, and no token in a unit.

## Premise

- Read: `PodUtils/podssh:crates/podssh-cli/src/node_install/` and `PodUtils/podnet:src/autostart.rs` in the asks.
- Not measured: `systemd-analyze security` of a unit.

## Approach

Import `node_install/` of podssh as the base, and merge `autostart.rs` of podnet. Add system units with
`User=`, `NoNewPrivileges`, `ProtectSystem=strict`, an empty capability set, `EnvironmentFile` or a
credential file, and `RestartPreventExitStatus`. Render to text for `--print-unit`. Uninstall leaves
nothing. The command line stays in each product (call C27).

## Prove

```sh
cargo test -p pod-unit --all-features
sh scripts/agent/check.sh
```

A test plants a token and fails. `ci.yml` runs `systemd-analyze verify`.

# T-103: `pod-control`: one JSON control API on a Unix socket or a named pipe (GitHub #35, #36)

**Source:** GitHub #35 (A-X-15, A-N-03) and GitHub #36 (G-08)
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** M
**Status:** open

## Problem

Each long-running tool needs the same local control API. cfb must not parse human output. G-08 needs
`drain`.

## Premise

- Read: the JSON of podnet `src/status.rs` (schema 1) and podssh `status`.
- Not measured: a client against two tools.
- podrunner is a consumer (PodUtils/podvm#227 section 2.7). It needs `drain` with jobs in flight (no new claim, each running job ends, then exit), and `status` that names each slot, its boundary, its hold and its principal (SPEC section 11.5 of the podrunner design review). One protocol in a process: say whether `pod_broker::control` stays a second protocol. podrunner uses its own module until this entry lands.

## Approach

JSON lines with a schema version in each answer. The methods `version`, `status`, `metrics`, `reload`,
`drain` and `stop`. One error code for an unknown method. A check of the peer user. A Unix socket and a
Windows named pipe. The JSON Schema goes to `schemas/`. A conformance test that a consumer runs.

## Prove

```sh
cargo test -p pod-control --all-features
sh scripts/agent/check.sh
```

# T-104: `pod-release`: the release index, its writer for CI and its verifier (GitHub #35, #36)

**Source:** GitHub #35 (A-X-13, A-X-24, A-X-02) and GitHub #36 (G-11)
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** M
**Status:** open

## Problem

cfb pins each file by SHA-256 and by the asks that a release passes. podnet and podssh each verify a
Sigstore bundle of `SHA256SUMS` in their own code. The installer `podutils` and `podnode` need one index.

## Premise

- Read: podnet `src/update.rs` and podssh `update/sigstore.rs` in the asks.
- Not measured: the verifier.

## Approach

One index schema (version, date, targets, file SHA-256, signature, SBOM link, acceptance). The writer is a
shared CI script. The verifier comes from podnet and podssh, with the release identity as a setting (the
ruling "Services: what stays" keeps the identity with each consumer). The JSON Schema goes to `schemas/`.

## Prove

```sh
cargo test -p pod-release --all-features
sh scripts/agent/check.sh
```

A test changes one byte and the verifier fails.

# T-106: `pod-testkit`: the stand-ins of the edge, the relay and the WARP API as binaries for a consumer's CI (GitHub #35, #36)

**Source:** GitHub #35 (A-X-18) and GitHub #36 (G-10)
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

cfb and podrunner need loopback stand-ins as programs, so that their CI drives real binaries with no
internet route.

## Premise

- Read: podssh `scripts/fake-*.py`, the test host of podnet, T-028 (it moves the WARP stand-ins).
- Not measured: a stand-in as a binary.

## Approach

A feature of `pod-testkit` builds the stand-ins of the edge, the relay and the WARP API as binaries, with
documented ports and fixtures. Each speaks bytes and HTTP only (`docs/design.md`, section 4). The fake
GitHub API and the fake control plane stay in their projects. The HTTP base serves the fake forge job
source of G-10.

## Prove

```sh
cargo test -p pod-testkit --all-features
sh scripts/agent/check.sh
```

## Start condition

T-025, T-028 and T-029 moved their stand-ins to `pod-testkit`.

# T-146: `podrunner`: run GitHub CI and other CI on any machine (the plan, GitHub #47)

**Source:** the owner, 2026-11-10 ("the tool that lets me run GitHub or other CI on any machine"); GitHub #47; A-X-01 of #35; G-01 to G-12 of #36.
**Category:** research
**Milestone:** M8
**Priority:** P2
**Effort:** L
**Status:** open

## Problem

The owner asks for one tool that runs GitHub CI, and other CI, on any
machine: a host with no root, no namespaces, no docker daemon and a proxy
for the only way out.

## Premise

- Read: `docs/research/ci/podrunner-plan.md` (the plan, with its sources);
  `docs/research/fleet/asks.md`; podvm's parity table.
- Not measured: the official runner under podvm (phase 0 of the plan).

## Approach

The plan and GitHub #47 give the phases with their checklists:

- Phase 0 (measure) and the podvm items of phase 1 are podvm work: they
  go now, in podvm's record.
- `podrunner` (a static launcher of the official `actions/runner`, one
  fresh podvm unit for each job) and the crate `pod-dockerapi` (a Docker
  Engine API subset over podvm, one socket for each job) wait for Q8 to
  Q14 (`TODO/PROGRESS.md`).
- A native Rust runner is a conditional phase 5.

## Prove

The checklist of GitHub #47: each item gets its own entry and proof. The
plan passes the path check:

```sh
python3 -I scripts/check-design-paths.py
```

# T-160: `pod-broker`: a listener bound to one job, with no token in the unit (PodUtils/podvm#227)

**Source:** PodUtils/podvm#227 section 2.1 (COORD-021 of podvm, 2026-11-10)
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

podrunner (a project that runs CI jobs, GitHub #47) holds each unit's egress on one socket: the socket is the identity of the unit, and no job token reaches a unit on the `advisory` hold. `Broker::serve_unix` and `Broker::serve_tcp` serve all jobs and read the job from `Proxy-Authorization` on each connection. `podnet pod -x http://unix:/PATH` sends no token. A held payload cannot name its job unless a token is in its environment. The design review of podrunner allows a token in the environment only for hold `none`. The consumers are podrunner (its egress module) and podvm (`--net broker`, PodUtils/podvm@T-1665).

## Premise

- Read, at `5cedb23`: `crates/pod-broker/src/serve.rs:158` is `lease`. It takes the job from `Proxy-Authorization` (`Bearer`, or Basic with the token as the password) and refuses with 407 when the header is missing. The issue cites lines 155 to 175. The function runs from line 158 to line 182. The call is at `crates/pod-broker/src/serve.rs:100`.
- Read: `crates/pod-broker/src/broker.rs:145` (`serve_tcp`) and `crates/pod-broker/src/broker.rs:161` (`serve_unix`) each serve every job. No call binds one listener to one job. `Broker::drop_job` (`crates/pod-broker/src/broker.rs:128`) ends the connections of a job.
- Read: `crates/pod-broker/README.md` writes the unix upstream as `http+unix:PATH` at line 47. `PodUtils/podnet:crates/podnet-netns/src/upstream/tests.rs` parses `http://unix:/PATH` and `socks5://unix:///PATH`. The README form does not parse in `podnet pod -x`: podnet's `Proxy::parse` in `PodUtils/podnet:crates/podnet-netns/src/upstream.rs` has no `http+unix:` prefix. The documentation defect is fixed in the commit that adds this entry (see the Done line of the COORD issue).
- Read: section 0 item 4 of the issue says that SPEC section 9.5 of the design review writes `socks5://unix:PATH`. `pod-broker` speaks HTTP CONNECT and absolute-form HTTP only, not SOCKS5. podnet parses a SOCKS5 unix proxy, so the text parses but the broker does not answer SOCKS5. This is a note for the spec, not for podcrates. The working form is `http://unix:/PATH`.
- Not measured: the listener. Not read: the design review (the owner holds the zip).

## Approach

Add `Broker::serve_unix_job(job, listener)`, and the same for TCP on loopback if the host needs it. Each connection on that listener belongs to that job. The broker does not read `Proxy-Authorization` on it. `drop_job` closes the listener and its connections. `serve_unix` and `serve_tcp` stay as they are. The audit line follows call C31: it names the job and holds no path, query or header. The README names `http://unix:/PATH`.

## Prove

```sh
cargo test -p pod-broker --all-features
sh scripts/agent/check.sh
```

The tests that the issue names:

1. A connection on the listener of job A that sends no `Proxy-Authorization` gets the policy of job A. A header with the token of job B on that listener is refused with 407 and audited.
2. `drop_job` of A ends the listener of A and each open connection. A new connect is refused.
3. Two job listeners in one broker do not share a policy. A planted wrong policy fails the test.
4. The audit line names the job and holds no path, query or header (call C31).
5. The README names `http://unix:/PATH`.

## Start condition

The owner names the plan of GitHub #47 (DR-1) and the place of the code (DR-2). The ruling "CI tracker" (2026-11-10) leaves #47 as it is, and the ruling "Priority" (2026-11-09) keeps M8 waiting. The owner can lift the wait for this entry.

Depends on T-099 (done). No other crate.

# T-161: `pod-broker`: send the name to an upstream that takes names only (PodUtils/podvm#227)

**Source:** PodUtils/podvm#227 section 2.2 (COORD-021 of podvm, 2026-11-10)
**Category:** feature
**Milestone:** M8
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

A proxy-only host can have an upstream CONNECT proxy that takes names only. Two measured sandbox hosts break the current chain: a proxy that resets CONNECT to an IP literal (`PodUtils/podnet:docs/target-environment.md`), and a host with no working resolver where the proxy allows ports 443, 80 and 8443 (`PodUtils/podssh:docs/target-environment.md`). The broker sends the checked address, so it cannot reach the hosts of the runner there. This is inferred and not run (the design review, unknown U10). The consumer is podrunner on proxy-only hosts.

## Premise

- Read: `crates/pod-broker/src/broker.rs:185` (`upstream` takes one `Option<pod_proxy::Proxy>` and dials the checked address) and `crates/pod-broker/src/serve.rs:274` (`connect` dials `Target::Addr`). The issue cites `serve.rs:273-287` and `broker.rs:183-191`. The code agrees to within a few lines.
- Read: call C28 says that through an upstream proxy the broker sends the checked address and never the name. This entry changes C28 for an upstream that takes names only. The default stays the checked address. Write the change as a new call when the work starts.
- Not read: `APPENDIX-DEFERRED.md` of the design review (the owner holds the zip). Not run: the two sandbox hosts.

## Approach

When the upstream is a CONNECT proxy that takes names only, send the name in the CONNECT. Check the name against the deny list and against one `pod-dns` resolve before the dial. The four rules of the design review:

1. At start, send a canary name that resolves to a non-public address. Only if the upstream refuses it, report the upstream as "refuses non-public". Else name-forwarding runs as hold `advisory` and never serves `untrusted`.
2. Flag an allow-list entry under a suffix that anyone can register.
3. Test a resolver that rebinds a name between the pre-check and the dial.
4. Test against a copy of the names-only proxy fixture (`PodUtils/podssh:scripts/box/proxy.py` or `PodUtils/podnet:scripts/box/proxy.py`, both 0BSD).

Add a fuzz target if a new parser comes in.

## Prove

```sh
cargo test -p pod-broker --all-features
cargo test --manifest-path fuzz/Cargo.toml --lib
sh scripts/agent/check.sh
```

The tests are the four rules above: the canary, the flagged suffix, the rebinding resolver and the copy of the proxy fixture.

## Start condition

The owner names the plan of GitHub #47 (DR-1) and the place of the code (DR-2). The ruling "CI tracker" (2026-11-10) leaves #47 as it is, and the ruling "Priority" (2026-11-09) keeps M8 waiting. The owner can lift the wait for this entry.

Depends on T-099 (done) and `pod-dns`. Not on T-100.

# T-163: Schemas and types: job offer, unit capability, run report and public report (PodUtils/podvm#227)

**Source:** PodUtils/podvm#227 section 2.4 (COORD-021 of podvm, 2026-11-10)
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

githome (GitHub #36) and cfb (GitHub #35) must pin one schema for the work of a runner (the design review: single point S14, ruling Q-A8). `schemas/` holds `log.v1.schema.json` and `broker-policy.v1.schema.json` only. The design review holds drafts of four schemas with valid and invalid examples and a checker. The consumer is podrunner, which keeps its own copies until this lands.

## Premise

- Read: `schemas/` holds two files, `log.v1.schema.json` and `broker-policy.v1.schema.json`. This agrees with the issue.
- Not read: the drafts of the design review (the owner holds the zip).
- Not measured: the examples.

## Approach

Add four JSON Schemas, each version 1: `job-offer`, `unit-capability`, `run-report` and `run-report-public`. Add serde types for them behind a feature, in a crate that podcrates chooses. Reserve no new crate name before the owner reserves one. Start after podrunner's traits exist, so that the types follow the contracts. Do not do the renumbering of questions that the design review proposes: that edits #47, and #47 stays as it is.

## Prove

```sh
cargo test --workspace --all-features
sh scripts/agent/check.sh
```

The tests that the issue names:

1. Each valid example of the design review passes. Each invalid one fails for the reason that its `EXPECT.json` names.
2. The offer schema refuses a `payload` from a `github-scaleset` source, a credential in the environment, an image by tag, and a tier sent by a source.
3. The public report holds no host name and no boundary detail.
4. The log schema of `pod-core` and these schemas use one `schema` field rule.

## Start condition

The owner names the plan of GitHub #47 (DR-1) and the place of the code (DR-2). The ruling "CI tracker" (2026-11-10) leaves #47 as it is, and the ruling "Priority" (2026-11-09) keeps M8 waiting. The owner can lift the wait for this entry.

Podrunner's WP-01 (the traits) comes first.
