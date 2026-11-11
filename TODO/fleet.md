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

- Read, at `5cedb23`: `crates/pod-broker/src/serve.rs:177` is `lease`. It takes the job from `Proxy-Authorization` (`Bearer`, or Basic with the token as the password) and refuses with 407 when the header is missing. The issue cites lines 155 to 175. The function runs from line 158 to line 182. The call is at `crates/pod-broker/src/serve.rs:119`.
- Read: `crates/pod-broker/src/broker.rs:157` (`serve_tcp`) and `crates/pod-broker/src/broker.rs:173` (`serve_unix`) each serve every job. No call binds one listener to one job. `Broker::drop_job` (`crates/pod-broker/src/broker.rs:140`) ends the connections of a job.
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

- Read: `crates/pod-broker/src/broker.rs:280` (`upstream` takes one `Option<pod_proxy::Proxy>` and dials the checked address) and `crates/pod-broker/src/serve.rs:309` (`connect` dials `Target::Addr`). The issue cites `serve.rs:273-287` and `broker.rs:183-191`. The code agrees to within a few lines.
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
dockers"); PodUtils/podvm#243 (PodUtils/podvm@T-1717, PodUtils/podvm@T-1718).
**Category:** feature
**Milestone:** M7
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

podvm gets incus as a back end or an adapter (the owner's ask of DR-5).
podvm studies it first (PodUtils/podvm@T-1717: `incusd` as root of a user
namespace, the cost against runc, a static `incusd`, the map of the docker
verbs), then builds a back end over the REST API of incus on its Unix
socket (PodUtils/podvm@T-1718). If the client of that API is generic, it
belongs in podcrates as `pod-incus`, and podrunner can use it as a unit
kind.

## Premise

- Read: PodUtils/podvm#243 (no ask of a crate now).
- Not read: the REST API of incus; the owner's study `errand-box`.

## Approach

Wait for podvm's COORD issue after PodUtils/podvm@T-1717. Then: a client of the incus
REST API over a Unix socket (instances, images, exec, files, operations
and their events), static, with no `lxc` or `incus` binary; a stand-in
server in `pod-testkit` for the tests; the refusal by name where no incus
answers.

## Prove

```sh
cargo test -p pod-incus --all-features
```

## Start condition

podvm's COORD issue `(to podcrates)` after PodUtils/podvm@T-1717 asks for
the crate.

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
