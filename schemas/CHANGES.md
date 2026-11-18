# The changes of the schemas

One row for each change of a schema of `schemas/`. A reader that pins a
schema by its SHA-256 finds here the hash of each tag and each added
field. `scripts/check-schemas.py` (CI) checks the rows against the files.

* **Tag**: the first tag that holds the change, or `next` when no tag holds
  it yet. The release sets `next` to its tag (`docs/maintenance.md`, "A
  release").
* **Schema**: `NAME.vN`, the file `schemas/NAME.vN.schema.json`.
* **Field**: the dotted path of the field; a branch of a tagged union is a
  part of the path (`hello.host`); `*` is the whole schema.
* **Change**: `new` (a new schema id), `added` (an optional field, or a
  value of a field), `text` (no change of a field: words, the generator),
  `changed`, `removed`, `required` (the changes that need a new schema
  id), `withdrawn` (the file is removed).
* **SHA-256**: of the file at the tag; for `next`, of the file now.

The rules (the ruling "Schema ids", `crates/pod-job/README.md`, "Versions
of a record"):

1. A change of a schema file comes with a row in the same commit. The rows
   of one schema and one tag have one hash, the hash of the file at that
   tag.
2. A changed type, unit or meaning of a field, a removed field and a new
   required field get a new schema id (`NAME.v2`), with a row `new`, and a
   COORD issue `(to all)`. The old id keeps its file.
3. An added optional field keeps the id, with a row `added`. The hand-off
   COORD issue of its tag names it.

A row of a tag before the ruling "Schema ids" (v0.18.0) can show a change
that kept its id.

| Tag | Schema | Field | Change | What | SHA-256 |
| --- | --- | --- | --- | --- | --- |
| v0.8.0 | `broker-policy.v1` | `*` | new | The policy of a job of the egress broker, with the field `version` (T-099). | bdbcae9d106c9080fdaab5a7502aad74cadf26530e1cb75960306ecd2d294139 |
| v0.8.0 | `log.v1` | `*` | new | One JSON log line of a tool (T-101). | 051c577f927b43804bfd6d3d7ae084acad9617c98d74e96414adce96084a5f19 |
| v0.17.0 | `job-offer.v1` | `*` | new | The job offer of a source (T-163, from podrunner). | 97b61be8ae9ba292d87ada330eabdbe93a1472a0f91398d8255aec83a0b8fcd5 |
| v0.17.0 | `run-report-public.v1` | `*` | new | The end of one job for a source that is not `operator_controlled` (T-163). | df77c2139e34598a314a44e5a9fe54bd33f9c5cc93a388d9cf354234609edbdd |
| v0.17.0 | `run-report.v1` | `*` | new | The end of one job for the operator (T-163). | e2b6f139e0de843cdb1986f5d521058b6ca78736d8bf6f0382c29a8b35841898 |
| v0.17.0 | `unit-capability.v1` | `*` | new | What one unit kind of one host gives (T-163). | e6baab91c340569a6607e1a84f0c3cf9e63681c20078dda6f377055d1a87ad50 |
| v0.18.0 | `broker-policy.v1` | `version` | removed | The version is the field `schema` (T-177). | 2f5949c063c3021372857005a241fce0e970aea762c91393f606d0cba783fcf2 |
| v0.18.0 | `broker-policy.v1` | `schema` | required | The constant 1, the first key (T-177). | 2f5949c063c3021372857005a241fce0e970aea762c91393f606d0cba783fcf2 |
| v0.18.0 | `job-offer.v1` | `policy` | changed | An empty object or a policy of `broker-policy.v1` (T-178). | 9433afeb8856be61491927a28bb43d6554dc7484458e0395b3ca39a7cde146f5 |
| v0.18.0 | `log.v1` | `schema` | required | The constant 1, the first key (T-177). | d1b4b66317abbe13f21263dc1debe579375e80c73cd68f0d65a2c68d387f8502 |
| v0.26.0 | `run-report-public.v1` | `exit_reason` | added | The value `limit` (T-226). | 573f6cff7fa88594b75fb913abd578fc4b4cc21f768a34851a7feebace02487d |
| v0.26.0 | `run-report.v1` | `claim_to_first_step_s` | added | Seconds from the claim to the first step; private (T-226). | 05e4ab5895a08b8135c45ab5bb58d8087261e485836ada43b788ae0b95d90ba6 |
| v0.26.0 | `run-report.v1` | `job_images` | added | The digests of the images of the job; private (T-226). | 05e4ab5895a08b8135c45ab5bb58d8087261e485836ada43b788ae0b95d90ba6 |
| v0.26.0 | `run-report.v1` | `exit_reason` | added | The value `limit` (T-226). | 05e4ab5895a08b8135c45ab5bb58d8087261e485836ada43b788ae0b95d90ba6 |
| v0.38.0 | `job-offer.v1` | `trust_inputs.actor_class` | added | The values `agent` and `bot` (T-505). | 0ded4a8bc72b0adde6cc5bf089792bc86062a96236af32363b9741b13e0468d1 |
| v0.40.0 | `run-report-public.v1` | `cpu_s` | added | The CPU time of the unit: a count or `unknown` (T-527). | 7938d9ed348e7b136c53621c5309221746c5e7a80228ce0d503ec245182a4eea |
| v0.40.0 | `run-report-public.v1` | `peak_rss_mib` | added | The peak memory of the unit: a count or `unknown` (T-527). | 7938d9ed348e7b136c53621c5309221746c5e7a80228ce0d503ec245182a4eea |
| v0.40.0 | `run-report.v1` | `cpu_s` | added | The CPU time of the unit: a count or `unknown` (T-527). | 5f01a96a0ac2b6b373824b98b4031bd67ac0b602cdb01c5e5edfe4ece4e9ce3d |
| v0.40.0 | `run-report.v1` | `peak_rss_mib` | added | The peak memory of the unit: a count or `unknown` (T-527). | 5f01a96a0ac2b6b373824b98b4031bd67ac0b602cdb01c5e5edfe4ece4e9ce3d |
| v0.42.0 | `hostprobe-report.v1` | `*` | new | The report of pod-hostprobe (T-107). | 6044f5bfd753d2994430a34545434b8ecf2436080182f855a2e98e91538e60ea |
| v0.42.0 | `job-offer.v1` | `unit_need.container_engine` | added | The job needs a container engine inside the unit (T-553). | bde159f4eb118c0f1c71cf2dead47a08fa76df0d662236b58088918d154e46b4 |
| v0.42.0 | `unit-capability.v1` | `container` | added | The value `engine` (T-553). | fa28669044f4c85ac56e08b4fe8d51b5a552e29b8aa8df4c7a05a2ed31b95a8b |
| v0.52.0 | `hostprobe-report.v1` | `findings.rows.name` | added | The row `mount(tmpfs,/tmp) in unshare(NEWUSER+maps,NEWNS)` (T-670). | cdd37ab17bb035047f806acc54582e11fd1740a117e69d4a5202ba59e339e50a |
| v0.59.0 | `hostprobe-report.v1` | `findings.rows.name` | added | The row `chroot(/tmp) in unshare(NEWUSER+maps)` (T-744). | ad72e1bf4abadc5ecc8525ebf4d7a1d2cbb4b102c5a4bb6a0927060797f512cf |
| v0.60.0 | `hostprobe-report.v1` | `findings.rows.name` | added | 20 rows of the host classes of podssh and podnet (T-714, T-715, T-745). | b69b45f344a9e62af1721eeb231e3b7d6084be1583046fc25cd79f1944bf4ccf |
| v0.61.0 | `hostprobe-report.v1` | `findings.rows.name` | added | 9 rows that send a packet out (T-751). | ec59f6207da7a3017f489f7b48ae391e13e17b40d66b76354acb7ff1d33c90d9 |
| v0.70.0 | `hostprobe-report.v1` | `findings.rows.name` | added | The rows of a clone with new user and network namespaces, and `process_vm_writev(own pid)` (T-866). | 681b17f1f4ee445a9e60e5bad23fd994dfb40ae8efe14028a9ddec4d4e9939f4 |
| v0.71.0 | `job-offer.v1` | `cache` | added | `key` and `mode` (`rw`, `ro`, `none`) (T-923). | 83faf2bcc5d71eece4950a63e5f5257398751ef13a9c602aa8c08899790575d5 |
| v0.71.0 | `job-offer.v1` | `sandbox` | added | `image` by digest, `command`, `idle_s`, `max_s` (T-923, call C783). | 83faf2bcc5d71eece4950a63e5f5257398751ef13a9c602aa8c08899790575d5 |
| v0.71.0 | `job-offer.v1` | `trust_inputs.forge` | added | The host name of the forge, in lower case (T-923). | 83faf2bcc5d71eece4950a63e5f5257398751ef13a9c602aa8c08899790575d5 |
| v0.71.0 | `run-report.v1` | `arch` | added | `x86_64` or `aarch64` (T-923). | 325ec2a9929af72b0e761beb97af07a69dd4ab43abdf190588077d072608bb8d |
| v0.71.0 | `run-report.v1` | `assigned` | added | The real job that the runner got, and the verdict `keep` or `end` (T-923). | 325ec2a9929af72b0e761beb97af07a69dd4ab43abdf190588077d072608bb8d |
| v0.71.0 | `run-report.v1` | `cache_read_bytes` | added | A count or `unknown` (T-923). | 325ec2a9929af72b0e761beb97af07a69dd4ab43abdf190588077d072608bb8d |
| v0.71.0 | `run-report.v1` | `cache_written_bytes` | added | A count or `unknown` (T-923). | 325ec2a9929af72b0e761beb97af07a69dd4ab43abdf190588077d072608bb8d |
| v0.71.0 | `run-report.v1` | `destroy_s` | added | Seconds, rounded up (T-923). | 325ec2a9929af72b0e761beb97af07a69dd4ab43abdf190588077d072608bb8d |
| v0.71.0 | `run-report.v1` | `disk_peak_bytes` | added | A count or `unknown` (T-923). | 325ec2a9929af72b0e761beb97af07a69dd4ab43abdf190588077d072608bb8d |
| v0.71.0 | `run-report.v1` | `image_digest` | added | `sha256:HEX` (T-923). | 325ec2a9929af72b0e761beb97af07a69dd4ab43abdf190588077d072608bb8d |
| v0.71.0 | `run-report.v1` | `log_tail` | added | UTF-8, at most 65536 bytes, scrubbed by the writer (T-923). | 325ec2a9929af72b0e761beb97af07a69dd4ab43abdf190588077d072608bb8d |
| v0.71.0 | `run-report.v1` | `net_rx_bytes` | added | A count or `unknown` (T-923). | 325ec2a9929af72b0e761beb97af07a69dd4ab43abdf190588077d072608bb8d |
| v0.71.0 | `run-report.v1` | `net_tx_bytes` | added | A count or `unknown` (T-923). | 325ec2a9929af72b0e761beb97af07a69dd4ab43abdf190588077d072608bb8d |
| v0.71.0 | `run-report.v1` | `prepare_s` | added | Seconds, rounded up (T-923). | 325ec2a9929af72b0e761beb97af07a69dd4ab43abdf190588077d072608bb8d |
| v0.71.0 | `run-report.v1` | `run_s` | added | Seconds, rounded up (T-923). | 325ec2a9929af72b0e761beb97af07a69dd4ab43abdf190588077d072608bb8d |
| v0.71.0 | `run-report.v1` | `runner_version` | added | The version of the runner (T-923). | 325ec2a9929af72b0e761beb97af07a69dd4ab43abdf190588077d072608bb8d |
| v0.71.0 | `run-report.v1` | `start_s` | added | Seconds, rounded up (T-923). | 325ec2a9929af72b0e761beb97af07a69dd4ab43abdf190588077d072608bb8d |
| v0.71.0 | `run-report.v1` | `unknown_reasons` | added | Why each measured field is `unknown` (T-923, call C783). | 325ec2a9929af72b0e761beb97af07a69dd4ab43abdf190588077d072608bb8d |
| v0.72.0 | `broker-policy.v1` | `*` | text | The file comes from the type `Policy` of pod-broker; the words come from its doc comments (T-922). | fe30116083fa2b22f4fd1b228064ebbed23242ea482ef5c9ac7d3c5b2dc6c31c |
| v0.72.0 | `log.v1` | `*` | text | The file comes from the audit `Line` of pod-broker; the words come from its doc comments (T-922). | 4c4ac30ca0f843883d76f1af409793318a971010f9e3cccf4f9d128bd7c650ef |
| v0.72.0 | `source-reply.v1` | `*` | new | The reply of a source of jobs to podrunner: `hello`, `events`, `credential`, `keep`, `end`, `done`, `error`; from podrunner's source wire, version 1, at `ca8c308` (T-922). `done` refuses an unknown field. | d4e434e6f3cd8cd2ac966abf2eb153ee6483e3877f64da3bd3daeaf0b611ddae |
| v0.72.0 | `source-request.v1` | `*` | new | A request of podrunner to a source of jobs: `hello` (with `host`), `next`, `ack`, `claim`, `mint`, `report`, `audit`, `withdraw`, `release`, `assigned`; from podrunner's source wire, version 1, at `ca8c308` (T-922). | a446eef6c18081f46fb3fd3f6a4b81f4411df74853ca69b7aa70000a5adad35b |
| v0.72.0 | `status.v1` | `*` | new | The status of a host of podrunner, from its `status.v1` at `ca8c308` (T-922); `schema` is the constant 1, and an unknown field is refused. | 0d2eb66aead37ab03ad70f1a81367371ec161a949a480d97d0ac23eee4fcc3c9 |
| next | `status.v1` | `sources.draining` | added | An operator drained the source: it takes no new offer; `false` when it is missing (T-947, PodUtils/podrunner#305). | 91bcc68be2584495a4928e1a4a0a9d390cc6d5967130c2ae31ee608419694fcc |
| next | `status.v1` | `max_units` | added | The most units of the host across its kinds; `null` or missing is no cap (T-947). | 91bcc68be2584495a4928e1a4a0a9d390cc6d5967130c2ae31ee608419694fcc |
| next | `status.v1` | `units` | added | The units that run on the host now; `0` when it is missing (T-947). | 91bcc68be2584495a4928e1a4a0a9d390cc6d5967130c2ae31ee608419694fcc |
