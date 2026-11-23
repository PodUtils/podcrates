# pod-harness

The verification harness of PodUtils/podnet#501. It measures how visible a
road is to a middlebox. Three classifiers read one capture, in increasing
cost. A matrix holds one row for each road and each deployment profile. A
zip makes one run relocatable.

The harness is deterministic and offline. No test uses the network.

## Rules

* **The result is a table.** `Table::render` prints the road, the profile,
  the classifier, the score and the date. `Table::json` prints the same
  rows. The harness prints no pass count. A pass count hides a row that
  never ran.
* **A row that did not run is `skip`.** It stays in its place with a named
  reason. It is never omitted and never counted as a pass.
* **The budget is 600 s.** The test
  `the_full_matrix_reports_a_table_with_each_road_and_profile` measures a
  run of the 33 fixture rows and fails when it passes 600 s. The CI job
  runs the test and the binary.
* **The benign cover site is measured.** The table prints the
  false-positive rate of the cover-site sample, and the test requires a
  rate of 0.1 or less.

A score is in 0..1. A higher score means that the road looks more like the
traffic of the model. The score is a measurement of this fixture, not a
label of the road. The centroids of `flow.json` and the rules of
`rules.json` are the reference measurement of the fixtures in this crate.
A different capture gives a different distance and a different score.

## The classifiers

| Classifier | Input | Model | Cost |
| --- | --- | --- | --- |
| `field::FieldMatcher` | SNI, ALPN, JA3, JA4, TLS record sizes | `rules.json` | one parse of the ClientHello |
| `flow::FlowStats` | size mean, standard deviation, p10, p50, p90, inter-arrival histogram | `flow.json` | one pass over the packets |
| `prefix::Prefix` | the first N bytes, as byte n-grams | `prefix.json` | one pass over the prefix |

Each classifier returns a score and a named reason. `FieldMatcher` names
each rule that matched. `FlowStats` names the distance to the tunnel and
the benign centroid. `Prefix` names the labels of the grams that matched.

JA3 is the MD5 of its specification string. JA4 is the specification
fingerprint: the readable part, then the truncated SHA-256 of the sorted
ciphers, then the truncated SHA-256 of the sorted extensions and the
signature algorithms. The test `the_published_ja4_example_is_reproduced`
reads the published JA4 example. The JA3 test names its exact input string
and its MD5, computed with `md5sum` from the specification. A GREASE value
is never counted and never hashed.

## A run

```sh
cargo run -p pod-harness -- --data crates/pod-harness/fixtures
cargo run -p pod-harness -- --data crates/pod-harness/fixtures --json
```

The binary prints the table to stdout and the elapsed time to stderr. A
usage error exits 64. A model or fixture that cannot be read exits 78. A
run over its budget exits 69.

The fixture files:

| File | What |
| --- | --- |
| `fixtures/rules.json` | the field rules and their weights |
| `fixtures/flow.json` | the tunnel and benign centroids |
| `fixtures/prefix.json` | the byte n-gram table |
| `fixtures/cases.json` | the road, the profile, the capture, the faults or the skip |
| `fixtures/captures/*.cap` | the captures |

## The archive

`Archive::build` makes one zip. The zip holds the fixtures, the models, the
middlebox simulator source and its decisions, one expected score file for
each row, the expected table, and `run.sh`. It holds no repository path and
no toolchain.

```sh
sh run.sh /path/to/pod-harness
sh run.sh --list
sh run.sh
```

The first form runs a prebuilt binary that the caller gives. The second
prints the manifest with the size and the SHA-256 of each file. The third
prints a named skip and the manifest. It does not report a pass. No binary
is in the zip, so a host without a `pod-harness` binary gets the skip.

The test `the_archive_unzips_and_its_manifest_lists_each_file` unzips the
archive into a temporary directory and checks each file against the
manifest. `run_sh_lists_the_manifest_and_names_a_skip_with_no_binary` runs
`run.sh` with no binary. `each_row_has_an_expected_score_file` checks the
expected file of every row, including a skipped row.

## The capture format

A capture is UTF-8 text. The first line that is not a comment is
`pod-capture/1`. Each later line is one packet: the direction (`c2s` or
`s2c`), the timestamp in microseconds and the bytes in hexadecimal.

```text
pod-capture/1
# one packet from the client, then one from the server
c2s 0 1603010040
s2c 1250 1603030020
```

A line that starts with `#` is a comment. `Capture::load` and
`Capture::save` read and write the format. The middlebox simulator writes
the same format, so its output loads as a capture.

## The middlebox simulator

`Middlebox` applies a local fault model with no raw socket and no network
namespace. `pod-sidecar` is not in this worktree, so the model is this
crate's own. The faults are `pad`, `chunk`, `delay` and `drop`. Each
decision records the packet, the fault and what changed. `Middlebox::apply`
returns a capture, and `Middlebox::decisions` returns the record.

## The matrix

A row is one road and one profile. The profiles are `direct`, `proxy` and
`restricted`. An outcome is `Scores` or `Skipped(reason)`. `Table` is the
matrix. The default fixtures have 11 roads and 3 profiles: 33 rows. Two
rows are skipped, because the icmp carrier needs a raw socket and no s3
capture exists in this lab. The other 31 rows are scored. A `proxy` case
applies the pad fault and a `restricted` case applies the delay fault, so
the table shows the same road under the three profiles.

## Exit codes

`pod_harness::exit` re-exports `pod_core::Class`. It adds no second table.
The six codes of the issue are in `CONTRACT` and in `Class::ALL`:

| Code | Class | When |
| --- | --- | --- |
| 64 | `Usage` | unknown or malformed name, carrier or query key |
| 66 | `NotFound` | missing key or identity file |
| 69 | `Unavailable` | carrier or listener disabled by policy |
| 71 | `Permission` | identity file permissions are wrong |
| 77 | `Refused` | authentication failed or wrong peer |
| 78 | `Suite` | protocol or suite mismatch |

`Class::Suite` shares code 78 with `Class::Config` and `Class::Revoked`, as
`pod_core` holds the table: a supervisor that stops on 78 stops the restart
loop for each of them. The six codes are distinct values. A script that
retries on 77 makes a different decision from one that retries on 78.

`exit::table()` writes the same table for the help of a binary.

## The claim check

`check_claims(text)` reads a document and returns each `Finding`. It fails
on a bare comparative from `faster`, `more secure`, `robust` and `better`
when no number and no ISO date are within two lines. It also fails on a
claim with no condition: a comparative, or a number with a unit from `s`,
`ms`, `us`, `min`, `minutes`, `%`, `MiB`, `MB`, `GiB`, `GB` and `runs`. A
condition is a word from `host`, `version`, `input`, `run`, `runs`,
`measured`, `fixture`, `platform`, `build` and `sample`. A word in inline
code or in a fenced code block is a quotation, not a claim. A heading and
a table row are not claims.

A claim that passes:

```text
The padded road is 0.21 less visible than the unpadded road on 2026-11-20;
host linux, version 1.0, input fixtures/tunnel.cap, 3 runs.
```

A claim that fails:

```text
The tunnel is faster.
```

## Tests

The unit tests hold the formats, the models, the scores, the claims, the
zip and the date. The integration tests hold the full matrix, the
unpadded and padded roads, the benign rate, the skipped row, the binary,
the exit codes, and the archive with `run.sh`.

```sh
cargo fmt --all --check
cargo clippy -p pod-harness --all-targets -- -D warnings
cargo test -p pod-harness
```

## License

0BSD. See [LICENSE](LICENSE).
