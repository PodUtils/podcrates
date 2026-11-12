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
  request-target in absolute form"). `crates/pod-broker/README.md:176`
  lists that 400. They agree with the review.
- The code moved since `63348de`, the commit of the review (T-161 and
  others). Today `serve_one` is at `crates/pod-broker/src/serve.rs:104`,
  `next` at `:220`, `check` at `:252` and `connect` at `:309`; `exchange`
  is at `crates/pod-broker/src/forward.rs:71`; `Broker::serve_unix_job` is
  at `crates/pod-broker/src/broker.rs:194` and `serve_tcp_job` at `:218`;
  the advice of `bind_unix` (a directory of mode 0700) is at
  `crates/pod-broker/src/control.rs:124-127`. The steps agree with the
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
  (`crates/pod-broker/src/serve.rs:266`). The path rules of a policy thus
  apply in this mode with no TLS end; a CONNECT needs a TLS end for them
  (`PATH_NEEDS_TLS_END`).
- Found, not in the review: `crates/pod-broker/src/serve.rs:355` says "the far end failed".
  The broker answers each status of 500 or more with these fixed words.
  The review asks for a 502 with the words of the verifier in the body.
  That changes a rule of the crate.
- Found, not in the review: `head` writes the `Host` with no port for port
  80 only (`crates/pod-broker/src/forward.rs:54-57`). Toward TLS the
  default port is 443.
- Found, not in the review: `next` refuses two `Host` headers but accepts
  none for a target in absolute form (`crates/pod-broker/src/serve.rs:220-244`).
  This mode needs exactly one.
- Found, not in the review: an upstream proxy gets the name of the origin
  only with `Settings::names_upstream` (T-161,
  `crates/pod-broker/src/serve.rs:314-325`); else the broker dials a
  checked address (`:327-339`). The acceptance "the proxy sees the name"
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
  review): `crates/pod-broker/src/broker.rs:110-136`. `load` writes
  `broker.job.load` with `job`, `mode`, `rules`, `ttl_s` (and `flagged`,
  T-161), and no identity of the policy.
- Read: `crates/pod-broker/src/jobs.rs:68-75` (`Load` holds a `Policy`)
  and `:77-89` (`JobInfo`: `job`, `mode`, `rules`, `flagged`,
  `expires_in`, `open`).
- Read: `crates/pod-broker/src/control.rs:42-56`: `info`, the JSON of a
  job. The `load` answer is `info` with more keys (`:86-92`), and `list`
  maps `info` (`:101-104`). One new field of `JobInfo` thus reaches both
  answers. The control socket checks the user of its peer (`:166-173`).
- Read: `crates/pod-broker/src/serve.rs:62-73`: `Note::line` starts both
  connection lines. The deny line sets `decision` and `rule` at `:347`,
  the allow line at `:364`. A refusal before the lease (408 or 407,
  `:107-122`) has no job, thus no hash.
- Read: `crates/pod-broker/src/policy.rs:70-82`: `Policy` derives
  `Serialize` with `deny_unknown_fields`. An empty `allow`, `deny` or
  `ports` and a missing `path` are not written (`skip_serializing_if`), so
  an empty list and a missing list give one hash. The rule `allow[{n}]`
  comes from `:234-235`.
- Read: `crates/pod-broker/Cargo.toml:22` (`sha2` is a dependency);
  `crates/pod-broker/tests/audit.rs:16` (`no_audit_line_holds_a_planted_secret`
  reads the audit lines only); `schemas/log.v1.schema.json` (`fields` is an
  object, so a new key is compatible). They agree with the review.
- Found, not in the review: `Entry` has a field `hash` already, the
  SHA-256 of the token (`crates/pod-broker/src/jobs.rs:102`). Name the new
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
