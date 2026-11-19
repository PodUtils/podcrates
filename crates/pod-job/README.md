# pod-job

The records of the job of a CI runner (PodUtils/podrunner#17), as serde
types and as JSON Schemas:

| Record | Type | Schema |
| --- | --- | --- |
| `job-offer.v1`: a job that a source offers | `JobOffer` | `schemas/job-offer.v1.schema.json` |
| `unit-capability.v1`: what one unit kind of one host gives | `Capability` | `schemas/unit-capability.v1.schema.json` |
| `run-report.v1`: the end of one job, for the operator | `RunReport` | `schemas/run-report.v1.schema.json` |
| `run-report-public.v1`: the end of one job, for a source that is not `operator_controlled` | `PublicReport` | `schemas/run-report-public.v1.schema.json` |
| `status.v1`: what the supervisor of a host of podrunner holds now | `Status` | `schemas/status.v1.schema.json` |
| `source-request.v1`: a request of podrunner to a source of jobs | `source::Request` | `schemas/source-request.v1.schema.json` |
| `source-reply.v1`: the reply of a source to one request | `source::Reply` | `schemas/source-reply.v1.schema.json` |

The schemas come from the types (ruling Q-A8) and go out with each tag.
githome (GitHub #36) and cfb (GitHub #35) pin them. The crate reads no
environment, does no I/O and has no pod dependency. T-163 made it from
podrunner's `crates/podrunner-schema`.

## Use

```rust
let offer: pod_job::JobOffer = serde_json::from_slice(&bytes)?;
offer.check(source_is_scaleset)?; // the rules that serde does not read
let report = pod_job::RunReport { /* ... */ };
report.check()?;
let public = report.public(); // no host, no detail of the boundary
```

serde refuses an unknown field and an unknown value. Each `check` gives
the reason of a refusal as text; the consumer gives it its own class of
failure.

## API

| Item | What |
| --- | --- |
| `SCHEMA` | the version of each record, 1; each record has `"schema": 1` |
| `ARCHES` | `x86_64`, `aarch64` |
| `JobId`, `UnitId`, `UnitIds`, `IdError`, `JOB_ID_MAX` | the id of a job from a source (`[A-Za-z0-9._-]{1,128}`, not `.` or `..`, never a path), the id of a unit (`u-N`) and the counter that makes it |
| `Tier`, `RefusalCode` | the tier of a job; the refusal codes (`ALL`, `as_str`, `is_wait`) |
| `JobOffer`, `JobOffer::check(scaleset)` | the offer; `offer::{TrustInputs, Event, ActorClass, Visibility, UnitNeed, LIMITS, is_digest}`; `UnitNeed::container_engine`: the job needs a container engine inside the unit |
| `Capability`, `Capability::check`, `Capability::limit(name)` | the capability record; `Boundary`, `Principal`, `ContainerRoute` (`none`, `hooks`, `engine`, `unknown`), `SecretDelivery`, `CredentialStore`, `TreeKill`, `EgressHold`, `LimitMode`, `Fact` |
| `RunReport`, `RunReport::check`, `RunReport::public` | the full report; `cpu_s` and `peak_rss_mib` of the unit (`Option<Residual>`: a count, or `unknown`; absent when no unit ran; T-527, PodUtils/podrunner#86); `UnitFields`, `ExitReason` (`limit`: the unit ended the job over a bound of its limits), `Residual`, `report::Unknown`; `claim_to_first_step_s` and `job_images` (digests; `check` refuses a tag) are private: `public()` leaves them out |
| `PublicReport`, `PublicReport::check` | the public report, with `cpu_s` and `peak_rss_mib` (they name no host) |
| `Status`, `Status::check`, `SourceStatus`, `KindStatus`, `PinStatus` | the status of a host; `KindStatus::probe` is a `Capability`; `check` reads the version and the probe of each kind; `SourceStatus::draining`, `Status::max_units` and `Status::units` are optional (T-947) |
| `source::{Request, Reply, OPS, LINE_MAX}` | the source wire: a request (tag `op`) and a reply (tag `reply`) |
| `source::{parse_request, parse_reply, request_line, reply_line}` | read one line (each fault is `bad-request`, with no value of the line in its detail); write one line with `"schema": 1` first and its end of line |
| `source::{Event, Offer, RealJob, HostFacts, KindFacts}` | the events of `next` (tag `event`), the real job of `assigned`, the facts of a host in `hello` |
| `source::{SourceCode, SourceError}` | the refusal codes of a source (`ALL`, `as_str`) and a refusal |
| `source::{Minted, MintedFile, Hidden}` | the credential of the reply `credential`: `Hidden` prints no byte in `Debug` and is wiped at its drop |

## Features

| Feature | What |
| --- | --- |
| `schema` | `schemars::JsonSchema` for each type, for the tests of a consumer that put a type of this crate in a schema of its own. Turn it on in `[dev-dependencies]` only: no binary needs schemars. |

## The rules

serde reads the form of a record. `check` applies the rules below. The
schema of each record holds each rule too, except the first one, so a
reader in another language gets the verdict of the types:

* An offer of a `github-scaleset` source has no `payload`. The kind of a
  source is in the configuration of the runner, not in the offer: only
  `check(true)` holds this rule.
* `schema` is 1. In each schema it is the constant 1, and it is required.
* `arch` is `x86_64` or `aarch64`.
* An offer: each limit is one of `wall`, `cpu`, `memory`, `pids`, `disk`;
  each name of `limits_required` has a value in `limits`; `payload` is
  `sha256:` and 64 lower-case hex digits (an image by tag is refused);
  `policy` is an empty object (no policy) or a policy of
  `broker-policy.v1`. The schema of the offer refers to
  `schemas/broker-policy.v1.schema.json` by its `$id`, so a validator needs
  that file as a resource too. `JobOffer::check` reads the top level of
  the policy (its keys, `schema` 1 and `mode`); the consumer reads the
  rest with `pod_broker::Policy::from_value` and refuses a bad policy with
  `egress-policy-unsupported`.
* A report: a `refused` report has a `refusal_code` and no unit; a
  `refusal_code` needs `refused`; a `withdrawn` report has no unit. In the
  public report, `unit_kind` and `boundary` stand for the unit.
* An offer has no `tier` field (the runner computes the tier from
  `trust_inputs`) and no field for a credential or the environment: serde
  and the schema refuse each unknown field. A capability has no
  `boundary` `isolated` and no `secret_delivery` `env`.
* The public report has no `host`, and of the unit only its kind and the
  class of its boundary.

A pattern of a schema has no look-around, as Rust's regex and Go's RE2
take none. `src/schemas.rs` writes the schemas and checks each example
with the types and with a validator of JSON Schema (boon).

The `schema` field: each schema of `schemas/` has it, as the constant N
of the name `NAME.vN.schema.json` and the first required key: the four
records, the log line (`schemas/log.v1.schema.json`) and the broker
policy (`schemas/broker-policy.v1.schema.json`).

After a change of a type, write the schemas again and commit them:

```sh
POD_JOB_WRITE_SCHEMAS=1 cargo test -p pod-job schemas
```

## The source wire

One request on one line, one reply on one line, UTF-8 JSON with `\n` at
the end. Each object starts with `"schema": 1`; another value, no value, a
line over 1 MiB (`LINE_MAX`), an unknown `op`, an unknown field and a bad
`job_id` are `bad-request`. The two schemas are two files, one for each
direction: a source reads `source-request.v1` and writes
`source-reply.v1`, and podrunner the reverse. Each is a union on its own
tag (`op`, `reply`), and the id of each can change alone. In each branch of
a union, `schema` is the constant 1 and the first required key.

The operations, the order of one job, the events and the conformance
tests are the contract of podrunner (`docs/contracts/source-driver.md` of
PodUtils/podrunner); the trait `JobSource`, its conformance suite and the
pipe stay in podrunner.

The credential: `Reply::Credential` holds a `Minted` of this crate
(`files`, each `name` and `content`), not podrunner's `Credential`. A
source in another language needs the form in the schema, so the form is
here; the `content` is a `Hidden`, which prints no byte in `Debug` and is
wiped (zeroize) at its drop. The writer of the files (mode 0600) and the
check of a plain file name stay in podrunner (`secret.rs`), which converts
a `Minted` to its `Credential`.

## The examples

`tests/examples/RECORD/` holds 40 valid and 47 invalid examples, for the
seven records; `tests/examples/expect.json` gives the reason of each
invalid one. A source-request or source-reply example goes through
`parse_request` or `parse_reply`. They are test data of the crate, not
schemas: `schemas/` holds the `*.schema.json` files and `CHANGES.md`
(`docs/design.md`, section 2).

## The refusal codes

The runner refuses a job with one of these codes (PodUtils/podrunner#21).
The run report of a refused job has `refusal_code` and no unit fields. A
`wait` code holds the offer until a slot comes; at `wait_deadline_s` the
runner sends `withdraw`. A `terminal` code refuses the job at once, and
the runner suppresses the job id for `terminal_suppress_s` (3600 by
default). `src/codes.rs` has the same list; a test compares them.

| Code | Kind | Meaning |
| --- | --- | --- |
| `no-slot-of-kind` | wait | No free slot of a kind that meets the job. |
| `port-claimed` | wait | Reserved. |
| `image-stale` | terminal | The image of the payload is older than its age limit. |
| `event-not-allowed` | terminal | The tier rule gives no tier: the event, the repository or the actor is not allowed. |
| `tier-mismatch` | terminal | The computed tier is less trusted than the tier of the set. |
| `tier-needs-guest` | terminal | The tier needs boundary `guest`, and the host has no such kind. |
| `boundary-not-available` | terminal | No kind gives the effective boundary. |
| `limit-not-enforceable` | terminal | No kind enforces a required limit (`wall` needs a verified tree kill). |
| `hold-not-available` | terminal | No kind gives the hold of the tier. |
| `egress-policy-unsupported` | terminal | The runner cannot apply the policy of the offer under the tier ceiling. |
| `arch-mismatch` | terminal | No kind of the host runs the architecture of the job. |
| `payload-not-pinned` | terminal | The payload, or the container image of a hook call, is not `NAME@sha256:HEX` of the pin list. |
| `repo-scope-refused` | terminal | A repository of a personal account (podrunner's call C3), or a repository other than the one of the host's configuration (podrunner's DR-4 mitigation). |
| `container-not-supported` | terminal | The job needs a container, and no kind has the container route; a hook call before `prepare_job`; services on a unit of the host network. |
| `container-option-refused` | terminal | A container option that the kind refuses, or that a rule of the hook checks refuses. |
| `build-not-supported` | terminal | A `docker build` or a Dockerfile action (podrunner's call C2). |
| `source-revoked` | terminal | The source revoked the credential of the runner. |
| `credential-unavailable` | wait | The source cannot mint now. |
| `unit-start-failed` | terminal | The unit did not start. |

## Parity

Against podrunner `crates/podrunner-schema` at `9ab7fd3`:

| podrunner | Here |
| --- | --- |
| [x] `ids.rs`: `JobId`, `UnitId`, `UnitIds`, `IdError`, `JOB_ID_MAX`, and their tests | `ids.rs`; the pattern of a job id has no look-around |
| [x] `codes.rs`: `Tier`, `RefusalCode`, and their tests | `codes.rs`; `the_table_of_codes_is_the_enum` reads the table above, not `docs/contracts/codes.md` |
| [x] `capability.rs`: the capability record and its tests | `capability.rs` |
| [x] `offer.rs`: the offer, `LIMITS`, `is_digest` | `offer.rs` |
| [x] `report.rs`: the reports, `RunReport::check`, `RunReport::public` | `report.rs`, with `PublicReport::check` (new) |
| [x] `schemas.rs`: the three tests, each with its name | `schemas.rs`; the examples go through the schemas too; the tests of T-163 are in `schemas/rules.rs` |
| [x] `schemas/{job-offer,unit-capability,run-report,run-report-public}.v1.json` | `schemas/NAME.v1.schema.json` of podcrates |
| [x] `schemas/examples/` | `tests/examples/` |
| [x] `docs/contracts/codes.md` | the table "The refusal codes" above |
| [ ] `secret.rs`: `Credential`, `CredentialFile`, `Secret` | stays in podrunner: no record of this crate holds a credential |
| [x] `schemas/status.v1.json` (at `ca8c308`) | `schemas/status.v1.schema.json` |
| [x] `podrunner-slots/src/status.rs`: `Status`, `SourceStatus`, `KindStatus`, `PinStatus` (at `ca8c308`) | `status.rs`, with `Status::check` (new); `Slots::status`, `key_age_s` and their tests stay in podrunner; `the_status_schema_is_generated_from_the_type` is `the_schemas_are_generated_from_the_types` here |
| [x] `podrunner-source/src/wire.rs` (at `ca8c308`) | `source/mod.rs`; its test `an_unknown_field_is_refused` in `source/tests.rs` |
| [x] `podrunner-source/src/event.rs`: `Offer`, `Event`, `RealJob` | `source/event.rs`; `Assigned` (the answer of `JobSource::assigned`) stays in podrunner |
| [x] `podrunner-source/src/host.rs`: `HostFacts`, `KindFacts` | `source/host.rs` |
| [x] `podrunner-source/src/lib.rs`: `SourceCode`, `SourceError` | `source/code.rs`, with `SourceCode::ALL` (new); `JobSource`, `Hello` stay in podrunner |
| [x] `podrunner-source/src/tests.rs`: `a_request_round_trips_with_its_schema`, `bad_lines_are_bad_requests_and_name_no_value` | `source/tests.rs`, each with its name; the tests of the pipe and of the conformance suite stay in podrunner |
| [ ] `secret.rs` as the credential of the wire | `source/minted.rs`: `Minted`, `MintedFile`, `Hidden` (see "The source wire") |
| [x] No fuzz target in podrunner | the target `job_offer` in `fuzz/` |

Differences from the source, each with a test:

* Each schema holds `schema` as the constant 1, `arch` as an enum, the
  names of the limits, the form of the payload, the rule of the required
  limits and the rules of a report. The draft took `schema: 2`, an
  unknown arch, an image by tag and each rule of a report
  (`each_example_passes_or_fails_for_its_reason`).
* The pattern of a job id had a look-around, which Rust's regex and Go's
  RE2 refuse (`the_job_id_pattern_is_the_rule_of_the_type`).
* `PublicReport::check`: the version and the rules of a report. The
  draft read a public report with no check, so it took `schema: 2`.
* The `$id` of each schema is its URL in podcrates.
* `Reply::Done` is `Done {}`: serde takes any field of a unit variant of a
  tagged enum, so `{"schema":1,"reply":"done","x":1}` passed
  (`an_unknown_field_is_refused`). The wire form is the same.
* `status.v1`: `schema` is the constant 1, an unknown field is refused, and
  the types read a status too (`Deserialize`), so githome can read one.
* `Minted` and `MintedFile` refuse an unknown field, as each object of the
  wire does; `Hidden` is wiped at its drop.

## Versions of a record

githome and cfb read these records (for example `cpu_s` and
`peak_rss_mib` of `run-report.v1`). A change of a field never changes a
schema id silently (the owner's ruling "Schema ids", 2026-11-18):

* A field that changes its type, name, unit or meaning, a removed field
  and a new required field give a new schema id (`NAME.v2`, with
  `schema` as the constant 2) and a COORD issue `(to all)`.
* An added optional field keeps the id. The hand-off COORD issue of its
  tag names it.
* `schemas/CHANGES.md` has one row for each change of a schema (the tag,
  the schema, the field, the kind of change, the SHA-256 of the file).
  `scripts/check-schemas.py` (CI) fails on a schema file that changes with
  no row, on a field added with no row `added`, and on a change of the
  first kind that keeps the old id.

The optional fields of `v0.71.0` (T-923; GitHub #223 G-16, G-18 to G-20,
G-36; the names of PodUtils/podrunner#294):

* `job-offer.v1`: `trust_inputs.forge` (the host name of the forge, in
  lower case); `cache` (`key`, `mode`: `rw`, `ro` or `none`); `sandbox`
  (`image` as `NAME@sha256:HEX`, `command` with one word or more,
  `idle_s`, `max_s`). The existing `payload` keeps its meaning (a digest),
  so the sandbox is its own field (call C783). `limits.disk` (bytes) was
  there.
* `run-report.v1`: `arch` (`x86_64` or `aarch64`), `assigned` (the real
  job that the runner got: `run_id`, `repository`, `job_name`, `verdict`
  `keep` or `end`, `code`), `image_digest` (`sha256:HEX`), `runner_version`,
  `prepare_s`, `start_s`, `run_s`, `destroy_s` (seconds, rounded up),
  `disk_peak_bytes`, `net_rx_bytes`, `net_tx_bytes`, `cache_read_bytes`,
  `cache_written_bytes` (a count or `unknown`), `unknown_reasons` (for
  each measured field that is `unknown`, why; a key must name such a
  field), `log_tail` (UTF-8, at most 65536 bytes, scrubbed by the writer).
  `host`, `unit.unit_kind` and `unit.boundary` were there. None of them is
  in the public report.

## Adoption map

| Consumer | Old | New |
| --- | --- | --- |
| podrunner | `podrunner-schema = { path = "crates/podrunner-schema" }` | `pod-job = { git = "https://github.com/PodUtils/podcrates", tag = "vX.Y.Z" }` |
| podrunner | `podrunner_schema::{JobId, UnitId, UnitIds, IdError, SCHEMA, Tier, RefusalCode}` | `pod_job::` with the same names |
| podrunner | `podrunner_schema::{Capability, Boundary, Principal, ContainerRoute, SecretDelivery, CredentialStore, TreeKill, EgressHold, LimitMode, Fact}` | `pod_job::` with the same names |
| podrunner | `podrunner_schema::{JobOffer, offer::*}` | `pod_job::{JobOffer, offer::*}` |
| podrunner | `podrunner_schema::{RunReport, PublicReport, UnitFields, ExitReason, Residual, report::Unknown}` | `pod_job::` with the same names |
| podrunner | `podrunner_schema::{Credential, CredentialFile, Secret}` | stay in podrunner (a crate or a module of its own) |
| podrunner | `podrunner-schema = { features = ["schema"] }` in `[dev-dependencies]` (`podrunner-slots`) | `pod-job = { ..., features = ["schema"] }` in `[dev-dependencies]` |
| podrunner | `schemas/{job-offer,unit-capability,run-report,run-report-public}.v1.json`, `schemas/examples/`, `PODRUNNER_WRITE_SCHEMAS` | removed: podcrates `schemas/NAME.v1.schema.json` at the tag, `crates/pod-job/tests/examples/`, `POD_JOB_WRITE_SCHEMAS` |
| podrunner | `docs/contracts/codes.md` | a link to "The refusal codes" of this README; its test `the_table_of_codes_is_the_enum` moves here |
| podrunner | `podrunner_slots::status::{Status, SourceStatus, KindStatus, PinStatus}` | `pod_job::` with the same names; `Slots::status` fills them |
| podrunner | `schemas/status.v1.json`, the test `the_status_schema_is_generated_from_the_type` | removed: `schemas/status.v1.schema.json` of podcrates |
| podrunner | `podrunner_source::wire::{Request, Reply, OPS, LINE_MAX, parse_request, parse_reply, request_line, reply_line}` | `pod_job::source::` with the same names; `Reply::Done` is `Reply::Done {}` |
| podrunner | `podrunner_source::{Event, Offer, RealJob, HostFacts, KindFacts, SourceCode, SourceError}` | `pod_job::source::` with the same names (podrunner-source can re-export them) |
| podrunner | `Reply::Credential { credential: Credential }` | `Reply::Credential { credential: pod_job::source::Minted }`; a conversion between `Minted` and `Credential` in podrunner |
| podrunner | the tests `an_unknown_field_is_refused`, `a_request_round_trips_with_its_schema`, `bad_lines_are_bad_requests_and_name_no_value` | removed: they are in `pod-job` |
| podrunner | `CHANGES.md` lines of the wire and of `status.v1` | `schemas/CHANGES.md` of podcrates (`source-request.v1`, `source-reply.v1`, `status.v1`) |

## Tests

`cargo test -p pod-job`. The fuzz target `job_offer` reads an offer from
untrusted bytes, checks it and encodes it again. The source wire has no
fuzz target yet (T-940).

## Sources

`SOURCES` lists each imported file with its blob: podrunner at `9ab7fd3`
(the four records), and at `d001608` (`status.v1` and the source wire,
imported at `ca8c308`), with the `taken` line of each block.
