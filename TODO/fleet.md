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

**Source:** GitHub #35 (A-X-05, A-V-03, A-N-13), GitHub #36 (G-04) and GitHub #223 (githome's G-35: a placeholder in the `policy` of an offer, swapped at the proxy)
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** L
**Status:** done

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

## Done

2026-11-19, this commit. The TLS end of `pod-broker` (calls C810 to C817):

- The policy: the field `credentials` of `broker-policy.v1` (`placeholder`, `secret`, `hosts`, `in`;
  row `next` of `schemas/CHANGES.md`); `pod-job` takes the key in the `policy` of an offer (the example
  `valid-policy-credentials.json`). The values come at the load (`Load::secrets`, `secrets` of the
  control line), never in a policy.
- The CA of a job (`ca::JobCa`: P-256 keys of `pod-acme`, rcgen with no crypto of its own), its leaves
  with the key id of the CA, `ca::bundle`, `ca::write_bundle`, `ca::env` (the six variables of
  `pod_tls::BUNDLE_VARS`, with `SSL_CERT_FILE`, `GIT_SSL_CAINFO`, `NODE_EXTRA_CA_CERTS` and
  `REQUESTS_CA_BUNDLE`); `JobInfo::ca_pem`, `Broker::ca_pem` and `ca_pem` in the load answer.
- `TlsEnd::new(provider, client_config)`: the tunnels to a host of a credential or to a host whose rules
  have paths, when the job speaks TLS first; the checks of the SNI, the `Host`, the target and the path;
  the swap in a header, Basic, a query and a body; the scrub of each answer (header, body by length,
  chunks, to the close, after 101); the bounds of C814; the audit fields of C817.
- Tests: `cargo test -p pod-broker --all-features`: 19 unit and 90 in `tests/` passed, 3 runs with no
  failure (`tests/tls_swap.rs` 11, `tests/tls_scrub.rs` 4, `tests/credentials.rs` 6, among them
  `a_planted_secret_is_in_no_log_line_no_audit_line_no_answer_and_no_error` and
  `a_leaf_verifies_with_openssl_in_its_strict_mode`). `cargo test -p pod-job --all-features`: 25 passed.
- The fuzz target `broker_inner` (`inner::read` and `replace::Replace` cut in two pieces): `cargo test
  --manifest-path fuzz/Cargo.toml --lib` 14 passed; it builds on `nightly-2026-11-18` with the flags of
  cargo-fuzz.
- Plants: `broker-tls-answer-not-scrubbed` (the real value in an answer), `broker-tls-swap-for-any-host`
  (a swap for a host outside the credential) and `broker-tls-sni-not-checked`; each fails its test.
- Found and written as entries: T-972 (`pod-lab` keeps its own P-256 key and PEM writer) and T-973 (the
  bundle of a job with curl, git, Python and Node.js in a lab).

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

From the dig study (T-833; reports 03, 11 and 12, `docs/research/dig-study.md`):

- One catalogue of methods, an enum with the wire name, the routing (local or delegated), a summary and the tier of each method. The dispatch routes on it, and an unknown name gets the one error code. Tests in both directions: each method in the catalogue is served, and each served method is in the catalogue; the local and the delegated sets do not overlap. Discovery and the JSON Schema are made from the catalogue.
- A tier for each method (read, operate, the master token). A refusal does not tell that a control method exists to a caller of a lower tier. A method whose effect outlives its token (a new principal, a kept forward target) takes the highest tier.
- A duplex frame: a line transport with one cap of length, a check of UTF-8 and an outcome for a line that is too long; requests of the service on the same connection, demultiplexed by id; a budget of callbacks for each request and a total for each connection, then a fault; a read timeout.
- Plants: a method that is served and not in the catalogue, a lower tier for a master method, a raised budget.

## Prove

```sh
cargo test -p pod-control --all-features
sh scripts/agent/check.sh
```

# T-104: `pod-release`: the release index, its writer for CI and its verifier (GitHub #35, #36)

**Source:** GitHub #35 (A-X-13, A-X-24, A-X-02), GitHub #36 (G-11) and GitHub #223 (githome's G-34: one signed index in each release of each product, and one verifier)
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** M
**Status:** done

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

## Done

2026-11-19, commits `aa860780` (the import) and `a9a36981` (the crate), and this commit (the record).
The crate `crates/pod-release` (calls C793, C794):

- The index: `release-index.v1` (`schemas/release-index.v1.schema.json`, generated from
  `pod_release::index::Index` by a test; its row `next` in `schemas/CHANGES.md`): the project, the
  tag, the date, the commit, the signature (`SHA256SUMS`, its bundle, the identity, the issuer),
  each file with its SHA-256, its size and its target, the SBOM and the acceptance.
- The writer: `scripts/release-index.py` (call C793), with its self-test in `ci.yml` and
  `scripts/agent/check.sh`. `release.yml` writes `release-index.json` before `SHA256SUMS`, which
  holds its line, from the next tag.
- The verifier: `sigstore::verify_with` (the bundle, with the caller's `Identity` of the workflow URI
  and the OIDC issuer, Rekor's promise and the entry's body, the chain to Fulcio at the time of the
  entry), `verify_release`, `Verified::check`, `Verified::index`, `verify::verify_dir`.
- Proof: `cargo test -p pod-release --all-features`: 53 passed. The published `SHA256SUMS`,
  `SHA256SUMS.sigstore.json` and `podlab-install.sh` of podcrates v0.76.0 verify
  (`crates/pod-release/tests/fixtures/v0.76.0/`); one changed byte of `podlab-install.sh`, of
  `SHA256SUMS` and of each checked value of the bundle fails, and so does another tag, repository,
  workflow or issuer. The plants `release-no-signature` and `release-any-identity` are caught.
- Remains, each in an entry of its own: the bundle of Rekor v2 (T-964), the verifier as a command
  for a CI that is not Rust and for an installer (T-965), the trials of the consumers and the
  hand-off (T-966), minisign (T-967), the floor of rollback in the index (T-830), the acceptance
  file (T-105).

# T-106: `pod-testkit`: the stand-ins of the edge, the relay and the WARP API as binaries for a consumer's CI (GitHub #35, #36)

**Source:** GitHub #35 (A-X-18) and GitHub #36 (G-10)
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** M
**Status:** partial

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

## Done

2026-11-19, the G-10 part (call C792): `pod-fake-github` (T-921) and `pod-fake-githome` (T-948) are the fake sources of forge jobs that podrunner and githome pin, each a static program of each tag with its image on ghcr.io. The A-X-18 part waits for cfb's interface of each stand-in program, asked on #223 (the reply comment to cfb).

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
  fresh podvm unit for each job) is the work of PodUtils/podrunner (index
  PodUtils/podrunner#1; the ruling "podrunner online", C49). podcrates
  gives its crates (T-160 to T-165). The crate `pod-dockerapi` (a Docker
  Engine API subset over podvm, one socket for each job) comes with phase
  3 (Q10: decide at phase 3). The owner answered Q9 to Q14 on 2026-11-11
  (`docs/decisions.md`); Q11 puts a local mode (`podrunner run`) into the
  plan, with the native runner (`pod-actions-*`) as the way to it.
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
**Status:** done

## Problem

podrunner (a project that runs CI jobs, GitHub #47) holds each unit's egress on one socket: the socket is the identity of the unit, and no job token reaches a unit on the `advisory` hold. `Broker::serve_unix` and `Broker::serve_tcp` serve all jobs and read the job from `Proxy-Authorization` on each connection. `podnet pod -x http://unix:/PATH` sends no token. A held payload cannot name its job unless a token is in its environment. The design review of podrunner allows a token in the environment only for hold `none`. The consumers are podrunner (its egress module) and podvm (`--net broker`, PodUtils/podvm@T-1665).

## Premise

- Read, at `5cedb23`: `crates/pod-broker/src/serve.rs:180` is `lease`. It takes the job from `Proxy-Authorization` (`Bearer`, or Basic with the token as the password) and refuses with 407 when the header is missing. The issue cites lines 155 to 175. The function runs from line 158 to line 182. The call is at `crates/pod-broker/src/serve.rs:119`.
- Read: `crates/pod-broker/src/broker.rs:178` (`serve_tcp`) and `crates/pod-broker/src/broker.rs:194` (`serve_unix`) each serve every job. No call binds one listener to one job. `Broker::drop_job` (`crates/pod-broker/src/broker.rs:155`) ends the connections of a job.
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

None. The owner named podrunner on 2026-11-11 (the ruling "podrunner online"; call C49: DR-1 and DR-2 are yes). Take the entry when podrunner's work needs it (`TODO/PROGRESS.md`, Work order).

Depends on T-099 (done). No other crate.

## Done

2026-11-11. `Broker::serve_unix_job(job, listener)`,
`Broker::serve_tcp_job(job, listener)` and `Broker::handle_bound(io, job)`;
`Jobs::lease_of` and `Jobs::watch`. A connection on a bound listener is the
job's with no `Proxy-Authorization`; a header must name the same job, else
407 with the audit line of the listener's job. A reload of the job keeps
the listener (it serves the new policy); a drop or the TTL ends it, with
each connection, and writes `broker.listener.end`. A job that is not
loaded is refused (`NotFound`). The README names the bound listener and
`http://unix:/PATH`.

- Tests (`tests/bound.rs`, 8): the 5 of the issue are
  `a_connection_is_the_job_of_its_listener_with_no_token`,
  `a_drop_ends_the_listener_and_its_connections`,
  `two_listeners_keep_two_policies`,
  `the_audit_line_names_the_job_and_no_path_query_or_header` and
  `the_readme_names_the_unix_form_that_podnet_parses`; also
  `a_reload_keeps_the_listener_with_the_new_policy`,
  `a_listener_of_a_job_that_is_not_loaded_is_refused`,
  `a_tcp_listener_on_loopback_is_bound_the_same_way`.
- Plant `broker-bound-listener-reads-the-token` (a bound listener that
  reads the token, as an unbound one): 5 of the 8 tests fail.
- Next: podrunner's egress module (PodUtils/podrunner#24) and podvm's
  `--net broker` (PodUtils/podvm@T-1665) take it with the next tag.

# T-161: `pod-broker`: send the name to an upstream that takes names only (PodUtils/podvm#227)

**Source:** PodUtils/podvm#227 section 2.2 (COORD-021 of podvm, 2026-11-10)
**Category:** feature
**Milestone:** M8
**Priority:** P3
**Effort:** M
**Status:** done

## Problem

A proxy-only host can have an upstream CONNECT proxy that takes names only. Two measured sandbox hosts break the current chain: a proxy that resets CONNECT to an IP literal (`PodUtils/podnet:docs/target-environment.md`), and a host with no working resolver where the proxy allows ports 443, 80 and 8443 (`PodUtils/podssh:docs/target-environment.md`). The broker sends the checked address, so it cannot reach the hosts of the runner there. This is inferred and not run (the design review, unknown U10). The consumer is podrunner on proxy-only hosts.

## Premise

- Read: `crates/pod-broker/src/broker.rs:301` (`upstream` takes one `Option<pod_proxy::Proxy>` and dials the checked address) and `crates/pod-broker/src/serve.rs:312` (`connect` dials `Target::Addr`). The issue cites `serve.rs:273-287` and `broker.rs:183-191`. The code agrees to within a few lines.
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

None. The owner named podrunner on 2026-11-11 (the ruling "podrunner online"; call C49: DR-1 and DR-2 are yes). Take the entry when podrunner's work needs it (`TODO/PROGRESS.md`, Work order).

Depends on T-099 (done) and `pod-dns`. Not on T-100.

## Done

2026-11-11. `Settings::names_upstream` (default false) and
`Settings::canary` (default `localhost`); `Broker::check_upstream()` gives
`UpstreamCheck` (`RefusesNonPublic`, `TakesNonPublic`, `Unknown`) and writes
`broker.upstream.canary`. In names mode the broker checks the name and each
address of its one resolve as before, then dials `Target::Name` once; the
audit line holds `"sent":"name"` and the checked address. `names::OPEN_SUFFIXES`
and `open_suffix`: the load flags each allow rule under an open suffix
(`JobInfo::flagged`, `Compiled::flagged`, `"flagged"` in the control answer,
the `broker.job.load` line at `warn`). Call C51 changes C28. The README has
the section "An upstream that takes names only". No new parser: no fuzz
target.

- Tests (`tests/names.rs`, 7): the canary
  (`the_canary_tells_an_upstream_that_refuses_a_non_public_name_from_one_that_takes_it`,
  `a_canary_with_no_answer_is_unknown`); the flag
  (`an_allow_rule_under_an_open_suffix_is_flagged_at_load`,
  `an_open_suffix_holds_the_suffix_and_each_name_under_it_only`); the
  rebinding resolver (`the_broker_sends_the_name_once_and_never_a_second_answer`,
  `names_mode_keeps_each_check`); the copy of the policy of the fixture
  of podnet (`PodUtils/podnet:scripts/box/proxy.py`) in Rust
  (`an_upstream_that_takes_names_only_needs_names_mode`).
- Plant `broker-names-mode-sends-the-address`: 2 of the 7 tests fail.
- Not run: the two sandbox hosts. Next: podrunner sets `names_upstream`
  from its probe of the host and holds a job `advisory` when the canary is
  not `refuses-non-public` (PodUtils/podrunner#24).

# T-163: Schemas and types: job offer, unit capability, run report and public report (PodUtils/podvm#227)

**Source:** PodUtils/podvm#227 section 2.4 (COORD-021 of podvm, 2026-11-10)
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** M
**Status:** done

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

None. The owner named podrunner on 2026-11-11 (the ruling "podrunner online"; call C49: DR-1 and DR-2 are yes). Take the entry when podrunner's work needs it (`TODO/PROGRESS.md`, Work order).

Podrunner's WP-01 (the traits) comes first.

## Done

2026-11-11. The crate `pod-job` (call C50) from PodUtils/podrunner:crates/podrunner-schema and `schemas/` at `9ab7fd3`:
the four records (`JobOffer`, `Capability`, `RunReport`, `PublicReport`), their ids, `Tier`, `RefusalCode`, `SCHEMA`
and the `check` of each record. `secret.rs` stays in podrunner (a `skip` line of `crates/pod-job/SOURCES`). The
schemas `job-offer.v1`, `unit-capability.v1`, `run-report.v1` and `run-report-public.v1` are in `schemas/`, written
from the types by a test (`POD_JOB_WRITE_SCHEMAS=1`); schemars is a dev-dependency and the feature `schema`. The
examples (11 valid, 19 invalid, `expect.json`) are in `crates/pod-job/tests/examples/`, as `docs/design.md`,
section 2, gives `schemas/` the `*.schema.json` files only. The commits: `pod-job: import PodUtils/podrunner ...`
(the files with no change) and `pod-job: the records of a CI runner's job ...` (the adaptation, the tests, the record).

- `cargo test -p pod-job --all-features`: 16 passed. The four tests of the Prove:
  1. `each_example_passes_or_fails_for_its_reason`: the 11 valid examples pass and the 19 invalid ones fail for the
     reason of `expect.json`. A validator of JSON Schema 2020-12 (boon) gives each example the same verdict on the
     generated schemas, but `invalid-scaleset-payload.json`: the kind of a source is in the configuration.
  2. `the_offer_refuses_what_a_source_may_not_send`: a payload of a `github-scaleset` source (`check(true)` only); a
     credential in `env`, `environment`, `credential`, `trust_inputs.env` and `unit_need.env`; an image by tag or by
     name; a `tier` at the top and in `trust_inputs`. No property of the offer schema names a tier, an environment, a
     credential, a secret or a token.
  3. `the_public_report_holds_no_host_and_no_boundary_detail`: the public schema has 9 properties and no other; the
     public report of each valid full report holds no host, unit id or payload; the types and the schema refuse a
     public report with `host`, `unit`, `unit_id`, `boundary_proof`, `principal`, `egress_hold`, `payload`,
     `residual_processes` or `exit_code`.
  4. `each_schema_names_its_version_one_way`. The rule of the log schema: the version is in the file name and in the
     `$id` only, with no field, and `additionalProperties` is false, so a line with `schema` is refused. The broker
     policy has `version` with the constant 1. The four records have a required `schema` with the constant 1. The
     rules differ: the test holds the common part (the file name, the `$id`, draft 2020-12, a compile on Rust's regex)
     for each file of `schemas/`, and lists the two others. T-177 gives them one rule.

  Also `the_job_id_pattern_is_the_rule_of_the_type`, `the_rules_of_a_report_are_the_rules_of_its_schema` (36
  reports) and the 10 tests of the source with their names.
- The plant, one time, not kept: a field `tier: Option<Tier>` in `JobOffer`. Three tests fail:
  `the_offer_refuses_what_a_source_may_not_send` (`expect_err("tier")` on `Ok`),
  `each_example_passes_or_fails_for_its_reason` (`job-offer/invalid-tier-field.json` passes) and
  `the_schemas_are_generated_from_the_types`. After the revert, the 16 pass.
- Fixed in place. The draft schemas did not hold the version (`schema: 2` passed), the arch, the names of the limits,
  the form of the payload, the required limits and the rules of a report. The pattern of a job id had a look-around,
  which Rust's regex and Go's RE2 refuse. The public report had no check. `TODO/PROGRESS.md` named the calls C1 to
  C48 (C49 existed). `README.md` named `v0.8.0` as the last release. `docs/HANDOFF.md` listed done entries as work.
- Findings, as entries: T-177 (one rule for the version field), T-178 (the `policy` of an offer is not held to
  `broker-policy.v1`).
- The fuzz target `job_offer` (read, `check`, encode, read again), with the 11 offers as seeds:
  `cargo test --manifest-path fuzz/Cargo.toml`: 3 passed (23 targets).
- `sh scripts/agent/check.sh`: fmt, clippy, lint, doc, record, size and paths 0, static skipped (no zig), test 0
  (`cargo test --workspace --all-features`: 1975 passed, 15 ignored). `python3 -I scripts/check-sources.py --clone
  PodUtils/podrunner=DIR pod-job`: 44 files, 0 changes. cargo-deny 0.20.2 (`check advisories licenses bans sources`;
  `--all-features check licenses sources`): ok. The new licences: schemars and schemars_derive MIT; dyn-clone,
  ref-cast and serde_derive_internals MIT OR Apache-2.0. boon and its tree are dev-dependencies (MIT, Apache-2.0,
  Unicode-3.0, and MIT-0 of `borrow-or-share`); cargo-deny lists none of them.
- Next: the main session reserves `pod-job` on crates.io and releases it. podrunner takes it at the next tag (the
  adoption map of `crates/pod-job/README.md`) and removes its copies.

# T-177: One rule for the version field of each schema of `schemas/`: the log line and the broker policy

**Source:** found by the import of `pod-job` (T-163, its test 4)
**Category:** defect
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The schemas of `schemas/` name their version in three ways. Each schema carries the version N in its file name
(`NAME.vN.schema.json`) and in its `$id`. The four records of `pod-job` also have a required field `schema` with the
constant N. `schemas/log.v1.schema.json` (one log line) has no version field, and its `additionalProperties` is
false, so a line that has a `schema` field is refused. `schemas/broker-policy.v1.schema.json` names the field
`version` (`pod_broker::policy::VERSION`). githome (GitHub #36) and cfb (GitHub #35) pin these schemas: a reader that
wants to refuse a record of another version needs one rule for each document.

## Premise

- Read: the six files of `schemas/`; `crates/pod-broker/src/policy.rs` (`VERSION`, the field `version`);
  `crates/pod-broker/src/audit.rs` (the lines in the form of the log schema); `crates/pod-core/README.md` (the keys
  of a log line).
- Measured: the test `each_schema_names_its_version_one_way` of `pod-job` (`crates/pod-job/src/schemas/rules.rs`)
  holds the two files in its list `OTHER_RULE`.
- Not measured: the consumers that write or read log lines and policies (podrunner, podvm's `--net broker`).

## Approach

Give each schema the rule of the records: a required field `schema` with the constant N, the first key of the
object. The broker policy renames `version` to `schema` (in `Policy`, the TOML form, the README, the control lines,
the examples and the tests of `pod-broker`, the static probe). The log line gets `"schema": 1`, and the audit of
`pod-broker` writes it. A format of the wire can change in any version (`docs/design.md`, section 1, rule 3); the
release notes name the change, and the hand-off tells podrunner and podvm. Then empty `OTHER_RULE`.

## Prove

```sh
cargo test -p pod-job --all-features each_schema_names_its_version_one_way
cargo test -p pod-broker --all-features
cargo test -p pod-core --all-features
```

The test of `pod-job` has an empty `OTHER_RULE`, and it reads each file of `schemas/`.

## Done

2026-11-11. Each schema of `schemas/` has a field `schema`, the constant N
of its name and its first required key. The broker policy renames
`version` to `schema` (`pod_broker::policy::SCHEMA`; `Policy::schema`; the
TOML form, the README, the control lines, the tests and the static probe).
`schemas/log.v1.schema.json` requires `schema`; `pod_core::LOG_SCHEMA` is 1,
and each audit line of `pod-broker` starts with `{"schema":1,`. The README
of `pod-core` names the key. `OTHER_RULE` is gone.

- Tests: `each_schema_names_its_version_one_way` of `pod-job` reads each of
  the 6 files with one rule; `the_json_schema_agrees_with_the_policy`
  (`pod-broker`); `no_audit_line_holds_a_planted_secret` checks the first
  key; `the_log_schema_names_each_class` (`pod-core`) checks the constant.
- Breaking: a policy with `version` is refused (unknown field). The next
  release notes name it, and the hand-off tells podrunner and podvm.

# T-178: `pod-job`: the `policy` of an offer is a `broker-policy.v1` policy

**Source:** found by the import of `pod-job` (T-163)
**Category:** defect
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`JobOffer::policy` says that it is the egress policy of the `broker-policy.v1` schema of `pod-broker`, and an empty
object is no policy. But the type and `schemas/job-offer.v1.schema.json` take any JSON object
(`additionalProperties: true`). The valid example `crates/pod-job/tests/examples/job-offer/valid-limits-and-need.json`
of podrunner's draft holds `{"allow": ["github.com:443"]}`, which `broker-policy.v1` refuses: it has no `version` and
no `mode`, and an allow rule is an object with a `host`. A free object can also carry any value from a source, a
credential too, past the check of the offer.

## Premise

- Read: `crates/pod-job/src/offer.rs`, the example, `schemas/broker-policy.v1.schema.json`,
  `crates/pod-broker/src/policy.rs`. At `9ab7fd3`, podrunner's placement
  (PodUtils/podrunner:crates/podrunner-slots/src/placement.rs) reads only whether the policy is empty.
- Not measured: what podrunner gives to the broker from an offer (PodUtils/podrunner#24, T-161).

## Approach

The schema of `policy` is an empty object or the `broker-policy.v1` schema, by its `$id` (`$ref`). The type stays a
JSON object, so `pod-job` does not depend on `pod-broker` and its tokio; the consumer reads the policy with
`pod_broker::Policy::from_value` and refuses a bad one with `egress-policy-unsupported`. The example holds a valid
policy (the podrunner session agrees, as the examples are its draft). The test of the examples gives the validator
`broker-policy.v1.schema.json` as a resource, and a test shows an offer with a policy that `broker-policy.v1` refuses
is refused by the schema of the offer. If T-177 renames `version`, take it first.

## Prove

```sh
cargo test -p pod-job --all-features
```

## Done

2026-11-11. The schema of `policy` is `anyOf` an empty object and a `$ref`
to `broker-policy.v1` by its `$id` (`pod_job::SCHEMAS`); the type stays a
JSON object, so `pod-job` has no pod dependency. `JobOffer::check` reads the
top level of a policy (the keys of `offer::POLICY_KEYS`, `schema` 1, `mode`,
the types of `allow` and `deny`) and refuses another with "the policy is
not a broker-policy.v1 policy". The validator of the tests takes
`broker-policy.v1.schema.json` as a resource. The example
`valid-limits-and-need.json` holds a valid policy; its old policy is the
new example `invalid-policy-free-object.json`. The README names the rule.

- Tests: `the_policy_of_an_offer_is_a_broker_policy` (3 good policies, 6
  refused by the check and the schema, 3 deep faults refused by the
  schema); `each_example_passes_or_fails_for_its_reason` (11 valid, 20
  invalid).
- The examples are podrunner's draft: the hand-off of the next release
  tells podrunner the new example.

# T-179: `pod-incus`: a client of the incus REST API (the owner's name, 2026-11-11)

**Source:** the owner reserved `pod-incus` on 2026-11-11 (GitHub #1); the
ruling "podrunner DR-5" (incus "seems to be best of both microvms &
dockers"); PodUtils/podvm#243; PodUtils/podvm#253 (COORD-034 of podvm, the
ask after PodUtils/podvm@T-1717; PodUtils/podvm@T-1718).
**Category:** feature
**Milestone:** M7
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podvm gets incus as a back end (the owner's ask of DR-5). Its study
(PodUtils/podvm@T-1717, experiment 433) measured an unprivileged user's
`incusd` (root of the user's own namespace) on the cloud host, incus 6.0.0:
the REST API on `INCUS_DIR/unix.socket` imports an image, launches, runs
`exec`, stops, starts and deletes. podvm asks for the client as a crate
(PodUtils/podvm#253); until a tag, podvm writes its own minimal client in
PodUtils/podvm@T-1718 and moves to the crate at the take. podrunner can use
it as a unit kind.

## Premise

- Read: PodUtils/podvm#253. The calls that podvm needs:
  `GET /1.0` (server facts); `POST /1.0/images` with the body as a stream
  (a unified tarball, or split meta and rootfs), then
  `GET /1.0/operations/ID/wait`; `POST /1.0/instances` (`source` of type
  `image` by alias or fingerprint, `config`, `devices`);
  `PUT /1.0/instances/NAME/state` (start, stop);
  `POST /1.0/instances/NAME/exec` with `wait-for-websocket` (stdin, stdout,
  stderr and control websockets) or `record-output`, the exit code in the
  operation's `metadata.return`; `GET /1.0/instances?recursion=1`;
  `DELETE /1.0/instances/NAME`; `GET /1.0/instances/NAME/console`;
  `GET`/`POST /1.0/instances/NAME/files?path=`.
- The floor is incus 6.0 LTS. podvm keeps the choice of the back end, the
  conversion of an OCI image to a unified tarball, the `lxc.init.cmd`
  script and the start of a user's `incusd`.
- Not read: the REST API of incus at its source (lxc/incus `doc/rest-api`
  and `shared/api`); read it first, at a pinned commit.

## Approach

1. HTTP/1.1 over a Unix socket with no TLS (the HTTP helpers of
   `pod-proxy`), and the operation model of incus: a background
   operation, its wait, its error by name.
2. The calls above, typed (serde), each with the incus version it needs.
3. `exec` with `record-output` first (stdout and stderr apart, the exit
   code); then the websockets of `wait-for-websocket` over the Unix socket
   (`pod-ws`), stdin too.
4. A stand-in incus server on a Unix socket for the tests (in the crate's
   tests or `pod-testkit`), with no `incusd`; the refusal by name where no
   incus answers.
5. Static musl, as each crate; no `lxc` or `incus` binary.

The first release holds 1, 2, 4, 5 and `record-output`; the websockets
come in the same release if they fit.

## Prove

```sh
cargo test -p pod-incus --all-features
```

## Start condition

Met: podvm's COORD issue (PodUtils/podvm#253) came after
PodUtils/podvm@T-1717.

## Done

2026-11-11. The crate `pod-incus` (`crates/pod-incus`, 2,235 lines of
`src/`, 1,477 of tests; no file is over 500), new code from the API of
lxc/incus at `63377d9bacba35715edf1bcfb4fd155485a6a6fb` (`main`,
2026-11-10): `doc/rest-api.md`, `doc/rest-api.yaml`,
`doc/api-extensions.md`, `shared/api/*.go`, the handlers of
`cmd/incusd/` and `client/incus_*.go`. The 6.0 floor is checked at the
tag `v6.0.0`: each extension of `NEEDED`, the paths of the logs of exec,
the wait and the frames of `shared/ws` are the same. 6.0.0 has no
`image_import_alias`, so the aliases of an import go by
`POST /1.0/images/aliases`. The commit: `pod-incus: a client of the REST
API of incus on its Unix socket ... (T-179)`.

- The transport: HTTP/1.1 on the Unix socket, one connection for each
  call, the reader of answers of `pod-proxy`; a stream body by its length
  or in chunks; the envelope (`sync`, `async`, `error`; the type decides,
  as the wait sends 200 before its body); the wait
  (`GET /1.0/operations/ID/wait?timeout=N`, the limit a setting; at the
  limit `ErrorKind::Timeout`). `Error` names the call (the path with no
  query, so no secret) and its kind: `NoServer`, `Incus` (status,
  `error_code`, `error`), `Operation` (`status_code`, `err`), `Timeout`,
  `Protocol`, `Unsupported`, `Usage`, `Io`, `WebSocket`; `class()` and
  `pod_core::Failure::from` (stage `host`).
- The calls of podvm's table, typed with serde: `server` (and
  `Server::check` against `NEEDED`), `import_image` (unified, or split as
  multipart `metadata` and `rootfs` or `rootfs.img`; `X-Incus-public`,
  `-filename`, `-properties`, `-fingerprint`), `create_alias`,
  `create_instance`, `set_state`, `start`, `stop` (`force`, `timeout`),
  `instances` (`recursion=1`), `instance`, `delete_instance`,
  `console_log`, `exec_recorded` (`record-output`: the two logs read and
  deleted, the exit code from `metadata.return`), `exec_attached`
  (`wait-for-websocket`: the sockets `control`, `0`, `1`, `2` on the same
  Unix socket with the session of `pod-ws`; the empty text message of
  incus ends a stream; `Control::signal`; `Attached::output` reads both
  outputs at once), `get_file`, `put_file`, `operation`, `wait`,
  `wait_success`, `cancel`. The settings: the socket, the project, the
  user agent, the limits; no variable of the environment.
- `cargo test -p pod-incus --all-features`: 44 passed (13 unit tests, 31
  against a stand-in incus on a Unix socket in `tests/common/`, with no
  `incusd`: the operation model with 202 and the wait, the limit of a
  wait, a failed and a canceled operation, the error words of incus, a
  socket with no server and one with no listener refused by name, each
  call, the websockets with stdin, the barrier and a signal). 60 runs of
  the tests in a loop passed. A planted defect (no check of a final state
  after the wait) fails `a_wait_ends_at_its_limit_with_the_operation_running`.
- `cargo test --workspace --all-features`: 2085 passed, 0 failed, 16
  ignored. The static probe calls `Client::server` on a socket that does
  not exist; `cargo zigbuild --release --target x86_64-unknown-linux-musl
  -p static-probe` and `scripts/check-static.sh` pass, and the probe
  prints the refusal by name.
- After podvm's own client (PodUtils/podvm@T-1718 at podvm `3a9d434`)
  named the calls it uses: `Client::alias` (`GET /1.0/images/aliases/NAME`,
  a 404 is `None`; an escaped name with `/` and `:`), the one call of that
  list that the crate did not have. The doc of `ImageData` names podvm's
  finding of incus 6.0.0: a `metadata.yaml` with `creation_date: 0` is
  refused. A planted defect (a 404 as an error) fails
  `a_unified_tarball_is_imported_from_a_stream_of_unknown_length`.
- The first release holds podvm's table with `record-output` and the
  websockets. Not in it: interactive exec (a PTY) and the console
  websocket; podvm did not ask for them.

# T-180: `pod-mask`: the confidential execution of pmask as a crate (the owner's name, 2026-11-11)

**Source:** the owner reserved `pod-mask` on 2026-11-11 (GitHub #1); the
ruling "podrunner DR-6 and pmask" ("will be used for everything/everywhere
we need a trusted confidential execution but hardware doesn't support it,
we will use this later for many of our crates").
**Category:** feature
**Milestone:** M8
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

The owner's project pmask hides the metadata of a process tree from other
users of a host, delivers secrets on sealed memfds, and keeps an encrypted
store. The crates (`pod-supervise`, `pod-broker`, `pod-tls`,
`pod-identity`) and podrunner's trusted jobs on a host at boundary
`redirect` (DR-4) can use it later. `docs/research/pmask.md` has the study.

## Premise

- Read: `docs/research/pmask.md` (pmask at `5a3c972`, `v0.1.0-beta.3`):
  ptrace is necessary for the redaction; a static build is x86_64 and
  aarch64 only; the library takes the `waitpid(-1)` domain and a global
  lock; `--no-dumpable` stops the memory reads of `pod-hold`.
- Not measured: `pmask probe` on a host of podrunner's H0.

## Approach

When the owner names the time: `pod-mask` holds the parts that a crate
calls (the self-mask of argv and environ, `SecBuf`, the sealed memfd of a
secret, `PR_SET_DUMPABLE`), and runs a payload under the `pmask` binary as
a separate process where a hold needs the tree; each part is a seam that a
consumer can swap. The findings of the study come first: the targets past
x86_64 and aarch64, the fault switches of the environment, the files over
500 lines.

## Prove

```sh
cargo test -p pod-mask --all-features
```

## Start condition

The owner names the time ("later"). pmask is in rapid development; read it
again at its newest tag first.

# T-183: `pod-broker`: a raw byte that is not visible ASCII in a path is read as Latin-1

**Source:** the run 42958692411 of `fuzz.yml` (the target `broker_target`)
**Category:** defect
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`target::normal_path` pushed each byte of a path that is not `%` as
`char::from(byte)`. A raw UTF-8 character (`ч`, `0xd1 0x87`) became two
Latin-1 characters, so the path that the policy checks and the far end
gets was not the path that the job sent, and a second pass changed it
again. A raw control byte (a tab) passed, though its escape is refused.
The input of the run: `http://api.e/ampm/os/xle.com\xd1\x87s/x?page=1`.

## Premise

- Read: `crates/pod-broker/src/target.rs`; RFC 3986 (a request-target
  holds only visible ASCII; another byte is an escape).

## Approach

Refuse a raw byte outside `0x21..0x7f` in a path, with the escape that it
needs in the words. Keep the input as a test of the crate and as a seed.

## Prove

```sh
cargo test -p pod-broker --test target
cargo test --manifest-path fuzz/Cargo.toml
```

## Done

2026-11-11. `normal_path` refuses a raw byte outside `0x21..0x7f` ("a raw
byte 0xd1 in a path: escape it as %D1"); an escape of the byte stays as
it is. Test: `a_raw_byte_that_is_not_visible_ascii_is_refused_in_a_path`
(`tests/target.rs`); the seed `raw-utf8-in-path` of `broker_target`. The
stable test of `fuzz/` passes with it.

# T-191: `pod-broker`: a listener that takes a plain request in origin form and opens verified TLS to its `Host` (PodUtils/podnet#437)

**Source:** PodUtils/podnet#437 (COORD-019 of podnet), the owner's donor
review: satmux 5. The donor: Th0rgal/satmux at
`2922ebb835d0eb939ddbdfa3b5e7e0f247a0dc0c` (MIT), `handle_gh_unix_tls` in
satmux.py; the transport of cli/go-gh, pkg/api/http_client.go on its
branch `trunk` (no commit pinned).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

A podrunner unit, a podvm guest or the sandbox of a coding agent has a Unix
socket of the broker and no network route. Its agent runs `gh pr list` and
needs `api.github.com`. Some tools ignore `HTTPS_PROXY`, and a sandbox can
have no shared TCP loopback, so CONNECT is not possible. GitHub CLI with
`http_unix_socket` and `curl --unix-socket` send plain HTTP/1.1 in origin
form (a path and a `Host`) to a Unix socket: go-gh sets both its dial and
its TLS dial to the Unix dial, and turns keep-alive off. The broker answers
such a request with 400. A tool with a Unix socket and no support of a
proxy thus has no way out.

The donor reads the head (at most 64 KiB, 10 s for each read) and takes the
target from `Host` (`api.github.com` when there is no `Host`). It connects
through its SOCKS5 door by name, runs a verified TLS handshake (15 s),
removes the `Connection` header of the client, sends `Connection: close`,
and copies both ways until the origin closes: one request on each
connection. Its gaps: it takes each `Host`, and its socket is at a fixed
path of `/tmp`, with mode 0600 set after the bind.

## Premise

- Read (this tree): `crates/pod-broker/src/target.rs:187-189`:
  `parse_absolute` refuses a target in origin form ("a proxy takes a
  request-target in absolute form"). `crates/pod-broker/README.md:269`
  lists that 400. They agree with the review.
- The code moved since `63348de`, the commit of the review (T-161 and
  others). Today `serve_one` is at `crates/pod-broker/src/serve.rs:104`,
  `next` at `:223`, `check` at `:255` and `connect` at `:312`; `exchange`
  is at `crates/pod-broker/src/forward.rs:71`; `Broker::serve_unix_job` is
  at `crates/pod-broker/src/broker.rs:215` and `serve_tcp_job` at `:239`;
  the advice of `bind_unix` (a directory of mode 0700) is at
  `crates/pod-broker/src/control.rs:140-143`. The steps agree with the
  review.
- Read: `crates/pod-broker/Cargo.toml` has no `pod-tls`. But `pod-proxy`
  depends on `pod-tls` (`crates/pod-proxy/Cargo.toml:27`) with its default
  feature `async` (`crates/pod-tls/Cargo.toml:12`): the build of the
  broker holds `pod-tls` already, and a direct dependency adds no crate.
  `Context::connect_over` (`crates/pod-tls/src/context.rs:162`) is a
  verified handshake over an open stream, with no fall back to plain text.
  `pod-tls` has no pod dependency but `pod-core`
  (`crates/pod-tls/Cargo.toml:41`).
- Read: `check` takes the path of a plain request
  (`crates/pod-broker/src/serve.rs:269`). The path rules of a policy thus
  apply in this mode with no TLS end; a CONNECT needs a TLS end for them
  (`PATH_NEEDS_TLS_END`).
- Found, not in the review: a 502 or a 504 names what failed
  (`crates/pod-broker/src/words.rs`, T-609, C469); `refused` writes the
  words of each refusal (`crates/pod-broker/src/serve.rs`). A 502 with
  the words of the verifier in the body thus changes no rule of the
  crate.
- Found, not in the review: `head` writes the `Host` with no port for port
  80 only (`crates/pod-broker/src/forward.rs:54-57`). Toward TLS the
  default port is 443.
- Found, not in the review: `next` refuses two `Host` headers but accepts
  none for a target in absolute form (`crates/pod-broker/src/serve.rs:223-247`).
  This mode needs exactly one.
- Found, not in the review: an upstream proxy gets the name of the origin
  only with `Settings::names_upstream` (T-161,
  `crates/pod-broker/src/serve.rs:317-328`); else the broker dials a
  checked address (`:330-342`). The acceptance "the proxy sees the name"
  holds with that setting only.
- The consumer: the egress module of podrunner serves one listener of the
  broker for each unit (PodUtils/podrunner@T-024). T-100 is the TLS end of
  a CONNECT, with a CA that the job must trust. This mode is the other
  direction: the client sends plain HTTP, the broker is the TLS client, and
  the job needs no CA.
- Not run: the bridge of the donor; `gh` against `api.github.com`. Open
  questions of the review (Unverified): the releases of `gh` use this
  transport (the review read `trunk`); `gh` never sends
  `Expect: 100-continue`.

## Approach

Licence: satmux is MIT. Write new Rust code from this description, with no
donor text.

- A listener option (working name `OriginTls`) on `serve_unix_job` and
  `serve_tcp_job`, off by default: each listener of today keeps the 400 for
  origin form. The host gives a `pod_tls::Context` and a `Trust`; the
  crate reads no variable.
- The steps: read the head within `head_timeout` (408, 431 and 400 as
  today). Refuse a CONNECT, an `Upgrade`, an `Expect: 100-continue` and a
  target that is not in origin form, with 501 and words. Require exactly
  one `Host` (else 400; no default host). Parse it with
  `parse_authority` and the default port 443. Take the lease of the job of
  the listener. Run `check` with the path (403 before a dial). Run
  `connect` (the checked addresses, through the way out). Run
  `connect_over(up, host, trust, 15 s)` (502 on a failure, never plain
  text). Send the head with `Host` (no port for 443), `Connection: close`
  and no hop-by-hop header, then the body in its framing. Copy the answer
  back until the origin closes. One request on each connection.
- The answer of a failed verification: make a call in `docs/decisions.md`.
  The recommendation: the body holds the words of the verifier only, on one
  line, and never the chain of an upstream; each other 5xx keeps the fixed
  words.
- The allow list is the policy of the job: no new list. The broker adds no
  credential. The `Authorization` of the client goes only to the host that
  the policy allowed.
- The audit line has a kind of its own, with the host, and the reason of a
  refusal or of a failed verification.
- HTTP/1.1 only: no HTTP/2, no WebSocket, no keep-alive, no change of an
  answer. A credential swap and a CA for each job stay in T-100.
- The README: the mode, its answers, and the socket in a directory of mode
  0700.

## Prove

```sh
cargo test -p pod-broker --all-features
```

The tests, against a local TLS server with a test CA in the `Trust`:

- An origin-form request on the socket gets the body, and the server saw
  the SNI `HOST`. The request comes from
  `curl --unix-socket S http://HOST/path`, or the same bytes from the test.
- A certificate of another name: 502, and no byte of the origin reaches
  the client (the negative case).
- A `Host` that the allow list does not hold: 403, and the dial count of a
  fake way out is 0 (the negative case).
- No `Host`, two `Host` headers, a target in absolute form, a CONNECT, an
  `Upgrade`: each gets its 400 or 501, with no dial.
- A listener with no `OriginTls`: the 400 of today for origin form (the
  negative case).
- A `POST` body of 1 MiB arrives whole. A head over 16 KiB gets 431. A
  client that sends nothing gets 408 at `head_timeout`.
- With `names_upstream` and an upstream proxy as the way out, the proxy
  gets a CONNECT to the name of the origin.
- A live check in `.github/workflows/live.yml`, with a policy that allows
  `api.github.com` only: `gh config set http_unix_socket S`, then
  `gh api /rate_limit`.

# T-194: `pod-broker`: a hash of the policy in the load answer, the list answer and each audit line (PodUtils/podnet#437)

**Source:** PodUtils/podnet#437 (COORD-019 of podnet), the owner's donor
review: tunnet 2. The donor: tunnetio/Tunnet at
`fceb3f201fa88425ff3930ed059f394a60aca6b5`: hash.rs of the crate
tunnet-policy-engine, `policy_content_hash` in policy.rs of tunnet-common,
cmds_policy.rs of tunnet-cli (all three Apache-2.0).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

An audit line of `pod-broker` names the rule that decided a connection as
`allow[2]`: an index into a policy that a later `load` of the same job
replaces. A CI host runs podrunner with the broker. A job loads policy A;
then the source loads policy B for the same job id (one more host). The
next day an operator reads "decision allow, rule allow[2]" in
`audit.jsonl`, and index 2 of A and index 2 of B are different hosts. With
a hash of the policy on each line, the operator finds the
`broker.job.load` line with the same hash, and the policy file that the
source keeps for that hash. The source can also compare the hash of `list`
with the hash of what it sent (a drift check).

The donor names a policy document by a hash of its canonical JSON: it
serializes the document to a value, sorts the keys of each object
(recursive), keeps the order of each array, serializes again and hashes
the bytes (BLAKE3). Two files that differ only in the order of their keys
or in spaces get one hash; each change of meaning gets another.

## Premise

- Read (this tree; the code moved since `63348de`, the commit of the
  review): `crates/pod-broker/src/broker.rs:110-151`. `load` writes
  `broker.job.load` with `job`, `mode`, `rules`, `ttl_s` (and `flagged`,
  T-161), and no identity of the policy.
- Read: `crates/pod-broker/src/jobs.rs:68-75` (`Load` holds a `Policy`)
  and `:88-103` (`JobInfo`: `job`, `mode`, `rules`, `flagged`,
  `expires_in`, `open`).
- Read: `crates/pod-broker/src/control.rs:54-68`: `info`, the JSON of a
  job. The `load` answer is `info` with more keys (`:99-108`), and `list`
  maps `info` (`:117-120`). One new field of `JobInfo` thus reaches both
  answers. The control socket checks the user of its peer (`:182-189`).
- Read: `crates/pod-broker/src/serve.rs:62-73`: `Note::line` starts both
  connection lines. The deny line sets `decision` and `rule` at `:350`,
  the allow line at `:367`. A refusal before the lease (408 or 407,
  `:107-122`) has no job, thus no hash.
- Read: `crates/pod-broker/src/policy.rs:71-83`: `Policy` derives
  `Serialize` with `deny_unknown_fields`. An empty `allow`, `deny` or
  `ports` and a missing `path` are not written (`skip_serializing_if`), so
  an empty list and a missing list give one hash. The rule `allow[{n}]`
  comes from `:258-259`.
- Read: `crates/pod-broker/Cargo.toml:22` (`sha2` is a dependency);
  `crates/pod-broker/tests/audit.rs:16` (`no_audit_line_holds_a_planted_secret`
  reads the audit lines only); `schemas/log.v1.schema.json` (`fields` is an
  object, so a new key is compatible). They agree with the review.
- Found, not in the review: `Entry` has a field `hash` already, the
  SHA-256 of the token (`crates/pod-broker/src/jobs.rs:117`). Name the new
  value `policy_hash`, and never put the hash of the token in an answer or
  a line.
- Found, not in the review: the order of each array stays, as the review
  asks, so a new order of `deny` or of `ports` with the same meaning gives
  another hash. That is a false difference, never a false sameness.
- Not run here: the model of the canonical form in Python of the review
  (the order of the keys does not change the hash; the order of an array
  does).
- The consumer: the egress module of podrunner loads the policy of each
  job (PodUtils/podrunner@T-024) and can compare the hash.

## Approach

Licence: Tunnet is Apache-2.0 for its policy engine, its CLI and
tunnet-common (the parts that the review cites), MPL-2.0 for its agent,
core, posture, update and service, and AGPL-3.0-only for its control,
audit, relay, edge and operator. Write the implementation from this
description, not from the donor source: no MPL or AGPL part is used, and
no Apache-2.0 text goes into the crate, so the crate needs no notice.

- `policy_hash(&Policy) -> String`: `serde_json::to_value`, then the
  canonical form, then `sha256:` and the first 16 hex digits of the
  SHA-256 (64 bits: an identity among the policies of one job, not a
  secret, no proof of origin). The canonical form sorts the keys of each
  object explicitly, because the order of a `serde_json::Map` depends on
  the feature `preserve_order`, which each crate of a build can turn on.
  It keeps the order of each array: `allow[2]` is an index, so a new order
  is a change.
- SHA-256 in place of BLAKE3: `sha2` is a dependency already.
- Compute the hash at `load` and keep it in `Compiled`. Add `policy_hash`
  to `JobInfo` (thus to the `load` answer and to each job of `list`), to
  the `broker.job.load` line, and to `Note` (thus to each
  `broker.connection` line, deny and allow).
- A policy that does not compile has no hash; the failure line of today
  stays.
- The hash covers the policy only, never the token. No compare-and-set
  (`if_hash`): a source reads `list` first.
- The README: the control answers and the keys of the audit lines.

## Prove

```sh
cargo test -p pod-broker --all-features
```

The tests:

- Two `load` requests of one policy with another order of keys: one
  `policy_hash`.
- Two policies with the same rules in another order: two hashes.
- `load` A, then `load` B of one job id: a connection line has the hash of
  B, and the two `broker.job.load` lines have different hashes.
- `list` gives the hash of the loaded policy and holds no token;
  `no_audit_line_holds_a_planted_secret` still passes.
- The plant: a hash of `Mode` only fails the tests "the same rules in
  another order" and "one more rule".

# T-196: `pod-broker`: `bind_unix` leaves a window with the mode of the umask (PodUtils/podnet#444)

**Source:** PodUtils/podnet#444 (COORD-026 of podnet; the donor review, satmux 6, PodUtils/podnet#429; podnet's fix `3c6173f`, PodUtils/podnet@T-238)
**Category:** defect
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`control::bind_unix` binds the socket at its path and then sets its
mode. Until the `chmod`, the socket has the mode of the umask (0755, 0775
or 0777 for 022, 002 and 000). In a shared directory another user can
connect in that window. The control refuses a peer of another user, but a
listener of one job (T-160) takes no token: a connection in the window is
a connection of the job.

## Premise

- Read: `crates/pod-broker/src/control.rs` (`bind_unix`; its doc asked
  for a directory of mode 0700); podnet's report and fix.
- Read: the other binds of a Unix listener in podcrates:
  `pod-supervise` binds in the directory of its container (mode 0700,
  T-175); `pod-guest` binds in a test only.

## Approach

Bind in a new directory of mode 0700 beside the path (a name that another
user cannot guess, `mkdir(2)` refuses a name that is there), set the mode
there, move the socket to the path with `link(2)` (it refuses a path that
is there: `AddrInUse`), and remove the stage. A path whose stage does not
fit in `sun_path` binds at the path. The socket keeps the name of its
bind. Test each umask in a new process.

## Prove

```sh
cargo test -p pod-broker --test control
```

## Done

2026-11-11. `control::bind_unix` binds through `control/bind.rs`: the
socket is bound in a new directory of mode 0700 beside the path (the name
`.pod-broker-` and 16 hex digits of the random keys of the standard
hasher; `mkdir(2)` refuses a name that is there, 8 tries), gets its mode
there, moves to the path by `link(2)` (a path that is there gives
`AddrInUse`), and the stage goes. A stage that does not fit in `sun_path`
(103 bytes) binds at the path. The test
`a_socket_has_its_mode_under_each_umask` runs a worker under the umasks
000, 002 and 022: a thread reads the mode of the path during 200 binds,
and no mode other than 0600 shows; no stage stays. The plant
`broker-bind-at-the-path` (a bind at the path, then a chmod) fails it: the
thread saw 0777 in each of 3 runs. `cargo test -p pod-broker
--all-features` passes.

# T-226: `pod-job`: the time to the first step, the job images and the exit reason `limit` in a report (podrunner's T-075)

**Source:** a direct message of the podrunner session, 2026-11-11 (its T-075; its SPEC sections 10.4 and 11.4)
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podrunner reports the time from the claim to the hook of the first step
of a job, and the digests of the images of the job, and `run-report.v1`
has no field for them. A unit that ends a job over a bound of its limits
(disk now) reports `failure` with an audit line `limit`: the reason is
lost in the report.

## Premise

- Read: podrunner's ask (a direct message): `RunReport` gets
  `claim_to_first_step_s: Option<u64>` and `job_images: Vec<String>`
  (a serde default, skipped when empty), `ExitReason::Limit`, and
  `PublicReport` the same reason; the two fields stay private.
- `crates/pod-job/src/report.rs`, the generated schemas of `schemas/`,
  the examples of `crates/pod-job/tests/examples/`.

## Approach

The two fields in `RunReport`, not in `PublicReport`; `check` refuses a
job image that is not a digest (`offer::is_digest`); the schema of
`job_images` holds the pattern of a digest. `ExitReason::Limit`
(`limit`) in both reports, with the rules of each other reason of a unit.
The schemas again (`POD_JOB_WRITE_SCHEMAS=1`), a valid example with the
new fields, an invalid example with an image by tag.

## Prove

```sh
cargo test -p pod-job --all-features
```

## Done

2026-11-11. `ExitReason::Limit`; `RunReport::claim_to_first_step_s` and
`RunReport::job_images` (private: `public()` leaves them out, and the
public schema and types refuse them); `check` refuses an image by tag;
`json_schema::digests`. Examples `run-report/valid-limit-with-images.json`
and `run-report/invalid-job-image-tag.json`. The test of the rules covers
8 reasons of a unit (40 reports: 18 pass, 22 are refused); the test of
the public report refuses the two private fields. `cargo test -p pod-job
--all-features`: 17 passed; clippy 0. The schemas of `schemas/` hold the
new fields and the reason.

# T-242: Docs: the names of the renames of 2026-11-12 (`pods`, `podstat`, `podforge`) and the stale text of `pod-git` and of the podrunner plan (GitHub #78)

**Source:** GitHub #78
**Category:** chore
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

GitHub #78 lists the places that use the old names after the owner's
renames of 2026-11-12 (`PodUtils/pod` is `pods`, `podwatch` is
`podstat`, `podutils` is deleted, `podforge` is new in place of
`pod-git`), the call C65 that still states the old call after its
"Superseded", the entries of `pod-git` that do not name `podforge`, and
the section 9 of the podrunner plan that says that `podrunner` has no
session.

## Premise

- Read: GitHub #78 and the body of GitHub #1 (the names reserved on
  crates.io; `pods` and `podstat` wait for their registration).
- `scripts/check-crates-io-names.py` reads section 4 of `docs/design.md`
  and takes only the repository of podcrates; no workflow runs it.

## Approach

The ruling "Renames" in `docs/decisions.md`; C65 cut to a pointer; the
products of the fleet and the names held in `docs/design.md` (each
project name of GitHub #1); A-X-09 of `docs/research/fleet/asks.md`; the
blockers of T-217 to T-220 name `PodUtils/podforge`; the check of the
names takes the repository of each project name; section 9 of
`docs/research/ci/podrunner-plan.md` marked superseded.

## Prove

```sh
cargo todo check
python3 -I scripts/check-design-paths.py
```

## Done

2026-11-12. Each place of GitHub #78 changed. The milestone of T-217 to
T-220 stays M4 (the owner decides the wave of `podforge`). Section 4 of
`docs/design.md` has a paragraph "Project names"; the check reads it, and
each project name must name the repository of its name under PodUtils
(`podvm-*`: PodUtils/podvm). `python3 -I scripts/check-crates-io-names.py`:
84 names, 2 faults: `pods` and `podstat` are missing until the owner's
job registers them (GitHub #1).

# T-251: `pod-actions-expr` and `pod-actions-workflow`: the expressions and the workflow files of GitHub Actions, for podrunner's local mode (GitHub #84)

**Source:** GitHub #84 (item P5-3 of GitHub #47); PodUtils/podrunner@T-046 (blocked on these crates); the owner's answer to Q11 (2026-11-11: "yes plan it")
**Category:** feature
**Milestone:** M8
**Priority:** P3
**Effort:** L
**Status:** done

## Problem

podrunner's local mode (`podrunner run workflow.yml`, PodUtils/podrunner#48)
compiles a workflow file into job offers with no forge. It needs the
expression language of GitHub Actions (`${{ }}`: the literals, the
operators, the functions `contains`, `startsWith`, `endsWith`, `format`,
`join`, `toJSON`, `fromJSON`, `hashFiles`, `success`, `failure`,
`always`, `cancelled`, and the contexts) and the model of a workflow file
(`on`, `jobs`, `needs`, `strategy.matrix`, `if`, `env`, `defaults`,
`steps`). The names are reserved on crates.io (GitHub #1); no crate exists.

## Premise

- Read: GitHub #84 and #47; `docs/research/ci/podrunner-plan.md`
  (phase 5, the optional `pod-actions-workflow`);
  `docs/research/ci/ghactions-local.md` (the study of the owner's archive);
  PodUtils/podrunner@T-046 (P3, later).
- Not measured: the conformance of an expression evaluator against the
  official runner.
- podrunner's ask (a direct message of its session, 2026-11-12, for
  PodUtils/podrunner@T-046): (1) `pod-actions-workflow`: a workflow file
  into a typed model, no forge, no network: `on` (data), `env`,
  `defaults.run`, `permissions`, `concurrency`; each job (`id`, `name`,
  `runs-on`, `needs`, `if`, `strategy` with `matrix` include and exclude,
  `fail-fast`, `max-parallel`, `env`, `defaults`, `timeout-minutes`,
  `continue-on-error`, `container`, `services`, `outputs`); each step
  (`id`, `name`, `if`, `run` or `uses` with `with`, `env`, `shell`,
  `working-directory`, `continue-on-error`, `timeout-minutes`); an
  unknown key kept as raw data and named; a reusable workflow (`uses:` of
  a job) named, not expanded. (2) `pod-actions-expr` at job level only
  (the runner evaluates the steps): the job `if`, `strategy.matrix` with
  `fromJSON`, `runs-on`, `needs.<id>.outputs` and `.result`; the contexts
  github (a local stand-in), env, vars, inputs, matrix, needs, strategy;
  the functions contains, startsWith, endsWith, format, join, toJSON,
  fromJSON, success, failure, always, cancelled; a missing context or
  function is an error by name; no I/O (no `hashFiles`). (3) An expansion
  helper: the ordered concrete jobs (one for each matrix cell), the edges
  of `needs`, and each skipped job with its reason. Static, no C; tests
  from the examples of the Actions documents.

## Approach

The start condition holds: podrunner asked (above).
`pod-actions-expr` (a parser and an evaluator with the loose equality and
the case rules of GitHub, a fuzz target, a conformance list with test
vectors), then `pod-actions-workflow` (the model, the matrix expansion,
the order of `needs`), each pure Rust and static.

## Prove

```sh
cargo test -p pod-actions-expr -p pod-actions-workflow
```

## Start condition

podrunner asks for its local mode (PodUtils/podrunner@T-046): it asked on
2026-11-12.

## Done

2026-11-12, commit `e555d0b` (the two crates, the fuzz targets, the
static probe, calls C105 to C109, T-280) and the commit of this section.

- `crates/pod-actions-expr`, no dependency: `Expr` (`parse`, `evaluate`,
  `contexts`, `functions`, `has_status_function`), `Template` (a text with
  `${{ }}` parts: one expression keeps its kind, text around gives a
  string), `Condition` (an `if:` with its implicit `success() &&`, and the
  `${{ }}` pitfall), `Value` and `Object` (the coercion, the loose
  equality and order, `toJSON` and `fromJSON`), `Contexts` with `Status`
  and `Limits`, `Error` by name. The README has the conformance list: each
  rule of GitHub with its tests.
- `crates/pod-actions-workflow` on saphyr-parser 0.3.4 (C105): `parse`
  (the model of the ask; an unknown key in `extra` with a `Warning`; a
  reusable workflow named, not expanded; each template and `if:` must
  parse), `Graph` (order, edges, needs, ancestors; a missing need and a
  cycle by name), `Run` (`ready`, `expand`, `finish`, `cancel`) and `plan`
  (a dry run): the concrete jobs of each matrix with `include` and
  `exclude` (also from `fromJSON` of an output), `runs-on`, the name,
  `timeout-minutes`, `continue-on-error`, the `strategy` context, and each
  skip with its condition and the results before it (C107, C108).
- `cargo test -p pod-actions-expr -p pod-actions-workflow --all-features`:
  86 passed (45 and 41; each test file of the operator, coercion,
  function, filter and matrix examples of the documents). clippy of the
  two crates with `-D warnings`: 0. `cargo todo check`: 0.
  `scripts/check-size.py`, `scripts/check-design-paths.py`,
  `scripts/check-probe.py --no-tree`: 0 faults.
- The fuzz targets `actions_expr` and `actions_workflow` (`fuzz/`): their
  bodies ran on their seeds and on 300,000 mutations each, with no panic,
  in a scratch crate; the probe function of `examples/static-probe` ran
  there too. The build of the fuzz workspace and of the static probe
  waits for CI: the disk of the session had less than 10 GB free.
- Not covered: the expansion of a reusable workflow and its
  `workflow_call` inputs, the evaluation of `env`, `container`,
  `services`, `concurrency` and `environment` of a job (kept as nodes for
  the runner), the step level (the runner's), a run against GitHub
  (T-280).

# T-280: `pod-actions-expr` and `pod-actions-workflow` against GitHub: a live run of the conformance vectors

**Source:** T-251 (its Premise: the conformance against the official runner is not measured); calls C107 and C108
**Category:** measurement
**Milestone:** M8
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

The two crates follow the documents of GitHub and the code of
`actions/runner`, but no run on GitHub compared their answers. The README
of `pod-actions-expr` lists the known differences (strict JSON in
`fromJSON`, the fold of a character with a two-character upper case, the
sum of the memory limit, the escapes of `toJSON`). Calls C107 and C108
are not measured: the status functions of a job over its ancestors, the
match of a matrix value with case, the default `max-parallel`, the name of
a matrix job whose `name` does not read `matrix`.

## Premise

- Read: the conformance table of `crates/pod-actions-expr/README.md`, the
  rules of `crates/pod-actions-workflow/README.md`, calls C107 to C109.
- Not measured: each line of the tables against GitHub.

## Approach

A workflow of `live.yml` (or its own, by hand) that runs the expression
vectors of the tests of `pod-actions-expr` in a job (`echo` of
`toJSON(...)` of each one) and a set of matrix and needs examples; a step
then runs a test binary of podcrates on the same vectors and compares. A
difference becomes a defect entry or a line of the known differences.

## Prove

```sh
cargo test -p pod-actions-expr -p pod-actions-workflow
```

# T-309: `pod-actions-workflow`: a `Run` that owns its workflow (podrunner's ask)

**Source:** a direct message of the podrunner session, 2026-11-12, for PodUtils/podrunner@T-087 (its `LocalSource` at `5dd3251`).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`Run<'w>` borrowed its `Workflow`. A job source of podrunner owns its run
and must be `Send + 'static`, so it cannot keep the workflow and the run
together; podrunner leaks one `Workflow` for each source
(`Box::leak`). The ask: a `Run` that owns its workflow, with the same
`ready`, `expand`, `finish`, `result` and `is_finished`. podrunner found
no defect of the crate: its `LocalSource` passes its conformance suite.

## Premise

- Read: `crates/pod-actions-workflow/src/expand.rs` (`Run`, `plan`) and its
  uses in the crate, its tests and `fuzz/src/targets/actions.rs`.

## Approach

`Run<W: Borrow<Workflow>>`: `&Workflow` as before, `Workflow` or
`Arc<Workflow>` to own it. `ready` gives owned ids, so that a caller
expands each one while it holds the list.

## Prove

```sh
cargo test -p pod-actions-workflow --all-features
```

## Done

2026-11-12. The commit that names T-309.

- `Run<W: Borrow<Workflow>>` with `Run::new(workflow: W, base)` and
  `Run::workflow()`; `ready()` gives `Vec<String>`. Breaking: a type
  `Run<'w>` is `Run<&'w Workflow>`; a caller passes `&id` to `expand` and
  `finish`. `Run::new(&workflow, ...)` and `plan` are as before.
- Test `a_run_can_own_its_workflow`: a run over `&Workflow`, over
  `Workflow` and over `Arc<Workflow>` (each of the last two
  `Send + 'static`) gives the same jobs. `cargo test -p
  pod-actions-workflow --all-features`: 42 passed; clippy is clean.

# T-322: `pod-actions-expr`: a public view of the parts of a `Template` (podrunner's ask)

**Source:** a direct message of the podrunner session, 2026-11-12, for PodUtils/podrunner@T-089 (`podrunner run FILE`).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podrunner writes the runner's `format()` token of a template (the literal
text with `{0}`, `{1}` and the expressions in order). `Template` keeps its
parts private, so podrunner would need its own copy of the scan of
`${{ }}`.

## Premise

- Read: `crates/pod-actions-expr/src/template.rs` (`Part`, `Template`).

## Approach

A public `TemplatePart` (`Text(&str)`, `Expr(&Expr, index)`) and
`Template::parts()`, in the order of the text.

## Prove

```sh
cargo test -p pod-actions-expr --all-features
```

## Done

2026-11-12. The commit that names T-322.

- `TemplatePart::{Text, Expr}` and `Template::parts()`; two literal texts
  are never next to each other; the index of an expression points at its
  text (`Expr::text`) in the template.
- Test `a_template_gives_its_parts_in_order` (an expression with `}}` in a
  string literal among them). `cargo test -p pod-actions-expr
  --all-features`: 46 passed; clippy is clean.

# T-425: podswarm: record each ask of #107 and each file of the spec of #108 as entries, when the index of #107 is complete

**Source:** GitHub #107 and #108 (podswarm, a new consumer).
**Category:** chore
**Milestone:** M8
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podswarm files 33 asks to PodUtils in #107 (20 to podcrates, P-nn; 8 to
podvm; 5 to podnet) and the text of its spec in the comments of #108. It
names 16 rulings that the owner gives before podswarm has code. The
issues and comments are still being filed: the body of #107 says that its
index is not complete. The record has no entry for an ask of podswarm.

## Premise

- Read: the bodies of #107 and #108 (2026-11-13 00:23 and 00:25 UTC).
- Not measured: the asks themselves (not yet indexed).

## Approach

When a comment on #107 says that the index is complete: read each ask
P-nn and the files of the spec that it names; write one entry for each
ask (a new crate, a feature, a fix) with its Problem and Approach, and map
each issue in `TODO/issues.md` and `TODO/github.txt`. The asks to podvm
and podnet are for those sessions. Place the entries by the owner's
answer to Q15; until then they wait, as M8 does.

## Prove

```sh
cargo todo check
```

## Correction

2026-11-13: the owner's ruling "podswarm" (`docs/decisions.md`) settles
Q15 and the 16 rulings; their list is now in the text of that ruling.
Call C245 places each entry: T-439, T-444, T-447 and T-448 help a sibling
and leave M8; the others stay in M8 and wait until the podswarm session
starts. T-438 is done: the owner reserved the seven names.

## Done

2026-11-13: 21 entries for the 20 asks of podswarm to podcrates (P-1 has
two). Commits `5e11651` and `dacc68f`, then the commit of the map, of Q15
and of this section.

- Read: GitHub #107 and its comment (the index is complete), #109 to
  #125, the comments of podswarm on #1, #82 and #88, and the archive of
  the spec (files 21, 22 and 26) in place of the comments of #108.
- New crates: T-430 and T-431 (`pod-proc`), T-432 (`pod-tty`), T-433
  (`pod-fuse`), T-434 (`pod-cas`), T-435 (`pod-view`), T-436
  (`pod-attest`), T-437 (`pod-ssh`), T-438 (the seven names; crates.io
  answered 404 for each on 2026-11-13).
- Changes of crates: T-439 (`pod-ws`), T-440 (`pod-tls`), T-441
  (`pod-proxy`), T-442 (`pod-link`), T-443 and T-444 (`pod-hostprobe`),
  T-445 and T-446 (`pod-hold`), T-447 (`pod-guest`), T-448 (`pod-enter`),
  T-449 (`pod-job`), T-450 (`pod-core`).
- Overlap: the report with its schema (P-5) is T-107; its Premise names
  podswarm, and T-443 holds the gap. P-3 is narrowed to the enrolment
  mode (T-304 has the rest); P-10 to the argv form, the stderr pipe and
  the wait.
- Priority P2 for an ask that podswarm's M0 or M1 uses (spec, file 22,
  sections 3 and 4: M1, the first remote release, builds on M0): 10
  entries. P3 for the others: 11 entries. Each is M8 and waits for Q15.
- The asks to podvm (#124) and to podnet (#125) are for those sessions.
  The 16 rulings are under Q15 in `TODO/PROGRESS.md`.
- `cargo todo check`: the record agrees.

# T-430: `pod-proc`: a spawner with keepers, the containment rungs, a fence and the tree kill on each target (GitHub #113)

**Source:** GitHub #113 (podswarm P-1).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** L
**Status:** open

## Problem

podswarm starts many short attempts on a worker. It needs one spawner,
started before any thread, that runs each attempt in a keeper, kills the
whole tree at its end or at a fence time (also when the caller is
stopped), and reports the exit, the CPU time, the peak memory and the
residual count. The parts are in three crates, Linux only, and serve one
detached payload for each launcher process. podvm and podrunner take the
tree kill of `pod-supervise` and can take the same crate.

## Premise

- Read (2026-11-13): GitHub #113; podswarm spec, file 21, sections 6.1
  and 7 (ask P-1, milestone M0); file 22, section 3; file 12.
- In podcrates now: `crates/pod-supervise/src/launcher/tree.rs` has the
  walk of the descendants and `TREE_KILL_MS` (5 s);
  `crates/pod-supervise/src/end.rs` has `Residual`, a copy of `Residual`
  of `crates/pod-job/src/report.rs` with the same JSON (a number or
  `"unknown"`); `crates/pod-enter/src/limits.rs` has `Resource` and
  `Limit`; `crates/pod-hostprobe/src/sys.rs` gives
  `set_child_subreaper`, `reap_any` and `set_rlimit`. `pod-supervise` and
  `pod-enter` build on Linux only (`#![cfg(target_os = "linux")]`).
- The base is T-162 (done): the tree kill, the wall timeout, rusage and
  the residual count of `pod-supervise`.
- The name: crates.io answered 404 for `pod-proc` on 2026-11-13 (the
  reservation is T-438).
- Not measured: the rungs `Cgroup`, `Pidns`, `Reaper`, `Job` and
  `Session` on their systems; ConPTY.
- The owner's ruling "podswarm" (2026-11-13, `docs/decisions.md`; call
  C245): this ask helps podswarm alone. podvm and podrunner have the tree
  kill of `pod-supervise` (T-162) and asked for no rung. It stays in M8
  and waits until the podswarm session starts.

## Approach

A new crate `pod-proc` by the procedure of `docs/design.md`, section 8,
with the interface of file 21, section 6.1: `start(Options)` before any
thread, `Spawner::{rung, spawn, signal, fence, event_fd, next_event}`,
`Plan` (argv, environment, cwd, `Stdio` with `Pty`, limits, umask,
`setns` and a compiled `pod-confine` set on Linux) and `Exit` with
`pod_job::Residual`. No tokio. The rungs by target: `Cgroup` (a delegated
cgroup2 leaf, `cgroup.kill`), `Pidns` (the keeper is PID 1), `Subreaper`,
`Reaper` (FreeBSD `PROC_REAP_KILL`), `Job` (a Windows Job Object),
`Session` (macOS and the BSDs); a rung that a target cannot give is
absent there. The terminal of an attempt: `openpty` on Unix, ConPTY on
Windows. The spawner and each keeper install a panic hook that calls
`abort`. Move `tree` out of `pod-supervise` (it depends on `pod-proc` and
keeps its API), remove `pod_supervise::end::Residual` for
`pod_job::Residual`, and move `Resource` and `Limit` out of `pod-enter`
(it re-exports them). The crate goes into `examples/static-probe` and
into `docs/design.md`, section 4. The exec takeover is T-431.

## Prove

```sh
cargo test -p pod-proc --all-features
cargo test -p pod-supervise --all-features
cargo test -p pod-enter --all-features
python3 -I scripts/check-crates-io-names.py
```

The tests: two attempts at once, one leaks a `setsid env -i sleep`
grandchild (both exits and residual counts are right, only its tree is
killed); a `SIGKILL` of the caller, of the spawner and of a keeper leaves
no attempt process after the kill grace plus 2 s; with no `/proc` the
residual count is `unknown`; a fence time that passes while the caller is
stopped kills each tree; `Stdio::Pty` gives a terminal and a window
change reaches it. The static probe builds on the 14 targets; the
adoption trials of podvm and podrunner pass.

## Start condition

The podswarm session starts, or a sibling asks for it (the owner's ruling
"podswarm", 2026-11-13).

# T-431: `pod-proc`: the exec takeover of a new process (`PTRACE_SEIZE`, the stop at the exec, the stub) (GitHub #113)

**Source:** GitHub #113 (podswarm P-1).
**Category:** feature
**Milestone:** M8
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

The spreading of podswarm moves a new process of a build to another
machine. A supervisor seizes the process at its `execve`, decides, and
lets it run here or replaces it with a stub that runs the program far
away. The process keeps its pid and the exit status of a local run.

## Premise

- Read (2026-11-13): GitHub #113 (the module `takeover`); podswarm spec,
  file 21, sections 6.1 and 6.5; file 07, section 3.2; file 22, section 6
  (M3, spreading).
- In podcrates now: no code seizes a process. `pod-hold` gives the
  listener, the memory reads and the replies under it
  (`crates/pod-hold/src/listen.rs`, `crates/pod-hold/src/mem.rs`,
  `crates/pod-hold/src/reply.rs`); the marker and the argv reads are
  T-445.
- Not measured: `PTRACE_SEIZE` under Docker, AppArmor and
  `ptrace_scope` (the spec's risk K-03).
- The owner's ruling "podswarm" (2026-11-13, `docs/decisions.md`; call
  C245): this ask helps podswarm alone (its spreading). It stays in M8 and
  waits until the podswarm session starts.

## Approach

The module `pod_proc::takeover` on Linux x86_64 and aarch64 (after T-430
and T-445): `PTRACE_SEIZE`, the stop at the exec, the trampoline, the
exec of the caller's stub, the detach. It uses the items of `pod-hold`
for the filter, the notices and the reads of the stopped task.

## Prove

```sh
cargo test -p pod-proc --all-features
```

The tests, with a stub of the test: a `fork` plus `execve` that moves; a
`posix_spawnp` (`CLONE_VM | CLONE_VFORK`) that moves; a local exec that
stays and runs untouched; a failed exec whose caller sees `ENOENT`; then
a caller with threads and a `#!` script. A moved process keeps its pid
and the exit status of a local run.

## Start condition

T-430 and T-445 are done. The podswarm session starts, or a sibling asks
for it (the owner's ruling "podswarm", 2026-11-13).

# T-432: `pod-tty`: the guard of the caller's terminal (raw mode, size, restore, escape line) on Unix and Windows (GitHub #111)

**Source:** GitHub #111 (podswarm P-11).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

`podswarm run HOST -- CMD` attaches a far command to the caller's
terminal: raw mode, the window size and its changes, the escape line
(`~.`), and a restore at exit, on a panic and on `SIGTERM` or `SIGHUP`.
podssh has this code in its client (`podssh-ssh`, `terminal/` and
`escape.rs`); a second copy in podswarm drifts.

## Premise

- Read (2026-11-13): GitHub #111; podswarm spec, file 21, section 6.7;
  file 18, section 4.1; file 22, section 4 (M1).
- In podcrates now: `pod-term` serves the far side of a session with no
  pty (`crates/pod-term/src/echo.rs`, `crates/pod-term/src/escape.rs`,
  `crates/pod-term/src/window.rs`); it reads and writes bytes only. No
  crate touches the local terminal.
- Not read: the modules of `PodUtils/podssh:crates/podssh-ssh` that the
  cut takes.
- The name: crates.io answered 404 for `pod-tty` on 2026-11-13 (T-438).
- The owner's ruling "podswarm" (2026-11-13, `docs/decisions.md`; call
  C245): this ask helps podswarm alone: podssh keeps its own terminal
  code, and the crate has no other consumer. It stays in M8 and waits
  until the podswarm session starts.

## Approach

A new crate `pod-tty` cut from podssh's `terminal/` and `escape.rs` by
the procedure of `docs/design.md`, section 8, with the interface of file
21, section 6.7: `Size` (with `cells()` to `pod_term::window::Size`),
`is_terminal`, `size`, `RawMode::enter` (termios `cfmakeraw` on Unix; the
console input and output modes with VT on Windows), `raw_active`, `Watch`
(tokio: `SIGWINCH`, `SIGTERM`, `SIGHUP`; a 500 ms poll on Windows) and
`escape`. Its two globals (the saved state and the raw flag, for the
panic hook and the console handler) are named in its documentation. The
parts of podssh that need `russh` stay in podssh. The crate goes into
`examples/static-probe` and `docs/design.md`, section 4; podssh's take is
its adoption trial.

## Prove

```sh
cargo test -p pod-tty --all-features
python3 -I scripts/check-crates-io-names.py
```

The tests: a process on a test pty enters raw mode and ends by drop, a
panic, `SIGTERM`, `SIGHUP` and the end of a session whose command died by
`SIGKILL`; the termios bytes after each equal the bytes before. Windows:
the console modes after a drop and after a panic equal the modes before.
One `Resize` for one window change. `~.` after a newline gives
`Disconnect`; `~.` inside a line passes; `~~` gives one `~`.

## Start condition

The podswarm session starts, or a sibling asks for it (the owner's ruling
"podswarm", 2026-11-13).

# T-433: `pod-fuse`: a FUSE server with no libfuse, with the write opcodes, `READDIRPLUS` and invalidation (GitHub #110)

**Source:** GitHub #110 (podswarm P-16).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** L
**Status:** open

## Problem

The live view of podswarm shows the caller's directory on the far machine
at the same path: it reads on demand and writes back. It needs a writable
FUSE server behind a trait of the caller, with invalidation notices and
more than one reader. The FUSE server of podcrates is read only and
inside `pod-enter`.

## Premise

- Read (2026-11-13): GitHub #110; podswarm spec, file 21, section 6.3;
  file 06, section 8.2; file 22, section 4 (M1).
- In podcrates now: `crates/pod-enter/src/fuse.rs`,
  `crates/pod-enter/src/fuse/wire.rs` and `crates/pod-enter/src/fuse/fs.rs`
  serve a fixed tree from a forked single-thread child with no libfuse.
  Each opcode of `OP_MUTATING` answers `EROFS`; no `READDIRPLUS`, no
  notices, no cloned descriptors, no trait for the tree.
- The name: crates.io answered 404 for `pod-fuse` on 2026-11-13 (T-438).
- Not measured: a real mount here (the session has no `/dev/fuse`).
- The owner's ruling "podswarm" (2026-11-13, `docs/decisions.md`; call
  C245): this ask helps podswarm alone: the read-only rung of `pod-enter`
  (podvm) gains nothing from a writable server. It stays in M8 and waits
  until the podswarm session starts.

## Approach

A new crate `pod-fuse` with the wire and the mount of `pod-enter`, by the
procedure of `docs/design.md`, section 8, and the interface of file 21,
section 6.3: `mount` (`mount(2)` with `fd=`, `nosuid`, `nodev`; no
`fusermount3`), the trait `Fs`, `serve`, and `Session::notifier`
(`inval_entry`, `inval_inode`). Add the write opcodes (`CREATE`, `MKDIR`,
`UNLINK`, `RMDIR`, `RENAME`, `SETATTR`, `SYMLINK`, `LINK`, `WRITE`,
`FSYNC`, `RELEASE`), `READDIRPLUS`, the `INIT` flags of the issue and
cloned session descriptors. Keep the forked single-thread server. The
read-only rung of `pod-enter` moves onto the crate with its tests
unchanged. A fuzz target of the request parser. The text fix of
`fuse.rs` is T-448.

## Prove

```sh
cargo test -p pod-fuse --all-features
cargo test -p pod-enter --all-features
python3 -I scripts/check-crates-io-names.py
```

The tests: one socket-pair test for each write opcode (the reply bytes
and the call on a test `Fs`); `READDIRPLUS` gives each entry with its
attributes in one reply; the notices write the bytes of the kernel ABI; a
real mount of an in-memory `Fs` as root and in a new user namespace where
`/dev/fuse` opens (else `skipping, REASON`); two readers on cloned
descriptors. The static probe builds on the 14 targets; podvm's trial
through `pod-enter` passes.

## Start condition

The podswarm session starts, or a sibling asks for it (the owner's ruling
"podswarm", 2026-11-13).

# T-434: `pod-cas`: a BLAKE3 content store with resume, placement, sweep and tree manifests (GitHub #109)

**Source:** GitHub #109 (podswarm P-12).
**Category:** feature
**Milestone:** M8
**Priority:** P3
**Effort:** L
**Status:** open

## Problem

podswarm keeps the files of the live view, the staged inputs and outputs
of queued tasks and the checkpoints in one store keyed by the BLAKE3 of
their bytes, with a transfer that resumes at an offset and a placement by
reflink. podssh `run HOST:DIR` and podrunner are candidates, not proven
consumers. podswarm says that podcrates can wait for a stable API from
its code.

## Premise

- Read (2026-11-13): GitHub #109; podswarm spec, file 21, section 6.2;
  file 15, sections 1 and 2; file 22, section 9 (M6); file 26, risk K-34.
- In podcrates now: no general content store. `pod-oci` writes an OCI
  image layout with blobs by their SHA-256 digest
  (`crates/pod-oci/src/layout.rs`); it is not a store keyed by BLAKE3
  with a resume by offset, a placement by reflink and a sweep.
- The name: crates.io answered 404 for `pod-cas` on 2026-11-13 (T-438).
- Not measured: the `blake3` crate with its C and assembly paths on the 14
  targets.
- The owner's ruling "podswarm" (2026-11-13, `docs/decisions.md`; call
  C245): this ask helps podswarm alone: podssh and podrunner are
  candidates only. It stays in M8 and waits until the podswarm session
  starts.

## Approach

A new crate `pod-cas` with the interface and the formats of file 21,
section 6.2: `Id` (`b3:` and keyed `b3k:`), the layout
`<root>/blobs/<2 hex>/<62 hex>` with partial files in
`<root>/blobs/.partial/`, a writer that hashes while it writes and
resumes at the size held, `read_range`, `place` (`FICLONE`, else a copy;
never a hard link), `sweep` with an age guard, `Manifest` and `missing`,
and `transfer` (the header and the answers `have`, `done`, `err`). The
`blake3` crate, with the feature `pure` on a target where its C does not
build. Fuzz targets of the manifest parser and the transfer header.

## Prove

```sh
cargo test -p pod-cas --all-features
python3 -I scripts/check-crates-io-names.py
```

The tests: a cut write resumes at its offset and gives the id of an uncut
write; bytes that do not match their id are refused and the partial file
goes; a chunk at a wrong offset is refused with no sparse file; the sweep
keeps a young blob; a write to a placed file does not change the object;
a manifest round trip. The static probe builds on the 14 targets.

## Start condition

The podswarm session starts, or a sibling asks for it (the owner's ruling
"podswarm", 2026-11-13). podswarm shows the API in its code (its milestone
M6).

# T-435: `pod-view`: a live view of a directory (records, a frame codec, an ordered write-back log) (GitHub #114)

**Source:** GitHub #114 (podswarm P-17).
**Category:** feature
**Milestone:** M8
**Priority:** P3
**Effort:** L
**Status:** open

## Problem

The far machine of podswarm sees the caller's directory live: records
with a version, a lazy validation, the frames of the view, a cache, an
upper layer and a write-back log that the home applies in order, with
conflict copies and held items. podssh `run HOST:DIR` and podrunner's job
workspaces are candidates. podswarm files the final ask after its M2,
when its four view rungs share one API; the issue states the shape now.

## Premise

- Read (2026-11-13): GitHub #114; podswarm spec, file 21, sections 6.4
  and 9; file 06, sections 5, 6, 9 and 10; file 22, section 5 (M2); file
  26, risk K-34 and ruling R-16.
- In podcrates now: no crate of a live view. `pod-enter` has a read-only
  FUSE server; `pod-link` carries streams with credit windows.
- It takes T-433 (`pod-fuse`) and T-434 (`pod-cas`).
- The name: crates.io answered 404 for `pod-view` on 2026-11-13 (T-438).
- The owner's ruling "podswarm" (2026-11-13, `docs/decisions.md`; call
  C245): this ask helps podswarm alone: podssh and podrunner are
  candidates only. It stays in M8 and waits until the podswarm session
  starts.

## Approach

A new crate `pod-view` with the interface of file 21, section 6.4:
`Index` (`open`, `record`, `apply`), the modules `frame` (a `u32` length,
a `u8` op with a zstd flag, varint fields; 1 MiB, 4 MiB after inflation)
and `writeback` (`Log`, `Op`, `Held`, `Conflict`), the trait `ViewRung`
and its public `conformance` suite, the clock offset of mtimes. No
transport. Fuzz targets of the frame codec and of the record decoder.

## Prove

```sh
cargo test -p pod-view --all-features
python3 -I scripts/check-crates-io-names.py
```

The tests (loopback, temporary directories, frames on an in-memory
pipe): a cut pipe during write-back applies each op once in `seq` order;
a home file changed after `base_ver` gives a conflict copy; a watcher
overflow makes the next `record` validate again; a far clock at plus and
minus 30 s; a held mass unlink; a refused `nid`, symlink write and write
of `.git/hooks/x`; the `conformance` suite passes a test rung and fails a
planted bad rung.

## Start condition

T-433 and T-434 are done. podswarm files the final ask after its M2. The
podswarm session starts, or a sibling asks for it (the owner's ruling
"podswarm", 2026-11-13).

# T-436: `pod-attest`: the check of an SEV-SNP report and the expected measurement of a guest set (GitHub #115)

**Source:** GitHub #115 (podswarm P-19).
**Category:** feature
**Milestone:** M8
**Priority:** P3
**Effort:** L
**Status:** open

## Problem

The shield `sealed` of podswarm sends no file or key to a guest before
the caller checked its SEV-SNP report against a measurement that the
caller computed. podvm can use the same check for confidential guests.

## Premise

- Read (2026-11-13): GitHub #115; podswarm spec, file 21, section 6.6;
  file 13, section 8; file 22, section 11 (M8).
- In podcrates now: `pod-guest` has the plan of a QEMU guest
  (`crates/pod-guest/src/plan.rs`); no crate checks an attestation or
  computes a launch measurement. T-180 (`pod-mask`) is confidential
  execution with no hardware support, not this.
- The name: crates.io answered 404 for `pod-attest` on 2026-11-13
  (T-438).
- Not measured: the `sev` crate with `crypto_nossl` on x86_64 musl (the
  spec lists it as a gap).
- The owner's ruling "podswarm" (2026-11-13, `docs/decisions.md`; call
  C245): this ask helps podswarm alone: podvm has no confidential guest
  and asked for no check. It stays in M8 and waits until the podswarm
  session starts.

## Approach

A new crate `pod-attest` with the interface of file 21, section 6.6:
`verify(Platform, report, certs, data, Policy)` and
`expected_measurement(GuestSet, vcpus)`, on the `sev` crate
(`default-features = false`, `snp`, `crypto_nossl`) or equal pure Rust
code, no OpenSSL. The ARK and ASK of each product are embedded; the
caller gives the VCEK and the CRL; the crate fetches nothing. The checks
in order, each refusal named by its field: `chain`, `signature`,
`measurement`, `report_data`, `policy`, `vmpl`, `tcb` (with the
component), `chip`. TDX later. The licence of the dependency tree goes
through `deny.toml`; the test vector is public with its source and
licence beside it. Fuzz targets of the report and certificate parsers.

## Prove

```sh
cargo test -p pod-attest --all-features
cargo deny --locked --all-features check licenses sources
python3 -I scripts/check-crates-io-names.py
```

The tests: a public vector passes; one flipped bit in each checked field
fails by that field; another `data` fails on `report_data`; a floor above
the reported TCB fails on `tcb` with the component; the golden
measurements for 1, 2, 4 and 16 vCPUs. The crate links static on x86_64
musl.

## Start condition

The podswarm session starts, or a sibling asks for it (the owner's ruling
"podswarm", 2026-11-13).

# T-437: `pod-ssh`: an SSH client library on `russh` (host key policy, auth chain, exec channel) (GitHub #112)

**Source:** GitHub #112 (podswarm P-13).
**Category:** feature
**Milestone:** M8
**Priority:** P3
**Effort:** L
**Status:** open

## Problem

The `ssh` road of podswarm runs the system `ssh`. A caller with no
OpenSSH client (Windows) needs an SSH client library: a host key policy
on the caller's `known_hosts`, an auth chain (agent, key files, a prompt)
and an exec channel with stdin, stdout, stderr and the exit status or
signal, over any byte stream. podssh has this code in `podssh-ssh`.

## Premise

- Read (2026-11-13): GitHub #112; podswarm spec, file 21, section 6.8;
  file 04; file 22, section 13 (M10).
- In podcrates now: the workspace has `russh` 0.65 (`ring`, `rsa`) in
  `Cargo.toml`; `crates/pod-devtunnel/src/ssh.rs` runs a client over a
  stream with the keys of a tunnel, the auth `none` and `direct-tcpip`.
  No `known_hosts`, no agent, no exec channel.
- T-041 (open) brings podssh's SSH test server into `pod-testkit`; the
  tests of this entry can use it.
- The name: crates.io answered 404 for `pod-ssh` on 2026-11-13 (T-438).
- Not read: `PodUtils/podssh:crates/podssh-ssh`.
- The owner's ruling "podswarm" (2026-11-13, `docs/decisions.md`; call
  C245): this ask helps podswarm alone: podssh keeps its own client, and
  the crate has no other consumer. It stays in M8 and waits until the
  podswarm session starts.

## Approach

A new crate `pod-ssh` cut from `podssh-ssh` on the workspace `russh`,
with the interface of file 21, section 6.8: `HostPolicy` (the files,
fixed fingerprints, a callback for an unknown key; a changed or revoked
key always fails), `Auth` (agent, key file, keyboard-interactive,
password, after `none`), `Prompt`, `connect` over a `pod_core::BoxedIo`,
`Client::exec`, `Exec::split`, `Ended::wait`. The crate reads no
`ssh_config` and no variable. `pod-devtunnel` can move onto it later.

## Prove

```sh
cargo test -p pod-ssh --all-features
python3 -I scripts/check-crates-io-names.py
```

The tests against an OpenSSH `sshd` on the loopback with keys made at
test time: a 1 GiB stream keeps its BLAKE3 hash; a half close reaches the
command; exit status 0, 7 and 255 and an exit signal pass; stderr has its
own stream; a changed host key fails by name; `No` for an unknown key
ends the connection; the auth order. The static probe builds on the 14
targets with Windows.

## Start condition

The podswarm session starts, or a sibling asks for it (the owner's ruling
"podswarm", 2026-11-13).

# T-438: The names `pod-proc`, `pod-tty`, `pod-fuse`, `pod-cas`, `pod-view`, `pod-attest` and `pod-ssh` on crates.io (GitHub #1)

**Source:** GitHub #1 (podswarm P-21).
**Category:** release
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podswarm asks the owner to reserve the names of its seven new crates at
0.0.0 with the trusted publisher of podcrates, so that its fallback code
uses the final names from the start. A name that is not free gets
another name from the owner.

## Premise

- Read (2026-11-13): the comment of podswarm on GitHub #1; podswarm spec,
  file 21, section 7 (ask P-21, milestone M0); file 26, ruling R-03.
- Measured: `https://crates.io/api/v1/crates/NAME` answered 404 ("does
  not exist") for each of the seven names on 2026-11-13.
- `docs/design.md`, section 10: the owner reserves a new name before its
  first publish; `pod-*` names are of podcrates only. T-387 is the same
  step for `pod-devtunnel`.

## Approach

When the owner rules R-03: the owner reserves each name; the session adds
the names to the list of GitHub #1 and to `docs/design.md`, section 4
(with the crate of each when it exists, else under "Names held"), so that
`scripts/check-crates-io-names.py` reads them.

## Prove

```sh
python3 -I scripts/check-crates-io-names.py
```

## Start condition

The owner answers R-03 and reserves the names.

## Correction

2026-11-13: crates.io answers each of the seven names at `0.0.0` with the
repository `https://github.com/PodUtils/podcrates` (created between 01:14
and 02:17 UTC). The owner's ruling "podswarm" settles R-03 (call C244).

## Done

2026-11-13. Commit `228ea87`: the seven names are in "Names held, with no
crate" of `docs/design.md`, section 4 (call C244). GitHub #1 lists them in
its section "Registered on 2026-11-13".
`python3 -I scripts/check-crates-io-names.py`: 94 names, 1 fault
(`pod-devtunnel`: missing; T-387); each of the seven gives `0.0.0` and
the repository of podcrates.

# T-449: `pod-job`: `LimitMode::Polled`, a limit that a keeper measures and enforces by a kill (GitHub #118)

**Source:** GitHub #118 (podswarm P-8).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

A keeper that sums the use of a tree once a second and kills the tree
above the limit is stronger than `reported` and weaker than `enforced`.
On a host with no delegated cgroup2 it is the only hold on memory. A
consumer must report it as one of the two wrong values today.

## Premise

- Read (2026-11-13): GitHub #118; podswarm spec, file 21, section 7 (ask
  P-8, milestone M0); file 12.
- In podcrates now: `LimitMode` in `crates/pod-job/src/capability.rs` has
  `Enforced`, `Reported` and `None` (kebab case); no crate of the
  workspace matches on it; podrunner reads it.
- The owner's ruling "podswarm" (2026-11-13, `docs/decisions.md`; call
  C245): this ask helps podswarm alone: podrunner reads `LimitMode` and
  has no keeper that polls. It stays in M8 and waits until the podswarm
  session starts.

## Approach

Add `LimitMode::Polled` (`polled`) between `Enforced` and `Reported`, with
its text ("measured by the supervisor at a fixed period; the tree is
killed above the limit; the use can pass the limit for up to one
period"). Generate the schema `unit-capability.v1` again with the
feature `schema`. Name the new value in the release notes.

## Prove

```sh
cargo test -p pod-job --all-features
```

The tests: `"polled"` decodes to `LimitMode::Polled` and encodes back;
the generated schema holds `polled` and the schema check passes.

## Start condition

The podswarm session starts, or a sibling asks for it (the owner's ruling
"podswarm", 2026-11-13).

# T-505: `pod-job`: the actor classes `agent` and `bot` in `job-offer.v1` (PodUtils/podrunner#80)

**Source:** PodUtils/podrunner#80 (COORD-020 of podrunner, for PodUtils/podrunner@T-106).
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

githome states the class of the actor of each job in
`trust_inputs.actor_class` of `job-offer.v1`: `agent` for a change that
an agent made, `bot` for the forge itself. `ActorClass` of `pod-job` has
`member`, `outside` and `unknown` only, so such an offer does not parse
and podrunner refuses it.

## Premise

- Read: PodUtils/podrunner#80 (2026-11-13); `crates/pod-job/src/offer.rs`,
  `src/schemas.rs`, `schemas/job-offer.v1.schema.json`.

## Approach

`ActorClass::{Agent, Bot}` (`agent`, `bot`), `outside` kept; the schema
written again; an example of each, and one with githome's word
`external`, which stays refused.

## Prove

```sh
cargo test -p pod-job --all-features
```

## Done

2026-11-13, commit `0ed9763`. `ActorClass` has `member`, `outside`,
`agent`, `bot` and `unknown`; the type's doc names each. The schema
`schemas/job-offer.v1.schema.json` lists the two words in the same
`enum` (a doc comment on a variant would make it a `oneOf`, so the
variants carry plain comments). New examples: `valid-agent-change.json`,
`valid-bot-push.json`, and `invalid-actor-external.json` (refused:
`unknown variant `external``). The test of the examples counts 14 valid
and 22 invalid; the README said 11 and 19, which was out of date, and
now says 14 and 22. `cargo test -p pod-job --all-features`: each passes.

# T-527: `pod-job`: the CPU time and the peak of resident memory of a job in `run-report.v1` (githome's G-06, #83)

**Source:** #83 (githome's comment of 2026-11-13 12:52, G-06); PodUtils/podrunner#86 (COORD-026 of podrunner, for its T-108).
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

githome reads `cpu_s` and `peak_rss_mib` of `run-report.v1` for the
record of a job and its cost view (`unknown` stays null, never 0). The
report of `pod-job` has `wall_s` only, and `deny_unknown_fields`, so a
report with the two fields does not parse, and podrunner cannot send
them.

## Premise

- Read: #83 (2026-11-13); `crates/pod-job/src/report.rs`,
  `schemas/run-report.v1.schema.json`, `src/schemas/rules.rs`.

## Approach

Two fields in `RunReport` and `PublicReport`, in the form of `Residual`
(a count or the word `unknown`, as PodUtils/podrunner#86 asks): `cpu_s`
(the CPU time of the processes of the unit, user and system, from its
start to its destroy, in seconds) and `peak_rss_mib` (their highest
resident memory, in MiB); absent when no unit ran. The schemas written again; an example of each
report with them.

## Prove

```sh
cargo test -p pod-job --all-features
```

## Done

2026-11-13, commit `4409dc1`. `RunReport::{cpu_s, peak_rss_mib}` and
`PublicReport::{cpu_s, peak_rss_mib}` (`Option<Residual>`: a count or
`unknown`; absent when no unit ran); `RunReport::public` copies them. The two schemas name them. The
fields of the public report are one list (`PUBLIC`) in the test that holds
the public report to no host detail: CPU time and memory are facts of the
job, not of the host. New examples `run-report/valid-usage.json` and
`run-report-public/valid-usage.json` (16 valid and 22 invalid in all).
`cargo test -p pod-job --all-features`: each passes. A struct literal of
`RunReport` or `PublicReport` in a consumer needs the two fields.

# T-553: `pod-job`: the container route `engine` and the need `container_engine`, for a container engine inside the unit (PodUtils/podrunner#91)

**Source:** PodUtils/podrunner#91 (COORD-031 of podrunner; PodUtils/podrunner@T-103, githome's G-09).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`services:`, `docker build`, Dockerfile actions and Docker-in-Docker need a container engine. podrunner runs one inside the unit (a guest of the `microvm` kind), with its socket inside the unit only. `ContainerRoute` has `none`, `hooks` and `unknown`: the capability record cannot say that the unit has its own engine, and an offer cannot ask for one.

## Premise

- Read: PodUtils/podrunner#91; `crates/pod-job/src/capability.rs` (`ContainerRoute`), `src/offer.rs` (`UnitNeed`), `src/schemas.rs` (the schemas generated from the types, and the examples).

## Approach

`ContainerRoute::Engine` (serde `engine`). `UnitNeed::container_engine: bool`, default false, so that placement sends such a job only to a kind with `engine`. The schemas `unit-capability.v1` and `job-offer.v1` are written again from the types. A valid example of each.

## Prove

```sh
cargo test -p pod-job --all-features
```

## Done

2026-11-14. `ContainerRoute::Engine` and `UnitNeed::container_engine`; `schemas/unit-capability.v1.schema.json` and `schemas/job-offer.v1.schema.json` written again; the examples `unit-capability/valid-engine.json` and `job-offer/valid-engine-need.json` (18 valid, 22 invalid). The plant `job-container-engine-dropped` (no variant `engine`) fails `each_example_passes_or_fails_for_its_reason` on `valid-engine.json`.

# T-564: `pod-actions-store`: the actions by tag, a sorted lock of commits and digests, a store of tarballs and a git forge (PodUtils/podrunner#94)

**Source:** PodUtils/podrunner#94 (COORD-034 of podrunner; PodUtils/podrunner@T-118, commit `241acb8`).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podrunner asks for a crate `pod-actions-store` from its code of the actions by tag. The lock file: each line is `OWNER/REPO@REF COMMIT sha256:HEX`, and the lines are sorted. `workflow_uses` collects each remote `uses:` of a `pod_actions_workflow::Workflow`. `resolve` follows each composite action until no new reference is left. The `Forge` trait has `resolve(repo, ref)` and `tar(repo, commit)`; the `Git` forge uses `ls-remote` with the peeled tag, a shallow fetch and `git archive`. `Mode` is `Online`, `Frozen` or `Offline`. The store keeps `OWNER_REPO/SHA.tar.gz`, and the digest covers the uncompressed tar, so the version of git or gzip does not change it. githome and the reusable workflows of podrunner's local mode (PodUtils/podrunner@T-131) need the same resolve and lock; PodUtils/podrunner@T-130 will add image tags to the same lock. podrunner keeps its copy until a tag of podcrates has the crate.

## Premise

- Read: PodUtils/podrunner#94; podrunner at `241acb8`: PodUtils/podrunner:crates/podrunner-local/src/lock.rs, src/store.rs, src/store/tests.rs, tests/store.rs, Cargo.toml, and `archive_name` of PodUtils/podrunner:crates/podrunner-broker-fake/src/actions.rs.
- The workspace has `flate2` (the Rust backend), `tar` (no default feature) and `sha2`; podrunner uses `ring` for the digest.
- `pod-actions-store` is not on crates.io (the crates.io API, 2026-11-14).

## Approach

Import the four files, then adapt them to the rules of `docs/design.md`, section 1: a typed `Error`, the scratch directory of `Git` and the `except` of `workflow_uses` as parameters, `sha2` for the digest (calls C320 to C324). Keep podrunner's names and its tests. Add a test of `Git` against a bare repository made in the test, the live test in `live.yml`, the crate in the static probe, and plants for the sort of the lock, the frozen mode that must not reach the forge, and the digest of the uncompressed tar.

## Prove

```sh
cargo test -p pod-actions-store --all-features
cargo test -p pod-actions-store --test store -- --ignored --nocapture
sh scripts/plant-each.sh actions-store-lock-unsorted actions-store-frozen-reaches-the-forge actions-store-digest-of-the-gzip actions-store-forge-commit-unchecked actions-store-git-tag-not-peeled
```

## Done

2026-11-14. The commits that name T-564: the import of the four files at `241acb8`, then the crate.

- `crates/pod-actions-store`: `lock::{Lock, Pin, HEADER, is_commit}` (`render_with`, `write_with` for the consumer's comment), `store::{Forge, Git, Mode, Resolved, Store, resolve, workflow_uses, digest, archive_name}`, `Error` and `LineFault` (calls C320 to C324). The digest is `sha2`; `flate2` and `tar` are the workspace's; `Cargo.lock` takes no new crate. The static probe calls `workflow_uses`, `digest` and a lock read back.
- `cargo test -p pod-actions-store --all-features`: 13 unit tests (podrunner's nine, each with its name, and `a_lock_file_is_written_whole_and_its_errors_name_the_file`, `frozen_asks_the_forge_nothing_for_a_reference_not_in_the_lock`, `the_digest_is_of_the_uncompressed_tar`, `a_forge_that_gives_no_commit_id_is_refused`), 3 tests of `Git` against a bare repository made in the test (`tests/git.rs`), 1 doc test; passed. The live test `tests/store.rs` passed here (`actions/upload-pages-artifact@v3` and `actions/upload-artifact@v4`), and runs in the job `live` of `live.yml`.
- Plants, each caught by hand: `actions-store-lock-unsorted` (`sorted: `), `actions-store-frozen-reaches-the-forge` (`frozen reached the forge`), `actions-store-digest-of-the-gzip` and `actions-store-forge-commit-unchecked` (their tests fail), `actions-store-git-tag-not-peeled` (`an annotated tag gives its commit`).
- clippy and rustfmt are clean; `check-windows.sh` and `check-macos.sh`: 0 faults; `check-probe.py`, `check-publish-order.py`, `check-design-paths.py`, `check-workflows.py`, `check-sources.py pod-actions-store` (4 files, 0 changes) agree.
- Next: the owner reserves `pod-actions-store` on crates.io (`check-crates-io-names.py`: 95 names, 1 fault: `pod-actions-store` missing); a release by a git tag does not need it. podrunner takes the crate at the next tag.

# T-565: `pod-actions-results`: the ArtifactService of the results API and its blob routes, one store for each run (PodUtils/podrunner#95)

**Source:** PodUtils/podrunner#95 (COORD-035 of podrunner; PodUtils/podrunner@T-119).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podrunner asks for a crate `pod-actions-results` from crates/podrunner-broker-fake/src/artifacts.rs of PodUtils/podrunner@T-119 (commit `0ff670d`; 0BSD). githome and a forge mode need the same service; the cache service of PodUtils/podrunner@T-120 is the next part of the same API. The ask:

- Serve the twirp JSON routes of `github.actions.results.api.v1.ArtifactService`: `CreateArtifact`, `FinalizeArtifact`, `ListArtifacts`, `GetSignedArtifactURL` and `DeleteArtifact`.
- Serve the block blob calls of the signed URLs: a whole PUT, `comp=block` and `comp=blocklist`.
- One store serves each run. Each URL carries a random signature. A name is unique in a run. The finalize checks the size and the SHA-256. A run has a limit of bytes.

The measured facts of the ask: `actions/download-artifact@v4` refuses a `google.protobuf.StringValue` that is not a plain string in the JSON; the client reads its run and job ids from the `Actions.Results:RUN:JOB` scope of `ACTIONS_RUNTIME_TOKEN`; runner v2.338.2 sets `ACTIONS_RESULTS_URL` from `ResultsServiceUrl` in the data of the `SystemVssConnection` endpoint. podrunner keeps its code until a tag of podcrates has the crate; nothing is blocked.

## Premise

- Read (2026-11-14): PodUtils/podrunner#95; podrunner at `0ff670d`: crates/podrunner-broker-fake/src/artifacts.rs, src/artifacts/tests.rs (5 tests), src/lib.rs (the broker's `serve`, which gives each request to `ArtifactStore::route` before its own routes), src/message.rs (the job token with `Actions.Results:1:2`, and `ResultsServiceUrl`), its Cargo.toml, and PodUtils/podrunner@T-119.
- The source checks a name at `CreateArtifact` only: `FinalizeArtifact`, `GetSignedArtifactURL` and `DeleteArtifact` join any name to the directory of the store. It reads `/dev/urandom` and keeps a signature of zeros when the read fails. It compares a signature with `!=`. It reads a whole blob into memory at the block list, at the finalize and at the download.
- Measured: `https://crates.io/api/v1/crates/pod-actions-results` answered 404 on 2026-11-14: the name is not reserved.
- Not measured here: a run of the v4 actions against the crate (podrunner's parity row `07-artifacts.yml` does it on the adoption).

## Approach

A new crate with no HTTP server: `ArtifactStore::answer(method, path, body, base)` gives a typed `Answer` (the zip of a download as an open file), and `route` keeps podrunner's form (call C325). The layout `DIR/NAME/` stays; the upload moves into the directory of its name, and each call checks its name (C326). The signature comes from `pod_core::entropy` and is compared in constant time (C327). The defects of the wire are fixed, each with a test (C328). `token::backend_ids` and `results_scope` hold the scope of the runtime token (C329). The README holds the facts of the wire. The name on crates.io is an action of the owner (`docs/design.md`, section 10).

## Prove

```sh
cargo test -p pod-actions-results --all-features
cargo clippy -p pod-actions-results --all-targets --all-features -- -D warnings
sh scripts/plant-each.sh results-sha256-unchecked results-no-limit results-sig-length-only results-digest-wrapped results-name-escapes
python3 -I scripts/check-probe.py --no-tree
python3 -I scripts/check-publish-order.py
python3 -I scripts/check-design-paths.py
```

## Done

2026-11-14. The import commit (podrunner at `0ff670d`, the blobs of `SOURCES`) and the commit `pod-actions-results: the ArtifactService of the results API and its blob routes, one store for each run`. `ArtifactStore` (`new`, `with_entropy`, `answer`, `route`, `list`, `dir`, `used`), `Answer` and `Payload`, `SERVICE`, `BLOBS`, `LIMIT`, `token::{backend_ids, results_scope}` (calls C325 to C329). `cargo test -p pod-actions-results --all-features`: 5 tests of podrunner with their names, 14 tests of the public API. Each plant fails its test: `results-sha256-unchecked` (`a_wrong_digest_or_size_fails_the_finalize`), `results-no-limit` (`an_upload_needs_its_signature_and_the_run_has_a_limit`), `results-sig-length-only` (`the_signature_is_compared_in_full`), `results-digest-wrapped` ("a wrapper is a plain string"), `results-name-escapes` (`a_name_outside_the_store_is_refused`); `sh scripts/plant-each.sh --apply`: 0 stale. clippy of the crate and `cargo fmt --all --check`: clean. `scripts/check-probe.py --no-tree`: 39 crates, 0 faults; `scripts/check-publish-order.py`: 41 crates, no cycle; `scripts/check-design-paths.py`: ok; `scripts/check-size.py`: 0 faults; `scripts/check-sources.py pod-actions-results`: 0 changes at `8088f68` (the file differs from `0ff670d` only in the visibility of its helpers); `scripts/agent/check-windows.sh` and `scripts/agent/check-macos.sh`: 0 faults each. `scripts/check-crates-io-names.py`: 95 names, 1 fault, `pod-actions-results` missing: the owner reserves it (TODO/PROGRESS.md, "Owner actions").

# T-566: `pod-actions-results`: a fuzz target of the input of a stranger

**Source:** T-565 (a finding of its work).
**Category:** chore
**Milestone:** M8
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

`pod-actions-results` reads input of the client of a run: the query of a signed URL, the XML of a block list, the JSON bodies of the twirp calls and the claims of `ACTIONS_RUNTIME_TOKEN`. No fuzz target reads them; the other actions crates have one (`fuzz/src/targets/actions.rs`).

## Premise

- Read: the report of T-565; `crates/pod-actions-results/src/wire.rs`, `src/token.rs`, `src/blob.rs`.

## Approach

A feature `fuzzing` that gives `query`, `block_list` and `backend_ids`, and a target that feeds them bytes and runs one `ArtifactStore::answer` over a scratch store. List it in `fuzz/README.md` and in the seeds test; the feature goes in ALLOWED of `scripts/check-probe.py`.

## Prove

```sh
cargo test --manifest-path fuzz/Cargo.toml --lib
```

## Done

2026-11-14. The commit that names T-566. The feature `fuzzing` of `pod-actions-results` gives `fuzzing::{query, block_list, range}` (call C432), and it is in ALLOWED of `scripts/check-probe.py`. The target `actions_results` (`fuzz/src/targets/results.rs`, `fuzz/fuzz_targets/actions_results.rs`) reads the four parsers, then a script of up to 24 requests to an `ArtifactStore` and a `CacheStore` with a fixed random source in a scratch directory (call C433). Its 4 seeds (`fuzz/seeds/actions_results/`): an artifact in blocks with each call, a cache entry with three ranges, names that are paths with other refusals, and a runtime token with a block list. `fuzz/README.md` lists it; the seeds test (`each_target_has_a_binary_and_seeds`) and `each_target_survives_mutations` take it from `TARGETS`. `cargo test --manifest-path fuzz/Cargo.toml --lib`: 13 passed, 4 ignored; clippy of the fuzz library with `-D warnings`: clean. Plants, each caught by hand on the mutations of the seeds: `fuzz-results-range-past-end` and `fuzz-results-download-past-end` (`12 bytes at 0 of 11`: the seeds reach the download past the signature and the finalize).

# T-587: `pod-actions-store`: podrunner's additions since the import (image lines of the lock, checkouts, `HEAD`, images by tag) (PodUtils/podrunner#99)

**Source:** PodUtils/podrunner#99 (COORD-039 of podrunner; PodUtils/podrunner@T-130, PodUtils/podrunner@T-133).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

`pod-actions-store` is the copy of podrunner's code at `241acb8` (T-564). podrunner added code since then, so it cannot take the crate yet: image lines in the lock (`docker://NAME:TAG sha256:HEX`, `Lock::images`), a checkout of another repository pinned and kept as an action with no action file (`checkout_target`, `workflow_trees`, `add_trees`), the resolve of `REF` = `HEAD` in `Git` (`git ls-remote URL HEAD`), and the images of a workflow by tag with their resolve against the lock (`images.rs`, `ImageSource`, `by_tag`).

## Premise

- Read: PodUtils/podrunner#99; on podrunner's main: PodUtils/podrunner:crates/podrunner-local/src/lock.rs, src/store.rs, src/images.rs and their tests; T-564 and calls C320 to C324.

## Approach

Import the changes of the three files at podrunner's commit (`scripts/check-sources.py`), with the rules of the crate (a typed `Error`, no network in a unit test). `images` is its own module. Keep podrunner's names and tests. Tests and a plant for each addition.

## Prove

```sh
cargo test -p pod-actions-store --all-features
```

## Done

2026-11-14. The commit that names T-587: podrunner's additions at `872b43f` (`SOURCES`: 6 files, 0 changes).

- `crates/pod-actions-store`: `Lock::images` and the lines `docker://NAME:TAG sha256:HEX`; `store::{checkout_target, workflow_trees, add_trees}`; `HEAD` in `Git::resolve`; the module `images` (`ImageSource`, `by_tag`, `workflow_images`, `resolve`); `Error::{Checkout, Tree, ImageExpression, Registry, ImageMoved, ImageNotLocked}`, `CheckoutFault`, `LineFault::ImageForm` (calls C376 to C378). `images::PodvmPull` stays in podrunner. A lock line of an image must be in the form of `by_tag`, and a digest of a source must be `sha256:HEX` (C378). The static probe calls `workflow_images` and reads back a lock with an image line.
- `cargo test -p pod-actions-store --all-features`: 18 unit tests (podrunner's 3 new tests with their names: `a_checkout_of_another_repository_is_a_tree_to_pin`, `the_images_of_a_workflow_that_a_tag_names`, `a_tag_is_pinned_by_the_lock_and_a_moved_tag_fails_by_name`; and `each_fault_of_a_checkout_is_typed_and_a_tree_keeps_the_rules_of_the_lock`, `an_image_is_named_by_tag_and_the_lock_reads_its_pins_back`), 4 tests of `Git` (`git_resolves_head_to_the_default_branch` is new), 1 doc test; passed.
- Plants, each caught by hand: `actions-store-lock-images-dropped` (`an image line: `), `actions-store-checkout-expression-taken` (`Expression`), `actions-store-git-head-unresolved` (`HEAD is the default branch`), `actions-store-image-moved-taken` (`a moved tag fails`), `actions-store-image-digest-unchecked` (`no digest is refused`). The patch of `actions-store-lock-unsorted` is made again for the new `render_with`; it is caught (`sorted: `). `sh scripts/plant-each.sh --apply`: 0 stale.
- clippy of the crate and of `static-probe`, rustfmt and `cargo doc` with `-D warnings`: clean; `check-size.py`: 0 faults; `check-probe.py`: 40 crates, 0 faults; `check-design-paths.py`: ok; `check-sources.py pod-actions-store`: 6 files, 0 changes at `872b43f`.

# T-588: `pod-actions-results`: the CacheService and the replacement of an artifact on a rerun (PodUtils/podrunner#99)

**Source:** PodUtils/podrunner#99 (COORD-039 of podrunner; PodUtils/podrunner@T-120, PodUtils/podrunner@T-125, PodUtils/podrunner@T-135).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

`pod-actions-results` is the copy of podrunner's code at `0ff670d` (T-565). podrunner added since then: the CacheService (v2) of its fake broker, with the scopes of a ref (`with_scopes`), a random entry id, `Range` downloads and eviction at the limit (`cache.rs`); and `ArtifactStore::replacing`, so that a rerun replaces an artifact of the same name. A measured fact: `actions/download-artifact@v4` takes the digest as a plain string.

## Premise

- Read: PodUtils/podrunner#99; on podrunner's main: PodUtils/podrunner:crates/podrunner-broker-fake/src/cache.rs, src/artifacts.rs and their tests; T-565 and calls C325 to C329.

## Approach

Import `cache.rs` as a module of the crate and `replacing` into `ArtifactStore`, with the rules of the crate. Keep the digest a plain string in `wire::Answer` (the plant `results-digest-wrapped` holds it). Tests and a plant for each addition.

## Prove

```sh
cargo test -p pod-actions-results --all-features
```

## Done

2026-11-14. The commit that names T-588: podrunner's additions at `872b43f` (`SOURCES`: 4 files, 0 changes).

- `crates/pod-actions-results`: `ArtifactStore::replacing`; the module `cache` with `CacheStore` (`new`, `with_entropy`, `with_scopes`, `answer`, `route`, `entries`, `dir`), `cache::{SERVICE, BLOBS, LIMIT, Routed}`; the field `Answer::headers`, and `Payload::File` at the first byte to send (calls C379 to C382). The digest of `ListArtifacts` stays a plain string (`results-digest-wrapped` is caught). The fixes against the source: a signature of the `Entropy` compared in constant time, the limits of a block id, the reads in parts, the limit over the uploads that are not finalized, a decimal random entry id, an order of the entries with no tie, the digest of `[scope, key, version]` as JSON, a record only in its own directory (C380 to C382). The static probe calls `CreateCacheEntry`.
- `cargo test -p pod-actions-results --all-features`: 11 unit tests (the 5 tests of the artifacts and the 6 tests of the cache of podrunner, each with its name; the two sleeps of 1.1 s are gone), 15 tests of `tests/store.rs` (`a_rerun_replaces_an_artifact_of_the_same_name` is new), 6 tests of `tests/cache.rs` (new); passed.
- Plants, each caught by hand: `results-replace-ignored` (`already_exists`), `results-cache-scope-ignored`, `results-cache-range-whole` and `results-cache-no-eviction` (their tests of podrunner fail), `results-cache-entry-id-fixed` (`two runs over one store give two ids`), `results-cache-no-limit` (`the blocks of an upload hold the limit`), `results-cache-names-joined` (`the key and the version are not joined`), `results-cache-record-anywhere` (`the copy names another directory`); `results-digest-wrapped` and `results-sig-length-only` are still caught. `sh scripts/plant-each.sh --apply`: 0 stale.
- clippy of the crate and of `static-probe`, rustfmt and `cargo doc` with `-D warnings`: clean; `check-size.py`: 0 faults; `check-sources.py pod-actions-results`: 4 files, 0 changes at `872b43f`. T-566 (the fuzz target of the crate) is open: no fuzz target to change.

# T-600: `pod-actions-results`: an upload directory of a crashed run stays under `.pending` of the cache

**Source:** T-588 (a finding of its work).
**Category:** defect
**Milestone:** M8
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

The cache store of `pod-actions-results` stays across runs. Each upload that is not finalized has a directory `DIR/.pending/RANDOM/` with its blocks and its blob. A run that crashes, or a client that never finalizes, leaves that directory, and no later store removes it: the disk use grows with no bound. A store cannot remove each directory of `.pending` when it opens, because two stores (two runs) can share one directory, and the other store can have an upload in progress.

## Premise

- Read: `crates/pod-actions-results/src/cache.rs`, `src/cache/service.rs`, `src/cache/blob.rs` after T-588.
- Not measured: how long a real upload of `actions/cache@v4` takes; a large cache of 10 GB in blocks of 8 MiB takes minutes, not hours.

## Approach

`CacheStore::sweep(now)` removes each directory of `.pending` that the store does not hold and whose newest time of change (the directory, its `blocks` and its `blob`) is older than `PENDING_AGE` (one day) before `now`. `CacheStore::new` and `with_entropy` call it with the time of the system. A test gives `sweep` a time in the future, and, on Unix, sets the time of a directory back to prove the sweep of `new`. A plant takes the age away.

## Prove

```sh
cargo test -p pod-actions-results --all-features --test cache
sh scripts/plant-each.sh results-cache-pending-kept
```

## Done

2026-11-14. The commit that names T-600. `cache::PENDING_AGE` (one day) and `CacheStore::sweep(now)`: it removes each upload of `DIR/.pending/` that the store does not hold and whose directory, `blocks` and `blob` did not change for `PENDING_AGE`; `new` and `with_entropy` call it with the time of the system (call C431). `cargo test -p pod-actions-results --all-features`: 11 unit tests, 15 tests of `tests/store.rs`, 8 tests of `tests/cache.rs` (`an_upload_that_no_run_finalizes_goes_after_a_day` and, on Unix, `a_new_store_removes_the_stale_uploads_of_a_crashed_run` are new); passed. The plant `results-cache-pending-kept` is caught by hand (`a stale upload is removed`); `sh scripts/plant-each.sh --apply`: 0 stale. clippy of the crate, rustfmt and `cargo doc` with `-D warnings`: clean.

# T-572: `pod-actions-workflow`: the filters of `on:` (types, branches, tags, paths and their -ignore forms) (PodUtils/podrunner#97)

**Source:** PodUtils/podrunner#97 (COORD-037 of podrunner).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podrunner runs each workflow of a directory whose `on:` takes an event, as GitHub does (PodUtils/podrunner@T-125). `pod-actions-workflow` keeps `on:` as a `Node`, and `Workflow::events()` gives only the names of the events. podrunner asks for a function that tells whether a workflow takes an event, with the rules of GitHub's "Workflow syntax": `types`, `branches`, `branches-ignore`, `tags`, `tags-ignore`, `paths`, `paths-ignore`; the patterns `*`, `**`, `?`, `+`, `[]` and `!`, in order; a filter with its `-ignore` form refused; a push with only tags filters does not run for a branch, and the reverse; `paths` after `branches`. githome needs the same decision for its own events.

## Premise

- Read: PodUtils/podrunner#97; `crates/pod-actions-workflow/src/lib.rs`, `src/model.rs`, `src/read.rs`, `src/error.rs`, `src/data.rs`; `docs/design.md`, section 1 and the row of the crate.
- The rules come from GitHub's documents "Workflow syntax for GitHub Actions" (`on.<event>.types`, `on.<push|pull_request>.<branches|tags|paths>`, "Filter pattern cheat sheet") and "Events that trigger workflows" (the default types of `pull_request`). No run against GitHub here.

## Approach

A module `on` with `Trigger` (podrunner's fields), `On`, `Event` and `Filter`, and a module `pattern` with `Pattern` (a compiler of the patterns and a matcher of patterns times length). `Workflow::takes` and `Workflow::triggers`. Three typed errors: `FilterWithIgnore`, `OnlyNegated`, `BadPattern`. Record the calls C340 to C344. Tests with the examples of the documents for each filter; plants for the order of `!`, a `*` that crosses `/`, the tags-only rule and a filter with its `-ignore` form. The static probe and the fuzz target `actions_workflow` call the filters.

## Prove

```sh
cargo test -p pod-actions-workflow --all-features
sh scripts/plant-each.sh actions-workflow-bang-order-ignored actions-workflow-star-crosses-slash actions-workflow-tags-only-takes-branches actions-workflow-filter-with-ignore-taken
cargo test --manifest-path fuzz/Cargo.toml --lib
```

## Done

2026-11-14. The commit `ace4fbd` (`pod-actions-workflow: the filters of on: and Workflow::takes (T-572)`).

- `crates/pod-actions-workflow`: `Trigger` (`event`, `action`, `git_ref`, `paths`; `Trigger::new`), `Workflow::takes(&Trigger) -> Result<bool, Error>`, `Workflow::triggers() -> Result<On, Error>`, `On` (`read`, `takes`), `Event` (`takes`), `Filter` (`matches`, `takes`), `Pattern` (`parse`, `matches`), `DEFAULT_PR_TYPES`; `ErrorKind::{FilterWithIgnore, OnlyNegated, BadPattern}` (calls C340 to C344). The static probe decides one push; the fuzz target `actions_workflow` reads the filters of each file that parses.
- `cargo test -p pod-actions-workflow --all-features`: 54 passed (9 new in `tests/on.rs` with the examples of the documents, 3 new unit tests of the patterns). `cargo test --manifest-path fuzz/Cargo.toml --lib`: 13 passed.
- Plants, each caught by hand: `actions-workflow-bang-order-ignored` (`a_later_bang_removes_a_match_and_a_later_pattern_adds_it_again`), `actions-workflow-star-crosses-slash` (`must not match a/b`), `actions-workflow-tags-only-takes-branches` (`a_filter_of_one_kind_of_ref_stops_the_other_kind`), `actions-workflow-filter-with-ignore-taken` (`a_filter_with_its_ignore_form_is_refused`); `sh scripts/plant-each.sh --apply`: 0 stale.
- `cargo fmt --all --check`, clippy of the crate, of `static-probe` and of the fuzz library, and `cargo doc` of the crate with `-D warnings`: clean. `scripts/check-size.py`: 0 faults; `scripts/check-probe.py --no-tree`: 40 crates, 0 faults; `scripts/check-design-paths.py`: ok; `scripts/agent/check-windows.sh`: 0 faults.
- Next: podrunner takes `Workflow::takes` at the next tag (PodUtils/podrunner@T-125).

# T-577: `pod-actions-workflow`: the `workflows` filter of `workflow_run`

**Source:** T-572 (a finding of its work).
**Category:** feature
**Milestone:** M8
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

`Workflow::takes` (T-572) does not read `on.workflow_run.workflows`: `Trigger` has no field for the name of the workflow that ran. A `workflow_run` workflow thus runs after each workflow, not only after the ones that it names.

## Premise

- Read: `crates/pod-actions-workflow/src/on.rs`; GitHub's "Events that trigger workflows" (`workflow_run`: `workflows`, `types`, `branches`, `branches-ignore`).

## Approach

Add `Trigger::workflow: Option<&str>` and `Event::workflows`. A name matches whole, with case; `None` passes. `branches` applies to the head branch of the run that ended, as `git_ref` gives it. A test and the plant `actions-workflow-workflows-ignored`; call C350.

## Prove

```sh
cargo test -p pod-actions-workflow --test on
sh scripts/plant-each.sh actions-workflow-workflows-ignored
```

## Done

2026-11-14. The commit `1018c9a` (`pod-actions-workflow: the workflows filter of workflow_run (T-577)`). `Trigger::workflow` and `Event::workflows` (call C350). `cargo test -p pod-actions-workflow --all-features`: 55 passed (the new test `workflow_run_takes_the_workflows_that_it_names`). The plant `actions-workflow-workflows-ignored` fails that test, by hand. clippy of the crate and of the fuzz library, `cargo doc` of the crate with `-D warnings` and `cargo fmt --all --check`: clean.

# T-576: `pod-actions-workflow`: the calls of reusable workflows in `Run` (inputs, secrets, outputs, limits) (PodUtils/podrunner#98)

**Source:** PodUtils/podrunner#98 (COORD-038 of podrunner).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** L
**Status:** done

## Problem

podrunner runs a job that calls a workflow (`jobs.<id>.uses: ./.github/workflows/x.yml` or `OWNER/REPO/.github/workflows/x.yml@REF`, PodUtils/podrunner@T-131, blocked on this entry). `pod-actions-workflow` names the call (`Job::uses`, `Job::with`, `Job::secrets`) but does not expand it. podrunner asks for the expansion in `Run`, as GitHub does it ("Reusing workflows"): the caller gives the text of each called workflow; each called job becomes a job of the run (`CALLER/CALLEE`, named `CALLER / CALLEE`); `with:` is checked against `on.workflow_call.inputs`; `secrets:` or `secrets: inherit` give the names of the secrets; `on.workflow_call.outputs` become `needs.<caller>.outputs`; the caller ends with the result of GitHub; GitHub's limits on nesting and on the count of called workflows are refused by name. githome runs the same calls.

## Premise

- Read: PodUtils/podrunner#98; `crates/pod-actions-workflow/src/expand.rs`, `src/run.rs`, `src/graph.rs`, `src/model.rs`, `tests/expand.rs`.
- The rules come from GitHub's "Reusing workflows" (the examples of inputs and secrets, of a matrix, of outputs, of nested calls; the limits) and "Contexts" (`inputs`, `secrets`, `jobs` of `on.workflow_call.outputs`). No run against GitHub here.

## Approach

`src/call.rs`: the trait `Called`, `CallLimits`, `Secrets`, `Call`, and the interface of `on.workflow_call` with its checks. `Run` keeps one scope for each workflow of the run (the top one and each call) and one node for each job; `expand` of a caller with a source reads, checks and adds the called jobs; the end of the last called job ends the caller. Move the concrete jobs to `src/instance.rs` to keep each file under 500 lines. Calls C351 to C356. Tests with the examples of the documents; plants for the inputs not checked, `secrets: inherit` given with no `secrets:`, the levels not bounded and the outputs dropped. The static probe and the fuzz target run a call.

## Prove

```sh
cargo test -p pod-actions-workflow --all-features
sh scripts/plant-each.sh actions-workflow-call-inputs-unchecked actions-workflow-call-inherits-unasked actions-workflow-call-levels-unbounded actions-workflow-call-outputs-dropped
cargo test --manifest-path fuzz/Cargo.toml --lib
```

## Done

2026-11-14. The commit `406750a` (`pod-actions-workflow: the calls of reusable workflows in Run (T-576)`).

- `crates/pod-actions-workflow`: the trait `Called` (`text(uses, from)`), `Run::with_called`, `Outcome::Called(Vec<Call>)`, `Call` (`job`, `name`, `uses`, `matrix`, `inputs`, `secrets`, `jobs`), `Secrets` (`Run`, `Passed { from, items }`, `names`), `CallLimits` in `Base::calls`, `plan_with`, `Instance::{inputs, secrets}`; `ErrorKind::{CallText, InCalled, CallLimit, Input, Secret}` (calls C351 to C356). New files `src/call.rs` (316 lines) and `src/instance.rs` (164 lines); `src/expand.rs` has 449 lines.
- `cargo test -p pod-actions-workflow --all-features`: 65 passed (10 new in `tests/call.rs`). `cargo test --manifest-path fuzz/Cargo.toml --lib`: 13 passed; the fuzz target `actions_workflow` runs `plan_with` with a source that gives the input again. The static probe runs one call.
- Plants, each caught by hand: `actions-workflow-call-inputs-unchecked` (`with_is_checked_against_the_inputs_of_the_called_workflow`), `actions-workflow-call-inherits-unasked` (`secrets_are_passed_by_name_inherited_only_when_asked_and_checked`), `actions-workflow-call-levels-unbounded` (`the limit of levels stops the loop`), `actions-workflow-call-outputs-dropped` (`the_outputs_of_the_called_workflow_are_needs_of_the_jobs_after_the_call`); `sh scripts/plant-each.sh --apply`: 0 stale.
- `cargo fmt --all --check`, clippy of the crate, of `static-probe` and of the fuzz library, `cargo doc` of the crate with `-D warnings`: clean. `scripts/check-size.py`: 0 faults; `scripts/check-probe.py --no-tree`: 40 crates, 0 faults; `scripts/check-design-paths.py`: ok; `scripts/agent/check-windows.sh`: 0 faults.
- Next: podrunner takes the expansion at the next tag (PodUtils/podrunner@T-131).

# T-578: `pod-actions-workflow`: a check of the keys and the filters of `on:`, as GitHub refuses a file

**Source:** T-572 (a finding of its work; call C340).
**Category:** feature
**Milestone:** M8
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

`Workflow::parse` does not read the filters of `on:` (call C340). A file that GitHub refuses still parses: `branches` with `branches-ignore`, `tags` on `pull_request`, or a key that its event does not take. Only `Workflow::takes` and `Workflow::triggers` find the first two faults, and nothing refuses a key that is not valid for its event.

## Premise

- Read: the report of T-572; calls C340 to C344; the file src/on.rs of the crate (T-572).

## Approach

Keep `parse` as it is, so that podrunner's local runs do not change. Add `Workflow::check() -> Vec<Problem>` (or a result with each fault): the keys that each event takes (the GitHub docs, "Events that trigger workflows"), and `triggers()` read once. podrunner and githome call it where they want GitHub's refusal. Tests with the examples of the docs, and a plant.

## Prove

```sh
cargo test -p pod-actions-workflow --all-features
```

## Done

2026-11-14, commit a8e299b. `Workflow::check() -> Vec<Error>` gives each
fault of `on:` that GitHub refuses, all of them in the order of the file:
an event that GitHub does not know (`ErrorKind::UnknownEvent`), a key
that its event does not take (`ErrorKind::KeyNotTaken`, as `tags` on
`pull_request`), a filter beside its `-ignore` form, a filter of `!`
patterns only, a bad pattern, a value of another kind, and a `schedule`
that is not a list of `cron` entries. `event_keys(event)` gives the keys
of each event, from "Events that trigger workflows" and "Workflow syntax"
of github/docs (call C360). `parse`, `triggers` and `takes` do not change.
`tests/check.rs`, 6 tests: each of the 64 examples of `on:` of the two
documents passes; a key that its event does not take on 8 events; a filter
with its `-ignore` form on 5 events; an unknown event in the three forms
of `on:`; `schedule`; six faults of one file in their order, with their
paths. `cargo test -p pod-actions-workflow --all-features`: 71 passed.
The plant `actions-workflow-check-pr-takes-tags` (`pull_request` takes
`tags`) fails 3 tests of `tests/check.rs`, with the one of its `EXPECT`
(checked by hand). `cargo clippy -p pod-actions-workflow --all-targets
--all-features -- -D warnings` and its `cargo doc` with `-D warnings`:
clean.

# T-613: `pod-actions-store` and `pod-actions-results`: podrunner's changes after `872b43f` (T-139, T-149, T-154 of podrunner)

**Source:** T-608 (a finding of its work).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`scripts/check-sources.py` reports 3 changes of `pod-actions-store` and 1 of `pod-actions-results` against podrunner at `872b43f`: podrunner's `de6850e` (PodUtils/podrunner@T-139, `--local-repository`), `afa619d` (PodUtils/podrunner@T-154, the fetch through one egress grant) and `9dbefd1` (PodUtils/podrunner@T-149, `--web`) changed the files of the two crates after the pin. podrunner keeps its copy until it takes the crates, so each change that it needs must be in the crates first.

## Premise

- Read: the three commits of podrunner (a local clone); PodUtils/podrunner:crates/podrunner-local/src/store.rs, src/store/tests.rs, tests/store.rs, src/localrepo.rs, src/fetch.rs and PodUtils/podrunner:crates/podrunner-broker-fake/src/artifacts.rs at `9dbefd1`.

## Approach

Port each change that a consumer of the crate needs, with its test and a plant; for each other change, write the reason. Move the pins and the blob ids, and run `check-sources.py` on both crates until each reports 0.

## Prove

```sh
cargo test -p pod-actions-store -p pod-actions-results --all-features
python3 -I scripts/check-sources.py pod-actions-store pod-actions-results
```

## Correction

2026-11-15: `nightly.yml` run 43699451813 at `98017c3`: the test `the_git_command_takes_the_proxy_of_the_grant_and_https_only` fails on both Windows jobs. Windows keeps the case of the first removed spelling (`https_proxy`), and the test reads `HTTPS_PROXY`. The code is correct. T-679 has the fix of the test.

## Done

2026-11-14, commits 0457efc (`pod-actions-store`) and 87daa5a
(`pod-actions-results`); calls C462 and C463. Each change of podrunner is
one that it needs, and is ported:

- `de6850e` (PodUtils/podrunner@T-139): `Forge::is_local(key)`, default
  `false`; a local reference goes to the store and never to the lock, in
  each mode, and a remote action that it names keeps the rules of the
  lock. Test `a_local_reference_goes_to_the_store_and_not_to_the_lock`.
- `afa619d` (PodUtils/podrunner@T-154): `Git::proxy` and `Git::command`:
  the protocol of the base only (`file` for a base with no scheme), no
  submodule, and with a proxy only that proxy. Test
  `the_git_command_takes_the_proxy_of_the_grant_and_https_only`
  (podrunner's, with a `file://` case). Not ported: its `fetch.rs` (the
  relay of an egress grant) and the live test through it, which are
  podrunner's egress module.
- `9dbefd1` (PodUtils/podrunner@T-149): `ArtifactStore::zip_of(name)`,
  with the check of a name of the store; an artifact with no record gives
  none. Test `the_zip_of_a_finalized_artifact_and_of_nothing_else`.

`cargo test -p pod-actions-store --all-features`: 20 unit tests, 4 tests
of git, 1 doc test. `cargo test -p pod-actions-results --all-features`:
12 unit tests, 8 tests of the cache, 15 of the store. Plants, each caught
by hand: `actions-store-local-reference-locked`,
`actions-store-git-any-protocol`, `actions-results-zip-of-a-half-artifact`;
`plant-each.sh --apply`: 0 stale. clippy, rustfmt and `cargo doc` with
`-D warnings` are clean; `check-windows.sh` and `check-macos.sh`: 0
faults. The pins move to podrunner `9dbefd1`; `check-sources.py`:
`pod-actions-store` 6 files, `pod-actions-results` 4 files, 0 changes
each, through the mirror and with a clone.

# T-692: `pod-lab`: the hostile host classes as shared profiles, `podlab check` and `podlab run` (PodUtils/podrunner#180)

**Source:** PodUtils/podrunner#180 (COORD-061 of podrunner); PodUtils/podvm#424 (COORD-140 of podvm, its answer; PodUtils/podvm@T-1916).
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** L
**Status:** partial

## Problem

The first user runs of podrunner `v0.1.0-beta.6` found about 30 defects that CI did not find (podrunner #153, #155 to #178): most come from a host class that the runners of GitHub do not have, as they allow each operation. podrunner asks for a crate `pod-lab` and a static CLI `podlab`: the profiles as data, `podlab check PROFILE` (the profile holds, proved with the rows of `pod-hostprobe`, so a lab that allows what it should refuse fails its own check), `podlab run PROFILE -- CMD` (the exit code and the output of CMD in the lab), and a step of GitHub Actions that brings up a profile on a runner. The profiles of the ask: `userns-off`, `bind-denied` (`bind` and `connect` of AF_INET and AF_INET6 give EACCES, AF_UNIX holds), `getsockname-denied` (EOPNOTSUPP), `proxy-only`, `small-tmp` (`TMPDIR` on a tmpfs of 1 GiB), `root-unreadable` (`readdir("/")` gives EACCES), `nonroot-nocaps` (no capability, `NoNewPrivs`, a seccomp filter), `no-kvm`, `arm64-tcg`, `inherited-fds` (descriptors 3 to N open). podvm adds `chroot-denied`, `userns-mount-denied` (AppArmor's `apparmor_restrict_unprivileged_userns`), `cgroup-v1`, `landlock-old` (ABI below 4), `proc-masked` (no `/proc`, or `hidepid`), `noexec-tmp`, and `wsl2` (the kernels of WSL2, PodUtils/podvm#396 to #402; a profile of the machine tier only); podvm holds the machine-tier form of each (a guest with the knobs of a profile) and the parts that a rung with no root can hold (a seccomp filter, a tmpfs, inherited descriptors). podnet holds the network profiles (`proxy-only`, `bind-denied`).

## Premise

- Read: PodUtils/podrunner#180, PodUtils/podvm#424; `pod-hostprobe` (the probe rows), `pod-confine` (seccomp and Landlock), `pod-enter` (the rungs), `pod-netlab`.
- Not measured: a profile on a runner.

## Approach

Part 1 (the next tag after `v0.53.0`): the crate `pod-lab` with the profiles as data (a name, the parts, the rows of `pod-hostprobe` that each part makes refuse or allow), `check` of a profile against a `pod_hostprobe` report, and `run` of the parts that a host with no root holds (a seccomp filter of chosen calls with their errno through `pod-confine`, a tmpfs or a `TMPDIR`, inherited descriptors, a user namespace with a mount where the host has it); `podlab` as a static binary. Part 2: the Actions step (`lab/action.yml`) and the profiles of podvm's machine tier (a guest form that podvm drives) and of podnet. The name `pod-lab` on crates.io is an owner action. A call names the form and the order.

## Prove

```sh
cargo test -p pod-lab --all-features
```

## Done

Part 1, 2026-11-15, commits ccbd75a and the next one; calls C673 to
C680.

- `crates/pod-lab`: the table of 16 profiles (the 10 of
  PodUtils/podrunner#180 and the 6 of PodUtils/podvm#424). Each part
  names its rows of `pod-hostprobe` with the errno, the identity facts, or
  a fact that a lab measures from inside, and its mechanism (`Hold`).
- `check`, `check_with`, `check_report` (feature `report`): each part
  holds, fails or is unseen, with each row and its errno. A skipped row
  never holds.
- `run`: a lab process (a new start of the binary, one thread) sets the
  parts, measures the profile from inside, each row in a probe child,
  then replaces itself with the command. A part that does not hold
  stops the command (exit 125), unless `partial`. Held with no root: a
  seccomp filter of `pod-confine` (`userns-off`, `getsockname-denied`,
  `chroot-denied`, `userns-mount-denied`, `landlock-old`, the default
  list of `nonroot-nocaps`), Landlock (`bind-denied`, `root-unreadable`,
  `no-kvm`, `proc-masked`), a tmpfs in a user namespace (`small-tmp`,
  `noexec-tmp`), descriptors 3 to 9 (`inherited-fds`). Not held here:
  `proxy-only`, `cgroup-v1`, the arch part of `arm64-tcg` (part 2).
  `nonroot-nocaps` holds only for a caller that is not root.
- `podlab` (feature `cli`, clap derive): `list`, `check PROFILE
  [--report FILE]`, `run [--partial] PROFILE -- CMD...`. The exit codes
  and the environment are one typed table (`codes.rs`).
- `pod-hostprobe`: `SYS_GETSOCKNAME`, `getsockname`, `Statfs::f_blocks`,
  the names of `EAFNOSUPPORT` and `EBADFD`.
- Each profile that a lab here holds ran with `podlab run` as uid 65534
  on this host, with a payload that shows the part (the refused call,
  the hidden path, the noexec mount, the status fields).
- Tests: 24 unit tests and 1 test of the help; `tests/lab.rs`, 5 cases
  (`inherited-fds`, `bind-denied` with AF_UNIX socket, bind and connect
  (PodUtils/podnet#585), `small-tmp`, a refused run, a failed check from
  inside), as root and as uid 65534. `scripts/ci/userns-tests.sh` runs
  it where a skip fails. Plants (group `lab`): 8 caught by
  `plant-each.sh --group lab`.
- Static: `static-probe` links `pod-lab` with `cli`; the static job
  builds `podlab` on each Linux target and runs `check-static.sh`.
  `podlab` for `x86_64-unknown-linux-musl` (cargo zigbuild) is static.
- The facts that no row measures: T-693, T-694, T-695. UDP in
  `bind-denied`: T-696.

Part 2 stays: the Actions step (`lab/action.yml`), the machine-tier form
of each profile for podvm, and the network profiles of podnet.

# T-693: `pod-hostprobe`: the rows of the socket class (AF_INET6, connect, AF_UNIX, getsockname)

**Source:** T-692 (the profiles `bind-denied` and `getsockname-denied` of `pod-lab`).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** partial

## Problem

`pod-lab` checks `bind-denied` and `getsockname-denied` against a report of
`pod-hostprobe`. The report has one socket row,
`bind(127.0.0.1:0)+listen`. A check of a report cannot observe the bind of
`[::1]:0`, a TCP connect, the socket, bind and connect of AF_UNIX, or
`getsockname`: these parts are `unseen`. A lab measures them from inside
(`LabFact`), so `run` holds them, but a host that a user reports cannot
show them.

## Premise

- Read: `crates/pod-hostprobe/src/probes.rs` (the census rows),
  `crates/pod-lab/src/inside.rs` (the measurements of a lab).
- Not measured: the rows on a host with no IPv6 and on a host whose policy
  refuses `connect`.

## Approach

Add census rows to `pod-hostprobe`, each in its own child, with the
errno: `bind([::1]:0)+listen`, `connect(127.0.0.1:1)`,
`socket+bind+connect(AF_UNIX abstract)`, `getsockname(TCP socket)`. A
host with no IPv6 gives `EAFNOSUPPORT` on the socket: a denial, not a
skip. Then move the `LabFact`s of these parts to `Observe::Row` in
`crates/pod-lab/src/table.rs`, and keep the measurement of a lab through
the rows. The JSON Schema of the report takes the new names.
`connect(127.0.0.1:1)` is a row since T-714 (C732), and
`tcp-connect-refused` reads it; the rows of `[::1]`, of AF_UNIX and of
`getsockname` wait.

## Prove

```sh
cargo test -p pod-hostprobe --all-features
cargo test -p pod-lab --all-features
```

# T-694: `pod-hostprobe`: the rows of `TMPDIR` and of the inherited descriptors

**Source:** T-692 (the profiles `small-tmp`, `noexec-tmp` and `inherited-fds` of `pod-lab`).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

`small-tmp` and `inherited-fds` of `pod-lab` have no row in
`pod-hostprobe`: the size of the file system of `TMPDIR`, and the
descriptors from 3 up that a process has at its start. A check of a
report gives `unseen` for both. `exec(/tmp/execprobe)` names `/tmp`, not
`TMPDIR`.

## Premise

- Read: `crates/pod-hostprobe/src/probes/census.rs`, `src/writable.rs`
  (the space of a directory), `crates/pod-lab/src/inside.rs`.
- Not measured: a host whose `TMPDIR` is not `/tmp`.

## Approach

Add rows that read the facts of the probe process: `statfs(TMPDIR)` (the
size and `ST_NOEXEC`; the path is a setting of the caller, as the
candidates of `writable`), and `fcntl(F_GETFD) of 3..N` (each descriptor
open at the start, and its close-on-exec flag). A row of a fact gives
`ok` with the value in its reason. Then `pod-lab` observes
`tmpdir-small` and `fds-open` with these rows.

## Prove

```sh
cargo test -p pod-hostprobe --all-features
cargo test -p pod-lab --all-features
```

# T-695: `pod-hostprobe`: the rows of the machine, the cgroup version and the `/proc` mount

**Source:** T-692 (the profiles `arm64-tcg`, `cgroup-v1` and `proc-masked` of `pod-lab`).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

Three parts of `pod-lab` have no measurement at all: the machine of the
host (`arm64-tcg`), the cgroup version of `/sys/fs/cgroup` (`cgroup-v1`),
and the form of `proc-masked` with `hidepid`, where `/proc/self/mem`
opens and the processes of other users are hidden. A check gives
`unseen` for each.

## Premise

- Read: `crates/pod-hostprobe/src/cgroup.rs`, `src/binfmt.rs`
  (`SELF_MACHINE`), `src/mounts.rs`.
- Not measured: a host with cgroup v1, a host with `hidepid`.

## Approach

Add rows: `uname(2) machine` (ok with the machine in the reason),
`statfs(/sys/fs/cgroup)` (the magic of cgroup2 or of a tmpfs of v1), and
the options of the `/proc` mount from `/proc/self/mountinfo`
(`hidepid=`). Then replace each `Observe::Nothing` of these parts in
`crates/pod-lab/src/table.rs` with an expectation of a row, and add an
`Expect` that reads a value of a reason where a row gives one.

## Prove

```sh
cargo test -p pod-hostprobe --all-features
cargo test -p pod-lab --all-features
```

# T-696: `pod-lab`: `bind-denied` refuses UDP bind and connect of AF_INET and AF_INET6

**Source:** T-692; PodUtils/podnet#585 (the filter refuses AF_INET and AF_INET6 for `bind` and `connect`).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

The lab of `bind-denied` holds its parts with a Landlock ruleset of ABI 4
that handles the TCP bind and the TCP connect. A UDP bind and a UDP
connect of AF_INET and AF_INET6 still succeed in the lab. podnet names
both families for `bind` and `connect`, with no word on the protocol.

## Premise

- Read: `crates/pod-lab/src/hold.rs` (`landlock_tcp`),
  `crates/pod-confine/src/spec.rs` (`UDP_RIGHTS`, ABI 10).
- Not measured: a kernel with Landlock ABI 10 in this session.

## Approach

Where the ABI is 10 or more, handle `LANDLOCK_ACCESS_NET_BIND_UDP` and
`LANDLOCK_ACCESS_NET_CONNECT_SEND_UDP` too, with no port rule. Add a part
`udp-refused` with a `LabFact` (a UDP bind of `127.0.0.1:0` gives
`EACCES`) that does not hold under ABI 10, so that `run` refuses it with
the reason and `--partial` runs without it. Ask podnet whether its class
refuses UDP (a COORD issue) before the part is in the default profile.

## Prove

```sh
cargo test -p pod-lab --all-features
```

# T-700: `pod-manual`: one crate for the help and the manual of each command line of PodUtils (PodUtils/podnet#584)

**Source:** PodUtils/podnet#584 (COORD-090 of podnet; PodUtils/podnet@T-359); the owner's rulings "Discipline" and "The text of a command line" (2026-11-15).
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** L
**Status:** partial

## Problem

Each command line of PodUtils writes its help and its manual by hand. The agents hardcode the text in many places and forget to change it, and users read the code to find a flag, a variable or an exit code. The owner's ruling "Discipline" (2026-11-15): each command line takes one shared crate for its help and its manual; each has `CLI man` (with `--no-pager`); the help comes from the model of the command tree, and a mechanical check finds each drift. podnet's `src/manual/` (about 1,970 lines at `78a3911`, tested by `tests/cli/manual.rs`) has the parts: `model.rs` (the command tree read from `clap`), `roff.rs` (the man page), `text.rs` and `pager.rs` (`podnet man`, a pager on a terminal, `--no-pager`, `--json`), `facts.rs` (a typed table of the environment variables and the exit codes), `inventory.rs` (the component inventory) and `PodUtils/podnet:scripts/gen-llms.py` (`llms.txt` and `llms-full.txt` from `podnet man --json`). podnet freezes these files for fixes until the take.

## Premise

- Read: PodUtils/podnet#584, PodUtils/podssh#267, PodUtils/podvm#425, PodUtils/podrunner#181.
- Not read: podnet's `src/manual/` at `78a3911` (the read of its contents through the mirror gave 403 from this session); the help of podssh, podvm, podrunner and `podlab`.

## Approach

Take podnet's `src/manual/` and `PodUtils/podnet:scripts/gen-llms.py` at `78a3911`, with the generic part of podssh's `PodUtils/podssh:crates/podssh-cli/src/man/` at `cfc3ae3` (the model filled from a program's tables), as the crate `pod-manual` (call C681), by the procedure of `docs/design.md`, section 3, with `SOURCES`. The crate holds the model, the roff, text and JSON writers, the pager, and the typed table of variables and exit codes that each program fills; the program keeps only its facts. The crate gives a test helper that each program calls in its own tests: the help, the man page, the JSON manual and the `llms` files are generated, and a test fails when a checked-in file differs from the generated one or when a variable or an exit code that the code reads has no fact. `podlab` (T-692) is the first command line of podcrates that takes it. The needs of the consumers (PodUtils/podssh#267, PodUtils/podvm#425, PodUtils/podrunner#181) are parts of this entry:

1. The model is a type that the program fills (podssh parses OpenSSH's grammar, podvm docker's and podman's, podrunner its own options, each by hand); a reader of `clap` is one way to fill it. The same table can drive the parser (podrunner), and a test checks each row against the parser (podssh).
2. Other names: of a command (podssh `irc` for `chat`, `scp` and `sftp` for `cp`) and of the program by argv0 (podvm as `docker` and `podman`); the help shows the name of argv0.
3. `CLI man SECTION` (or `VERB`): one command or one topic; topics of notes and examples (podssh, about 60).
4. The help of each subcommand (`CLI COMMAND --help`, `CLI help COMMAND`) from the model (podrunner).
5. The rules between options: excludes, needs, one of (podrunner); its contract tests can come from them.
6. The kind of a flag: wired, accepted and ignored, refused with what to use instead (podssh); a status against another tool (implemented, degraded, stub, refused, with its text; podvm's parity with docker and podman).
7. Typed tables that a program adds (podssh: the `-o` keywords, files, retired names), each rendered by each writer; the variables in groups (podvm, about 60 `PODVM_*`); the keys of a configuration file whose keys are options (podrunner's `run.toml`).
8. The exit codes: the classes of `pod_core::failure` (64 to 80, podrunner), docker's 125 to 127, and a row "the code of the payload" (podvm).
9. The writers: plain text that needs nothing of the host; man(7) that groff and mandoc render with no warning; one JSON object; `llms.txt` and `llms-full.txt` from the JSON.
10. The pager: only on a terminal; `PAGER` when the user names it; else `less` only where a probe finds it; never a helper otherwise; `--no-pager` (podssh's rule). No dynamic library.
11. A test of the exit codes against the facts: a code that a program gives with no fact fails (podnet, PodUtils/podnet#585; podnet has the test of the variables both ways).
13. The rules of the text (the owner's ruling "The text of a command line", 2026-11-15), checked by a test of the crate over each string of the model, the facts and the tables, and by a check of the strings of a release binary:
    - no emoji and no pictograph;
    - valid UTF-8 only, with no control character but the newline and the tab; no escape sequence of a terminal in the text (a writer adds a colour only on a terminal);
    - no id of a work entry (`T-NNN`, `C123`, `Q12`, an issue number of a record) and no text quoted from a work record;
    - ASD-STE100: short sentences (a limit of words for each sentence, 20 in a procedure and 25 in a description), active voice, one instruction for each sentence.
    A program that breaks a rule fails its own tests. The rule applies also to each error and each log line that a program writes from a fact.
14. A helper for the text of an error: "OPTION: what is wrong; fix: what to do" (podrunner).

Not needed by a consumer: a page for each subcommand, translations. The handoff of the tag names the step of each consumer: its help and its manual come from `pod-manual`, `CLI man --no-pager` works, and the drift test runs in its CI.

## Prove

```sh
cargo test -p pod-manual --all-features
```

## Done

Part 1, 2026-11-15, commits b003f1fc and c3835991 (local branch
`manual`); calls C682 to C687.

- `crates/pod-manual` (about 4,000 lines): the model (`Manual`,
  `Command` with other names, `Flag` with `FlagKind` and `Parity`, `Arg`,
  `Rule`, `Topic` of blocks), the facts (`VarGroup`, `Var`, `Exit` with
  `Code::{Value, Range, Signal, Payload}`, `FileFact`, `ConfigFile`,
  `Table`; `failure_classes` of `pod_core::failure`, feature `core`), the
  reader of clap and the rows of its parser (feature `clap`), the writers
  (text, man(7), JSON and its reader, `llms.txt` and `llms-full.txt` from
  the JSON), the help of each command (`help::request`, with `--json`),
  `CLI man [SECTION...]` (`select`), the pager (C682), the text rules
  (`rules`, C683), the checks of a program (`check::{texts, model,
  generated, variables, exit_codes, flags}`), the reader of the source
  (`scan`), the form of an error (`error::fault`), `Manual::named` for
  `argv[0]`.
- From podnet `78a3911`: PodUtils/podnet:src/manual/ and
  PodUtils/podnet:scripts/gen-llms.py (in Rust, C686). From podssh
  `cfc3ae3`: PodUtils/podssh:crates/podssh-cli/src/man/ (model, text,
  roff, json; the reader of the variables of its `facts/tests.rs`),
  PodUtils/podssh:crates/podssh-cli/src/pager.rs, the form of
  PodUtils/podssh:crates/podssh-cli/src/flags.rs and
  PodUtils/podssh:scripts/interop-man.sh (now `scripts/check-man.sh`).
  `crates/pod-manual/SOURCES`.
- `scripts/check-strings.py`: the text rules over `.rodata` of a release
  binary (C684); `scripts/check-man.sh`: groff and mandoc render
  `BINARY man --roff` with no warning and show each flag. The static job
  runs both on `podlab`.
- `podlab`: `podlab man [--no-pager] [--roff | --json] [SECTION...]`;
  the help of each command from the model (C687); the exit codes and the
  variables are `codes::facts()`, one table; `man/podlab.1` and
  `man/llms.txt` are generated; its tests call each check
  (`crates/pod-lab/src/bin/podlab/tests.rs`).
- Tests: `pod-manual` 54 unit tests and 7 of `tests/clap.rs`; `pod-lab`
  24 unit tests, 6 of the binary (1 ignored: the writer of the
  generated files), 4 of `tests/cli.rs`; each passes as uid 65534 too.
  Plants: 8 of group `manual` and 3 of group `lab`, each caught by
  `plant-each.sh`. Their first run missed two: the row of the code of the
  payload hid a code of the program with no fact, and the walk test did
  not see a skipped string; c3835991 fixed both checks.
- `podlab` for `x86_64-unknown-linux-musl` (cargo zigbuild) is static,
  `check-strings.py` finds no fault in it, `check-man.sh` passes on it,
  and `podlab man --no-pager` prints the manual (257 lines).
  `check-strings.py` reads 32-bit ELF files too (x86 and ARM, zig cc).
- The parts of the Approach: 2, 3, 4, 6, 7, 8, 9, 10, 11, 13 and 14 are
  in. Of 1, the model and the check of each row against the parser are
  in; a parser driven by the model is not. Of 5, the rules are in the
  model, the writers and the reader of clap; contract tests from them are
  not.
- Findings: T-704 (the usage errors of clap in `podlab`), T-705 (the
  strings of Mach-O and PE binaries).

What stays: a parser driven by the model and contract tests from the
rules (podrunner's take), the take of each consumer (podnet, podssh,
podvm, podrunner: its help and its manual from `pod-manual`, `CLI man
--no-pager`, the checks in its CI), and the hand-off of the tag on
PodUtils/podcrates#181.

# T-701: `pod-lab`: the host classes of podssh and podnet as profiles, with the lane of each project (PodUtils/podssh#268, PodUtils/podnet#585)

**Source:** PodUtils/podssh#268 (COORD-081 of podssh), PodUtils/podnet#585 (COORD-091 of podnet), the answers to PodUtils/podcrates#180; the owner's ruling "Discipline" (2026-11-15).
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

The first table of `pod-lab` (T-692) holds the classes of podrunner and podvm. podssh and podnet name more host classes that their users met. podssh: `proxy-only` (CONNECT only to ports 443, 80 and 8443; a direct `connect` gives EACCES or is dropped; UDP gives EPERM; no resolver), `proxy-tls-reterminate` (the proxy's own CA for some names, `CONNECT host:22` answers 200 and then gives no byte), `proxy-per-name` (200 for one name and 504 for another), `sni-allowlist` (TLS reset after a ClientHello with a name not on the list, or with no SNI), `no-proxy-no-dns` (IP literals only), `ws-upgrade-refused`, `listen-denied` (`bind` passes, `listen` is refused), `no-ptmx` (no `/dev/ptmx`, no `/dev/pts`, no controlling terminal), `no-passwd-entry`, `noexec-home`, `no-proc`, `seccomp-nonewprivs` (`unshare`, `mount`, `chroot`, `ptrace` refused), `ro-etc-ssl` and `no-tun`. podnet: `udp-denied`, `no-net-admin`, `no-loopback` (a fresh network namespace with no `lo` address), `ipv6-only`, `raw-denied` (raw and packet sockets give EPERM); its `bind-denied` lets AF_UNIX through. Each project names its lane: podssh (`doctor`, `ssh` and `proxy` through a relay and a node, `cp` of 200 MiB each way, `serve` with `vi`, `less` and Ctrl-C, `pipe`, a session that outlives a killed relay link) and podnet (its commands for each class, in #585).

## Premise

- Read: PodUtils/podssh#268, PodUtils/podnet#585.
- Not measured: each class on a runner. Some classes need a proxy or a network namespace (part 2, podnet's network profiles).

## Approach

After part 1 of T-692: a profile for each class, in the form of part 1. The classes that a seccomp filter, a mount or an environment can hold with no root (`listen-denied`, `udp-denied`, `raw-denied`, `no-ptmx`, `no-passwd-entry`, `noexec-home`, `no-proc`, `seccomp-nonewprivs`, `no-tun`, `ro-etc-ssl`) go into `run`. The network classes (`proxy-*`, `sni-allowlist`, `no-proxy-no-dns`, `ws-upgrade-refused`, `no-loopback`, `ipv6-only`) come from podnet as data in the form of part 1 (podnet's hand-off), with a probe row for each where `pod-hostprobe` has none (a finding entry for each missing row). The documents of `pod-lab` name the lane of each project.

## Prove

```sh
cargo test -p pod-lab --all-features
```

## Done

2026-11-15, commits e038b30 and the next ones; calls C699 to C705.

- 18 profiles in the table (34 in all). A lab with no root holds 12:
  `listen-denied` (seccomp of `listen`, EACCES), `udp-denied` and
  `raw-denied` (seccomp of `socket(2)` by family and type, EPERM, and
  `io_uring_setup`), `no-ptmx` (a tmpfs on `/dev` with no `ptmx` and no
  `pts`, a FIFO at `/dev/tty`, a new session), `no-passwd-entry`
  (binds of `/etc/passwd` and `/etc/group` with no line of the ids),
  `noexec-home` (a `noexec` bind of `HOME`), `no-proc` (an empty tmpfs on
  `/proc`), `ro-etc-ssl` (a read-only bind), `seccomp-nonewprivs` and
  `no-net-admin` (a drop of each capability, also for root), `no-tun`
  (Landlock) and `no-loopback` (a network namespace). The mounts are in
  one user and mount namespace (`src/mounts.rs`).
- The six network classes that need a proxy, a resolver or a route out
  (`proxy-tls-reterminate`, `proxy-per-name`, `sni-allowlist`,
  `no-proxy-no-dns`, `ws-upgrade-refused`, `ipv6-only`) are in the table
  with their parts held elsewhere: `podlab list` names them, `podlab run`
  refuses them (exit 125). Their rows and labs: T-716.
- Each part names its rows of `pod-hostprobe`, or a lab fact with its
  entry: the socket facts T-714, the file facts T-715.
- `Call` takes conditions (`When::Bits`, `When::Equal`); `IdFact::NoNetAdmin`;
  `Hold::hide` stops at a directory that does not exist.
- `crates/pod-lab/LANES.md`: the commands of podssh, podnet, podrunner and
  podvm for each profile; `llms.txt` lists it. The manual names `HOME` and
  `TMPDIR`; `man/podlab.1` and `man/llms.txt` regenerated.
- Tests: `tests/lab/hosts.rs`, one run of each held class with a payload
  that shows each part, and a refused network class; the test profile
  `caps-dropped`. `no-ptmx` runs in a child with a pseudo terminal as its
  controlling terminal. As root each case runs as root, then as uid 65534;
  both pass, also under `scripts/ci/userns-tests.sh`.
- Plants (group `lab`): 11 new, each caught by `plant-each.sh`. The rules
  of `raw-denied` have no plant: only a run as root proves them (C704).


# T-702: one POSIX installer for each project, hermetic, idempotent and fast, run in the labs (the ruling "Installer")

**Source:** the owner's ruling "Installer" (2026-11-15); podrunner's lessons (asked in the COORD issue of the ruling).
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** M
**Status:** partial

## Problem

A user spends about 20 minutes to set up a project of PodUtils. The owner's ruling "Installer" (2026-11-15): each project publishes one installer in POSIX `sh` that runs on any host (each profile of `pod-lab`), installs and sets up the project as fast as possible, is hermetic (it reads nothing of the host that it does not need, writes only under its own prefix, and needs no root, no package manager and no compiler) and is idempotent (a second run changes nothing and says so; an interrupted run leaves no half install). podrunner learnt this on its own first user runs (PodUtils/podrunner#153, #155 to #178).

## Premise

- Read: PodUtils/podrunner#183 (`PodUtils/podrunner:scripts/install.sh`, about 320 lines, with `PodUtils/podrunner:scripts/install-pins.py`, `PodUtils/podrunner:scripts/make-bundle.sh` and `PodUtils/podrunner:scripts/test-install.sh`), PodUtils/podnet#586 (`PodUtils/podnet:scripts/install.sh`, 501 lines, `PodUtils/podnet:docs/install.md`), PodUtils/podssh#269 (no installer: three commands in its README).
- Read: PodUtils/podvm#427: podvm has no installer (five lines of its README); its release gives `podvm-ARCH` with a `.sha256` beside each (it can also publish one `SHA256SUMS`); after the binary, links `docker` and `podman` to it (optional; ask before it shadows a `docker` on `PATH`); the end check is `podvm version --verbose` and `podvm probe` (no network, under a second); under QEMU user mode `uname -m` gives the emulated machine, which is the right binary.
- Not read: the code of each installer.
- The 20 minutes of podrunner: five sources (one a workflow artifact that needs `gh` and a token, and expires), a SHA-256 and an attestation for each by hand, an image that the pinned podvm could not load, a first command of about eight flags, then the host (a small `/tmp`, no loopback bind, a proxy with a credential).

## Approach

1. Collect podrunner's installer and its lessons, and the installer of each other project, by the COORD issue of the ruling.
2. podcrates gives the shared part: a POSIX `sh` core (the detection of the OS and the architecture, the download of the static binary of the release with the first fetch tool that a probe finds, the check of its SHA-256 against the `SHA256SUMS` of the release, an atomic move into the prefix, the PATH line, a second run that changes nothing, `--prefix`, `--version`, `--dry-run`, an offline install from a file, and the proxy of the environment), with the text rules of T-700. Each project keeps its name, its files and its setup steps in a small part of its own.
3. A test runs the installer of each project in each profile of `pod-lab` (T-692, T-701) with `shellcheck --shell=sh` and `dash`, `busybox sh` and `bash --posix`: a first run, a second run that changes nothing, and a run cut in the middle. The time of a run is measured and has a limit.
4. podcrates publishes its own installer for `podlab`.

The needs of the projects, for the core:
- From podrunner: the pins (each version and SHA-256) are written into the published installer by the release from the pin files, and a check fails on a difference; a copy of the tree with no pins refuses a network install. Each file goes to a temporary name in the prefix, is checked, then moved; a second run checks each file against its pin and downloads only what is missing or wrong. An offline bundle as `.tar.gz` (busybox has `tar` and `gzip`, not always `unzip`). The free space of each target directory is checked before a download, and the error names the knob that fixes it. The proxy comes from `https_proxy`, `HTTPS_PROXY` and `no_proxy`, and a credential never reaches a file or argv. A configuration file at the end, so the first command is short. The last step is a self-check that runs the program (`doctor`) and names the host fact on failure. Each file is an asset of a release, never a workflow artifact. Test the file that the release publishes, not a copy.
- From podnet: the first downloader that a probe finds (`curl`, `wget`, `aria2c`, `python3`, `perl`, `busybox wget` with a warning); the SHA-256 tool that a probe finds; the cosign signature of `SHA256SUMS` where `cosign` is there; the new binary runs once in the install directory before it replaces the old one; INT, TERM and HUP remove the temporary files; `$XDG_BIN_HOME`, else `~/.local/bin`; a host that loses its disk at each start (an install directory that stays).
- From podssh: the target triple of each host (musl on Linux, both Macs, the `.exe` of Windows), the quarantine of macOS, and a prefix that can run programs (some hosts mount `$HOME` with `noexec` while `/tmp` runs them).

## Prove

```sh
sh scripts/install/test.sh
python3 -I scripts/install/pins.py check scripts/install/podlab.sh
sh scripts/plant-each.sh --group install
```

## Done

2026-11-15, partial: the core, the installer of `podlab`, the pins, the
release assets of `podlab` and the test are in (commits `12cede3e` and
`e14a2744`; calls C706 to C712). Each project's take and the hand-off are
left.

- The core (`scripts/install/core.sh`, POSIX `sh`, 494 lines): the target
  triple (musl on Linux, both Macs, also under Rosetta, the `.exe` of
  Windows under a POSIX shell; `uname -m` under QEMU user mode); the first
  downloader that a probe finds (`curl -q`, `wget --no-config`, `python3`,
  `busybox wget` with a warning); the proxy of the environment
  (`HTTPS_PROXY` to `https_proxy`, `NO_PROXY` to `no_proxy`; the output
  shows no credential); the first SHA-256 tool that a probe finds; the
  prefix (`--prefix`, `$XDG_BIN_HOME`, `~/.local/bin`), its write and exec
  check with the reason (`noexec` from `/proc/self/mounts`); the free
  space against the sizes of the pins, with the knob `--prefix`; a
  temporary name in the prefix, the pin, a run of `--version`, then `mv`;
  a second run that downloads nothing and says so; INT, TERM and HUP stop
  the child and remove the temporary files; the next run removes the
  files of a run cut by KILL; `--version`, `--dry-run`, `--offline FILE`
  (`.tar.gz` or a directory), `--add-path FILE`; the hooks `inst_files`,
  `inst_version_ok`, `inst_setup`, `inst_selfcheck`.
- `scripts/install/podlab.sh`: the installer of `podlab`; its self-check
  is `podlab list`. `scripts/install/pins.py`: `sums`, `stamp` (the core
  inline and the pins: version, targets, SHA-256 and size of each file),
  `check` (the tree copy holds no pins), `bundle` (a tar.gz with the
  same bytes for the same input).
- `release.yml`: each run (the dry run too) builds `podlab` static for
  x86_64 and aarch64 musl with `SOURCE_DATE_EPOCH` (the versions of zig
  from `ci.yml`), checks it (`check-static.sh`, `check-strings.py`),
  stamps `podlab-install.sh`, makes `podlab-VERSION.tar.gz` and
  `SHA256SUMS`, installs from the bundle twice; a release attaches each
  file. `ci.yml`: the job `install` (`test.sh --strict`, with `dash`,
  `busybox-static` and `shellcheck`, and the sysctl of the user
  namespaces), in the needs of `all`.
- `sh scripts/install/test.sh`: 30 checks passed here, in about 5 s (the
  real `podlab` as the asset; `busybox` 1.35 on `PATH`): the pins; a first
  run; a second run with no download; a dry run; a replaced file; a file
  that does not run; a corrupt file; a run cut by KILL, TERM, INT and HUP;
  the offline bundle; no free space; `curl`, `wget`, `python3` and
  `busybox`, each also through a proxy with a credential (not in argv, a
  file or the output); `--add-path`; `dash`, `bash --posix`,
  `busybox sh` and `sh`; `podlab run` of `small-tmp`, `noexec-tmp`,
  `noexec-home` (the default prefix refused: noexec), `inherited-fds`,
  `no-proc`, `no-passwd-entry` and `ro-etc-ssl`, as root. Not here:
  `shellcheck` (the job `install` runs it); the labs as a user with no
  root (the job `install`).
- Plants (group `install`): `install-second-run-downloads-again`,
  `install-corrupt-file-installs`, `install-cut-run-leaves-a-half-file`,
  each caught.
- Found and fixed in the work: `scripts/check-size.py` read `.rs` files
  only; it now reads `.sh` and `.py` files too (`AGENTS.md`: each source
  file), with a case of its self-test.
- Docs: `docs/install.md`, `docs/session-protocol.md` (item 6),
  `docs/design.md` (the paths and the workflows).

Left: the first run of the job `install` and of the dry run of
`release.yml` on GitHub; then a COORD issue `(to all)` that names the tag
of the core; then the take of podrunner, podnet, podssh and podvm (each
its own installer on the core, its setup, its self-check and its
lanes); T-717 (the signature of the assets) and T-718 (the test on macOS
and Windows).

2026-11-16: `ci.yml` run 43779481815 at `f84fb23` failed in the job `install`: `shellcheck` (0.9.0, `--strict`) found SC2034 on the pins (read by name through `eval`), SC2317 on the default hooks (called by name), SC1003 on a backslash in a `case` pattern and SC2209 on a plain word in an assignment. Each is fixed with a directive that gives the reason, or with the other form; the test passed with `--strict` here (31 checks) after `shellcheck` was installed. The same run failed in the job `lint`: two plants of `pod-lab` were stale after T-737 (a session ran only its new plant); both are made again and caught.

# T-717: the installer core: a signature of the release assets, checked where `cosign` is there

**Source:** T-702 (the needs of podnet: the cosign signature of `SHA256SUMS`).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The installer core (T-702) checks each file against the pins that the
release writes into the installer. The installer itself comes over the
network with no check: a user who takes `podlab-install.sh` from a mirror
trusts that mirror. podnet's installer checks the cosign signature of
`SHA256SUMS` where `cosign` is there, with the identity of `release.yml`
at the tag.

## Premise

- Read: PodUtils/podnet:scripts/install.sh (`verify_signature`),
  `.github/workflows/release.yml`, `scripts/install/core.sh`.
- Not measured: a keyless signature of `sigstore` in a run of
  `release.yml` (it needs `id-token: write`).

## Approach

`release.yml` signs `SHA256SUMS` (which lists the installer, the bundle
and each binary) keyless with cosign, and attaches the bundle of the
signature. The core gets a step: where `cosign` is there and the run is
not offline, it downloads `SHA256SUMS` and its signature, checks the
identity `https://github.com/REPO/.github/workflows/release.yml@refs/tags/vVERSION`,
and checks each pin against `SHA256SUMS`. `docs/install.md` gives the
command for a check of the installer by hand. A plant: a changed
`SHA256SUMS` that the check must refuse.

## Prove

```sh
sh scripts/install/test.sh
```

## Done

2026-11-16, the commit of this entry (with T-718).

- `scripts/install/core-net.sh` (new): the network part of the core (the
  proxy, the downloader, the download) and the step of the signature,
  `inst_signature`. Where `cosign` is on `PATH` (or `PODLAB_COSIGN=CMD`)
  and the run is not offline, before a download: it gets `SHA256SUMS` and
  `SHA256SUMS.sigstore.json` of the release, runs `cosign verify-blob`
  with the identity
  `https://github.com/REPO/.github/workflows/release.yml@refs/tags/vVERSION`
  and the issuer `https://token.actions.githubusercontent.com`, then
  checks each pin of the target against `SHA256SUMS`, and the installer
  itself when it runs from a file. A failure is a refusal with the output
  of `cosign`; no file changes. No `cosign`, `PODLAB_COSIGN=none` or
  `--offline` gives one line, and the pins check each file. `TUF_ROOT`
  is a temporary directory of the prefix (call C741). The dry run names
  the check. `scripts/install/core.sh` keeps the rest; `pins.py` puts
  both files inline and checks that the tree copy includes both.
- `release.yml`: a run with a version makes the tag and starts itself on
  the tag; the run on the tag (`id-token: write`) signs `SHA256SUMS`
  keyless (`sigstore/cosign-installer` v4.1.3 by its commit, cosign
  v3.1.4, the same pins as podnet), verifies it, refuses a changed copy
  and the identity of a branch, runs the published installer with
  `cosign` against `dist`, and attaches `SHA256SUMS.sigstore.json` to the
  release. `docs/install.md` gives the check by hand;
  `docs/maintenance.md` and `docs/session-protocol.md` give the release
  steps and the file `core-net.sh` that a project takes.
- `sh scripts/install/test.sh --strict`, `PODLAB` the release build of
  `podlab`: 36 checks passed, 0 skipped, 0 n/a (`shellcheck` 0.9.0,
  `busybox` 1.36.1, `dash`). The case `sig`, with a stand-in of `cosign`
  that checks its arguments and the SHA-256 of the blob: a pass with the
  identity of the tag, SHA256SUMS and its bundle downloaded once, no file
  in `HOME`; the dry run and the offline run; a changed `SHA256SUMS`, a
  pin that it does not hold, no bundle and a changed installer are each
  refused with no file in place; no `cosign` and `PODLAB_COSIGN=none`
  give one note and install; a missing `PODLAB_COSIGN` is refused.
- The real `cosign` v3.1.4 (SHA-256 of `cosign_checksums.txt`) with
  `inst_signature` of the core against `SHA256SUMS` and its bundle of
  PodUtils/podnet v0.4.0-beta.13: verified; a changed copy refused
  ("invalid signature when validating ASN.1 encoded signature"); the
  identity of v0.4.0-beta.12 refused; no file in `HOME`.
- Plant `install-changed-sums-installs` (the core ignores the result of
  `cosign`): caught. The plant `install-cut-run-leaves-a-half-file` was
  stale after the split of the core; its patch is made again, caught.
- `shellcheck scripts/install/*.sh`: 0 findings.
  `scripts/check-workflows.py`, `check-size.py`, `check-design-paths.py`:
  0 faults.

2026-11-16, the first run on a tag: `release.yml` run 43828067548 on
`v0.61.0` (`cbbd016`). `cosign sign-blob` wrote `SHA256SUMS.sigstore.json`
with the identity of `release.yml` at the tag `v0.61.0`;
`cosign verify-blob` gave "Verified OK", and refused a changed copy
("invalid signature") and the identity of `refs/heads/main` ("no matching
CertificateIdentity"). The published installer printed "the signature of
SHA256SUMS: ok (cosign, ...release.yml@refs/tags/v0.61.0)". The release
holds `SHA256SUMS` and `SHA256SUMS.sigstore.json`.

# T-718: the installer core: the test on macOS and Windows

**Source:** T-702 (the needs of podssh: the target triple of each host, the quarantine of macOS).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The installer core (T-702) has the paths of macOS (`aarch64-apple-darwin`
also under Rosetta, the quarantine attribute) and of Windows under a POSIX
shell (`.exe`, `MINGW*`, `MSYS*`, `CYGWIN*`), but its test runs on Linux
only. `df -Pk`, the `noexec` reason, `kill -0` and the signals behave in
another way on these hosts, and podssh and podnet release binaries for
them.

## Premise

- Read: `scripts/install/core.sh`, `scripts/install/test.sh`,
  `.github/workflows/nightly.yml`.
- Not measured: the test under `/bin/sh` of macOS (bash 3.2 in POSIX
  mode) and under Git Bash of a Windows runner.

## Approach

A job of `nightly.yml` runs `sh scripts/install/test.sh` on `macos-15` and
on `windows-2025` (Git Bash), with fake assets for the triple of the host
(the cases that need `podlab run` and `setsid` skip there, by name). The
test learns the triple of the host from the core, so the fixture names
`podlab-aarch64-apple-darwin` or `podlab-x86_64-pc-windows-msvc.exe`.

## Prove

```sh
sh scripts/install/test.sh
```

## Done

2026-11-16, the commit of this entry (with T-717).

- `nightly.yml`: the job `install-os` runs `sh scripts/install/test.sh
  --strict` on `macos-15` and `windows-2025` (`shell: bash`, Git Bash),
  with `wget` from Homebrew on macOS, a `python3` that runs `python`
  where Windows has only the stub of the Microsoft Store, and a time
  limit of 90 s for each run under Git Bash. Not in `ci.yml`, as the wip
  had it: call C740.
- `scripts/install/test.sh`: the core gives the target and the `.exe` of
  the host, and the fake assets have those names
  (`podlab-aarch64-apple-darwin`, `podlab-x86_64-pc-windows-msvc.exe`);
  a part that the OS does not have is `n/a` and does not fail with
  `--strict` (macOS: `busybox`, the labs, `shellcheck`; Windows also
  `dash`, `wget`, the cut run); `shasum` where there is no `sha256sum`;
  the cut run makes a new session with `python3` (no `setsid` on macOS);
  `SYSTEMROOT` for a native program of Git Bash. The cases `downloaders`,
  `proxy`, `exe` and `sig` are in `scripts/install/test-cases.sh` (the
  limit of 500 lines). The case `exe`: the target of Windows installs
  `podlab.exe`, a prefix `C:\...` goes through `cygpath`, and the CR of
  `curl` of Windows is not shown. `mirror.py` stops when its port file
  goes (a signal of Git Bash does not reach a native Python), and
  `pins.py` writes LF on each host.
- Linux: `sh scripts/install/test.sh --strict` gives 36 passed, 0
  skipped, 0 n/a.
- A host of Windows and of macOS on Linux, by a `uname` (and `sysctl`)
  that gives `MINGW64_NT-10.0-26100 x86_64` or `Darwin arm64`, and a
  `PATH` with no `dash`, `busybox`, `wget` and `shellcheck` (Windows) or
  no `busybox` and `shellcheck` (macOS): `--strict` gives 21 passed, 0
  skipped, 9 n/a (windows), and 25 passed, 0 skipped, 5 n/a (macos). This
  proves the logic of the test, not the hosts.

2026-11-16, the first run on GitHub: nightly 43817142905 at `1739b5f`,
the jobs `install-os (macos-15, 30)` (133131677417) and
`install-os (windows-2025, 90)` (133131677339) passed `sh
scripts/install/test.sh --strict` under the `/bin/sh` of macOS and the Git
Bash of Windows.

# T-703: a mechanical check of the open issues and their comments against the record, for each project (the ruling "Issues")

**Source:** the owner's ruling "Issues" (2026-11-15).
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** M
**Status:** partial

## Problem

The open issues of the projects of PodUtils and their comments go stale and drift from the work: an issue stays open after its work is done, a closed issue holds work that is open, and an old comment states a plan that changed. They mislead external users and testers, and they mislead the agents of the project itself. The owner's ruling "Issues" (2026-11-15) asks for a mechanical solution. podcrates has only `scripts/check-github-numbers.py`, which compares `TODO/github.txt` with the state of each number on GitHub; nothing ties the state of an issue to the state of its entries.

## Premise

- Read: `scripts/check-github-numbers.py`, `crates/pod-todo/src/github.rs`.
- Read: PodUtils/podssh#269, PodUtils/podnet#586, PodUtils/podrunner#184. Each maps an issue in `TODO/issues.md` (a row `| #N | Subject | State | Entries |`, or a bold head `**#N, ...**` for an issue of many asks) and lists each number in `TODO/github.txt`; each entry's `**Source:**` names the issue. The State column is written by hand: it is the part that drifts. `record-issues.yml` (`check-github-numbers.py`) compares `github.txt` with GitHub each day.
- Read: PodUtils/podvm#428: the same map (`podvm-gate` checks each number against `github.txt`, by hand); it asks for a kind field (`report`, `coord`, `request`; a request has no entry), the tool's block in place of the `state:` lines of a COORD issue, and a dry run that prints each change before the workflow closes or reopens an issue.

## Approach

1. `pod-todo` (the shared checker of the records, T-001) gets a check of the issues: each issue of `TODO/issues.md` is open exactly when one of its entries is not `done`; an issue whose entries are all `done` must close; an issue with an open entry must stay open; an issue with no entry fails.
2. The status of an issue is one block in its body that a tool writes from the record (the entries, their status, the tag that carries the work, the date), between two markers. A comment never holds the current state. A session never edits the block by hand.
3. A workflow of each project (a reusable workflow of podcrates, at a tag) runs on each push to the default branch and each day: it writes the block of each issue, closes an issue whose entries are done (with a comment that names the tag), reopens a closed issue with an open entry, and fails when an open issue has no entry or no block. It uses `GITHUB_TOKEN` with `issues: write`.
4. Each project takes the workflow; podcrates first.

The kinds of issue, each with its rule (from the answers): an external report (a user or a tester; its author reads only that issue, so its block matters most); a COORD message (`COORD-NNN:` in the title) closes on the answer of the other project and its subject, not on the entries of the sender, and its `state:` lines move into the block; a run request (`[gh action] requested`) has no entry of its own and closes when the runs are recorded; a deferred issue (`[DEFERRED]`) stays open with blocked entries; a tracker (its body lists the asks of another project) closes when each ask is done. The State column of `TODO/issues.md` becomes generated, or leaves. A session account that cannot start a workflow relies on the daily schedule, and on each push where the account can start one.

## Prove

```sh
cargo test -p pod-todo
python3 -I scripts/issue-sync.py --self-test
```

## Done

2026-11-15, partial: the check, the block, the sync and the workflow of
podcrates are in (calls C692 to C698); the take of each project is left.

- The kinds (`crates/pod-todo/src/issues.rs`, C692): report, coord,
  request, deferred and tracker from the title, or from the column `Kind`
  of the map; pull from the line of pull requests. `TODO/issues.md` has the
  column `Kind` for each row. `read_map` reads the columns by the header,
  and the paragraph of a bold head as its entries.
- The rules (`cargo todo check`, C693): a report, a deferred issue or a
  tracker names one entry or more, and is open in `TODO/github.txt`
  exactly when one entry is not done. The rows that they found (C698):
  #1 is closed in the record; #124, #125 and #131 are coord.
- The block (`cargo todo issues --block N`, `--json`;
  `crates/pod-todo/src/block.rs`, C694, C695): the kind, the state, each
  entry with its status, its title and its tag, and the date of the
  record, between `<!-- pod-todo:state -->` and
  `<!-- /pod-todo:state -->`. Each done entry of the map gets a tag.
- The sync (`scripts/issue-sync.py`, C696, C697): the block written or
  replaced (the rest of the body kept), a close with one comment, an open
  again, `--dry-run`, the guard of the ruling "Strangers", and a failure
  on drift that it cannot fix. `.github/workflows/issue-sync.yml`: each
  push to `main`, each day, by hand (`dry_run`), and `workflow_call` with
  the input `podcrates_ref`; the dry run, then the sync.
- Tests: `cargo test -p pod-todo --lib`, 46 unit tests passed (15 new in
  `issues` and `block`); `python3 -I scripts/issue-sync.py --self-test`,
  16 checks passed. Plants: `issues-closed-with-an-open-entry`,
  `issues-open-with-each-entry-done`, `issues-report-with-no-entry`,
  `issues-block-with-no-end-marker` (group `record`),
  `issue-sync-edits-a-stranger`, `issue-sync-dry-run-writes`,
  `issue-sync-drops-the-body` (group `agent`), each caught.

Left: the first run of `issue-sync.yml` on `main` (a dispatch with
`dry_run` first, then the sync, which closes #1 on GitHub); then a COORD
issue `(to all)` that names the tag, and the take of podssh, podnet,
podrunner and podvm (`docs/session-protocol.md`, item 5): each adds the
column `Kind` where a title does not give the kind, fixes the rows that the
rules find, and calls the workflow. The State column of the map can then
become the state alone.

The same day, the dry run of `issue-sync.yml` on GitHub (run 43762153730, PodUtils/podcrates#190): 171 issues would get the block, #1 would close with a comment, 14 closed issues of a tester outside the team stay as they are, and the run failed on #190 alone, which the record did not map yet. A session opens each request and message before its record can map it, and a request freezes `main`, so an unmapped issue younger than `GRACE_DAYS` (3) is now a note, not a fault. The run would also have opened #189 again: a session closed it by hand before the record did. A session closes an issue through the record; the sync closes it on GitHub.

The same day, `adoption.yml` run 43762137913 at `bf13591` failed for podssh and podnet: each takes `pod-todo` of `main` in its trial, and the new rules of the issues found drift in both records (podssh: 17 problems, such as closed reports with open entries; podnet: 8). A new rule of a shared checker must not fail each consumer at once. The rules are now on only where `TODO/todo.toml` says `issue_rules = "on"` (podcrates says so); a consumer turns them on when it takes the sync, after it repairs its map. The drift that the rules found is in the hand-off of the next tag.

# T-704: `podlab`: the usage errors of clap in the form of `pod_manual::error::fault`

**Source:** T-700 (a finding of part 1).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

The help and the manual of `podlab` come from the model of `pod-manual`,
but its usage errors come from the text of clap:
`podlab man --roff --json` writes "error: the argument '--roff' cannot be
used with '--json'" and a usage line, then exits 2. The text is not in the
form "OPTION: what is wrong; fix: what to do" (the ruling "The text of a
command line" and part 14 of T-700), and no check of `pod-manual` reads
it.

## Premise

- Read: `crates/pod-lab/src/bin/podlab.rs`, `crates/pod-manual/src/error.rs`.
- Measured: the output of `podlab man --roff --json` (a `podlab` of the
  local branch `manual`).

## Approach

Map each kind of `clap::Error` that `podlab` can give (an unknown
argument, a missing argument, a conflict, a bad value, an unknown
command) to `pod_manual::error::fault`, with the option from the context
of the error and a fix from the model. A test gives each kind and checks
the text with `pod_manual::error::check`. The map can move to
`pod_manual::clap` for each clap program.

## Prove

```sh
cargo test -p pod-lab --all-features
```

# T-705: `check-strings.py`: the strings of Mach-O and PE binaries

**Source:** T-700 (a finding of part 1).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

`scripts/check-strings.py` reads the section `.rodata` of an ELF file
only. The consumers release binaries for macOS and Windows too (podssh,
podnet), and the ruling "The text of a command line" asks for a check of
the strings of each release binary.

## Premise

- Read: `scripts/check-strings.py`, the targets of `docs/design.md`,
  section 7.
- Not measured: the sections of a Mach-O and a PE binary of the static
  probe.

## Approach

Read the sections of the string literals of Mach-O (`__TEXT,__cstring`,
`__TEXT,__const`) and of PE (`.rdata`), with the same rules and the same
plant. The static job runs the check on the static probe of each target.

## Prove

```sh
python3 -I scripts/check-strings.py --plant /bin/ls
```


# T-714: `pod-hostprobe`: the rows of the socket classes of podssh and podnet (bind alone, listen, UDP, raw and packet sockets, the loopback)

**Source:** T-701 (the profiles `listen-denied`, `udp-denied`, `raw-denied` and `no-loopback` of `pod-lab`).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`pod-lab` observes five facts of the socket classes of podssh and podnet
only from inside a lab (`LabFact`): a TCP bind of `127.0.0.1:0` alone,
`listen(2)` after that bind (`EACCES` or `EPERM`), a UDP socket and its
connect of `AF_INET` and `AF_INET6`, a raw socket of `AF_INET` and a
packet socket (`EPERM`), and a TCP connect of `127.0.0.1` with no
loopback (`ENETUNREACH`). The one socket row of `pod-hostprobe`,
`bind(127.0.0.1:0)+listen`, cannot tell a refused bind from a refused
listen, and a fresh network namespace still takes the bind. A check of a
report gives `unseen` for each part, so a host that a user reports cannot
show the classes.

## Premise

- Read: `facts.rs` of `pod-lab` (the measurements; T-714 removes it),
  `crates/pod-hostprobe/src/probes/access.rs` (`p_tcp_listen`).
- Measured: in a fresh network namespace of Linux 6.18, `bind(127.0.0.1:0)`
  and `listen` pass, and `connect(127.0.0.1:1)` gives `ENETUNREACH`.
- Not measured: the rows on a host whose policy refuses `listen` alone.

## Approach

Add census rows, each in its own child, with the errno: `bind(127.0.0.1:0)`,
`listen after bind(127.0.0.1:0)` (a skip where the bind fails),
`socket(AF_INET, SOCK_DGRAM)+connect`, `socket(AF_INET6, SOCK_DGRAM)+connect`
(`EAFNOSUPPORT` is a denial), `socket(AF_INET, SOCK_RAW)`,
`socket(AF_PACKET, SOCK_RAW)` and `connect(127.0.0.1:1)` (its errno in the
reason). Then move the `LabFact`s of these parts to `Observe::Row` in
`crates/pod-lab/src/table/parts.rs`. The JSON Schema of the report takes
the new names. Do it with T-693, which adds the rows of `bind-denied`.

## Prove

```sh
cargo test -p pod-hostprobe --all-features
cargo test -p pod-lab --all-features
```

## Done

2026-11-16, commits 30aa8977, 7defab02, 3d63ff89 and the next one (local branch `rows`); calls C732 to C735.

- Seven census rows of `pod-hostprobe`, each in its own probe child
  (`src/probes/sockets.rs`): `bind(127.0.0.1:0)`, `listen after
  bind(127.0.0.1:0)` (a skip where the bind fails),
  `socket(AF_INET, SOCK_DGRAM)+connect` and
  `socket(AF_INET6, SOCK_DGRAM)+connect` (the errno of the step;
  `EAFNOSUPPORT` with no IPv6), `socket(AF_INET, SOCK_RAW)` (ICMP),
  `socket(AF_PACKET, SOCK_RAW)` and `connect(127.0.0.1:1)` (no wait over
  2 s; `ECONNREFUSED` is `ok` with the errno in the reason). New errno
  names and constants: `ENXIO`, `ENETUNREACH`, `ECONNREFUSED`,
  `EINPROGRESS`, `EISCONN`, `EAFNOSUPPORT`.
- `pod-lab`: `bind-allowed`, `listen-refused`, `udp-refused` (also of
  `proxy-only`), `raw-refused`, `loopback-absent` and `tcp-connect-refused`
  of `bind-denied` read the rows; the lab facts `BindAllowed`,
  `ListenRefused`, `UdpRefused`, `RawRefused`, `NoLoopback` and
  `InetConnectRefused` and `src/facts.rs` are gone. T-693 is `partial`:
  its connect row is in.
- Tests and plants: see T-745.

# T-715: `pod-hostprobe`: the rows of the file classes of podssh (absent paths, the terminal, the lines of the ids, `HOME`, read-only mounts)

**Source:** T-701 (the profiles `no-ptmx`, `no-passwd-entry`, `noexec-home`, `no-proc` and `ro-etc-ssl` of `pod-lab`).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`pod-lab` observes these facts only from inside a lab (`LabFact`): a path
that does not exist (`/dev/pts`, `/proc/self/exe`), no controlling
terminal (`tty_nr` of `/proc/self/stat`), `/dev/tty` that opens and has
no data, no line of the uid in `/etc/passwd` and of the gid in
`/etc/group`, `HOME` writable on a `noexec` mount, and `/etc/ssl` on a
read-only mount. A check of a report gives `unseen` for each part.

## Premise

- Read: `facts.rs` of `pod-lab` (T-714 removes it), `crates/pod-hostprobe/src/identity.rs`,
  `crates/pod-hostprobe/src/writable.rs`.
- Not measured: a host with no `/proc` (the probe children of
  `pod-hostprobe` start through `/proc/self/exe`, so a row there needs a
  measurement in the probe process itself).

## Approach

Add rows that read the facts of the probe process: `stat(/dev/pts)`,
`readlink(/proc/self/exe)` (in the probe process, not a child),
`tty_nr` and `open(/dev/tty, O_NONBLOCK)+read`, the lines of the ids
(ok with the uid and the gid in the reason), `statfs(HOME)` (`ST_NOEXEC`,
and a new file) and `statfs(/etc/ssl)` (`ST_RDONLY`). Then move the
`LabFact`s to `Observe::Row` in `crates/pod-lab/src/table/parts.rs`.

## Prove

```sh
cargo test -p pod-hostprobe --all-features
cargo test -p pod-lab --all-features
```

## Done

2026-11-16, commits 30aa8977, 7defab02, 3d63ff89 and the next one (local branch `rows`); calls C732 to C735.

- Eight census rows (`src/probes/files.rs`): `stat(/dev/pts)`,
  `readlink(/proc/self/exe)` (in the probe process: `Kind::Here`,
  `Probe::measure`), `tty_nr of /proc/self/stat`,
  `open(/dev/tty, O_NONBLOCK)+poll` (no read), `line of getuid() in
  /etc/passwd`, `line of getgid() in /etc/group`,
  `creat($HOME/.pod-hostprobe)` (with `noexec`) and `statfs(/etc/ssl)`.
  A row of a fact gives `ok` with `KEY VALUE` items (`verdict::items`,
  `Outcome::value`).
- `pod-lab`: `ptmx-absent`, `no-ctty`, `tty-opens`, `no-id-lines`,
  `home-noexec`, `proc-absent` and `etc-ssl-read-only` read the rows with
  `Expect::Value`; the lab facts `Absent`, `NoControllingTerminal`,
  `TtyOpens`, `NoPasswdEntry`, `HomeNoexec` and `ReadOnlyMount` are gone.
  `/dev/net/tun` keeps its row of the machine tier.
- Tests and plants: see T-745.

# T-716: `pod-lab`: the labs of the network classes of podssh and podnet (podnet's hand-off)

**Source:** T-701; PodUtils/podssh#268, PodUtils/podnet#585.
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

The table of `pod-lab` names six network classes whose parts no lab of
the crate holds: `proxy-tls-reterminate` (the proxy shows its own CA for
some names; `CONNECT host:22` gets 200 and then no byte),
`proxy-per-name` (200 for one name and 504 for another),
`sni-allowlist` (a reset after a ClientHello with a name not on the list,
or with no name), `no-proxy-no-dns` (no resolver; only IP literals
connect), `ws-upgrade-refused` (the proxy refuses the WebSocket upgrade)
and `ipv6-only` (no IPv4 route). With `proxy-only` of T-692, each part is
`Observe::Nothing`: no row of `pod-hostprobe` observes it, and
`podlab run` refuses each one (exit 125). podnet offered to send the
network profiles as data in the form of the table (PodUtils/podnet#585).

## Premise

- Read: `crates/pod-lab/src/table/parts.rs`, PodUtils/podnet#585 (the
  CI jobs `restricted-host` and `no-loopback` of podnet), `pod-netlab`.
- Not measured: a proxy or a route out in a network namespace of a user
  namespace on a runner.

## Approach

Take podnet's hand-off (the parts of each class as data). For each part,
a probe row where a probe can observe it from the host (a CONNECT through
`HTTPS_PROXY` with its status, the CA of a TLS handshake through the
proxy, a lookup of a name, a TCP connect to an IPv4 and an IPv6 literal,
a ClientHello with and with no SNI), each skipped where the host has no
proxy or no network. The lab: a network namespace with a proxy of the lab
on its loopback (`pod-netlab`), for the parts that need no outside
network; the others stay in the live jobs of podnet. Ask podnet by a COORD
issue for the hand-off.


Podnet's data (PodUtils/podnet#592): `PodUtils/podnet:docs/lab-profiles/` at `dbd432c`, one file for each of the six classes, in the form of `crates/pod-lab/src/table/`, each part with the way a lab can hold it (no root, a user namespace, root only, a stand-in proxy) and the podnet code it cites. The parts that podnet did not measure are marked "assumed". Take them into the table; say what changes.

## Prove

```sh
cargo test -p pod-lab --all-features
sh scripts/ci/userns-tests.sh
sh scripts/plant-each.sh --group lab
```

## Done

2026-11-16, commits fbc0eead, 03fae0c6, 8af451f3 and the next one (local
branch `net716`); calls C725 to C729.

- `crates/pod-lab/src/net.rs` and `src/net/`: the network lab
  (`Hold::LabNet(&Net)`, `LabFact::Net(NetFact)`). A network namespace with
  no route out, in the user namespace of the mounts, its loopback up; a
  stand-in process (two forks, a pidfd of the lab, one thread for each
  connection) with a `CONNECT` proxy on `127.0.0.1:3128` and a TLS server
  on port 443; two CAs made for each run (the pure provider of `pod-tls`,
  rcgen); `SSL_CERT_FILE` and the variables of a proxy for the command;
  the check of each fact from inside before the command.
- How each profile is held, each with no root (a user namespace; as root a
  network namespace alone):
  - `proxy-only`: no direct route (`ENETUNREACH`); the stand-in proxy with
    the made-up credential `podlab` (407 with none), the ports 443, 80 and
    8443 (403 for port 22); UDP refused by the seccomp rules of
    `udp-denied`. The credential is in `.github/secrets-allow.txt`.
  - `proxy-tls-reterminate`: the proxy terminates TLS of `listed.test`
    with its CA (in `SSL_CERT_FILE`), passes `other.test` to the CA of the
    server, answers 200 and then no byte on port 22.
  - `proxy-per-name`: 200 for `listed.test`, 504 for `other.test`, a reset
    for `192.0.2.1`.
  - `sni-allowlist`: no proxy; the server at `192.0.2.0/24:443` resets a
    ClientHello for another name or with no name. Not held: the filter on
    each address (T-747).
  - `no-proxy-no-dns`: no proxy; `/etc/resolv.conf` with no server,
    `/etc/hosts` with `localhost`, `hosts: files dns`; TLS to
    `192.0.2.1:443` checks the address.
  - `ws-upgrade-refused`: the proxy terminates TLS of `listed.test`, gives
    200 to a plain `GET` and 403 to an upgrade.
  - `ipv6-only`: `fd00:6c61:62::1` on the loopback, no IPv4 address but
    the loopback. Held on a kernel with IPv6 (a runner of GitHub). This
    host has no IPv6 (`socket(AF_INET6)` gives `EAFNOSUPPORT`): the run
    refuses it with that reason, and the test checks the refusal. The held
    path is not measured here. Not held: an IPv6 device with a default
    route (T-747).
  - Not held here: a route out of a lab to the real network (T-746 for
    the proxy classes, T-747 for the direct classes).
- The answer to podnet: what changed from
  `PodUtils/podnet:docs/lab-profiles/` at `dbd432c` (C729). The needs became lab facts; their rows are T-745.
  The names `good.lab` and `bad.lab` are `listed.test` and `other.test`;
  `192.0.2.1` and `fd00:6c61:62::1` stay. `source` is `podssh, podnet` for
  the five classes of podssh. `tls-reterminated` keeps podnet's two
  observations and adds that the other CA does not take the chain.
  `ssh-port-silent` waits 1 s, not 5 s. `answer-per-name` takes podnet's
  reset of an IP literal; `sni-reset` podnet's listed name that passes;
  `upgrade-refused` podnet's plain request that passes (403 for the
  upgrade); `no-ipv4-route` podnet's `ENETUNREACH` and the loopback that
  stays. `no-resolver` checks that `/etc/resolv.conf` names no server and
  that `127.0.0.1:53` refuses a TCP connect, not a query of the server of
  the file. `literal-reaches` is TLS to `192.0.2.1:443`, not `1.1.1.1`.
  `ipv6-reaches` reaches the address on the loopback, with no dummy
  device. Each part that podnet marked assumed ends its text with
  "Assumed: WHAT" (C727): `tls-reterminated`, `ssh-port-silent`,
  `answer-per-name` (the 504), `sni-reset`, `literal-reaches`,
  `upgrade-refused`, `ipv6-reaches`. `ipv6-only` gets the about "no IPv4
  route; IPv6 reaches out".
- Tests: 30 unit tests (the table: one network lab for each class, the
  assumed parts; the head of HTTP, the server name of a ClientHello of
  rustls, `nsswitch.conf`); 6 of the binary (1 ignored), 4 of
  `tests/cli.rs`; `tests/lab.rs` 26 cases, each as root and as uid 65534
  (52 lines "ok"), with `tests/lab/net.rs`: each network class with a
  payload that reads `HTTPS_PROXY` and `SSL_CERT_FILE`, then the removal of
  the certificate file. `scripts/ci/userns-tests.sh`: passes, no skip.
- Plants (group `lab`): 10 new (`lab-proxy-credential-ignored`,
  `lab-proxy-own-ca-dropped`, `lab-silent-port-answers`,
  `lab-sni-filter-open`, `lab-upgrade-passes`, `lab-literal-reset-dropped`,
  `lab-resolver-file-kept`, `lab-outside-address-everywhere`,
  `lab-proxy-variable-unset`, `lab-cert-file-left`), three patches written
  again, one EXPECT changed; `plant-each.sh --group lab`: 33 caught;
  `--apply`: 0 stale.
- Static: `podlab` for `x86_64-unknown-linux-musl` (cargo zigbuild,
  `SOURCE_DATE_EPOCH`): `check-static.sh`, `check-strings.py` and
  `check-man.sh` pass; the static binary holds each network lab here but
  `ipv6-only`.
- `man/podlab.1` regenerated; `LANES.md`: the network classes are held
  (stand-ins) in the lanes of podssh and podnet; the README has "A network
  lab". T-702 (the installer) can now run in the network labs (C711).
- Findings: T-745 (the probe rows), T-746 (a way out through the stand-in
  proxy), T-747 (the direct classes on each address).

# T-745: `pod-hostprobe`: the probe rows of the network classes (a CONNECT through the proxy, the CA of a handshake, a lookup, a direct connect, a ClientHello)

**Source:** T-716 (the part of its Approach that a probe of the host observes).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

The network labs of `pod-lab` measure each fact of a network class from
inside, through the stand-ins (`LabFact::Net`). A check of a report of a
host (`podlab check PROFILE`) gives `unseen` for each part of
`proxy-only`, `proxy-tls-reterminate`, `proxy-per-name`, `sni-allowlist`,
`no-proxy-no-dns`, `ws-upgrade-refused` and `ipv6-only`: no row of
`pod-hostprobe` observes a proxy, a resolver or a route. A user cannot
show that a host is of a class.

## Premise

- Read: `crates/pod-lab/src/net/measure.rs` (the facts), the Approach of
  T-716.
- Not measured: a row on a host with a real proxy, with no resolver, or
  with no IPv4 route.

## Approach

Rows of `pod-hostprobe` that observe the host, each skipped where the host
has no proxy (`HTTPS_PROXY`) or no network, each with a time limit: a
`CONNECT` through `HTTPS_PROXY` to a name and port of the settings, with
its status; the issuer of the leaf of a TLS handshake through the proxy;
a lookup of a name; a TCP connect to an IPv4 and an IPv6 literal, with the
errno; a ClientHello with a name and with none, and a reset. The names and
the addresses are settings of the probe, as a real host needs real ones.
Then the facts of the parts whose row a host gives move to `Observe::Row`,
and the lab keeps its stand-ins for its own check. The JSON Schema of the
report takes the new names.

## Decision

C735: no row reaches the network. This entry adds the rows that read what a host shows of its way out with no packet out; the rows that send a packet through a proxy or to a server are T-751, behind a setting of the caller that is off by default.

## Prove

```sh
cargo test -p pod-hostprobe --all-features
cargo test -p pod-lab --all-features
```

## Done

2026-11-16, commits 30aa8977, 7defab02, 3d63ff89 and the next one (local branch `rows`); calls C732 to C735.

- Five census rows that read the host with no packet past the loopback
  (`src/probes/network.rs`): `getenv(HTTPS_PROXY)` (the first of
  `https_proxy`, `HTTPS_PROXY`, `all_proxy`, `ALL_PROXY`: set, variable,
  scheme, credential 0 or 1, port; no host and no credential in the
  reason), `getenv(SSL_CERT_FILE)+read` (the certificates of the file),
  `read(/etc/resolv.conf)` (the `nameserver` lines; with none, a TCP
  connect to `127.0.0.1:53`), `read(/proc/net/route)` and
  `read(/proc/net/ipv6_route)` (a default route that is up and no reject
  route; `ipv6 absent` where the kernel has none). 88 rows in all; the
  JSON Schema of the report names each one (schema 1); the example
  `valid-host-classes.json`.
- `pod-lab`: the parts `proxy-variable` (`proxy-only`,
  `proxy-tls-reterminate`, `proxy-per-name`, `ws-upgrade-refused`),
  `no-proxy` (`sni-allowlist`, `no-proxy-no-dns`) and `cert-file`
  (`proxy-tls-reterminate`); `direct-refused`, `no-resolver` and
  `no-ipv4-route` read the rows. `Expect::Value`, `Expect::Shows` and
  `Observe::LabCheck` (C733): the connect out of the lab and its lookup
  stay as checks of the lab beside the rows. `NetFact::LoopbackReaches` is
  gone. The network lab sets its variables in the environment of the lab
  process before it measures (C734). `podlab list` and `man/podlab.1`
  follow.
- Tests: 160 unit tests of `pod-hostprobe` (the items of a reason, the
  field `tty_nr`, the lines of an id, the items of a proxy URL, the
  certificates, the `nameserver` lines, the default routes, the kinds of
  the rows); `tests/host_rows.rs` (no libtest): each row against what the
  test reads itself, as root and again as uid 65534 (a raw and a packet
  socket `EPERM`, a file in a `HOME` of root `EACCES`), also in
  `scripts/ci/userns-tests.sh`; `tests/children.rs`: 88 rows, the report
  reads back. `pod-lab`: 45 unit tests, 13 of them in `src/check/rows.rs`
  (a made-up report of each changed part through the JSON);
  `tests/lab.rs` each case as root and as uid 65534, each lab holds.
- Plants: `hostprobe-listen-row-measures-the-bind`,
  `hostprobe-connect-reset-denied`, `hostprobe-tty-nr-field-shifted`,
  `hostprobe-proxy-host-in-reason`, `hostprobe-route-reject-default`,
  `hostprobe-proc-row-in-a-child`, `lab-check-shows-fails`,
  `lab-check-labcheck-in-report`, `lab-check-value-missing-holds`; the
  patch and the EXPECT of `lab-proxy-variable-unset` written again.
  `plant-each.sh --group hostprobe`: 22 caught; `--group lab`: 36
  caught; `--apply`: 0 stale. `scripts/ci/userns-tests.sh` passes as root
  (each case again as uid 65534), with no skip.
- Not here: the rows that send a packet out (T-751); the row of a default
  route of IPv6 for `ipv6-reaches` (T-747).

# T-746: `pod-lab`: a way out through the stand-in proxy of a proxy class, for the live lanes of the consumers

**Source:** T-716 (the network labs have no route out; PodUtils/podssh#268 and PodUtils/podnet#585 name the live commands of each class).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

A network lab of `pod-lab` has no route out: a command reaches only the
stand-ins. The lanes of podssh (`podssh ssh` to GitHub through its relay)
and of podnet (`podnet tunnel --quick`, `warp proxy`) need the real
network through the proxy of the class, as the hosts of their users give
it: a real proxy-only host passes a `CONNECT` to a name that it allows.
In the lab such a name gets the answer of a name that is not on the list.

## Premise

- Read: `crates/pod-lab/src/net/standin.rs`, `crates/pod-lab/src/net/proxy.rs`;
  podnet's form in `PodUtils/podnet:docs/lab-profiles/proxy-tls-reterminate.md`
  at `dbd432c` (the proxy outside the namespace on a unix socket, a
  forwarder inside).
- Not measured: a forwarder from a network namespace of a user namespace
  to a unix socket of the host on a runner.

## Approach

An option of `podlab run` (`--way-out`) for the proxy classes: the lab
starts a second stand-in before it enters the network namespace; it
listens on a unix socket of a new directory of the temporary directory and
dials the real network for each `CONNECT` that the rules of the class pass
(a name of the settings, a port of the class). The stand-in proxy in the
namespace splices each such `CONNECT` through the unix socket. The rules of
the class (the credential, the ports, 403, 504, a reset, the silent port,
the CA of the proxy for a listed name) stay in the stand-in proxy. A test
runs a listener of the test outside the lab and reaches it from inside
through the proxy alone; a direct connect still gives `ENETUNREACH`.

## Prove

```sh
cargo test -p pod-lab --all-features --test lab
```

## Done

2026-11-16. `podlab run --way-out` (`Options::way_out`), for a profile with a stand-in proxy alone. The lab starts the process of the way out before its network namespace (`crates/pod-lab/src/net/out.rs`): it dials directly, or through the proxy of the host (the variables in the order of curl) with its credential. The stand-in proxy applies the rules of its class, then asks the way out for each `CONNECT` to a name that is not of the lab (`is_lab_name`) on a socket pair, and gets the descriptor of the connected stream (`net/pass.rs`, `SCM_RIGHTS`); where the class terminates TLS, it checks the real server with the roots of the host and shows a leaf of the CA of the lab (`net/relay.rs`; `pod-tls` takes the feature `rsa` for a real chain). The command and the arguments of the lab never hold the proxy of the host; `podlab run --way-out` and each serving process of the lab are not dumpable. Found and fixed: a direct way out dialed a name that the resolver of the host gives the loopback, so a command of the lab reached the services of the host; it now refuses a loopback or unspecified address after the lookup (test `a_direct_way_out_does_not_dial_the_loopback_of_the_host`, plant `lab-way-out-dials-the-host`). Found and fixed: the help of `--way-out` had a sentence of 30 words, which the text rules of `pod-manual` refused; the manual is generated again. Tests: `tests/lab/way_out.rs` runs a stand-in proxy of "the host" with a made-up credential and two servers on the loopback of the test; through each of `proxy-only`, `proxy-per-name`, `proxy-tls-reterminate` and `ws-upgrade-refused` the command reaches them by the proxy alone. Here (root, kernel 6.18): `cargo test -p pod-lab --all-features` (50 unit tests, the binary 7, `cli` 4, `lab` as root and again as uid 65534: each case ok, the four way-out cases ok).

# T-747: `pod-lab`: the direct network classes on each address: an SNI filter by `nf_tables`, an IPv6 device with a default route

**Source:** T-716 (C729: the parts that the lab holds at its own addresses only).
**Category:** feature
**Milestone:** M8
**Priority:** P3
**Effort:** M
**Status:** done

## Problem

`sni-allowlist` filters the ClientHello at `192.0.2.0/24:443` alone: a
command that connects to another address gets `ENETUNREACH`, not a reset.
`ipv6-only` has its address on the loopback with no IPv6 default route, so
a program that reads the routes (podnet's `pod_dns::route::has_default_route`)
sees no IPv6 route out. podnet's data names the full forms: `nf_tables` in
the namespace that redirects each TCP connect to port 443 to the filter
(`PodUtils/podnet:docs/lab-profiles/sni-allowlist.md`), and a dummy or TUN
device with `fd00:6c61:62::2/64` and an IPv6 default route
(`PodUtils/podnet:docs/lab-profiles/ipv6-only.md`), at `dbd432c`.

## Premise

- Read: podnet's two files; `crates/pod-lab/src/net/setup.rs`.
- Not measured: `nf_tables` and a dummy device in a network namespace of
  a user namespace on a runner; this host has no IPv6 in its kernel.

## Approach

In the network namespace, before the lab leaves the user namespace that
owns it: a netlink socket makes a dummy device with the IPv6 address and a
default route through it (`RTM_NEWLINK`, `RTM_NEWADDR`, `RTM_NEWROUTE`),
and `nf_tables` rules redirect each TCP connect to port 443 to the
stand-in filter (`NFT_MSG_NEWTABLE`, a `nat` chain of the hook `output`).
Where the kernel refuses `nf_tables` in a user namespace, the part says so
and the lab keeps the filter at its own addresses. A test connects to an
address that is not of the lab and gets the reset of the filter. With
the default route in the lab, `ipv6-reaches` also reads the row
`read(/proc/net/ipv6_route)` with `Expect::Value { key: "default", value:
"1" }` beside its lab fact (C735): the lab cannot hold that row now.

## Prove

```sh
cargo test -p pod-lab --all-features --test lab
```

## Done

2026-11-16, `7a6ddbe` (with the plants `30902f9`, `683ca7c`). The Approach
changed for the filter (call C764): a local route of `0.0.0.0/0` on the
loopback of the lab (`RTM_NEWROUTE`, table `local`), not `nf_tables`;
`Net::each_v4`. A ClientHello to port 443 of each IPv4 address reaches the
stand-in filter; another port gets `ECONNREFUSED`. `ipv6-only` makes the
dummy device `lab0` (`RTM_NEWLINK`) with `fd00:6c61:62::2/64`
(`RTM_NEWADDR`, no duplicate address detection) and the IPv6 default route
through it (`RTM_NEWROUTE`); the part `ipv6-reaches` reads the row
`read(/proc/net/ipv6_route)` with `default` 1. The netlink requests are in
`crates/pod-lab/src/net/link.rs`, with three unit tests. The lab checks
`NetFact::HelloElsewhere` at `198.51.100.7:443` (the listed name passes,
no name is reset); the payloads of `tests/lab/net.rs` check the other
address and the IPv6 default route on a device that is not the loopback.
Here: `cargo test -p pod-lab --all-features` passes as root and as uid
65534 (`ipv6-only` refuses here: no IPv6 in this kernel). CI at `bd2eef6`
(job `test`, `scripts/ci/userns-tests.sh`): "ipv6-only: ok" and
"sni-allowlist: ok" on the runner. Plant `lab-sni-filter-lab-addresses-only`
(the route left out) is caught by the check of `sni-reset`.

# T-751: `pod-hostprobe`: the rows of the network classes that send a packet out, behind a setting of the caller that is off by default

**Source:** T-745 (C735: no row reaches the network by default).
**Category:** feature
**Milestone:** M8
**Priority:** P3
**Effort:** M
**Status:** done

## Problem

A check of a report gives `unseen` for each part of a network class whose
fact needs a packet out: `proxy-reaches`, `proxy-credential` and
`port-refused` of `proxy-only`, `tls-reterminated` and `ssh-port-silent`,
`answer-per-name`, `sni-reset`, `literal-reaches`, `upgrade-refused` and
`ipv6-reaches`. The rows of T-745 read what a host shows with no packet
out (the variable of a proxy, the certificate file, the resolver, the
default routes). The lab measures the other facts through its stand-ins
(`LabFact::Net`). A host of a user cannot show them in a report.

## Premise

- Read: `crates/pod-lab/src/net/measure.rs` (the facts through the
  stand-ins), `crates/pod-hostprobe/src/probes/network.rs` (the rows with
  no packet out), C735.
- Not measured: a row through a real proxy, a real resolver or a real SNI
  filter.

## Approach

A setting of the caller (`Settings::network`, off by default) with the
names, the ports and the addresses of the rows, as a real host needs real
ones. With it, rows of the census, each in its child with a time limit:
a `CONNECT` through `HTTPS_PROXY` to a name and a port, with its status;
the same with no credential; the issuer of the leaf of a TLS handshake
through the proxy (the name of the CA, not the chain); a lookup of a name;
a TCP connect to an IPv4 and an IPv6 literal, with the errno; a
ClientHello with a name and with none. With no setting, each row is a skip
that names the setting. `podlab check` takes the setting as an option, and
the parts of the network classes read the rows; the lab keeps its
stand-ins as `Observe::LabCheck`.

## Prove

```sh
cargo test -p pod-hostprobe --all-features
cargo test -p pod-lab --all-features
```

## Done

2026-11-16. Part 1: `pod-hostprobe`: `Settings::network` (`Network`: a name and its port, another name, a closed port, an IP literal, a time limit), off by default; nine census rows of `Kind::Network` (`probes::NETWORK_ROWS`, `crates/pod-hostprobe/src/probes/send.rs`): four `CONNECT` rows through the proxy of the host (with and with no credential, another name, a literal), the closed port with an SSH banner, three TLS rows (a ClientHello of TLS 1.2 with no TLS library, `probes/send/hello.rs`; the certificate of `SSL_CERT_FILE` that issued the chain), and a lookup. With no setting each row is a skip that names it, and no child runs; the spawner gives the targets in `POD_HOSTPROBE_NETWORK` and drops that variable of the caller. The schema names the rows. Tests: `probes::send::tests` (the setting through the variable, the URL of a proxy and a reason with no host or credential, a `CONNECT` on the loopback with and with no credential, the end of a `CONNECT` with no status, the chain across records, the issuer in a PEM file, base64) and `the_rows_that_send_a_packet_are_off_by_default` (plant `hostprobe-network-rows-on-by-default`). Here: `cargo test -p pod-hostprobe --all-features` (174 unit tests). Part 2: `pod-lab`: the parts of the network classes that need a packet out read these rows (`crates/pod-lab/src/table/network.rs`): `proxy-reaches` (`status 200`, `hello 1`), `proxy-credential` (407), `port-refused` (403), `tls-reterminated` (`issuer file` for the name, `hello 1` for the other), `ssh-port-silent` (200, `bytes 0`), `answer-per-name` (200, 504, `end reset` for the literal), `sni-reset` (`hello 1`, then `end reset` for the other name and for no name), `literal-reaches` and `ipv6-reaches` (`hello 1` to the literal); the facts of the stand-ins stay beside them as `LabCheck`. `upgrade-refused` has no row; `no-resolver` keeps its rows with no packet out (a part holds only when each row holds, and the lookup is no fact of the class). A lab measures the rows against its stand-ins (`net::lab_targets`: `listed.test:443`, `other.test`, port 22, the address outside of IPv4 or IPv6). `podlab check PROFILE --network NAME:PORT` (`--network-other`, `--network-literal`, `--network-closed`) runs them on this host; with no option they are skips that name the setting. Tests: `check::rows::the_rows_that_send_a_packet_out_decide_the_network_parts` (holds, fails, unseen with no row and with a skip, through a report), `tests/cli.rs` `the_targets_of_the_rows_that_send_a_packet_out`, and the lab: each network class holds with the rows measured inside (TLS 1.2 hellos to the stand-ins included). Here (root, kernel 6.18): `cargo test -p pod-hostprobe --all-features` (174 unit tests), `cargo test -p pod-lab --all-features` (51 unit tests, the binary 7, `cli` 5, `lab` as root and as uid 65534: each case ok). The pod-manual model of `podlab` gives the three `requires` of the targets; the manual is generated again.


# T-723: `pod-todo`: a kind `reference` for an issue that holds no work (PodUtils/podvm#435)

**Source:** PodUtils/podvm#435 (COORD-145 of podvm).
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

podvm took `v0.55.0` and turned the rules of the issues on: 47 of its issues fail as "(report) names no entry". Each is a reference: an issue with the label `reference` that records a captured tree of another repository. It holds no work, stays open while podvm uses the capture, and closes by hand. None of the kinds fits: `report` and `tracker` need an entry, and `coord` or `request` would be a false record.

## Premise

- Read: PodUtils/podvm#435 (its author is a collaborator of the team).
- The label is not in the record, so `pod-todo` cannot read it offline: the title or the column gives the kind.

## Approach

A kind `reference`: the column `Kind` names it, or a title that starts with `[Reference]`. It does not follow its entries, which are notes. The sync writes its block and never closes or opens it.

## Prove

```sh
cargo test -p pod-todo --all-features
python3 -I scripts/issue-sync.py --self-test
sh scripts/plant-each.sh issue-sync-closes-a-reference
```

## Done

2026-11-16. `Kind::Reference` in `crates/pod-todo/src/issues.rs` (`[Reference]` titles and the column), the block says "a reference that holds no work", and `scripts/issue-sync.py` writes only the block of a reference (`BY_HAND`). Tests: `a_reference_needs_no_entry_and_its_entries_are_notes`, and a check of the sync self-test; the plant `issue-sync-closes-a-reference` is caught.

# T-724: `issue-sync.yml` builds `pod-todo` under the caller's `.cargo/config.toml` (PodUtils/podvm#437)

**Source:** PodUtils/podvm#437 (COORD-146 of podvm); podvm's dry run 43770947835.
**Category:** defect
**Milestone:** M8
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The reusable workflow `issue-sync.yml` checks out podcrates at `.podcrates`, under the workspace of the caller, and builds `pod-todo` there. Cargo reads each `.cargo/config.toml` from the current directory up, so it took podvm's (`[build] target = "x86_64-unknown-linux-musl"`), and the runner has no musl target: `can't find crate for std`. A caller with a build target, a linker or rustflags in its configuration meets the same.

## Premise

- Measured here: a copy of podcrates under a directory whose `.cargo/config.toml` names a bogus target fails to build `pod-todo` in place, and builds from a directory outside it. Run from outside, rustup does not find `rust-toolchain.toml` either, so the build must name the toolchain.

## Approach

The step builds from `$RUNNER_TEMP` with `--manifest-path`, `--target-dir "$RUNNER_TEMP/pod-todo"`, `RUSTUP_TOOLCHAIN` from `rust-toolchain.toml` of podcrates, and no `CARGO_BUILD_TARGET`, `RUSTFLAGS` or `CARGO_ENCODED_RUSTFLAGS`; the sync reads the binary there.

## Prove

```sh
python3 scripts/check-workflows.py
```

## Done

2026-11-16. The script of the step, run here under a workspace with a bogus target in `.cargo/config.toml` and in `CARGO_BUILD_TARGET`, built `pod-todo` at `$RUNNER_TEMP/pod-todo/release/pod-todo`.

# T-725: `pod-manual`: four gaps that podnet's take found, and false positives of `check-man.sh` and `check-strings.py` (PodUtils/podnet#589)

**Source:** PodUtils/podnet#589 (COORD-094 of podnet: its take of `pod-manual`, podnet T-359).
**Category:** defect
**Milestone:** M8
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

podnet moved its help and its manual onto `pod-manual` and keeps a workaround in its glue for each gap:

1. `pod_manual::clap::read` leaves the variable of each flag empty (clap's `env = ...`).
2. `clap::read` makes rules that name a positional argument, and `check::model` refuses them (7 rules in podnet).
3. The help of the crate has no command groups and no "start here" block, so podnet's plain `--help` stays clap's.
4. The variable scanner takes any quoted word in the form of a variable (a C constant, a crate name, a hash name): podnet's test needs an exempt list of 7 words.
5. The plant of `scripts/check-man.sh` is vacuous where the planted flag (`--add-door`) is also named in the help of other flags.
6. `scripts/check-strings.py` on podnet's release build fails on 84 runs of the data of `pod-services`, 12 strings "PKCS #9" and "PKCS #12" of libraries, and one run of binary bytes.

The text split of podnet's frozen `src/warp/export/command.rs` needs no port: `pod-warp` did not import that file (`crates/pod-warp/SOURCES`).

## Premise

- Read: PodUtils/podnet#589. Not read: podnet's glue at `9217b0b`, `13b1514`, `36f9621`.

## Approach

Fix each in the crate, with a test that fails before the fix and a plant: the reader takes `env`; a rule that names a positional argument is either kept with the argument in the model or left out with a reason; command groups and a lead block in the model and the help writer; the scanner reads only the calls that read the environment (`env::var`, `var_os`, `getenv`, clap's `env`), not each quoted word; the plant of `check-man.sh` uses a flag name that no text names; `check-strings.py` reads prose only (a work id needs a word around it, `#` after `PKCS` is no issue number) and skips the data that `pod-services` embeds, by a rule that a test proves. podnet drops its workarounds after the tag.

## Prove

```sh
cargo test -p pod-manual -p pod-lab --all-features
python3 -I scripts/check-strings.py --self-test
sh scripts/plant-each.sh --group manual
```

## Done

2026-11-15, commits ab5653eb, 22fcebcc, 50a53e9d and the next one
(local branch `manual2`); calls C713 to C716.

- `clap::read` fills `Flag::env` and the new `Arg::env` from clap's
  `env` (C713). A rule names a positional argument by its value name,
  and `check::model` takes a flag or an argument; a rule with a hidden
  argument is left out. Tests: `tests/clap.rs`
  `the_reader_gives_the_variable_of_each_flag`,
  `a_rule_with_a_positional_argument_passes_the_check_of_the_model`;
  `check::tests::a_rule_may_name_an_argument`.
- `Command::groups` (`Group`) and `Command::lead` (C715): the help, the
  text and the man page show "Start here" and a table for each group
  before the arguments and the options; `check::model` requires each
  command under it in one group. `podlab --help` takes three groups and
  three lead lines; `man/podlab.1` is written again. Tests:
  `check::tests::the_groups_name_each_command_once`,
  `the_help_shows_the_lead_and_each_group`, and the help test of
  `podlab`.
- `scan::variables_read` reads only the calls of `scan::CALLS`, clap's
  `env = "X"` and the constants of `scan::LISTS` (C714); `check::variables`
  counts the variable of each flag and argument of the model as a read of
  the parser. podnet's seven exempt words of a form of a variable are no
  read (`scan::tests::a_word_of_the_form_of_a_variable_is_no_read`).
- `scripts/check-man.sh`: the plant is a flag of a generated name; a
  spelling counts as a word of its own. On podnet's release binary at
  `aa8bae2`: the old script failed its plant (`--add-door`); the new one
  passes, 248 flag spellings. A copy with a vacuous check fails its plant.
- `scripts/check-strings.py` (C716): podnet's release binary at `aa8bae2`
  (`cargo build --release`, 6 min): 97 faults before, 0 after; 64 with no
  data of `pod-services`. `--self-test`: 14 cases (CI, lint job).
- `podlab` for `x86_64-unknown-linux-musl` (cargo zigbuild,
  `SOURCE_DATE_EPOCH`): `check-static.sh`, `check-strings.py` (and
  `--plant`: exit 1) and `check-man.sh` pass; `podlab man --no-pager`: 353
  lines.
- Tests: `pod-manual` 59 unit tests and 9 of `tests/clap.rs`; `pod-lab`
  25, 6 of the binary (1 ignored), 4 of `tests/cli.rs`. Plants: 11 new
  (group `manual`; `plants.yml` installs groff and mandoc); `plant-each.sh
  --group manual`: 19 caught; the three plants of the manual in group
  `lab`: 3 caught.
- Breaking changes of the API: `check::model` takes a rule that names an
  argument; `scan::variables_read` finds no quoted word outside a call;
  `check::variables` reads the variables of the model; new fields
  `Command::groups`, `Command::lead`, `Arg::env` (a struct literal with no
  `..Default::default()` fails to build); the feature `clap` turns on
  `clap/env`. The JSON schema stays 1.
- What podnet can drop after the tag: `fill_env` and the filter of the
  rules in `src/manual/mod.rs`; the words `pod_proxy`, `F_GETFL`,
  `F_SETFL`, `SO_ORIGINAL_DST`, `AF_INET`, `AF_INET6` and `SHA2_256` of
  `EXEMPT` (`TARGET` and `RUST_LOG` stay); and, when it answers `--help`
  from the model,
  `HELP_GROUPS`, `START_HERE`, the help template and the test E1 of
  `src/cli/help.rs`. `check-strings.py` needs no list of podnet: give
  `--data` the `services/` of the tag of the binary.
- Finding: T-726 (clap gives no getter of `requires`: the reader gives
  no `Rule::Needs`).

# T-726: `pod-manual`: the reader of clap gives no `Rule::Needs` (clap's `requires`)

**Source:** T-725 (the read of podnet's glue for PodUtils/podnet#589).
**Category:** defect
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`pod_manual::clap::read` gives the rules `OneOf` (a group) and `Excludes`
(a conflict) of a clap tree, but no `Needs`: clap 4.6 has no public getter
of `Arg::requires`. podnet has about 20 flags with `requires` (for
example `--auth-user` needs `--auth-password-file`); its manual and help
do not name these rules.

## Premise

- Read: `clap_builder` 4.6.7 `src/builder/arg.rs` (no `get_requires`),
  podnet `src/cli/opts.rs`, `src/cli/cmd/warp/args.rs` at `aa8bae2`.
- Not measured: the count of the rules of `requires` in podnet.

## Approach

Find each rule by the parser itself: for each flag of a command, parse
the path of the command with the flag alone (and a value, and the
required arguments), and read the arguments that a `MissingRequiredArgument`
error names that the flag added; each gives `Rule::Needs`. A test plants a
`requires` in the sample tree and finds its rule; `check::model` and the
writers already take `Needs`.

## Prove

```sh
cargo test -p pod-manual --all-features --test clap
```

## Done

2026-11-16, commits bf4adf45, 490c93f8, f810430f (local branch
`fix750`); call C737.

- clap 4.6.7 (`Cargo.lock`, the newest in the registry here) has no
  public getter of the requirements of an argument: `Arg` has no
  `get_requires`, `Command` has `get_arg_conflicts_with` alone, and
  `ArgGroup` has no getter of `requires`. A newer clap was not read (no
  network).
- `clap::read` finds each rule by the parser (`src/clap/needs.rs`): a copy
  of the tree with no `env`, no required argument or group, no
  `arg_required_else_help` and no `subcommand_required`; each shown
  argument is given alone after the path of its command (a positional
  after a value of each positional before it, `last` after `--`), with
  each shown possible value, else the first two of `needs::SAMPLES` that
  its parser takes; the arguments that clap's `MissingRequiredArgument`
  names and the base parse does not are its needs. A need of each value
  is `Rule::Needs`; a need of one possible value is `Rule::Needs` with
  `value` (`requires_if`). A need that another need gives is left out; a
  need of a group (no single argument) or of a hidden argument is left
  out. The feature `clap` turns on `clap/error-context` and `clap/usage`.
- `Rule::Needs` has the new field `value` (`Option<String>`; the JSON
  leaves it out when it is empty, schema 1); the sentence is
  `--mode tls needs --cert.`.
- Tests: `tests/clap.rs` `the_reader_gives_each_rule_of_requires` (a
  chain of two needs, `requires_if` on a possible value, a parser of
  addresses, a required positional and one after it, a group and a hidden
  flag left out, a needed flag whose `env` is `PATH`, a command with
  `arg_required_else_help`; the text and the JSON),
  `a_tree_with_no_requires_gives_no_needs`; podlab
  `the_rules_of_the_manual_are_the_rules_of_the_parser` (podlab's parser
  gives its one conflict and no need; a `requires` on `--allow-connect`
  and on the positional `SECTION` gives its rule). `man/podlab.1` does not
  change. Plants (group `manual`): `manual-clap-needs-dropped`,
  `-env-kept`, `-required-kept`, `-help-kept`, `-followed-through`,
  `-value-ignored`; `plant-each.sh --group manual`: 25 caught.
- For the release notes: `pod_manual::clap::read` gives clap's `requires`
  and `requires_if` (on a possible value) as `Rule::Needs`; the help, the
  text and the man page name them ("--user needs --password-file.").
  Breaking: `Rule::Needs` has the new field `value` (a pattern or a
  struct literal names it, or `..`). podnet can read its about 20 rules of
  `requires` in its manual with no glue.
- Finding: T-756 (a `requires` of a group has no form in the model).

# T-737: `podlab run --allow-connect PORT`: the way out to a proxy in `bind-denied` (PodUtils/podssh#278)

**Source:** PodUtils/podssh#278 (COORD-086 of podssh: its lane of PodUtils/podssh@T-791 on `pod-lab` of `v0.56.0`).
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`bind-denied` holds `tcp-connect-refused` with a Landlock ruleset that has no port rule, so each TCP connect fails, the connect to the proxy too. podssh's lane runs commands 2, 5 and 6 there, and each needs a way out. podssh's target host refuses each bind and lets a connect reach its proxy alone: `podssh doctor` gave 19 FAIL ("could not connect to the proxy: Permission denied"), and `podssh ssh` did not reach GitHub.

## Premise

- Read: PodUtils/podssh#278; podssh's runs in nine other profiles passed.

## Approach

`Options::allow_connect` and `podlab run --allow-connect PORT` (once for each port): the ruleset of `bind-denied` gets a rule that lets a TCP connect reach each port. Each bind and each connect to another port still give `EACCES`. The lane of podssh names it.

## Prove

```sh
cargo test -p pod-lab --all-features --test lab
sh scripts/plant-each.sh lab-allow-connect-dropped
```

## Done

2026-11-16. `hold::landlock_tcp(connect)` adds a `LANDLOCK_RULE_NET_PORT` rule with `LANDLOCK_ACCESS_NET_CONNECT_TCP` for each port; the lab process takes the ports as an argument. `tests/lab.rs` (`bind_denied`): a listener outside the lab, a run with its port: the connect to it works, a connect to port 1 and a bind give `EACCES`, as root and as uid 65534. The plant `lab-allow-connect-dropped` is caught. `crates/pod-lab/LANES.md` names the option for podssh.

# T-756: `pod-manual`: the model has no rule "needs one of" for clap's `requires` of a group

**Source:** T-726 (a finding of its work).
**Category:** feature
**Milestone:** M8
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

clap's `requires` can name a group (`Arg::requires("out")` with the group
`out` of `--json` and `--text`): the flag then needs one argument of the
group. `pod_manual::clap::read` finds the rule (T-726), but the model has
no form for it (`Rule::Needs` needs each of its names), so the reader
leaves it out and the help, the text and the man page do not name it.

## Premise

- Read: `crates/pod-manual/src/clap/needs.rs` (a missing argument that
  matches no single argument is left out), `crates/pod-manual/src/model.rs`
  (`Rule`).
- Not measured: the count of such rules in podnet, podssh and podvm.

## Approach

A rule of the model for a need of one of several names (a field of
`Rule::Needs`, or a new form), its sentence ("--audit needs one of --json,
--text."), its check in `check::model`, and its JSON field with a default
(schema 1). The probe maps clap's text of a group (`<--json|--text>`) to
the arguments of the group. A test with a `requires` of a group, and a
plant.

## Prove

```sh
cargo test -p pod-manual --all-features --test clap
sh scripts/plant-each.sh manual-clap-needs-group-dropped
```

## Done

2026-11-16, `d038158`, `d441f7b`, `b1da77a`.

- `Rule::Needs` has the new field `one` (`bool`; the JSON leaves it out
  when it is false, schema 1; a JSON with no `one` reads as a need of each
  name). The sentence is `--audit needs one of --json, --text.`, with a
  value `--mode signed needs one of --key, --agent.`. The help, the text,
  the man page, the JSON and `llms-full.txt` show it through
  `Rule::sentence` and the model. `check::model` refuses a need of one of
  less than two names. Call C762.
- The probe (`crates/pod-manual/src/clap/needs.rs`) matches clap's text of
  each group of the command (`<--json|--text>`: a flag as its usage, a
  positional as its value names) to the arguments of the group. A group
  of one argument is a need of that argument; a group with a hidden
  argument is left out; a need that another need gives is left out.
- Tests: `tests/clap.rs` `the_reader_gives_a_need_of_one_of_a_group` (a
  group of two flags, of an option and a positional, of three flags, of
  one flag, with a hidden flag, a `requires_if` of a group, a need
  through another need; each writer, the reader of the JSON,
  `check::model` and `check::flags`),
  `a_need_with_no_field_one_is_a_need_of_each`, `--audit` in
  `the_reader_gives_each_rule_of_requires`; `check::tests`
  `a_need_of_one_of_names_two_or_more`. `cargo test -p pod-manual
  --all-features`: 60 and 13 passed. podlab (`--bin podlab --test cli`):
  7 and 6 passed; its parser has no group need, `man/podlab.1` does not
  change.
- Plant `manual-clap-needs-group-dropped` (the key of a group with no
  angle brackets) is caught; `manual-clap-needs-followed-through` has a
  new patch; `plant-each.sh --group manual`: 25 caught, and
  `manual-man-plant-vacuous` needs groff, which the host of the run did
  not have.
- For the release notes: `pod_manual::clap::read` gives a `requires` of a
  group as a need of one of its arguments ("--audit needs one of --json,
  --text."). Breaking: `Rule::Needs` has the new field `one` (a struct
  literal or a pattern names it, or `..`).

# T-759: `pod-lab`: the stand-in proxy of a network lab reaches a server of the command on the loopback of the lab, with the rules of its class (PodUtils/podssh#282)

**Source:** PodUtils/podssh#282 (COORD-090 of podssh; PodUtils/podssh@T-791).
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

podssh ran `podlab run PROFILE -- podssh doctor --quick` in each network
lab of `v0.60.0`: "the stand-in proxy sends each CONNECT to the lab's own
stand-in TLS server, whatever the host and port. A CONNECT to
`127.0.0.1:8443`, where the command itself listened, got 200, and the
listener got no byte. The stand-in is not a relay, so podssh never reaches
the steps that tell these classes apart: the WebSocket upgrade
(`ws-upgrade-refused`); a re-terminated certificate on the relay's name
(`proxy-tls-reterminate`); the filter of SNI (`sni-allowlist`)." Commands
2 to 6 of podssh's lane (`release-short.sh --net`, ssh, cp, serve, pipe, a
session after a kill of its link) cannot run in the network labs. The ask:
"make the lab's proxy reach a server that the command runs inside the lab
[...] The class then acts on the bytes, with each lab's rule applied: the
upgrade refused; TLS re-terminated with the proxy's CA; SNI filtered; no
DNS." podssh then runs its relay role (`podssh relay serve --listen
127.0.0.1:PORT --cert ... --allow-private`) and a throwaway sshd on the
loopback, and its whole lane in each lab with no network.

## Premise

- Read: PodUtils/podssh#282; `crates/pod-lab/src/net/proxy.rs` (a
  `CONNECT` to a name of the lab, the loopback included, gets the stand-in
  server); `crates/pod-lab/src/net/relay.rs` (the splice and the
  re-termination of T-746, with the roots of the host).
- Not measured: a relay of podssh behind the stand-in proxy.

## Approach

An option of `podlab run` (`--serve PORT`, more than one) and of
`Options`: a `CONNECT` to `127.0.0.1:PORT`, `localhost:PORT` or a name of
the lab at such a port goes to the listener of the command on the loopback
of the lab, after the rules of the class (the credential, the ports, 403,
504, a reset, the silent port). On the bytes: in `ws-upgrade-refused` the
proxy re-terminates and refuses a request with `Upgrade: websocket` (403);
in `proxy-tls-reterminate` the proxy re-terminates a listed name with its
CA and checks the server of the command with a CA that the caller gives
(`--serve-ca FILE`), else with no check of the chain; in `sni-allowlist`
the stand-in server on `192.0.2.1:443` passes a ClientHello with a listed
name to the port of the command (one `--serve` port) and resets the others.
A test runs a TLS echo server of the test in each class and reaches it
only by the path of the class.

## Prove

```sh
cargo test -p pod-lab --all-features --test lab
```

## Done

2026-11-16. `podlab run --serve PORT` (`Options::serve`, once for each
port) and `--serve-ca FILE` (`Options::serve_ca`). The lab process gets
them as two arguments after `way-out` or `closed` (a strict parse of each
argument), loads the roots before the mounts, and passes one
`net::Local { ports, roots }` through `net::start` and the stand-in
process to `proxy::proxy` and `proxy::server` (`proxy::Ends`, beside the
way out). After each rule of the class (the credential, the ports, 504, the
reset of a literal, the silent port), the stand-in proxy sends a `CONNECT`
to a name of the lab at a served port to `127.0.0.1:PORT`:
`relay::connect` with `Upstream::Local` or `Upstream::Out` (the way out),
which splices, or, for a name that the class terminates, shows a leaf of
the CA of the proxy, checks the server of the command with the roots of
`--serve-ca` (502 with the reason where there are none, C749) and refuses
an upgrade where the class does. In `sni-allowlist` the stand-in server
splices a ClientHello with a listed name to the first served port (the
peeked record stays in the stream), and answers itself where nothing
listens (C748). `podlab` refuses the ports 0, 443 and 3128, `--serve`
with no network lab, `--serve-ca` with no `--serve` and a file of roots
that does not read (exit 2); `run` makes the same checks. A fix in place:
a splice clears the read time limit of the handshake, which ended a
stream of the way out after 30 s with no byte. The calls C747 to C749. The
help, the manual (an example), `README.md` ("The servers of the command")
and `LANES.md` name the options.

Tests: `net::proxy::tests` (a served port reached by each name of the lab,
407, the silent port, the stand-in for another name and port; 502 with no
roots, the leaf of the proxy, the request to the command and 403 for an
upgrade with roots; a listed ClientHello to the served port, a reset for
the others, the stand-in where nothing listens), `net::local::tests` (the
ports and the roots), `net::relay::tests` (an idle splice), the binary and
`tests/cli.rs` (the rules of the parser, the usage errors), and
`tests/lab/serve.rs`: `proxy-only` with `--serve 8443`, `proxy-tls-reterminate`
with no roots, `sni-allowlist` and `ws-upgrade-refused` with a CA of the
test, each with a payload server on the loopback of the lab. Here (root):
`cargo test -p pod-lab --all-features` (60 unit tests, the binary 7, `cli`
6, `lab` each case ok as root and as uid 65534 from a copy of the binary
in a directory of mode 755). Plants: `lab-serve-port-ignored`,
`lab-serve-tls-unchecked`, `lab-serve-sni-any-name`,
`lab-serve-port-of-a-stand-in`, `lab-splice-idle-cut` (unit tests) and
`lab-serve-ports-dropped`, `lab-serve-ca-dropped` (the lab, through
`scripts/ci/userns-tests.sh`).

# T-760: `pod-lab`: a network lab keeps the other proxy variables of the host (PodUtils/podssh#282)

**Source:** PodUtils/podssh#282 (a note of COORD-090 of podssh).
**Category:** defect
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podssh: "Inside a lab, the host's other proxy variables stay set:
`GLOBAL_AGENT_HTTPS_PROXY`, `DOCKER_HTTPS_PROXY`, `npm_config_https_proxy`,
`YARN_HTTPS_PROXY`, `CLOUDSDK_PROXY_*`, `JAVA_TOOL_OPTIONS`'s proxy.
podssh reads none of them, but a tool that does reaches past the lab's
proxy (or fails on the host's port). The manual says that the lab removes
`ALL_PROXY` and `NO_PROXY`; it says nothing of these."

## Premise

- Read: `crates/pod-lab/src/net.rs` (`PROXY_VARIABLES`: the eight
  variables of curl), `crates/pod-lab/src/lab.rs` (`environment`).
- Not measured: the tools that read each variable.

## Approach

The lab removes each variable of a proxy of a known tool, by a list with
the tool of each (the `GLOBAL_AGENT_*`, `DOCKER_*`, `npm_config_*`,
`YARN_*`, `CLOUDSDK_PROXY_*` forms), and sets the HTTPS and HTTP forms to
its stand-in proxy where the class has one, as for curl's. A
`JAVA_TOOL_OPTIONS` or `JDK_JAVA_OPTIONS` with `-Dhttps.proxyHost` or
`-Dhttp.proxyHost` loses those options alone. The manual and the README
list the variables. A test sets each of them for the lab process alone
(a new start of the test binary) and reads the environment of the command.

## Prove

```sh
cargo test -p pod-lab --all-features --test lab
```

## Done

2026-11-16. `pod_lab::net::vars`: `changes` gives the changes of the
environment of the lab process: each variable of `URL_VARIABLES`
(global-agent, npm, Yarn, a build of Docker), `SKIP_VARIABLES` and
`CLOUDSDK_VARIABLES` removed; where the class has a proxy, the URL forms set
to the stand-in proxy and the Google Cloud CLI set to its type, address,
port and credential; `JAVA_TOOL_OPTIONS`, `JDK_JAVA_OPTIONS` and
`_JAVA_OPTIONS` lose each option of a proxy and keep the others. The lab
applies them in `environment`, beside the variables of curl. The manual of
`podlab` names each variable (the drift check of `pod-manual` refused the
code until it did). Tests: `net::vars::tests` (a class with no proxy, a
class with a proxy and a credential, the options of Java) and the lab case
`tool-vars` in `proxy-only` (the command sees each URL form and the address
of the Google Cloud CLI as the stand-in proxy). Plant
`lab-tool-proxy-variables-kept`. Here (root): `cargo test -p pod-lab
--all-features` (54 unit tests, the binary 7, `cli` 5, `lab` as root and as
uid 65534: each case ok).

# T-773: the installer core: a tree copy with no pins stops on a host with cosign (PodUtils/podnet#597)

**Source:** PodUtils/podnet#597 (COORD-096 of podnet, its take of `v0.61.0`).
**Category:** defect
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podnet: "a project installer whose tree copy has no pins and runs from a
file (`sh scripts/install.sh`) stops on a host with cosign, because
`inst_signature` compares `$0` with `NAME-install.sh` of the signed
`SHA256SUMS`. podnet's tree copy does its own signature check first, then
replaces `inst_signature` with a no-op. If other projects run a tree copy
from a file, the core could skip the self-compare when the block of pins
is empty." The loop of `inst_signature` over the plan has the same form: a
copy with no pins has no pin to compare with the signed line.

## Premise

- Read: PodUtils/podnet#597; `scripts/install/core-net.sh`
  (`inst_signature`), `scripts/install/core.sh` (`inst_pin`).
- Not measured: a tree copy of the core on a host with cosign.

## Approach

Where the installer holds no pins (a tree copy that `pins.py stamp` did not
write), `inst_signature` takes the line of each file of the plan from the
signed `SHA256SUMS` as its pin, and does not compare the installer itself
(a tree copy is no asset of the release). A stamped installer keeps both
checks. With no cosign, a copy with no pins refuses as now. A case of
`scripts/install/test.sh` runs a tree copy from a file with a fake cosign:
it installs with the pins of the signed file, and a changed asset is
refused.

## Prove

```sh
sh scripts/install/test.sh --case sig
```

## Done

2026-11-16. podnet's tree copy (read through `forge.web.pkgforge.dev`,
PodUtils/podnet:scripts/install.sh at its `main`) keeps its block of pins
empty, checks the signature itself, and sets `INST_VERSION` and the pin of
its asset at run time (`podnet_pins`) before `inst_main`; only the compare
of the installer itself then stops it. The core keeps the pins of the plan
as they are and records at load, before any code of the project runs,
whether the file holds its own pins (`inst_stamped` in
`scripts/install/core.sh`, from `INST_VERSION` of the block of pins, which
comes before the core). `inst_signature` (`scripts/install/core-net.sh`)
does not compare a copy with no pins of its own with `NAME-install.sh` of
`SHA256SUMS`, and says so in one line; a stamped installer keeps the
compare. A copy with no pins and no project code that sets them still
stops at the check of `INST_VERSION`. Test: the case `sig` of
`scripts/install/test.sh` runs such a copy (the block of pins empty, the
stamped values set before `inst_main`) with the stand-in of cosign: the
signature is ok, the line says that the installer is not compared, and the
install passes; with the core of `HEAD~1` it fails with "is not
podlab-install.sh of the signed SHA256SUMS" (as podnet saw). Plant
`install-tree-copy-compared`. Here: `sh scripts/install/test.sh --case sig`
(5 passed), `shellcheck scripts/install/*.sh` (0 findings).

# T-792: `pod-lab`: with a way out, the certificate file of the lab holds the roots that the host trusts for its way out (PodUtils/podssh#285)

**Source:** PodUtils/podssh#285 (COORD-093 of podssh, its take of `v0.62.0`).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podssh: "The way out passes this cloud host's own proxy, which re-signs
TLS. The lab's `SSL_CERT_FILE` holds only the lab's CA, so podssh needed
the host's CA added inside the lab. A lab that adds the CA of the host's
way out where it re-signs would remove that step for each project." With
`podlab run --way-out`, a `CONNECT` that the class splices reaches a real
server (or the host's proxy that re-signs); the command checks it with
`SSL_CERT_FILE` of the lab, which holds the CAs of the lab alone
(`crates/pod-lab/src/net/standin.rs`, `write_cert_file`).

## Premise

- Read: PodUtils/podssh#285; `write_cert_file`; `host_roots` in
  `crates/pod-lab/src/net/out.rs` (the roots of the host for the classes
  that re-terminate: the file of `SSL_CERT_FILE`, else the system bundle).
- Not measured: a host proxy that re-signs, here.

## Approach

With a way out, the lab process reads the certificates that the host
trusts for its way out before it enters its namespaces (the file of
`SSL_CERT_FILE` of the host where it is set, else the system bundle, as
`host_roots`), and `write_cert_file` adds them after the CAs of the lab.
With no way out the file holds the CAs of the lab alone, as now. The
manual and the README of `podlab run --way-out` say so. A test of the way
out: a server of the test with a leaf of a CA of the test, the host's
`SSL_CERT_FILE` names that CA, and the command in `proxy-only` checks the
server through the way out with the lab's `SSL_CERT_FILE` alone.

## Prove

```sh
cargo test -p pod-lab --all-features --test lab
```

## Done

2026-11-16. `WayOut::pem` (`crates/pod-lab/src/net/out.rs`): the lab
process reads, before its namespaces, the certificate blocks of the file
of the host's `SSL_CERT_FILE`, else of its first system bundle (at most
8 MiB; `certificate_blocks` keeps each whole `CERTIFICATE` block and
drops other text). `write_cert_file` (`net/standin.rs`) adds them after
the CAs of the lab where the run has a way out; with no way out the file
holds the CAs of the lab alone. Tests: `net::out::tests::the_certificate_blocks_of_a_pem_text`,
and the way out of `proxy-only` (`tests/lab/way_out*.rs`): the payload
reaches the TLS server of the test past the lab, whose CA is the host's
`SSL_CERT_FILE`, and checks it with the lab's `SSL_CERT_FILE` alone. Plant
`lab-way-out-host-roots-left-out`. Here (root): `cargo test -p pod-lab
--all-features` (61 unit tests, the binary 7, `cli` 6, `lab` as root and as
uid 65534: each case ok).

# T-796: `pod-lab`: the stand-in and the way out ignore SIGPIPE, so that a client that resets does not end them (PodUtils/podvm#455)

**Source:** PodUtils/podvm#455 (COORD-162 of podvm, item 2).
**Category:** defect
**Milestone:** M8
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

podvm: "podvm sets SIGPIPE to `SIG_DFL` at the start of each process
(PodUtils/podvm@T-1769), and the lab process and its stand-in inherit it. A probe
that closes its socket before the stand-in writes its answer kills the
whole stand-in (`strace`: `killed by SIGPIPE`, `si_code=SI_USER`). Each
later `CONNECT` then gets "Connection refused", and `proxy-only` did not
hold on this host." The stand-in and the way out are forks of the lab
process (`crates/pod-lab/src/net/standin.rs`, `out.rs`), and they keep its
dispositions. rustls writes with `writev`, which raises SIGPIPE on a
socket whose peer reset it; the plain writes of `TcpStream` use
`MSG_NOSIGNAL`. No code of `pod-lab` set SIGPIPE.

## Premise

- Read: PodUtils/podvm#455; `shed` in `net/standin.rs` (the start of the
  stand-in and of the way out); `run` in `src/lab.rs` (the exec of the lab
  process: the consumer's binary through `pod_lab::dispatch`).
- Measured: `tests/lab.rs` with SIGPIPE at `SIG_DFL` in the lab process:
  the check of `proxy-only` before the command fails, "TLS to listed.test
  through the proxy shows the CA of the server: a read of the head:
  Connection reset by peer", then "the proxy: Connection refused".

## Approach

`shed` sets SIGPIPE to `SIG_IGN` (`rt_sigaction` with the
`kernel_sigaction` of `linux-raw-sys`, the layout of each target). Both
the stand-in and the way out call `shed` before their threads start. No
process that calls it runs an exec, so the command of the lab keeps its
own disposition. A test: the lab test sets SIGPIPE to `SIG_DFL` in the lab
process (the variable `POD_LAB_TEST_SIGPIPE_DEFAULT`), and a payload
resets its connection after the handshake, then gets its answer from the
stand-in again.

## Prove

```sh
cargo test -p pod-lab --all-features --test lab
sh scripts/plant-each.sh lab-sigpipe-default
```

## Done

2026-11-16, the commit of the fix. `ignore_sigpipe` in
`crates/pod-lab/src/net/standin.rs`, called by `shed`. Test: the case
`proxy-only` with the payload `sigpipe` (`tests/lab/net.rs`,
`tests/lab/net_payload.rs`): the lab process sets SIGPIPE to `SIG_DFL`
before `pod_lab::dispatch` (`tests/lab.rs`), the payload resets a TLS
connection through the proxy (`SO_LINGER` of 0 s), then a `CONNECT` and a
`GET` get 200. Before the fix the case fails as podvm saw it (the
check of the lab before the command: a reset, then "Connection refused");
after it each case passes as root and as uid 65534. Plant
`lab-sigpipe-default`. Two findings of the triage, fixed in place: the doc
comment of `host_roots` stood on `PEM_MOST` (`net/out.rs`), and `host_pem`
took the first system bundle that reads, also with no certificate; it now
takes the first with a certificate, as `pod_tls::roots::load` does. The
doc of `write_cert_file` names the roots of the host. Item 1 of the issue
(the roots of the host in the certificate file) is T-792, on `main` since
`c58f870` and in `v0.64.0`. podvm can drop its handler of SIGPIPE before
`pod_lab::dispatch` when it takes the tag after `v0.64.0`. Release notes:
`pod-lab`: the stand-in and the way out ignore SIGPIPE: a consumer that
starts the lab with SIGPIPE at its default needs no handler.

# T-797: `pod-lab`: the certificate file of a way out on a host with no bundle, and an `SSL_CERT_FILE` that cannot be read (PodUtils/podvm#455)

**Source:** PodUtils/podvm#455 (COORD-162 of podvm, item 1; the triage of T-796).
**Category:** defect
**Milestone:** M8
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

T-792 adds the roots of the host to the certificate file of a lab with a
way out (`host_pem` in `crates/pod-lab/src/net/out.rs`). Two gaps stay.
(1) A host with no `SSL_CERT_FILE` and no system bundle (a scratch or a
distroless image) gives an empty text, so a class that splices the real
chain (`proxy-only`) still fails with `UnknownIssuer`, while a class that
re-terminates works: `host_roots` takes `pod_tls::roots::load`, which adds
the built-in roots of `webpki-roots`. (2) A file of `SSL_CERT_FILE` that
cannot be read gives an empty text with no message; `host_roots` gives an
error for the same file.

## Premise

- Read: `host_pem`, `host_roots` and `start` in `net/out.rs`;
  `crates/pod-tls/src/roots.rs` (`load` and its built-in roots).
- Read: `webpki-roots` holds trust anchors, not certificates: it cannot
  become PEM. The crate `webpki-root-certs` holds the same roots as DER.
- Not measured: a host with no bundle.

## Approach

(1) Where `host_pem` finds no certificate, write the roots of
`webpki-root-certs` as PEM (base64 of each DER), or refuse `--way-out`
with the reason; take the first when its licence and its size in the
binary pass `deny.toml` and the size of `podlab`. (2) `host_pem` gives an
error for a file of `SSL_CERT_FILE` that does not read, and `start` gives
it as the error of the way out. A test of each: the lab process with an
`SSL_CERT_FILE` that names no file, and a `host_pem` with no bundle (a
list of paths as an argument).

## Prove

```sh
cargo test -p pod-lab --all-features
```

## Done

2026-11-16. `host_pem` (`net/out.rs`) is `host_pem_from(file, bundles)`:
the file of `SSL_CERT_FILE`, else the first system bundle that holds a
certificate, else the built-in roots of `webpki-root-certs` as PEM (the
DER of each root, the crate that `deny.toml` takes under
CDLA-Permissive-2.0; some 150 roots in `podlab`). An `SSL_CERT_FILE` that
does not read, or that holds no certificate, is an error that names it,
and `start` gives it as the error of the way out. Tests: the unit test of
the four sources (`net/out/tests.rs`), and the lab case of a run with a
way out whose `SSL_CERT_FILE` names no file (`tests/lab/way_out.rs`:
refused, with the name), as root and as uid 65534. Plants
`lab-host-pem-unread-file-empty`, `lab-host-pem-no-built-in-roots`.

# T-807: `pod-manual`: four forms that podssh's manual needs before its text, help and JSON move to the crate (PodUtils/podssh#288)

**Source:** PodUtils/podssh#288 (COORD-096 of podssh, PodUtils/podssh@T-790).
**Category:** feature
**Milestone:** M5
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podssh builds `pod_manual::Manual` from its own tables, and its tests run
each check of the crate with no exception (podssh `ee6b6bd`); `podssh man
--roff` is the writer of the crate. Its text form, `--help` and `--json`
stay podssh's, as the model has no place for four forms of its output:
(1) an empty reason of a kind of flag: `FlagKind::Ignored { why: "" }`
writes "Accepted and ignored:" and `Refused { instead: "" }` writes
"Refused. Use" with no flag (`crates/pod-manual/src/sections.rs`); (2) a
kind "not in this release": a flag that the parser takes and refuses with
its own exit code, not a flag of another tool; (3) the availability of a
command in a build: a verb that a build with no feature (Tailscale, iroh,
Tor) keeps in its help with one sentence why, and exits 70, named in the
JSON for each command; (4) a topic inside the section of a command: the
`-o` keywords of `podssh ssh`, each with its status and default.

## Premise

- Read: PodUtils/podssh#288; `crates/pod-manual/src/model.rs` (`FlagKind`,
  `Command`, the topics), `sections.rs`, `text/`, `json`.
- Not measured: podssh's text and JSON against the crate's writers with
  the four forms.

## Approach

(1) An empty `why` or `instead` writes "Accepted and ignored." or
"Refused." alone. (2) `FlagKind::NotInRelease { code, why }`: the parser
takes the flag and the program exits with `code`; the help, the man page,
the text and the JSON say so. (3) `Command::availability`: `Always`, or
`Feature { name, why, code }` (the help keeps the verb with its sentence;
the JSON names it for each command; a check holds the exit code to the
table of exit codes). (4) `Topic::within: Option<command>`: a topic that
the writers show inside the section of its command (the text, the man
page, the JSON), with each item's status and default. Tests of each
writer for each form, and the checks (`check::model`, `check::texts`)
extended. Name the tag on the hand-off.

## Prove

```sh
cargo test -p pod-manual --all-features
```

## Done

2026-11-16. The four forms of PodUtils/podssh#288 in the model and each
writer: (1) an empty `why` or `instead` writes "Accepted and ignored." or
"Refused." alone; (2) `FlagKind::NotInRelease { code, why }`: "Not in this
release: WHY. It exits CODE."; (3) `Command::availability`
(`Availability::Always`, or `Feature { name, why, code }`): a sentence in
the blocks of the command (the help, the text, the man page) and a field
of the command in the JSON, left out for `Always`; (4) `Command::topics`:
the keys of the topics that the section of the command shows after its
rules, with no section of their own; `man KEY` selects the command, and
the JSON section of the command carries them. `check::model` holds each
exit code of the two forms to EXIT STATUS, each key of `Command::topics`
to a topic, and a topic to one command; the text rules read the new
reasons. No struct literal of a consumer breaks: the new fields have a
default, and each consumer builds `Command` with `..Command::default()`.
Tests: `src/sections/tests.rs` (each form in the blocks, the text, the man
page and the JSON, which reads back; each finding of the check). Plants
`manual-empty-reason-dangles`, `manual-topic-inside-twice`,
`manual-feature-unsaid`, `manual-exit-code-unchecked`.

# T-830: Updates: a gate of health with a rollback, a check of loadability, a store of logs, scheduled updates, a floor of rollback in the feed (report 15 of the dig study, with T-104)

**Source:** the dig study of 2026-11-16 (an archive of the owner), report 15.
**Category:** feature
**Milestone:** M8
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

T-104 plans the release index and its verifier; the dig study adds a floor of rollback in a signed feed, an unprivileged fetcher with a privileged installer, a gate of health with a rollback, a check that a binary loads with no run, and a store of logs of the machine.

## Premise

- Read: report 15 of the dig study (checked by its authors); T-104.

## Approach

Take the floor of rollback and the delegation into T-104; then the gate of health with the rollback, and the check of loadability, as entries of their own when a consumer asks.

## Prove

```sh
cargo todo check
```

# T-832: The intake of the dig study for podswarm: NAT traversal, a DHT, gossip, sealed messages, content addressing, storage, sealed secrets

**Source:** the dig study of 2026-11-16 (an archive of the owner), reports 02, 05, 09, 10, 13, 17, 18.
**Category:** research
**Milestone:** M8
**Priority:** P3
**Effort:** L
**Status:** blocked

## Problem

Seven reports of the dig study design parts of a peer-to-peer system (a ladder of traversal, provider records, gossip with priority lanes, sealed messages, content addressing, storage, sealed secrets). They help podswarm alone.

## Premise

- Read: the seven reports (written from code reading; not compiled, not run); `INGEST.md` of the archive, which says not to ingest report 19 or ideas 3, 7 and 8 of report 17.

## Approach

When the session of podswarm starts: one entry for each report that it asks for, with its numbers; T-829 and T-825 keep their parts.

## Prove

```sh
cargo todo check
```

## Blocker

The ruling "podswarm" (`docs/decisions.md`): an ask that helps podswarm alone waits until its session starts.

# T-833: `pod-control`: a test that the catalogue is the dispatch, tiers of surfaces, a budget of callbacks (reports 03, 11, 12 of the dig study, with T-103)

**Source:** the dig study of 2026-11-16 (an archive of the owner), reports 03, 11, 12.
**Category:** feature
**Milestone:** M8
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

T-103 plans one JSON control API; the dig study adds a test in both directions that the catalogue of methods is the dispatch, tiers of the surfaces of a method, and a budget of the callbacks of a duplex call.

## Premise

- Read: reports 03, 11 and 12 of the dig study; T-103.

## Approach

Take the three into the scope of T-103 when it starts.

## Prove

```sh
cargo todo check
```

## Done

2026-11-17. The Approach of T-103 now names the three: one catalogue of methods with tests in both directions that it is the dispatch, a tier for each method with a refusal that does not tell of a control method, and a duplex frame with a budget of callbacks for each request and for each connection and a read timeout; and the plants that prove them. The code comes with T-103 (P1, M8).

# T-859: `pod-actions-workflow`: the error of an unknown need gives the place of the job, not of the need (PodUtils/podrunner#214)

**Source:** PodUtils/podrunner#214 (COORD-077 of podrunner, PodUtils/podrunner@T-331).
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`Graph::new` refused a need that is not a job of the file with the line and the column of the job's key: for `needs: nope` at line 7, the error said line 6, column 3 (`b:`). `podrunner run --validate` gives the error as it is, so a user reads the wrong line.

## Premise

- Read: PodUtils/podrunner#214; `crates/pod-actions-workflow/src/graph.rs`, `src/read.rs` (`needs`), `src/model.rs` (`Job`); `tests/expand.rs`.

## Approach

`Job` keeps the place of each id of `needs`; the error of an unknown need takes the place of that id (of the item, in a list). A test of one id and of a list; a plant.

## Prove

```sh
cargo test -p pod-actions-workflow --all-features --test expand
```

## Done

2026-11-17, 2059803e2: `Job::needs_marks` holds the place of each id of `needs`, in its order; `Graph::new` gives the place of the id that names no job. `a_missing_need_and_a_cycle_are_errors_by_name` holds `jobs.a.needs (line 5, column 12)` for `needs: ghost` and `(line 9, column 9)` for the item `nope` of a list. The tests of `pod-actions-workflow` pass. Plant `actions-unknown-need-marks-the-job`, caught.

# T-863: `pod-manual`: the changes of podssh to the imported files before its take (port review)

**Source:** the run 44096639741 of `upstream.yml` (the job `sources`, 2026-11-17).
**Category:** chore
**Milestone:** M0
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

`crates/pod-manual/SOURCES` has the podssh block at `5340fbb` with no `taken` line. podssh moved its manual onto `pod-manual` (PodUtils/podssh@T-790: `4d8c551`, `ee6b6bd`, `c4a2a64`, which removed its `man/text.rs` and `man/json.rs`). The check now reports each later change of its `man/` files and `flags.rs` (`5ad75b7`, `843e3c5`, `f1c50b0`, `4bf19f9`, `dc0df9f`, `e99ada7`, `0b12368`, `944be00`, `a808c26`, `1f2d8d3`), and `PodUtils/podssh:scripts/interop-man.sh`, so the job is red each day.

## Premise

- Read: the output of the run; `crates/pod-manual/SOURCES`; the diff of each commit before the take on the files of the block (`git log 5340fbb..c4a2a64 -- crates/podssh-cli/src/man crates/podssh-cli/src/flags.rs scripts/interop-man.sh` in a clone of podssh).

## Approach

1. Read each change before the take; a fix of the engine (the writers, the checks, the selection) is ported, a change of podssh's own content (its commands and flags) is not.
2. Add `taken` lines: the commit of the take for the block, and a path line for each file whose change before the take needs no port.
3. `scripts/check-sources.py pod-manual` gives 0 changes.

## Prove

```sh
python3 -I scripts/check-sources.py pod-manual
```

# T-866: `pod-lab`: a profile of the three limits together: no `AF_INET` bind, no network namespace, no write of another process's memory (PodUtils/podnet#649)

**Source:** PodUtils/podnet#649 (COORD-102 of podnet, its QA round; PodUtils/podnet@T-453), ask 1.
**Category:** feature
**Milestone:** M0
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podnet's defect PodUtils/podnet#610 hid on a host where three limits hold at once: no `AF_INET` bind, no network namespace, and `process_vm_writev` refused. No profile of `pod-lab` has the three together, so no lane runs a consumer there.

## Premise

- Read: PodUtils/podnet#649; `crates/pod-lab/src/profile.rs` and the labs that hold each limit alone.

## Approach

A profile that joins the three limits, with its row of `podlab run --list`, its check that each limit holds inside, and a test in the userns tests.

## Prove

```sh
sh scripts/ci/userns-tests.sh
```

## Done

2026-11-17, 78a6d8486, bb012c5eb. `pod-lab` has the profile `bind-netns-vm-denied` (source podnet), four parts: `tcp-bind-refused` (`bind(127.0.0.1:0)` EACCES, `connect(127.0.0.1:1)` allowed, inside the lab the bind of `[::1]:0` EACCES; held by `Hold::LandlockBind`, a Landlock ruleset of the TCP bind alone, ABI 4), `netns-refused` (`clone(CLONE_NEWUSER|NEWNET)` EPERM or ENOSPC; seccomp of `unshare` and `clone` with `CLONE_NEWNET`, `clone3` ENOSYS), `vm-write-refused` (`process_vm_writev(own pid)` EPERM; seccomp) and `unix-allowed`. `pod-hostprobe` has the two rows `clone(CLONE_NEWUSER|NEWNET)` and `process_vm_writev(own pid)` (90 rows; the schema of the report names them), so a check of a report sees the class on a host. Tests: `the_table`'s tests name the profile; `check::rows::bind_netns_vm_denied_reads_each_row` (a report of the class holds; an allowed write, a refused connect and an allowed namespace each fail their part); `tests/lab/hosts.rs` `bind_netns_vm_denied` runs a payload in the lab, as root and as uid 65534: a TCP bind EACCES, a connect ECONNREFUSED, `unshare(CLONE_NEWUSER|CLONE_NEWNET)` EPERM, `process_vm_writev` EPERM, AF_UNIX allowed. The manual of `podlab` lists the profile. Plant `lab-bind-hold-refuses-connect` (the hold refuses the connect too), caught.


# T-867: `pod-manual`: a check of each example of the model against the consumer's parser (PodUtils/podnet#649)

**Source:** PodUtils/podnet#649 (COORD-102 of podnet, PodUtils/podnet@T-451), ask 2.
**Category:** feature
**Milestone:** M0
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podnet's testers found flags that the help names and the command refuses (PodUtils/podnet#612, #647). `pod_manual::check::flags(manual, parser)` already holds each command and flag of the model against the parser; nothing holds each example (`Manual::examples`, `Example::command`) against it, and the long help can name a flag that is only in its text.

## Premise

- Read: `crates/pod-manual/src/check.rs` (`flags`, `ParserCommand`), `src/model.rs` (`Example`).

## Approach

`check::examples(manual, parse)`: each example's command line, split as a shell splits it, goes to the consumer's `parse` (its own parser, with no run), and a refusal is a finding with the example. A test with a stand-in parser; a plant.

## Prove

```sh
cargo test -p pod-manual --all-features
```

## Done

2026-11-17, f9ddb6e37: `pod_manual::check::examples(manual, parse)`: each example whose program is the manual's (after the variables that the shell sets) goes to the consumer's `parse` as the words after the program, as a POSIX shell splits them (quotes and a backslash hold; `|`, `;`, `&`, `<` and `>` end the command); a refusal is a finding with the example and the parser's words, and a quote that does not end is a finding too; an example of another program is not read. With `check::flags` (each command and flag of the model against the parser), a consumer tests each promise of its help. Tests: `a_line_splits_as_a_shell_splits_it`, `an_example_that_the_parser_refuses_is_a_finding`. The tests of `pod-manual` pass. Plant `manual-examples-not-parsed`, caught.


# T-883: the installer core: one proxy line for one proxy, and a default prefix on a noexec mount (PodUtils/podnet#650)

**Source:** PodUtils/podnet#650 (COORD-103 of podnet; PodUtils/podnet#645, confirmed by podnet; PodUtils/podnet@T-469).
**Category:** defect
**Milestone:** M0
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`inst_proxy` of `scripts/install/core-net.sh` prints one line for `https_proxy` and one for `http_proxy`: where both name one proxy (the usual case), `--dry-run` prints the same line twice. The default prefix (`$XDG_BIN_HOME`, else `~/.local/bin`) can be on a noexec mount; the install then stops, where another default could run programs.

## Premise

- Read: PodUtils/podnet#650; `scripts/install/core.sh` (`inst_resolve_prefix`, `inst_check_prefix`), `scripts/install/core-net.sh` (`inst_proxy`), `scripts/install/test.sh`.

## Approach

One line for each distinct proxy. A default prefix that cannot run programs gives way to the next default (`~/.local/bin`, `~/bin`), with one line that names the first and why; a prefix of `--prefix` never does, and where no default runs programs the refusal names `--prefix DIR`. A directory that the check made and cannot use goes again. Tests in `scripts/install/test.sh`: the dry run with both variables on one proxy has one line; in the lab `noexec-tmp`, `XDG_BIN_HOME` on `/tmp` gives way to `~/.local/bin`.

## Prove

```sh
sh scripts/install/test.sh --case dryrun --case labs
```

## Done

2026-11-17, a5609a8f6: `inst_proxy` prints one line for each distinct proxy. `inst_check_prefix` tries the prefix by `inst_try_prefix` (made, writable, runs a program); a default prefix (no `--prefix`) whose test program does not run gives way to `~/.local/bin`, then `~/bin`, with the line `FIRST cannot run programs (WHY): the install goes to DIR`; a directory that the try made and cannot use goes again; where none runs programs, and for each prefix of `--prefix`, the refusal stays as before and names `--prefix DIR`. Tests in `scripts/install/test.sh`: the dry run with `http_proxy` and `https_proxy` on one proxy has its line once; in the lab `noexec-tmp`, `XDG_BIN_HOME=/tmp/podlab-bin` gives way to `~/.local/bin`, which holds the program. Each of the two fails on the installer before the change (run by hand). `sh scripts/install/test.sh --case dryrun --case labs --case proxy --case first --case pins` passed (shellcheck: not on this host, CI runs it). podnet takes the new copy of `core.sh` and `core-net.sh` at the tag.

# T-899: `pod-hostprobe`: rows of a network namespace with a veth, a write of another process's memory, and the binds of AF_UNIX, abstract, VSOCK and NETLINK sockets (PodUtils/podnet#652)

**Source:** PodUtils/podnet#652 (COORD-105 of podnet; ask 3 of #221).
**Category:** feature
**Milestone:** M3
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

podnet's doctors (PodUtils/podnet:crates/podnet-netns/src/doctor.rs, PodUtils/podnet:crates/podnet-vnet/src/doctor.rs) probe these themselves. T-866 added `clone(CLONE_NEWUSER|NEWNET)` and `process_vm_writev(own pid)`.

## Premise

- Read: PodUtils/podnet#652.

## Approach

Rows: `unshare(CLONE_NEWNET)+veth` (in a user namespace), `write(/proc/CHILD/mem)` and `process_vm_writev(child pid)` of a child, and the binds of AF_UNIX (path), abstract, VSOCK and NETLINK sockets. The schema of the report names them. podnet reads the rows.

## Prove

```sh
cargo test -p pod-hostprobe --all-features
```

# T-900: The installer core: more downloaders, or a ruling, and the newest tag with an optional token (PodUtils/podnet#652)

**Source:** PodUtils/podnet#652 (COORD-105 of podnet; ask 3 of #221).
**Category:** feature
**Milestone:** M3
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

podnet's PodUtils/podnet:scripts/install.sh has its own downloaders (aria2c, httpie, python, perl) and finds the newest tag through the GitHub API or `releases.atom`; the core of podcrates has curl, wget, python3 and busybox, and takes a version.

## Premise

- Read: PodUtils/podnet#652.

## Approach

Decide the downloaders (a call: curl, wget, python3 and busybox cover the hosts of the labs; or add the others), and add a lookup of the newest tag (`releases.atom`, then the API with an optional token from the environment, never in argv), tested in the labs.

## Prove

```sh
sh scripts/install/test.sh
```

# T-902: `pod-actions-store`: `Lock::parse` keeps a digest in uppercase, and each fetch then fails (PodUtils/podrunner#288)

**Source:** PodUtils/podrunner#288 (COORD-082 of podrunner; PodUtils/podrunner#284).
**Category:** defect
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`Lock::parse` accepts a digest in uppercase hex and keeps the text as written; the store compares it with the lowercase SHA-256 of the tarball, so each action of such a lock fails with "the tarball's SHA-256 differs". The same holds for the digest of a `docker://` line.

## Premise

- Read: PodUtils/podrunner#288; `crates/pod-actions-store/src/lock.rs`.

## Approach

Lowercase each digest in `parse`, as the commit is. A test: a line in uppercase gives the same pin as in lowercase.

## Prove

```sh
cargo test -p pod-actions-store --all-features --lib lock
```

## Done

2026-11-17, 02d77fd97, 4a1dd17b8: `Lock::parse` keeps each digest of a pin and of a `docker://` line in lowercase. Test `a_digest_in_uppercase_is_the_same_pin`; the tests of `lock` pass. Plant `actions-store-digest-case-kept`, caught.


# T-905: `pod-lab`: a row of a failure in a lane: `podlab run --expect-exit CODE --expect-stderr TEXT` (PodUtils/podnet#649)

**Source:** PodUtils/podnet#649 (COORD-102 of podnet, PodUtils/podnet@T-452), ask 3.
**Category:** feature
**Milestone:** M0
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The lanes of `pod-lab` (`crates/pod-lab/LANES.md`) are commands that must end with 0: success paths only. A row of an error path (a dead relay, a refused proxy, no DNS) must end with its own code and its own line, and `podlab run` cannot say so: a script around it must split the error output and the code, and a row that wants a failure passes on any failure. podnet's QA round found defects on such paths that no lane ran.

## Premise

- Read: PodUtils/podnet#649, ask 3; `crates/pod-lab/LANES.md`; `crates/pod-lab/src/bin/podlab.rs`, the command `run`.
- podnet writes its own rows of failures first and gives them to podcrates when they hold.

## Approach

`podlab run --expect-exit CODE --expect-stderr TEXT`: exit 0 where the command ends with CODE and its error output holds TEXT; exit 4 (`EXIT_UNEXPECTED`) with one line for each difference. The error output goes through a pipe and on to the caller, and the check reads its tail (`pod_lab::expected`). `LANES.md` gives the form. Each consumer's rows of failures go into `LANES.md` when the consumer gives them.

## Prove

```sh
cargo test -p pod-lab --all-features --test cli --lib --bins
sh scripts/plant-each.sh lab-expect-exit-ignored
```

## Done

2026-11-18, 39ff7cbd0. `podlab run --expect-exit CODE --expect-stderr TEXT`: exit 0 where the command ends with CODE and its error output holds TEXT; else exit 4 (`EXIT_UNEXPECTED`, a fact of the manual) with one line for each difference. Each flag alone checks its part alone; a refused lab (125) and a command that does not start (126, 127) keep their codes. The error output goes through a pipe and on to the caller, and the check reads its last 4 MiB (`pod_lab::expected::{Wanted, tee, KEEP}`; `Options::stderr`). `LANES.md` gives the form of a row of a failure. The man page is generated again. Tests: `expected::tests` (each difference named, the tee copies each byte and keeps the tail, a closed error output does not stop the read), `tests/cli.rs` `a_row_of_a_failure_wants_its_code_and_its_line` (through the binary, in the lab `inherited-fds`). Plant `lab-expect-exit-ignored`, caught. The rows of failures of each consumer go into `LANES.md` when the consumer gives them.

# T-906: `pod-actions-store`: the ref of a checkout of another repository: empty, a full ref, a commit SHA (PodUtils/podrunner#290)

**Source:** PodUtils/podrunner#290 (COORD-084 of podrunner, PodUtils/podrunner@T-439; the report PodUtils/podrunner#251).
**Category:** defect
**Milestone:** M0
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`store::checkout_target` passes `with: ref` as it is written: an empty ref gives `owner/repo@`, which `add_trees` refuses. `git::Git::resolve` puts `refs/tags/` and `refs/heads/` before each ref: `refs/heads/main` becomes `refs/heads/refs/heads/main`, and a commit SHA is looked up as a tag or a branch. `actions/checkout` takes each of these forms, so a normal workflow fails before its first job.

## Premise

- Read: PodUtils/podrunner#290; `crates/pod-actions-store/src/store.rs` (`checkout_target`, `add_trees`) and `src/git.rs` (`resolve`, the fetch of a tree).

## Approach

`checkout_target`: an empty or blank ref is `HEAD`. `Git::resolve`: a full ref (`refs/heads/X`, `refs/tags/X`) is looked up as it is; 40 hex digits are the commit, given back in lowercase with no `ls-remote`, and the fetch takes that commit. A test for each form, against the stand-in git server of the tests.

## Prove

```sh
cargo test -p pod-actions-store --all-features
```

## Done

2026-11-18, 14b8a805a. `checkout_target`: `ref:` with no value, an empty or a blank ref gives `HEAD`. `Git::resolve`: 40 hex digits are the commit, given back in lowercase with no `ls-remote` (the fetch of the tree takes it); a ref that starts with `refs/` is looked up as it is, with its peeled line; a short ref is a tag, then a branch, as before. `store::fetch` already took a commit id with no forge. Tests: `a_checkout_of_another_repository_is_a_tree_to_pin` (the empty forms, a full ref kept), `tests/git.rs` `git_resolves_a_full_ref_and_a_commit_id` (a full branch, a full tag, a full annotated tag, a tag asked as a branch refused by name, an uppercase commit id, a commit id with no reachable forge, the tree of a commit id). Plant `actions-store-full-ref-prefixed`, caught.


# T-907: A reader of the zone files (TZif) and the cron of `on.schedule` in its `timezone` (PodUtils/podrunner#291)

**Source:** PodUtils/podrunner#291 (COORD-085 of podrunner, PodUtils/podrunner@T-423); the ruling "Shared code".
**Category:** feature
**Milestone:** M0
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

GitHub runs a cron of `on.schedule` at the local time of its `timezone` (an IANA name). podcrates has no code that reads a time zone, and podrunner must not write generic code that belongs here. The check of `pod-actions-workflow` (`check.rs`, `schedule`) also refuses each key of a schedule item but `cron`, so a workflow with `timezone` fails the check.

## Premise

- Read: PodUtils/podrunner#291; `crates/pod-actions-workflow/src/check.rs` (`schedule`); podrunner's cron (its `next`), which the ruling "Shared code" moves here when podcrates takes the cron.
- TZif: RFC 8536 (versions 1 to 3, the 64-bit data of version 2 and later, the POSIX TZ string of the footer).

## Approach

A module of zone files with no C library and no network: the zone of an IANA name from `$TZDIR`, else `/usr/share/zoneinfo`; a name with `..`, a leading `/` or a NUL is refused; the UTC offset at a UTC instant, with the footer after the last transition; a host with no zone files is a clear error. The cron `next(now)` in a zone: a local time in a gap runs once at the first valid time after the gap; a local time that repeats runs once, at its first instance (the rule of cronie and Vixie cron; GitHub does not document its rule). The check takes `timezone` (a string). Tests with small TZif files in the tests: a gap, a repeat, a zone with no DST, an instant after the last transition. Record the crate and the rule of a gap as a call.

## Prove

```sh
cargo test -p pod-time
cargo test -p pod-actions-workflow --all-features
```

## Done

2026-11-18: the crate `pod-time` (std alone). The import of `PodUtils/podrunner:crates/podrunner-local/src/cron.rs` at `d72b161` (`SOURCES`), then: `civil` (both ways, the weekday), the TZif reader (`tzif.rs`: versions 1 to 4, the 64-bit block, each count checked against the length before a read, a file of at most 1 MiB, a zone with leap seconds refused), the POSIX TZ string of the footer (`posix.rs`: `Jn`, `n`, `Mm.w.d`, rule times from -167 to 167 hours, quoted names; daylight time across the new year and all year), `Zone` (`load` from `$TZDIR`, else `/usr/share/zoneinfo`; `UTC` with no file; a name with `..`, a leading `/`, an empty part or a NUL refused; a host with no zone directory is `NoZoneFiles`; `offset_at`, `local`, `next_change`, `instants` as `One`, `Two` or `Gap`), and `Cron::next_in(zone, after)`: a local time in a gap runs once at the end of the gap, a repeated local time once at its first instance (call C779). `Cron::parse` gives `pod_time::Error` (kind `Cron`). The check of `pod-actions-workflow` takes `timezone` (a string; another type is "a zone name"). The fuzz target `tzif_zone` with the seeds of three zones of tzdata. `cargo test -p pod-time`: 4 and 10 passed (`tests/zone.rs`: no daylight time, the table then the footer, a gap, a repeat, a zone of the south, a table with no footer, the names, a directory, each prefix and each changed byte of a file, `Europe/Berlin` of the host).

# T-921: A fake GitHub as a binary for a consumer's CI: the App, the JIT configuration, the `workflow_job` webhooks and the runner broker (GitHub #223)

**Source:** GitHub #223 (githome's G-30, with A-X-18 and G-10 of GitHub #35 and #36)
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** L
**Status:** done

## Problem

githome tests its GitHub App path to podrunner with no account (the
ruling "Test accounts"). It needs a fake GitHub as a program that its CI
pins by release. podrunner has the parts as libraries
(`podrunner-fake`, `podrunner-broker-fake`); no binary serves them, and
podcrates has none. T-106 kept the fake GitHub API in its project; the
owner's ruling "Shared code" (2026-11-17) moves a capability that two
projects need into podcrates.

## Premise

- Read: GitHub #223 (G-30), PodUtils/podrunner#292, T-106.
- Not read: `PodUtils/podrunner:crates/podrunner-fake` and
  `PodUtils/podrunner:crates/podrunner-broker-fake` at the head of its
  `main`.
- Not measured: a binary.

## Approach

Take podrunner's fakes into podcrates (a feature of `pod-testkit` with a
binary, or a crate of its own; the entry decides by the size of the
code). The binary serves on loopback: the App JWT to an installation
token; `POST /repos/OWNER/REPO/actions/runners/generate-jitconfig`; the
`workflow_job` webhooks `queued`, `in_progress` (with `runner_name`) and
`completed`, signed with a webhook secret; the runner broker for each
minted configuration; and a control API: queue a job, choose which
queued job a runner gets, read the conclusions. Each test credential is
a made-up value in `.github/secrets-allow.txt`. The binary goes out with
each tag, with its `SHA256SUMS` signed. podrunner takes it in place of
its copies.

## Prove

```sh
cargo test -p pod-testkit --features github --test github -- the_second_job_runs_on_the_runner_minted_for_the_first --exact
cargo test -p pod-testkit --all-features
sh scripts/plant-each.sh testkit-github-assignment-ignored testkit-github-jwt-life-unbounded
```

A test queues two jobs of one repository with the same labels, gives the
second job to the runner minted for the first, and reads both
conclusions (the acceptance of G-30). It runs the binary
`pod-fake-github`. The `static` job of `ci.yml` checks the binary static
on each Linux target.

## Done

2026-11-18, this branch. The feature `github` of `pod-testkit` (call
C785) and its static binary `pod-fake-github`: the App (RS256 JWT of the
made-up key `src/github/made-up-key.pem` to an installation token), the
runners of a repository (`generate-jitconfig` with the three runner files
in `encoded_jit_config`, the list, the delete, a job by id), the broker of
each runner after podrunner's `podrunner-broker-fake` at `d001608`
(`SOURCES`; the OAuth token of a client assertion, the session, the long
poll, `acquirejob`, `renewjob`, `completejob`, the results service of the
logs), the `workflow_job` webhooks `queued`, `in_progress` and
`completed` with `X-Hub-Signature-256`, and the control API under
`/_fake/` (queue, assign, the jobs, the runners, the deliveries).
`stand::http_on` (a stand-in on the caller's listener) and `stand::call`
(one HTTP/1.1 request) are new in the kit. `cargo test -p pod-testkit
--all-features`: 12 unit tests with 7 new, and 3 new tests in
`tests/github.rs` (the acceptance through the binary, the refusals of the
App JWT and of the installation token, a runner that gets no job of
another repository or label). The two plants fail their tests. CI builds
the binary static on each Linux target, and `release.yml` attaches it for
x86_64 and aarch64 musl in `SHA256SUMS`. The help and the manual of the
binary from `pod-manual` are T-946. The take of podrunner in place of its
fake broker is podrunner's, on the hand-off of the next tag.

# T-922: The schemas of `schemas/` with one source each, a check of each against its types, and a line of each change (GitHub #223)

**Source:** GitHub #223 (githome's G-31)
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

githome pins the schemas of a tag by SHA-256 and checks its offers
against them. It asks for each schema as a file from the types of its
parser, a CI check that fails when the parser and the file differ, and a
line for each added field. The schemas of `pod-job` and of
`pod-hostprobe` are written from their types, and a test fails on a
difference (`crates/pod-job/src/schemas.rs`). Not known: the same for
`log.v1` and `broker-policy.v1`. No file lists the changes of a schema.
The schemas of podrunner's status and of its source wire are not in
podcrates.

## Premise

- Read: `schemas/`, `crates/pod-job/src/schemas.rs`, the ruling "Schema
  ids" (2026-11-18).
- Not read: the writers of `log.v1` and `broker-policy.v1`.
- Read: podrunner's answer, COORD-087 (PodUtils/podrunner#294), at
  podrunner `3a0cbbb`. `status.v1`: `PodUtils/podrunner:schemas/status.v1.json`
  from `PodUtils/podrunner:crates/podrunner-slots/src/status.rs` (`Status`,
  `SourceStatus`, `KindStatus`, `PinStatus`; `KindStatus.probe` is the
  `Capability` of `unit-capability.v1`), written by schemars with a test of
  drift. The source wire, version 1:
  `PodUtils/podrunner:docs/contracts/source-driver.md`,
  `PodUtils/podrunner:crates/podrunner-source/src/wire.rs` (`Request`, tag
  `op`: `hello`, `next`, `ack`, `claim`, `mint`, `report`, `audit`,
  `withdraw`, `release`; `Reply`, tag `reply`: `hello`, `events`,
  `credential`, `done`, `error`) and
  `PodUtils/podrunner:crates/podrunner-source/src/event.rs` (`Offer`,
  `Event`); each object has `"schema": 1`, and an unknown field is
  `bad-request`. Each field at `3a0cbbb` is the `v1` form; podrunner adds
  only optional fields until the move.
- podrunner changes the wire first (its T-486: the `audit` lines of
  githome's G-04; T-493: `assigned` with `keep` or `end`, G-15; T-495: the
  `host` of `hello`, G-17), with a line in its `CHANGES.md` (its T-510,
  githome's G-39).

- podrunner, PodUtils/podrunner#295 (16:30 UTC): its T-486 is done (`6ab9b0f`: each egress line of a unit goes to `audit` of its source, at most 1000, then `{ job_id, dropped }`); T-495 is partial (`ca8c308`: `hello` carries an optional `host`; open: the version of podnet); T-493 is partial (PodUtils/podrunner@T-530, and `assigned` of T-923, now in `v0.71.0`). The wire forms of the three are on podrunner `main` and in `PodUtils/podrunner:docs/contracts/source-driver.md`: the move can take them now.

## Approach

1. Each file of `schemas/` comes from its types, with a test that fails
   on a difference. Add the writer and the test where one is missing.
2. `schemas/CHANGES.md`: one line for each change of a schema (the tag,
   the schema, the field, added or changed). A check fails when a schema
   file changes with no line, and when a change that the ruling "Schema
   ids" gives a new id keeps the old id.
3. Move the types of podrunner's `status.v1` and of its source wire
   (the files above) into `pod-job`, with their schemas and examples (the
   ruling "Shared code": githome and podrunner read them; githome asked
   for it on GitHub #223). The coordinator sets the order with podrunner:
   the move takes the wire after podrunner's T-486, T-493 and T-495, so
   that each change lands once. podrunner then takes them from `pod-job`,
   and its `CHANGES.md` lines move to `schemas/CHANGES.md`.

## Prove

```sh
cargo test -p pod-job -p pod-hostprobe -p pod-broker --all-features
sh scripts/agent/check.sh
```

A field added to a parser with no change of its schema fails the check.

## Done

2026-11-18, 2bdb075, 9e24a2b and the commit of the move (the three parts):

1. Each file of `schemas/` comes from its types, with a test that fails on a
   difference: `broker-policy.v1` from `pod_broker::Policy` and `log.v1`
   from `pod_broker::Line` (`crates/pod-broker/src/schemas.rs`, with boon: 3
   new tests; `pod_core::LOG_LEVELS` is new). Plants:
   `broker-schema-not-written`, `broker-schema-open-with-rules`.
2. `schemas/CHANGES.md`: 50 rows from v0.8.0 to `next` (each tag, schema,
   field, kind and the SHA-256 of the file at the tag), with the fields of
   T-923 in v0.71.0. `scripts/check-schemas.py` (ci.yml, lint job;
   `scripts/agent/check.sh`) fails on a file with no row, on an added field
   with no row `added`, and on a removed, newly required or retyped field
   that keeps the id of the base tag. The release sets `next` to its tag
   (`docs/maintenance.md`). Plants: `schemas-change-with-no-row`,
   `schemas-removed-field-keeps-the-id`, `schemas-added-field-with-no-row`.
3. The types of podrunner's `status.v1` and source wire (at `ca8c308`) are
   in `pod-job`: `Status`, `SourceStatus`, `KindStatus`, `PinStatus`
   (`src/status.rs`), `pod_job::source::{Request, Reply, Event, Offer,
   RealJob, HostFacts, KindFacts, SourceCode, SourceError, Minted,
   MintedFile, Hidden}` with the parse and write of a line
   (`src/source/`). Schemas: `status.v1`, `source-request.v1`,
   `source-reply.v1`; 40 valid and 47 invalid examples get the same verdict
   from the types and from the schemas. The credential of the wire is
   `Minted` of `pod-job` (`Hidden`: `Debug` masked, zeroized); podrunner's
   `Credential` stays in podrunner. Fix: `Reply::Done` refused no unknown
   field (a unit variant of a tagged enum); it is `Done {}`
   (`an_unknown_field_is_refused` failed before the fix). Plants:
   `source-done-takes-any-field`, `status-schema-not-written`. `pod-job`
   lib: 23 tests. The adoption map for podrunner is in the README;
   `SOURCES` has the block of `ca8c308`. A fuzz target of the wire is T-940.

# T-923: `pod-job`: the fields of the job that githome and podrunner need (`trust_inputs.forge`, `limits.disk`, `cache`, the facts of a run report) (GitHub #223)

**Source:** GitHub #223 (githome's G-32: the crate half of podrunner's G-16, G-18, G-19 and G-20 of PodUtils/podrunner#292; G-36, the sandbox payload); the names: PodUtils/podrunner#294
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

githome sends and reads these fields, and `pod-job` has none of them:
`trust_inputs.forge` (the host of the forge of the job, for example
`github.com`); `limits.disk` in bytes; `cache` with `key` and `mode`
(`rw`, `ro` or `none`); and in the run report: the host, the unit kind,
the boundary, the image digest, the runner version, the timings, the
peak of the disk, the bytes of the network, the bytes of the cache and
`log_tail`.

## Premise

- Read: GitHub #223 (G-32), `crates/pod-job/README.md`, the ruling
  "Schema ids".
- Read: podrunner's names and units, COORD-087 (PodUtils/podrunner#294),
  its calls for githome's questions Q-1 to Q-8 on PodUtils/podrunner#292.
  Each field is optional:
  - `job-offer.v1`: `trust_inputs.forge` (string, the host name of the
    forge; G-16, podrunner T-494); `cache` (`{ key: string, mode: "rw" |
    "ro" | "none" }`; G-20, T-480); `payload` (`{ kind: "sandbox", image:
    "NAME@sha256:HEX", command: [string] }`), `idle_s` and `max_s`
    (integer seconds; G-36, T-506).
  - `limits.disk` exists (integer bytes; G-18, T-496): a kind that can
    enforce it gives the job `ENOSPC` at the size, another kind keeps the
    measured bound. podcrates reads that as the same meaning (the disk
    limit of the job), with the enforcement in `unit-capability.v1`
    (`LimitMode`), so the id stays under the rule "Schema ids"; podrunner
    agrees (PodUtils/podrunner#294).
  - `run-report.v1` (G-19, T-497; G-20): `host`, `unit_kind`,
    `boundary`, `image_digest` (`sha256:HEX`), `runner_version` (strings);
    `prepare_s`, `start_s`, `run_s`, `destroy_s` (integer seconds, rounded
    up, as `cpu_s`); `disk_peak_bytes`, `net_rx_bytes`, `net_tx_bytes`
    (integer bytes, through the egress module of the unit);
    `cache_read_bytes`, `cache_written_bytes` (integer bytes); `log_tail`
    (UTF-8, at most 65536 bytes, each credential scrubbed, only for a unit
    that failed before the runner connected). A field that a kind cannot
    measure is `unknown` with a reason, never 0.

## Approach

Take the names and units of the Premise (podrunner's calls). Each new
field is optional, so `v1` keeps its id (the ruling "Schema ids"); a
field that must be required gives `v2`. Add the fields, their
`check` rules and examples, write the schemas again, and name each field
in the hand-off COORD issue of the tag. All in one tag.

## Prove

```sh
cargo test -p pod-job --all-features
```

The parser takes and gives each field, and the schema holds it.

## Done

2026-11-18: the optional fields of the Premise (`crates/pod-job/src/parts.rs`, `offer.rs`, `report.rs`), with the names and units of podrunner but two (call C783): the sandbox of G-36 is the object `sandbox` (`image`, `command`, `idle_s`, `max_s`), as `payload` is the digest of a payload already; the reason of an `unknown` fact is in `unknown_reasons`. `host`, `unit.unit_kind` and `unit.boundary` were in `run-report.v1`, and `limits.disk` in `job-offer.v1`. `check`: a forge that is not a lower-case host name, a sandbox image that is not `NAME@sha256:HEX`, an empty command, an image digest that is not `sha256:HEX`, a `log_tail` over 65536 bytes, a key of `unknown_reasons` that names no `unknown` measured field. The schemas hold each rule but the bytes of `log_tail` (a schema counts characters): `schemas/job-offer.v1.schema.json` and `schemas/run-report.v1.schema.json` written again; the ids stay `v1` (each field is optional). Examples: `job-offer/valid-forge-cache-sandbox.json`, `invalid-sandbox-image-tag.json`, `invalid-cache-mode.json`, `invalid-forge-url.json`; `run-report/valid-phases-and-bytes.json`, `invalid-reason-of-a-count.json`, `invalid-image-digest-tag.json`, `invalid-arch.json`, `invalid-assigned-verdict.json`. podrunner added two fields from its census (PodUtils/podrunner#294, 09:15 and 11:30 UTC): `run-report.v1` `arch` (`x86_64` or `aarch64`; G-05, G-06) and `assigned` (`run_id`, `repository`, `job_name`, `verdict` `keep` or `end`, `code`; G-15); both are here. `cargo test -p pod-job --all-features`: 17 passed (20 valid and 29 invalid examples, each with the verdict of the types and of the schema). Plant `job-unknown-reason-of-a-count`, caught.

# T-925: `pod-incus`: the state of an instance, a profile with its devices, and the OOM count (PodUtils/podvm#582)

**Source:** PodUtils/podvm#582 (COORD-186 of podvm), for asks 1, 2 and 6 of PodUtils/podrunner#296 (COORD-089 of podrunner; githome's G-12 and G-18).
**Category:** feature
**Milestone:** M7
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

podvm's incus tier uses `pod-incus` of `v0.70.0`. Two reads that podrunner needs are not in its `Client`, and `get` and `run_json` are `pub(crate)`; by the ruling "Shared code", podvm writes no client of the incus API beside the crate.

1. The state of an instance: `GET /1.0/instances/NAME/state`, with `cpu.usage` (nanoseconds of CPU time), `memory.usage` and `memory.usage_peak` (bytes), the usage of each disk device where the pool reports it, `processes` and `status`. podvm gives the CPU time and the peak memory of an instance in `inspect` and in the run report (PodUtils/podrunner@T-483).
2. The pool of the root disk: the device `root` of `NewInstance::devices` must name the `pool` of the profile to set the `size` of the root disk (PodUtils/podrunner@T-482). podvm asks for `Client::profile(name)` (`GET /1.0/profiles/NAME`, with its `devices`), so that it copies the `root` device of the default profile and adds `size`. A refusal of the pool (a driver with no quota, for example `dir`) reaches podvm as an error with the words of incusd.
3. The OOM count of an instance (cgroup v2 `memory.events` `oom_kill`), where the state carries it; else say that incusd does not report it.

## Premise

- Read: PodUtils/podvm#582; `crates/pod-incus/src/client.rs`, `src/instances.rs`.
- Not read: the API document of incus for `InstanceState` and `Profile` at the version that podvm runs.

## Approach

`Client::instance_state(name) -> InstanceState` with the fields of ask 1 (each optional where incusd can omit it), `Client::profile(name) -> Profile` with its `devices` as a map of string maps, and an error of incusd kept with its words. Read the API document for an OOM field of the state; where there is none, say so in the doc and in the answer to podvm. Tests against the stand-in of incusd of the crate, with the JSON of the API document.

## Prove

```sh
cargo test -p pod-incus --all-features
```

## Done

2026-11-18: `pod_incus::state` (`crates/pod-incus/src/state.rs`): `Client::instance_state(name)` gives `InstanceState` (`status`, `status_code`, `disk` of each device with `usage` and `total`, `memory` with `usage`, `usage_peak`, `total` and the swap, `cpu.usage` in nanoseconds and `allocated_time`, `processes`, `pid`), the fields of `shared/api/instance_state.go` of incus; `Client::profile(name)` gives `Profile` with its `devices` (the `pool` of the `root` disk); a refusal of incusd keeps its words and its class (`Profile not found` is `NotFound`). Ask 3: the state of incus has no OOM count; the metrics have it (`incus_memory_OOM_kills_total` of `internal/server/metrics/types.go`, from `metrics.Memory.OOMKills`): `Client::oom_kills(name)` reads `GET /1.0/metrics` and gives the sample of the instance, `None` where no sample names it. Tests: `state::tests` (the parse of the metrics: a name matched whole, a time stamp after a value, another metric with the same start; the JSON of the state with `null` disks) and `tests/instances.rs` `the_state_the_profile_and_the_oom_kills_of_an_instance` against the stand-in of incusd. `cargo test -p pod-incus --all-features`: each target passes. Plant `incus-oom-metric-name-by-prefix`, caught.

# T-927: `pod-enter`: a new session with the pty as the controlling terminal of the payload (PodUtils/podvm#584)

**Source:** PodUtils/podvm#584 (COORD-188 of podvm; PodUtils/podvm@T-1970, OQ-02 of its outside pass).
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

`podvm run -t` opens a pty, and passes its slave to the payload as descriptors 0, 1 and 2 (`Fds::pass`). `pod-enter` makes no new session, so the slave is no controlling terminal of the payload: `os.tcgetpgrp(0)` gives `ENOTTY` while `isatty(0)` is true; the line discipline sends no SIGINT for `^C`, so Ctrl-C in `run -it` does nothing; the job control of an interactive shell does not work.

## Premise

- Read: PodUtils/podvm#584.
- Not read: the child path of `Fds` in `pod-enter` and of `spawn_joined`.

## Approach

An option of the plan, `Fds::ctty: Option<i32>` (the descriptor number in the child): in the child, after the `dup2` of the passed descriptors and before the exec, `setsid()` and `ioctl(fd, TIOCSCTTY, 0)`. A refusal of either is one line on the standard error of the payload, and the run goes on. The same for the entry of `exec` (`spawn_joined`). A test with a pty of the test: the child reads `tcgetpgrp(0)` as its own process group.

## Prove

```sh
cargo test -p pod-enter --all-features
```

# T-929: `static-probe`: the report of `pod-job` lacks the new fields of `RunReport`, so the workspace does not build

**Source:** the clippy run of T-882 (2026-11-18).
**Category:** defect
**Milestone:** M8
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`cargo clippy --workspace --all-targets --all-features -- -D warnings` fails at `examples/static-probe/src/main.rs`: `error[E0063]: missing fields cache_read_bytes, cache_written_bytes, destroy_s and 10 other fields in initializer of pod_job::RunReport`. T-923 (commit 62171e1) added optional fields to `RunReport`, and the probe still builds its report with the old fields only.

## Premise

- Measured: the clippy run above on the base 329a535 of T-882 (2026-11-18); the error is in `examples/static-probe` only.
- Read: `git log main -- examples/static-probe/src/main.rs` names no later change of the file.

## Approach

The report of `job()` in `examples/static-probe/src/main.rs` gives each new optional field as `None`, or builds from a constructor of `pod-job` that a later field does not break.

## Prove

```sh
cargo clippy -p static-probe --all-targets --all-features -- -D warnings
```

## Done

2026-11-18, 6dff7fc: the report of `job()` in `examples/static-probe/src/main.rs` comes from JSON through serde, so an optional field that `pod-job` adds needs no change of the probe. The run of the probe prints the refused report and its public form. `cargo clippy --workspace --all-targets --all-features -- -D warnings`: 0 faults. The scheduled nightly at `4161d8d` (run 44283168064) failed in each job that builds the workspace with this E0063; `6dff7fc` is the tag `v0.71.0`, with `ci` 44283332297 green. The test: the clippy step of `.github/workflows/ci.yml` (`--workspace --all-targets`) builds `examples/static-probe`, and failed on the code before the fix.

# T-940: `pod-job`: a fuzz target for one line of the source wire

**Source:** T-922 (the source wire of podrunner moved to `pod_job::source`)
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

`pod_job::source::parse_request` and `parse_reply` read lines from another process (a source, the scale-set sidecar, a remote mint). No fuzz target reads them; `job_offer` reads an offer alone.

## Premise

- Read: `crates/pod-job/src/source/mod.rs`, `fuzz/src/targets/job.rs`.

## Approach

Add the target `source_line` to `fuzz/`: each line goes through `parse_request` and `parse_reply`; a line that parses is written again with `request_line` or `reply_line` and reads back equal; the detail of each error holds no byte of the line longer than three bytes. Add it to the table of `fuzz/README.md` and to the stable test of the fuzz targets.

## Prove

```sh
cargo +nightly fuzz run source_line -- -max_total_time=300
```

# T-946: `pod-fake-github` and `pod-fake-githome`: the help and the manual from `pod-manual`, with the checks of each drift

**Source:** T-921 and T-948 (findings of their work); the owner's ruling "Discipline" (2026-11-15)
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

The ruling "Discipline" says that each command line takes its help and
its manual from `pod-manual`: `CLI man --no-pager`, a generated help, and
a test of each drift. The binary `pod-fake-github` of `pod-testkit`
(feature `github`) prints a help that it writes by hand. It has no `man`
command, and no test compares its help with its parser. Its routes, its
control API and its made-up credentials are only in
`crates/pod-testkit/README.md`. The binary `pod-fake-githome` (feature
`githome`, T-948) has the same fault: a help that it writes by hand, no
`man` command, and no test of its commands `serve` and `source`.

## Premise

- Read: `crates/pod-testkit/src/bin/pod-fake-github.rs`,
  `crates/pod-testkit/src/bin/pod-fake-githome.rs`,
  `crates/pod-manual/README.md`, the tests of `podlab`
  (`crates/pod-lab/src/bin/podlab/tests.rs`).
- Not measured: the size of the binary with `pod-manual` and `clap`.

## Approach

Give the binary a model of `pod-manual` (the options `--listen` and
`--hook`, the exit codes 0, 1 and 2, a topic for each part: the App, the
runners, the broker, the webhooks, the control API). The help and
`pod-fake-github man [--no-pager | --roff | --json]` come from it. Its
tests call `check::texts`, `check::model`, `check::exit_codes` and
`check::flags`. The `static` job of `ci.yml` runs
`pod-fake-github man --no-pager` and `scripts/check-man.sh` on it.
`pod-fake-githome` gets the same: a model with the commands `serve`
(`--listen`) and `source` (`--server`, the variable `POD_FAKE_GITHOME`),
and a topic for the wire, the control API and the records.
`pod-testkit` then depends on `pod-manual` under the features `github`
and `githome`; `scripts/check-publish-order.py` must accept the order.

## Prove

```sh
cargo test -p pod-testkit --all-features
python3 -I scripts/check-publish-order.py
```

# T-947: `pod-job`: `status.v1` has no `draining` of a source, no `max_units` and no `units` of a host, so podrunner keeps its own `Status` (PodUtils/podrunner#305)

**Source:** PodUtils/podrunner#305 (COORD-095; PodUtils/podrunner@T-501, PodUtils/podrunner@T-505 and PodUtils/podrunner@T-508)
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`pod_job::Status` and `pod_job::SourceStatus` refuse an unknown field.
podrunner's `status` has three fields more: `SourceStatus::draining` (an
operator drained the source: it takes no new offer), `Status::max_units`
(the most units of the host across its kinds; none is no cap) and
`Status::units` (the units that run now). podrunner cannot take
`pod_job::Status`, so its `schemas/status.v1.json` stays, and PodUtils/podrunner@T-508
waits.

## Premise

- Read: PodUtils/podrunner#305, `crates/pod-job/src/status.rs`,
  `schemas/CHANGES.md`, the ruling "Schema ids" (an optional field keeps
  the id).
- Not read: podrunner's `Status` at `e5a3d10`.

## Approach

Add `draining: bool`, `max_units: Option<u32>` and `units: u32` with
`#[serde(default)]` and a doc comment each. The schema `status.v1`
comes from the types again; `schemas/CHANGES.md` takes one row `added`
for each field with the tag `next`. A test: a status of before the
change (no field of the three) reads with the defaults; a status with
the three fields reads and writes back the same.

## Prove

```sh
cargo test -p pod-job --all-features status
python3 -I scripts/check-schemas.py
```

## Done

2026-11-18, commit "pod-job: status.v1 takes draining, max_units and units (T-947)". `SourceStatus::draining`, `Status::max_units` and `Status::units` have `#[serde(default)]`; `schemas/status.v1.schema.json` comes from the types again, with three rows `added` in `schemas/CHANGES.md`. The example `status/valid-capacity.json` has the three fields; `valid-running.json` has none and reads with the defaults (two tests in `status.rs`). The plant `status-units-required` fails the test of the defaults.

# T-948: `pod-testkit`: a fake of githome's source, as a program that podrunner's CI can pin (PodUtils/podrunner#306, githome's G-38 and G-40)

**Source:** PodUtils/podrunner#306 (COORD-096; githome's G-38 and G-40 on PodUtils/podrunner#292; PodUtils/podrunner@T-509)
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

githome's G-38 asks for a test of githome's source in podrunner's CI that
blocks a release of podrunner. podrunner cannot get githome's source:
`PodUtils/githome` is not reachable from its sessions and CI, githome has
no released bundle, and githome's session is offline until podrunner is
complete. G-38 names a second path: a fake of githome's wire that
`pod-testkit` keeps. podcrates has the types of the wire
(`pod_job::source`, T-922) and a fake GitHub as a binary (T-921), but no
fake of a source of jobs.

## Premise

- Read: PodUtils/podrunner#306, the rows G-38 and G-40 of
  PodUtils/podrunner#292 (githome's wire: `/_runner/source/OP`, JSON),
  `crates/pod-job/src/source/`, `crates/pod-testkit/src/github/` (T-921).
- Not read: `PodUtils/podrunner:docs/contracts/source-driver.md` and
  podrunner's `source-conformance` at the head of its `main`; githome's
  own source.

## Approach

A feature `githome` of `pod-testkit` with the binary `pod-fake-githome`.
`pod-fake-githome source` is a source program of kind `program`: it
speaks `pod_job::source` on stdin and stdout, and answers as githome's
source does by its wire of G-38: `hello` with its facts, `next` with
the queued offers, `claim`, `mint`, `report`, `withdraw`, `release` and
`assigned`, and the refusals of githome (each `SourceCode`). `pod-fake-githome serve`
holds the state and a control API on loopback (queue a job, read the
reports and the refusals), as the fake GitHub of T-921 does; the program
reaches it by an address in its environment. Each release attaches the
binary for x86_64 and aarch64 musl, in `SHA256SUMS`. A test runs the
binary with the conformance steps of `source-conformance` that the entry
reads from podrunner, and one job from offer to report.

## Prove

```sh
cargo test -p pod-testkit --features githome --test githome
```

## Done

2026-11-18, this branch. The feature `githome` of `pod-testkit` (call
C786) and its static binary `pod-fake-githome`. `pod-fake-githome serve
[--listen ADDR]` prints its base URL and holds the state: the routes
`/_runner/source/session` and `/_runner/source/OP` answer each operation
of the source wire for one session, by `pod_job::source`; the control API
under `/_fake/` queues a job (the fields of `job-offer.v1` with
`repository` and `tier`), sends `cancel`, `started` and `completed`,
plants a mint error, sets `unavailable` and `revoked`, and reads the jobs
and the records `reports`, `refusals`, `withdrawals`, `releases`,
`audits`, `mints`, `assignments` and `hosts`. `pod-fake-githome source
[--server URL]` is the program of a `[[source]]` of the kind `program`;
it finds `serve` by `POD_FAKE_GITHOME`. The rules come from podrunner's
fake source and `assigned` and the contract at `b1a1a28` (`SOURCES`);
the sessions, the host facts and the control API are new. The shared
`json_answer` and `body` of the fakes moved to `src/fake.rs`. `cargo test
-p pod-testkit --features githome --test githome`: 4 tests through the
binary (the 14 steps of podrunner's `source-conformance`, one job from
its offer to its report with the control API, the refusals and the
settings with two hosts, the program with no server), 50 runs with no
failure; 2 unit tests of the offers. The plants
`testkit-githome-claimed-twice` and `testkit-githome-host-facts-ignored`
fail their tests. CI builds the binary static on each Linux target, and
`release.yml` attaches it for x86_64 and aarch64 musl in `SHA256SUMS`.
`pod_job::source` and podrunner's contract agree. The help and the
manual of the binary from `pod-manual` are T-946. The job of podrunner's
CI (PodUtils/podrunner@T-509) is podrunner's, on the hand-off of the next tag.

# T-951: `pod-testkit`: `pod-fake-githome` passes the 14 tests of podrunner's `source-conformance`, not its test 15 of a cancelled job (PodUtils/podrunner#310)

**Source:** PodUtils/podrunner#310 (COORD-100; podrunner's `b0f8a36`, PodUtils/podrunner@T-502, githome's G-27)
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

podrunner's `source-conformance` has 15 tests since `b0f8a36`. Test 15, `a-cancelled-job-is-reported-and-released`: after `mint`, a `report` with `exit_reason: cancelled` and a `release` are taken, and a `claim` after them gets `unknown-job`. The test of T-948 runs the 14 tests of `b1a1a28`; no test shows that `pod-fake-githome` answers test 15, and one run of the 15 tests takes one more queued job. PodUtils/podrunner@T-509 runs the 15 tests with the release asset.

## Premise

- Read: PodUtils/podrunner#310; `crates/pod-testkit/tests/githome.rs`; `crates/pod-testkit/src/githome/` (the rules of `release`: a report with no refusal code ends the job).
- Not read: `PodUtils/podrunner:crates/podrunner-source/src/conformance.rs` and `PodUtils/podrunner:docs/contracts/source-driver.md` (Tests) at `b0f8a36` or later.

## Approach

Read test 15 at podrunner's head. Add it to `the_conformance_steps_of_podrunner_pass` with its own queued job; change the fake where its answer differs. Update the SOURCES line of `conformance.rs` and `source-driver.md` to the new commit, and the README line on the jobs that one run takes.

## Prove

```sh
cargo test -p pod-testkit --features githome --test githome
```

## Done

2026-11-18, commit "pod-testkit: pod-fake-githome passes test 15 of podrunner's source-conformance (T-951)". Test 15 at podrunner `0fd2218` (`a-cancelled-job-is-reported-and-released`): `the_conformance_steps_of_podrunner_pass` claims a job, mints, reports `exit_reason: cancelled`, releases, and gets `unknown-job` for a claim after it. The fake passes it with no change: a release after a report with no refusal code ends the job. The test queues three jobs; a run ends two (tests 7 and 15). `crates/pod-testkit/SOURCES` names `conformance.rs` and `source-driver.md` at `0fd2218`; the README says to queue three jobs.

# T-952: `pod-testkit`: `pod-fake-githome` mints made-up files, so no runner of a job through podrunner's `serve` reaches the broker of `pod-fake-github` (PodUtils/podrunner#311)

**Source:** PodUtils/podrunner#311 (COORD-101; PodUtils/podrunner@T-527 and PodUtils/podrunner@T-509)
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

`pod-fake-githome source` speaks the source wire, and its `mint` gives made-up files. `pod-fake-github` has `generate-jitconfig` and the broker of a JIT runner, and no source wire. podrunner's `serve` can run an offer of the fake on a real unit, but no job of the official runner: the minted files reach no broker. podrunner's lane `serve-incus-kvm` (githome's host class) and its live job of each kind against githome's source need one job from the offer of the source to the conclusion of the broker.

## Premise

- Read: PodUtils/podrunner#311; `crates/pod-testkit/src/githome/` and `crates/pod-testkit/src/github/` (T-948, T-921; calls C785 and C786).
- Not read: the form of `encoded_jit_config` that the official runner reads at its version of podrunner's runner image.

## Approach

`pod-fake-githome serve --github URL` takes the base URL of a `pod-fake-github`. With it: `POST /_fake/jobs` queues the job in the fake GitHub too (its repository, labels and `steps`, else one `run:` step), through the control API of the fake GitHub; `mint` of a claimed job calls `generate-jitconfig` of the fake GitHub with the runner name of the claim and the labels of the offer, and answers the files of the decoded `encoded_jit_config`, so the runner takes that job from the broker; `GET /_fake/jobs/ID` of githome names the id of the job in the fake GitHub, whose conclusion `GET /_fake/jobs/ID` of the fake GitHub gives. With no `--github`, `mint` stays as it is (the 15 tests of `source-conformance` pass). A test: both fakes, one job, a claim and a mint; the minted `.runner` and `.credentials` take the job from the broker of the fake GitHub, and its conclusion and the report of the source are both read. The base URL is any loopback port, so podrunner names it in `[egress]`.

## Prove

```sh
cargo test -p pod-testkit --features githome,github --test githome
```

## Done

2026-11-19, this commit. `pod-fake-githome serve --github URL` links the
fake githome to a `pod-fake-github` (call C788). `POST /_fake/jobs` also
queues the job in the fake GitHub (the repository and the labels of the
offer, `self-hosted` when it has none, and the scripts of `steps`), and
`GET /_fake/jobs/ID` names it as `github_job_id`. `mint` of a claimed job
acts as the App: the key of `GET /_fake/app`, an installation token,
`generate-jitconfig` with the runner name of the claim and the labels of
the job, `POST /_fake/jobs/ID/assign` of that job to that runner, and the
three files of the decoded `encoded_jit_config`; a step that fails is
`credential-unavailable`. With no `--github`, `mint` gives the made-up
files, and the 15 steps of `source-conformance` pass unchanged. The App
side is the module `pod_testkit::github::client`, and the feature
`githome` takes the feature `github`. The test
`a_minted_runner_takes_its_job_from_the_linked_fake_github` runs both
binaries: the official runner of the minted files takes the job of the
claim, not an older queued job of its labels, and completes it; the report
of githome and the conclusion `success` of the fake GitHub are both read.
`cargo test -p pod-testkit --features githome,github --test githome`: 5
passed. The plant `testkit-githome-runner-gets-another-job` (no
assignment) fails the test.

# T-964: `pod-release`: the bundle of Rekor v2: the inclusion proof with its signed checkpoint, and the RFC 3161 timestamp

**Source:** the work of T-104 (2026-11-19): a finding in the verifier. P1 from 2026-11-19: podrunner's
T-562 (PodUtils/podrunner#320) and the self-update of podssh (which took `pod-release` at `c9772a3`)
use the verifier.
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

`pod_release::sigstore` trusts the time of an entry and its presence in the log by Rekor's signed entry
timestamp (`inclusionPromise`), as podssh's verifier did. A bundle of Rekor v2 has no promise: it has
the inclusion proof with a signed checkpoint, and the time comes from an RFC 3161 timestamp
(`timestampVerificationData`). When the signing configuration of Sigstore's public instance sends the
cosign of a release workflow to Rekor v2, the verifier refuses each new release. The bundle of
podcrates v0.76.0 (cosign v3.1.4) still has the promise, and also the proof and a timestamp.

## Premise

- Read: `crates/pod-release/src/sigstore.rs`, `crates/pod-release/src/sigstore/rekor.rs`, the bundle
  `crates/pod-release/tests/fixtures/v0.76.0/SHA256SUMS.sigstore.json`.
- Not measured: a bundle of Rekor v2; the date at which the public signing configuration moves.

## Approach

Check the inclusion proof (RFC 6962: the leaf hash of the canonicalized body, the audit path, the root)
and the signed note of the checkpoint with the key of the log. Check the RFC 3161 timestamp with the
keys of Sigstore's timestamp authority, compiled in beside the Fulcio chain, and take the time from it
when the entry has no promise. Keep the promise where it is there. A test takes each part of the bundle
of v0.76.0, and a fixture of a bundle of Rekor v2.

## Prove

```sh
cargo test -p pod-release --all-features
sh scripts/plant-each.sh release-inclusion-unchecked release-timestamp-unsigned
```

## Done

2026-11-20, this commit (the code in `c70b870d3`, the plants in `786c64b23`; calls C820 and C821).
`pod_release::sigstore` verifies the inclusion proof of RFC 6962 (`sigstore/proof.rs`) with the
checkpoint as a signed note of the log (ECDSA P-256 for Rekor v1, Ed25519 for Rekor v2; a witness
line is not read), and the RFC 3161 timestamp (`sigstore/stamp.rs`: the CMS SignedData read with the
`asn1-rs` of x509-parser, no new dependency) by the compiled chain of `timestamp.sigstore.dev`. The
body is a `hashedrekord` 0.0.1 or 0.0.2. `Roots` is `{ fulcio, logs, tsa }`, with the shard
`log2025-1` of Rekor v2 and the authority from Sigstore's public trusted root (root-signing `c9bda74`),
which `SOURCES` names and a test compares. Fixtures: the bundles of v0.76.0 and v0.77.0 with each part
removed and changed (`sigstore::tests::parts`); two bundles of Rekor v2 of sigstore-conformance
(`e275ffb`, Apache-2.0, `tests/fixtures/sigstore/`) with their staging trusted root
(`sigstore::tests::rekor2`). `cargo test -p pod-release --all-features`: 65 + 8 + 13 passed (1
ignored). `plant-each.sh`: the 2 new plants and the 2 plants of T-104 (new patches) caught.
`sh scripts/agent/check.sh`: each step 0 (the test step run alone, `cargo test --workspace --all-features`, after a removal of `target/debug` for the disk).

# T-965: `pod-release`: the verifier as a static command for the CI of a product and for an installer

**Source:** the work of T-104 (2026-11-19): GitHub #223 (githome's G-34: "one verifier"), GitHub #35
(A-X-13, A-X-24); PodUtils/podrunner#320 (COORD-110 of podrunner: PodUtils/podrunner@T-562 checks the index of
its release with the verifier of `pod-release` in its dry run).
**Category:** feature
**Milestone:** M8
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

The verifier of T-104 is a library: a Rust consumer calls `pod_release::verify::verify_dir`. The CI of
githome and cfb, and the umbrella installer of A-X-24, need a command that checks a release directory:
the bundle of `SHA256SUMS` by an identity, each file, and the index. Today they run `cosign verify-blob`
and `sha256sum -c`, which do not check the index.

## Premise

- Read: `crates/pod-release/src/verify.rs`; `crates/pod-lab/src/bin/podlab.rs` (a static binary of a
  crate with its help from `pod-manual`); `.github/workflows/release.yml`.
- Not measured: the interface that githome and cfb want.

## Approach

A binary `pod-release` (feature `cli`): `verify --identity URI [--issuer URL] DIR` and `index` (the
reader of an index), its help and its manual from `pod-manual`, its exit codes from the classes of
`Error`. `release.yml` builds it static for x86_64 and aarch64, checks it with `scripts/check-static.sh`,
runs it over its own signed files on the tag, and publishes it beside `podlab`. Ask githome and cfb on
GitHub #223 for the interface first.

## Prove

```sh
cargo test -p pod-release --all-features
sh scripts/agent/check.sh
```

## Done

2026-11-19, this commit. The binary `pod-release` of the crate (feature `cli`, calls C804 to C807):

- `pod-release verify --repo OWNER/REPO --tag TAG [--workflow FILE] DIR [FILE...]`, or
  `--identity URI [--issuer URL]`: the bundle `SHA256SUMS.sigstore.json` of `SHA256SUMS` by the signer
  (Sigstore's public roots, compiled in), then each file of `SHA256SUMS` that `DIR` holds (with
  `--all`: each file; with `FILE...`: these files), then `release-index.json` against `SHA256SUMS`, its
  schema and the signer (`Index::signed_as`). `pod-release sums [--all] DIR [FILE...]`: the same with
  no bundle, the check of a dry run. `pod-release index [--target TRIPLE] FILE`. `pod-release man`.
  It reads files only. Exit 0, 64 (usage), 66 (a needed file is not there), 69 (a file cannot be
  read), 77 (the signature, the signer, a SHA-256 or the index), the codes of the classes of
  `pod_core`, with one line `pod-release: CLASS: WHY` on the error output.
- Its help, its manual (`man/pod-release.1`, `man/llms.txt`) and its exit codes come from `pod-manual`;
  its tests run the checks of `pod-manual` (the text rules, the parser, the generated files, the
  variables, the exit codes, the rules of the flags, the examples, and the new shared `check::help`,
  which the tests of `podlab` take too).
- The library: `verify::verify_files` and `verify::check_files` with `Which` (`Present`, `All`,
  `Named`), `verify::read_file`, `Index::signed_as`; an index that `SHA256SUMS` does not name is
  refused; a missing file is `Error::NotFound`. `pod_testkit::tamper::flip` holds the one changed
  bit of the tests.
- Release: `release.yml` builds `pod-release-x86_64-unknown-linux-musl` and
  `pod-release-aarch64-unknown-linux-musl` static (checked by `scripts/check-static.sh` and
  `scripts/check-strings.py`), puts them in `SHA256SUMS` and the index, runs `pod-release sums --all
  dist` in each run (a changed index gives 77), runs `pod-release verify --all` on the tag after the
  signature (a changed file and another tag give 77), and pushes the image
  `ghcr.io/podutils/pod-release:TAG` (call C789). `ci.yml` builds and checks it on each Linux target
  with its strings and its man page; `scripts/agent/check.sh` checks it static on x86_64 (1,767,840
  bytes).
- Proof: `cargo test -p pod-release --all-features`: 55 + 8 + 13 passed. `tests/cli.rs` runs the
  binary over the files of v0.76.0 and v0.77.0 (`tests/fixtures/v0.77.0/`: `SHA256SUMS`, its bundle,
  the index, `podlab-install.sh`): each release verifies; one changed byte of `podlab-install.sh`,
  of `SHA256SUMS`, of the bundle and of the index, and another tag, repository, workflow or issuer,
  each give 77 and one line; a missing file with `--all` or by name gives 66; an index with a field
  that the schema does not know gives 77. The plants `release-index-unread` and
  `release-named-file-missing` are caught.
- What PodUtils/podrunner@T-562 runs in its release dry run, with the asset of a tag of podcrates
  from v0.78.0 pinned by its SHA-256:
  `pod-release-x86_64-unknown-linux-musl sums --all dist`, and on its tag after `cosign sign-blob`:
  `pod-release-x86_64-unknown-linux-musl verify --repo PodUtils/podrunner --tag "$GITHUB_REF_NAME"
  --all dist`.

2026-11-19, the ci run 44569634090 at `a5901c2`: `scripts/check-strings.py` read "Standard (PKCS) #1" (an OID name that x509-parser carries) in the powerpc64le binary as the id of a work entry. A `#NNN` after a name in capitals in parentheses is now the number of a standard, as one with no parentheses was; the self-test has the case, the old rule fails on it, and `#584` after a lower-case word in prose is still a fault.

# T-966: `pod-release`: the adoption trials of podnet and podssh, and the hand-off of the crate

**Source:** the work of T-880 (2026-11-19): `docs/design.md`, section 8, step 7.
**Category:** chore
**Milestone:** M8
**Priority:** P2
**Effort:** M
**Status:** partial

## Problem

podnet (PodUtils/podnet:src/update.rs, PodUtils/podnet:src/update/) and podssh
(PodUtils/podssh:crates/podssh-cli/src/update/) still hold their
own copies of the verifier and of the update. The ruling "Shared code" ends with one copy in
podcrates. No trial patch shows that each consumer builds and passes its tests of `update` on the
crate.

## Premise

- Read: the adoption map of `crates/pod-release/README.md`; the Done section of T-880.
- Not measured: the trial of either consumer.

## Approach

Write adoption/podnet/pod-release.patch and adoption/podssh/pod-release.patch (the glue of the
adoption map: the settings, `Source`, `Identity`, the lines of each step, the exit codes), and run
them in `adoption.yml`. Then hand the crate to each session by a COORD issue, with the tag that holds
it. Each project writes its own `release-index.json` with `scripts/release-index.py` in its release
workflow.

## Prove

```sh
cargo test -p pod-release --all-features
cargo todo check
```

## Done

2026-11-19, podssh's half: podssh took `pod-release` in `c9772a3` (PodUtils/podssh#329, PodUtils/podssh@T-850), with `v0.77.0` at `b3005e5`. Its `update/` keeps its flags, lines, exit codes, `trust()` and the lines of the way back; `sigstore.rs`, `release.rs`, `fetch.rs`, `replace.rs`, `keep.rs`, `trial.rs` and `trust/` are gone; the steps are `update::newest`, `signed`, `binary` and `install`, with `keep::rollback` and `replace::running()`; its update tests pass (4, 18 and the offline ones). `crates/pod-release/SOURCES` has `taken c9772a3` in the podssh block: `check-sources.py` gives 0 changes. podssh does not use `update::plan` (T-971). podnet's half waits for PodUtils/podnet@T-494, behind the asks of podrunner. podssh's `release.yml` writes `release-index.json` with `scripts/release-index.py` (taken at `v0.77.0` by its SHA-256) before `SHA256SUMS` from `679b940`; its run by hand 44571898795 passed (PodUtils/podssh#329: podssh's T-850 is done). podnet writes its index with its take.


# T-967: `pod-release`: a minisign signature beside the Sigstore bundle

**Source:** the row of `pod-release` in `docs/design.md`, section 4 (the plan of the crate).
**Category:** feature
**Milestone:** M8
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

The plan of the crate names minisign as a second form of a signature. No consumer and no ask names a
minisign key today: each release of PodUtils signs `SHA256SUMS` with keyless cosign alone.

## Premise

- Read: `docs/design.md`, section 4; the release workflows of podcrates, podnet and podssh (keyless
  cosign only).
- Not measured: a release with a minisign signature.

## Approach

When a consumer asks, check an Ed25519 minisign signature of `SHA256SUMS` (`SHA256SUMS.minisig`) by a
public key that the caller gives, with the Ed25519 of the workspace.

## Prove

```sh
cargo test -p pod-release --all-features
```

## Start condition

A consumer or an ask names a minisign key for its releases.

# T-971: `pod-release`: `update::plan` answers `UpToDate` both for the version of this build and for an older release

**Source:** PodUtils/podssh#329 (COORD-128 of podssh: its take of `pod-release`, "no ask")
**Category:** defect
**Milestone:** M8
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

podssh took `pod-release` and does not use `update::plan`. With no pinned tag, `plan` answers `UpToDate` both when the newest release is the version of this build and when this build is newer than the newest release. podssh says the two apart ("the newest release: nothing to do"; "this build is newer than TAG: nothing to do"), so it keeps its own comparison: a second copy of the rule.

## Premise

- Read: PodUtils/podssh#329; `crates/pod-release/src/update.rs` (`plan`), `crates/pod-release/src/version.rs`.
- Read: podssh's comparison at `c9772a3` (PodUtils/podssh:crates/podssh-cli/src/update/mod.rs).

## Approach

A variant `Plan::Newer` for an older release with no pinned tag, apart from `Plan::UpToDate` for the same version, with a test of both; podssh then calls `plan` and drops its comparison.

## Prove

```sh
cargo test -p pod-release --all-features plan
```

## Done

2026-11-19, this commit. `Plan::Newer`: with no pinned tag, a release older than this build is `Newer`, and a release of the version of this build is `UpToDate`. Read: podssh's comparison at `c9772a3` (PodUtils/podssh:crates/podssh-cli/src/update/mod.rs: `Ordering::Equal`, `Less`, `Greater`). Test `plan_tells_an_equal_release_from_an_older_one`; the two lines of `the_same_tag_is_not_installed_again` with an older release now give `Newer`. `cargo test -p pod-release --all-features`: 56, 8 and 13 passed. podssh can call `plan` in the next tag and drop its comparison.

# T-972: `pod-lab`: the CAs of a lab take the P-256 key and the PEM writer of `pod-acme`

**Source:** T-100 (a finding: the ruling "Shared code")
**Category:** chore
**Milestone:** M8
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

`crates/pod-lab/src/net/tls.rs` has its own P-256 key for rcgen (`Key`, from `getrandom` of
`pod-hostprobe`) and its own `pem`. `pod-acme` has the same (`key::Key`, `certs::pem`), and the CA of a
job of `pod-broker` (T-100) takes the copy of `pod-acme`. Two copies of one capability.

## Premise

- Read: `crates/pod-lab/src/net/tls.rs`, `crates/pod-acme/src/key.rs`, `crates/pod-acme/src/certs.rs`,
  `crates/pod-broker/src/ca.rs`.
- Not measured: the size of the dependency of `pod-lab` on `pod-acme`.

## Approach

`pod-lab` takes `pod_acme::key::Key` and `pod_acme::certs::pem`, or the two move to `pod-tls` (a feature
with rcgen) and `pod-acme`, `pod-broker` and `pod-lab` take them there. Keep the tests of the lab green.

## Prove

```sh
cargo test -p pod-lab --all-features
python3 -I scripts/codegraph.py new
```

# T-973: `pod-broker`: the CA bundle of a job with curl, git, Python and Node.js in a lab

**Source:** T-100 (a finding: the tests read the TLS end with rustls clients and `openssl verify` only)
**Category:** chore
**Milestone:** M8
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

The tests of the TLS end prove the swap and the scrub with rustls clients, and the leaf with
`openssl verify -x509_strict`. No test runs the clients of a job (curl, git over HTTPS, Python's
requests, Node.js) with the bundle of `ca::write_bundle` and the variables of `ca::env`, through the
broker, to a far end that checks the value.

## Premise

- Read: `crates/pod-broker/README.md`, "Placeholder credentials"; docs/decisions.md, C811 to C816.
- Not measured: each client with the bundle.

## Approach

A lab of `pod-lab` (T-692): the broker with its TLS end, a stand-in far end that ends TLS with a leaf of
the lab CA and answers with the `Authorization` that it got, and each client with the bundle and the
placeholder. Prove that the far end gets the value and the client prints the placeholder.

## Decision

The proof is the test target `tls_clients` of `pod-broker`, not a lab of `pod-lab` (call C822).

## Prove

```sh
cargo test -p pod-broker --all-features --test tls_clients -- --nocapture
sh scripts/ci/broker-clients-tests.sh
```

## Done

2026-11-20, this commit ("pod-broker: the clients of a job with the bundle of the job (T-973)"). The test
target `crates/pod-broker/tests/tls_clients.rs` (with `tests/clients/`) runs each client with a clean
environment, the bundle of `ca::write_bundle` and the variables of `ca::env`, through the broker on
127.0.0.1, to a far end with a leaf of its own CA that names the `Authorization` it got:

- curl, Python `urllib`, Python `requests`, Node.js (a CONNECT of its own) and Node.js `fetch` with
  `--use-env-proxy`: with no bundle each refuses the leaf; with the bundle the far end gets the value and
  the client reads the placeholder in the header and in the body; a host with no credential passes by
  the roots of the bundle, with no swap.
- git: `git clone` from `git http-backend` with the header of actions/checkout (`AUTHORIZATION: basic`)
  and with a token in the URL after a 401; a pack over `max_body` comes in chunks, scrubbed.
- curl: a body over `max_body` with `Expect: 100-continue`. cargo: `generate-lockfile` from a sparse
  registry with `auth-required`; the token is the whole value of `Authorization`.

The variables that each client reads are in `crates/pod-broker/README.md`, "Placeholder credentials".
No client refused the bundle, and no variable was missing: the broker did not change. `cargo test -p
pod-broker --all-features --test tls_clients`: 7 passed; `sh scripts/ci/broker-clients-tests.sh` (Node.js
26.13.1, `requests` 2.34.2): 7 passed, each line said. CI: the step "pod-broker with the clients of a job"
of `ci.yml`. Plants `broker-env-no-node-extra-ca-certs` and `broker-bundle-drops-the-roots`, caught. Found
and written as an entry: T-980 (the clients that read none of the variables).

# T-980: `pod-broker`: the bundle of a job for Go, Java, Deno, pip, npm and Hex

**Source:** T-973 (a finding: `tls_clients` runs curl, git, Python, Node.js and cargo only)
**Category:** chore
**Milestone:** M8
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

`ca::env` sets the six variables of `pod_tls::BUNDLE_VARS`. Some clients of a CI job read none of them:
the JVM reads a trust store (`javax.net.ssl.trustStore`), Deno reads `DENO_CERT`, Hex of Elixir reads
`HEX_CACERTS_PATH`, the Google Cloud CLI reads `CLOUDSDK_CORE_CUSTOM_CA_CERTS_FILE`. Such a client refuses
the leaf of the job. No test runs Go, pip or npm with the bundle.

## Premise

- Read: the documents of each client for its CA file; `crates/pod-broker/README.md`, "Placeholder
  credentials"; call C811.
- Not measured: each client of the list through the TLS end.

## Approach

Add a leg to `crates/pod-broker/tests/tls_clients.rs` for each client that the host has (Go with
`SSL_CERT_FILE`, pip with a stand-in index, npm with a stand-in registry, Java, Deno, Hex), with a skip
and its reason where it is missing. For a client that reads none of the six variables, give the job the
variable that it reads in `ca::env` (a variable of the job only: `BUNDLE_VARS` is also the order in
which `pod-tls` reads a bundle), or a file in its format (a PKCS#12 trust store for the JVM), and a test
that fails without it.

## Prove

```sh
cargo test -p pod-broker --all-features --test tls_clients -- --nocapture
```
