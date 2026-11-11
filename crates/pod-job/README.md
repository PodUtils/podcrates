# pod-job

The records of the job of a CI runner (PodUtils/podrunner#17), as serde
types and as JSON Schemas:

| Record | Type | Schema |
| --- | --- | --- |
| `job-offer.v1`: a job that a source offers | `JobOffer` | `schemas/job-offer.v1.schema.json` |
| `unit-capability.v1`: what one unit kind of one host gives | `Capability` | `schemas/unit-capability.v1.schema.json` |
| `run-report.v1`: the end of one job, for the operator | `RunReport` | `schemas/run-report.v1.schema.json` |
| `run-report-public.v1`: the end of one job, for a source that is not `operator_controlled` | `PublicReport` | `schemas/run-report-public.v1.schema.json` |

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
| `JobOffer`, `JobOffer::check(scaleset)` | the offer; `offer::{TrustInputs, Event, ActorClass, Visibility, UnitNeed, LIMITS, is_digest}` |
| `Capability`, `Capability::check`, `Capability::limit(name)` | the capability record; `Boundary`, `Principal`, `ContainerRoute`, `SecretDelivery`, `CredentialStore`, `TreeKill`, `EgressHold`, `LimitMode`, `Fact` |
| `RunReport`, `RunReport::check`, `RunReport::public` | the full report; `UnitFields`, `ExitReason`, `Residual`, `report::Unknown` |
| `PublicReport`, `PublicReport::check` | the public report |

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
  `sha256:` and 64 lower-case hex digits (an image by tag is refused).
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

## The examples

`tests/examples/RECORD/` holds 11 valid and 19 invalid examples (podrunner's
draft); `tests/examples/expect.json` gives the reason of each invalid one.
They are test data of the crate, not schemas: `schemas/` holds the
`*.schema.json` files only (`docs/design.md`, section 2).

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
| `payload-not-pinned` | terminal | The payload is not a digest of the pin list. |
| `repo-scope-refused` | terminal | A repository of a personal account (podrunner's call C3). |
| `container-not-supported` | terminal | The job needs a container, and no kind has the container route. |
| `container-option-refused` | terminal | A container option that the kind refuses. |
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
| [ ] `schemas/status.v1.json` | stays in podrunner: the status of a host of podrunner |
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

## Tests

`cargo test -p pod-job`. The fuzz target `job_offer` reads an offer from
untrusted bytes, checks it and encodes it again.

## Sources

`SOURCES` lists each imported file with its blob: podrunner at `9ab7fd3`.
