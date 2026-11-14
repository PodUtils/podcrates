# pod-actions-results

The ArtifactService of the results API of GitHub Actions, which
`actions/upload-artifact@v4` and `actions/download-artifact@v4` call at
`ACTIONS_RESULTS_URL`: the twirp JSON calls of
`github.actions.results.api.v1.ArtifactService` (`CreateArtifact`,
`FinalizeArtifact`, `ListArtifacts`, `GetSignedArtifactURL`,
`DeleteArtifact`) and the block blob calls of the signed URLs (a whole
`PUT`, `comp=block`, `comp=blocklist`, and the `GET` of the zip). One store
serves each run, so a job reads the artifact of an earlier job. T-565 made
it from podrunner's `crates/podrunner-broker-fake/src/artifacts.rs`
(PodUtils/podrunner#95); githome and a forge mode need the same service.
The module `cache` serves the CacheService of the same API to
`actions/cache@v4` and the cache of the `setup-*` actions, over a store
that stays across runs; T-588 added it and the store of a rerun from
podrunner's code (PodUtils/podrunner#99).

The crate has no HTTP server and does no network I/O: the caller gives each
request to the store and writes the answer. It reads no environment. The
store writes only in its directory.

## Use

```rust
use pod_actions_results::{ArtifactStore, Payload, LIMIT};

let store = Arc::new(ArtifactStore::new(&run_dir.join("artifacts"), LIMIT)?);
// In the server of each broker of the run, for each request:
match store.answer(&method, &path_and_query, &body, "http://10.0.2.2:8080/") {
    Some(answer) => {
        // answer.status, answer.content_type
        match answer.body {
            Payload::Bytes(bytes) => { /* write the bytes */ }
            Payload::File(file, len) => { /* stream `len` bytes of `file` */ }
        }
    }
    None => { /* another route of the server */ }
}
```

The cache, in the same server:

```rust
use pod_actions_results::cache::{self, CacheStore};

let cache = Arc::new(
    CacheStore::new(&cache_dir, cache::LIMIT)?
        .with_scopes(vec![git_ref, format!("refs/heads/{default_branch}")]),
);
let range = request.header("Range");                     // a part of a download
match cache.answer(&method, &path_and_query, &body, range, "http://10.0.2.2:8080/") {
    Some(answer) => { /* status, content_type, headers (Content-Range), body */ }
    None => { /* another route */ }
}
```

A rerun takes `ArtifactStore::new(dir, LIMIT)?.replacing()`.

`base` is the URL of the server as the client sees it, with a `/` at the
end; each signed URL starts with it. The server reads the whole body of a
request: a block of the client is 8 MiB, and podrunner's broker takes a
body of up to 16 MiB. The server gives `base` to the job as
`ACTIONS_RESULTS_URL` (see "The facts of the wire").

## API

| Item | What |
| --- | --- |
| `ArtifactStore::new(dir, limit)` | the store of one run in `dir`, with a limit of `limit` bytes; the signatures take the random bytes of the system |
| `ArtifactStore::with_entropy(dir, limit, entropy)` | the same with a `pod_core::entropy::Entropy`; when it fails, `CreateArtifact` fails with 500 `internal` |
| `ArtifactStore::replacing()` | the store of a rerun: a `CreateArtifact` of a name that has an artifact removes it first |
| `ArtifactStore::answer(method, path, body, base)` | the `Answer` to a request of the service or of a blob, or `None` for another route; `path` keeps its query |
| `ArtifactStore::route(method, path, body, base)` | the same as `(status, bytes)` in memory (podrunner's form) |
| `ArtifactStore::list`, `dir`, `used` | each artifact of the run by name, as `ListArtifacts` gives it (no signature); the directory; the bytes that the clients sent |
| `ArtifactStore::zip_of(name)` | the zip of the finalized artifact `name` (a view of the run); `None` for a name that is not valid or an artifact with no record |
| `Answer { status, content_type, headers, body }`, `Answer::into_bytes` | an answer; `headers`: the other headers (`Accept-Ranges`, `Content-Range` of the cache); `Payload::Bytes`, or `Payload::File(file, len)`: a file open at the first byte to send, and the number of bytes to send |
| `CONTENT_JSON`, `CONTENT_BLOB`, `CONTENT_TEXT` | the content types of a twirp answer, of a zip, and of a blob route's text |
| `SERVICE`, `BLOBS` | the path of each call (`SERVICE` and the method) and of each blob (`BLOBS`, `upload/ID` or `download/ID`, `?sig=`) |
| `LIMIT` | the default limit of a run: 4 GiB |
| `cache::CacheStore::new(dir, limit)`, `with_entropy(dir, limit, entropy)` | the cache store in `dir`, with a limit of `limit` bytes; the signatures and the ids of the entries take the random bytes of the system, or of `entropy` |
| `CacheStore::with_scopes(scopes)` | the scopes of the run: a new entry goes into the first; a lookup reads each in order |
| `CacheStore::answer(method, path, body, range, base)` | the `Answer` to a request of the CacheService or of a blob of the cache, or `None`; `range` is the `Range` header of a download |
| `CacheStore::route(method, path, body, range, base)`, `cache::Routed` | the same as `(status, bytes, headers)` in memory (podrunner's form, its `cache::Answer`) |
| `CacheStore::sweep(now)`, `cache::PENDING_AGE` | remove each upload under `.pending` that the store does not hold and that did not change for a day before `now`; `new` calls it |
| `CacheStore::entries`, `dir` | each entry, newest first, with its signature; the directory |
| `cache::SERVICE`, `cache::BLOBS`, `cache::LIMIT` | the path of each call and of each blob of the cache; the default limit, 10 GB as GitHub keeps for a repository |
| `fuzzing::{query, block_list, range}` (feature `fuzzing`) | the readers of a query, of a block list and of a `Range`, for the fuzz target `actions_results` of `fuzz/`; no consumer turns it on |
| `token::backend_ids(token)`, `BackendIds { run, job }` | the run and job ids of the scope `Actions.Results:RUN:JOB` in the claim `scp` of a runtime token, read as the client reads them (no check of the signature) |
| `token::results_scope(run, job)`, `token::RESULTS_SCOPE` | the scope for a token that a broker or a forge mints |

## The rules

* The store keeps `DIR/NAME/artifact.zip` (the zip of the client, as
  GitHub gives it) and `DIR/NAME/artifact.json` (the record: the ids of the
  run and of the job, the id, the name, the size, the time, the digest and
  the signature of the download). An upload that is not finalized is in
  `DIR/NAME/.upload-ID/`. The record goes in last, by a rename.
* A name is unique in a run, as on GitHub: a second `CreateArtifact` of a
  finalized name gets 409 `already_exists`; a finalize gets 409 when the
  name took a record in the meantime. A new `CreateArtifact` of a name that
  is not finalized drops the earlier upload, its URL and its blocks.
* A name is not empty, has at most 255 bytes, is not `.` or `..`, and has
  no control character and none of `" : < > | * ? \ /` (the characters
  that the client refuses). Each call checks it, and a name is one normal
  component of a path: no request reaches a file outside `DIR`. A record
  must hold its own name: on a file system that folds case, `Data` is not
  the artifact `data`.
* Each upload URL and each download URL carries its own signature: 16
  random bytes in hex. The store compares it in constant time. A wrong
  signature gets 403.
* The finalize reads the blob in parts, and checks its size and its
  SHA-256 (`sha256:HEX`) against `size` and `hash` of the request. A
  mismatch gets 400 `invalid_argument`, and the upload stays for another
  finalize. No blob gets 412 `failed_precondition`.
* The limit counts each byte that a client sends in a block or a whole
  blob, in each artifact of the run. The join of a block list counts none,
  and a delete gives none back. A call over the limit gets 413.
* A block id has at most 88 bytes (64 bytes before the base64, the limit
  of Azure); its file is its id in hex. A block list joins its blocks in
  order, in parts; a missing block gets 400.
* The ids of artifacts count up from 1. A store opened again over the
  artifacts of an earlier run starts above the highest id.

## The rules of the cache

* The calls: `CreateCacheEntry` (`key`, `version`) gives `ok` and
  `signed_upload_url`; `FinalizeCacheEntryUpload` (`key`, `version`,
  `size_bytes`) gives `ok` and `entry_id`; `GetCacheEntryDownloadURL`
  (`key`, `restore_keys`, `version`) gives `ok`, `signed_download_url` and
  `matched_key`. A refusal is `ok: false` with a `message`, as the client
  reads it.
* An entry is a key and a version in the scope of a ref:
  `DIR/entries/HASH/blob` and `entry.json`, where `HASH` is a digest of
  `[scope, key, version]` as JSON. An entry is written once in its scope.
  A record counts only in its own directory. A key is not empty, has at
  most 512 bytes and no `,`; a version is not
  empty and has at most 512 bytes.
* A lookup reads each scope in order: the exact key, then each restore key
  as a prefix, newest first, and only entries of the same version.
* Each URL carries a signature of 16 random bytes, compared in constant
  time; no random bytes gives `ok: false`, never a URL. The id of an entry
  is a random int63 in decimal digits.
* The blob calls are those of the artifacts: a whole `PUT`, `comp=block`
  (a block id of at most 88 bytes) and `comp=blocklist` (joined in parts).
  The finalize reads the blob in parts and checks its size. The record
  goes in last, by a rename.
* An upload that is not finalized is in `DIR/.pending/RANDOM/`. A store
  that opens removes each upload that it does not hold and whose
  directory, blocks and blob did not change for `PENDING_AGE` (one day):
  the upload of a crashed run. An upload of another store over the same
  directory changes more often, and stays.
* The limit counts the bytes of the uploads that are not finalized (413
  over it), and the entries: after a finalize, the oldest entries go until
  the store is within the limit.
* A download answers `Accept-Ranges: bytes`; a `Range: bytes=A-B`,
  `bytes=A-` or `bytes=-N` gives 206 with `Content-Range` and the file at
  the first byte of the part; a part outside the entry gives 416 with
  `Content-Range: bytes */LEN`.

## The facts of the wire

podrunner measured these with runner v2.338.2 and the v4 actions
(PodUtils/podrunner@T-119, PodUtils/podrunner#95):

* `actions/download-artifact@v4` refuses a `google.protobuf.StringValue`
  that is not a plain string in the JSON: the `digest` of
  `ListArtifacts` is `"sha256:..."`, never `{"value": "sha256:..."}`. A
  request is read in both forms (`name_filter`, `id_filter`, `hash`). An
  int64 is a string in the JSON, and a request is read as a string or a
  number. Tests: `an_artifact_goes_up_in_blocks_and_comes_down_whole`,
  `each_field_of_an_answer_is_a_plain_value`,
  `a_wrapper_or_a_plain_string_is_read_in_a_request`.
* The client reads its run and job ids (`workflow_run_backend_id`,
  `workflow_job_run_backend_id`) from the scope `Actions.Results:RUN:JOB`
  of the claim `scp` of `ACTIONS_RUNTIME_TOKEN`. A broker or a forge that
  mints the token puts the scope in it. Test:
  `the_scope_of_the_runtime_token_names_the_run_and_the_job`.
* Runner v2.338.2 sets `ACTIONS_RESULTS_URL` from `ResultsServiceUrl` in
  the `data` of the endpoint `SystemVssConnection` of the job message. The
  broker writes that key; this crate does not read the job message.

## Parity

Against podrunner `crates/podrunner-broker-fake/src/artifacts.rs` at
`0ff670d` (T-565), `artifacts.rs` and `cache.rs` at `872b43f` (T-588), and
`artifacts.rs` at `9dbefd1` (T-613):

| podrunner | Here |
| --- | --- |
| [x] `ArtifactStore::new`, `list`, `route`, `LIMIT` | the same names; `list` gives no signature |
| [x] the five calls of the service and the blob routes | `service.rs`, `blob.rs` |
| [x] the five tests of `artifacts/tests.rs`, each with its name | `src/tests.rs` |
| [x] `DIR/NAME/artifact.zip` and `artifact.json` | the same; the upload moves from `DIR/.pending/ID/` to `DIR/NAME/.upload-ID/` |
| [x] the random signature of `/dev/urandom` | `pod_core::entropy`; no random bytes is a failure, never a signature of zeros |
| [x] the SHA-256 of ring | sha2 (pure Rust) |
| [x] `rfc3339` | `pod_core::clock::rfc3339` |
| [x] `ArtifactStore::replacing` | the same |
| [x] `ArtifactStore::zip_of` (`9dbefd1`, T-149 of podrunner) | the same, with the check of a name of the store (one normal component); a name that starts with `.` is a name of an artifact here (no `.pending` in the store), with the test `the_zip_of_a_finalized_artifact_and_of_nothing_else` (new) |
| [x] `cache::CacheStore` (`new`, `with_scopes`, `entries`, `route`), `cache::LIMIT` | the module `cache`; `route` takes the `Range` header as podrunner's `header` |
| [x] the six tests of `cache/tests.rs`, each with its name | `src/cache/tests.rs`, with no sleep |

Differences from the source, each with a test (call C325 to C329):

* `ArtifactStore::answer` gives the zip of a download as an open file
  (`a_download_is_a_file_to_stream`); the block list and the finalize read
  a blob in parts, never whole.
* Each call checks the name; a name was checked at `CreateArtifact` only,
  so `DeleteArtifact` of `..` removed a directory outside the store when it
  held an `artifact.json` (`a_name_outside_the_store_is_refused`). The
  upload is in the directory of its name: `.pending` was a valid name.
* The signature is compared in constant time, and a store with no random
  bytes makes no URL (`the_signature_is_compared_in_full`,
  `no_random_bytes_gives_no_upload_url`).
* A finalize refuses a name that has a record
  (`a_finalize_refuses_a_name_that_another_writer_took`); a record of
  another name is not the artifact
  (`a_record_of_another_name_is_not_the_artifact`); a store opened again
  keeps its ids (`a_store_opened_again_keeps_its_ids`); a new upload of a
  name removes the blocks of the earlier one
  (`a_new_upload_of_a_name_drops_the_earlier_one`).
* A block id over 88 bytes, a call with no block id and an unknown `comp`
  get 400, and a refused or failed write counts no byte
  (`a_block_id_has_a_limit_and_a_block_list_its_blocks`,
  `the_limit_counts_each_byte_of_the_run`).
* `failed_precondition` has the status of twirp, 412 (podrunner: 400).

Differences of the cache from the source (calls C379 to C382):

* The signature is compared in constant time, and the signatures and the
  ids take the `Entropy` of the store
  (`the_signature_of_the_cache_is_compared_in_full`,
  `each_entry_has_a_random_id_and_no_random_bytes_gives_no_url`). The id of
  an entry is decimal: an int64 of protobuf.
* The limit counts the uploads that are not finalized; a block id has the
  limit of the artifacts; a block list joins its blocks in parts
  (`the_uploads_hold_the_limit_and_a_block_list_joins_its_blocks`).
* A download, whole or a part, is a file to stream
  (`a_range_of_the_cache_is_a_part_of_a_file_to_stream`).
* The directory of an entry is a digest of the scope, the key and the
  version as JSON (`a_key_that_holds_a_newline_is_not_another_entry`), and
  a record counts only in that directory
  (`a_record_in_the_directory_of_another_entry_is_not_read`).
* Each entry has an `order` strictly above the newest: two entries in one
  second keep "newest first", and the tests of podrunner need no sleep.

## Adoption map

| Consumer | Old | New |
| --- | --- | --- |
| podrunner | `podrunner_broker_fake::artifacts::{ArtifactStore, LIMIT}` | `pod_actions_results::{ArtifactStore, LIMIT}` |
| podrunner | `store.route(method, path, body, base)` in `serve` of the broker | the same call; or `store.answer(...)` to stream a download and to send its content type |
| podrunner | the `ring` digest and `/dev/urandom` of `artifacts.rs` | removed |
| podrunner | `DIR/.pending/` of an `--artifacts DIR` | `DIR/NAME/.upload-ID/` while an upload is not finalized |
| podrunner | `ArtifactStore::new(dir, LIMIT)?.replacing()` for a rerun | the same |
| podrunner | `podrunner_broker_fake::cache::{CacheStore, LIMIT}` | `pod_actions_results::cache::{CacheStore, LIMIT}` |
| podrunner | `cache.route(method, path, body, header, base)` and its `cache::Answer` | the same call, `(status, bytes, headers)`; or `cache.answer(...)` to stream a download |
| podrunner | a `--cache DIR` of the code of podrunner | a new directory: the digest of an entry changed |
| podrunner | `crates/podrunner-broker-fake/src/cache.rs` and `src/cache/tests.rs` | removed |
| podrunner | `crates/podrunner-broker-fake/src/artifacts.rs` and `src/artifacts/tests.rs` | removed; `pod-actions-results = { git = "https://github.com/PodUtils/podcrates", tag = "vX.Y.Z" }` |

## Tests

`cargo test -p pod-actions-results`: the 5 tests of the artifacts of
podrunner and `the_zip_of_a_finalized_artifact_and_of_nothing_else`
(`src/tests.rs`), the 6 tests of the cache of podrunner
(`src/cache/tests.rs`), 15 tests of the public API of the artifacts
(`tests/store.rs`) and 8 of the cache (`tests/cache.rs`). The plants of
the group `actions`: `results-sha256-unchecked` (the finalize
takes any digest), `results-no-limit` (the run has no limit),
`results-sig-length-only` (the compare of a signature reads the length
alone), `results-digest-wrapped` (the `digest` of a list as
`{"value": ...}`) and `results-name-escapes` (a name is a path) (T-565);
and `results-replace-ignored` (a rerun keeps the artifact),
`results-cache-scope-ignored` (a lookup reads one scope),
`results-cache-range-whole` (a `Range` gives the whole entry),
`results-cache-no-eviction` (no entry goes over the limit),
`results-cache-entry-id-fixed` (the id of an entry is the counter),
`results-cache-no-limit` (the limit counts one body),
`results-cache-names-joined` (the digest of an entry joins its names) and
`results-cache-record-anywhere` (a record counts in any directory)
(T-588); `results-cache-pending-kept` (the sweep removes nothing) (T-600);
`actions-results-zip-of-a-half-artifact` (`zip_of` gives the zip of an
artifact with no record) (T-613).

The fuzz target `actions_results` (`fuzz/`, T-566) reads a query, a
block list, a runtime token and a `Range`, then gives a script of
requests to an `ArtifactStore` and a `CacheStore` in a scratch
directory. Its plants: `fuzz-results-range-past-end` and
`fuzz-results-download-past-end` (a file of an answer names a byte past
its end).

## Sources

`SOURCES` lists each imported file with its blob: podrunner at `872b43f`.
