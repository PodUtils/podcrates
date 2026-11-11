# The refusal codes, version 1

podrunner refuses a job with one of these codes (GitHub #21). The run
report of a refused job has `refusal_code` and no unit fields. A `wait`
code holds the offer until a slot comes; at `wait_deadline_s` podrunner
sends `withdraw`. A `terminal` code refuses the job at once, and podrunner
suppresses the job id for `terminal_suppress_s` (3600 by default).
`crates/podrunner-schema/src/codes.rs` has the same list; a test compares
them.

| Code | Kind | Meaning |
| --- | --- | --- |
| `no-slot-of-kind` | wait | No free slot of a kind that meets the job. |
| `port-claimed` | wait | Reserved. |
| `image-stale` | terminal | The image of the payload is older than its age limit. |
| `event-not-allowed` | terminal | The tier rule gives no tier: the event, the repository or the actor is not allowed. |
| `tier-mismatch` | terminal | The computed tier is less trusted than the tier of the set. |
| `tier-needs-guest` | terminal | The tier needs boundary `guest`, and the host has no such kind. |
| `boundary-not-available` | terminal | No kind gives the effective boundary. |
| `limit-not-enforceable` | terminal | A required limit is not enforced by any kind (`wall` needs a verified tree kill). |
| `hold-not-available` | terminal | No kind gives the hold of the tier. |
| `egress-policy-unsupported` | terminal | The policy of the offer cannot be applied under the tier ceiling. |
| `arch-mismatch` | terminal | No kind of the host runs the architecture of the job. |
| `payload-not-pinned` | terminal | The payload is not a digest of the pin list. |
| `repo-scope-refused` | terminal | A repository of a personal account (call C3). |
| `container-not-supported` | terminal | The job needs a container, and no kind has the container route. |
| `container-option-refused` | terminal | A container option that the kind refuses. |
| `build-not-supported` | terminal | A `docker build` or a Dockerfile action (call C2). |
| `source-revoked` | terminal | The source revoked the credential of podrunner. |
| `credential-unavailable` | wait | The source cannot mint now. |
| `unit-start-failed` | terminal | The unit did not start. |
