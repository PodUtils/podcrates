# pod-release

The release of a product of PodUtils: its index, the check of its
signature and of its files, and the self-update of a running binary.

* T-104 (GitHub #35 A-X-13, GitHub #36 G-11, githome's G-34 on GitHub
  #223): one release index for each release of each product, its writer
  for CI (`scripts/release-index.py`, call C793) and one verifier.
* T-880 (the ruling "Shared code"): the update of a running binary from
  podnet (`src/update.rs`, `src/update/`) and podssh
  (`crates/podssh-cli/src/update/`), from the better of the two copies.

## Use

```rust
use pod_release::{Identity, Roots, verify::verify_dir};

// The identity is the product's: its release workflow at the tag.
let who = Identity::github("PodUtils/podnet", "release.yml", "v0.80.0");
let checked = verify_dir(dir, &who, &Roots::sigstore())?;
// checked.files: each file of SHA256SUMS in dir, checked;
// checked.missing: each one that is not there; checked.index: the index.
```

An update (feature `fetch`), step by step:

```rust
use pod_release::update::{self, Plan, Source};

let source = Source::github("PodUtils/podssh", "release.yml");
let fetcher = pod_release::fetch::Fetcher::new(client); // the consumer's pod_proxy::Client
let (newest, _from) = update::newest(&fetcher, &source, &current).await?.ok_or(..)?;
if update::plan(&newest, &current, false, false, false) == Plan::Install {
    let signed = update::signed(&fetcher, &source, &newest).await?;
    let asset = pod_release::version::asset("podssh", &pod_release::version::release_target(TARGET));
    let bytes = update::binary(&fetcher, &source, &signed, &asset).await?;
    let replaced = update::install(&exe, &bytes, "podssh", &newest, pod_release::trial::LIMIT)?;
}
```

## API

- `sigstore::verify(bundle, artifact, identity)`, `verify_with(.., roots)`:
  the keyless cosign bundle (`application/vnd.dev.sigstore.bundle*`) of
  a file, with no network: the digest, the signature of the leaf, the
  chain of the leaf to Fulcio's root at the time of Rekor's entry, code
  signing, the URI and the OIDC issuer of the leaf (`Identity`), Rekor's
  signed entry timestamp over the canonical entry, and the entry's body
  against the file, the signature and the leaf. Gives `Logged { time,
  index }`.
- `Identity { uri, issuer }`, `Identity::github(repo, workflow, tag)`.
- `Roots::sigstore()`: the Fulcio chain and the Rekor key of Sigstore's
  public instance, compiled in; `Roots::from_pem(fulcio, rekor)`.
- `Sums::parse`, `Sums::get(name)`, `Sums::check(name, bytes)`,
  `Sums::names()`: `SHA256SUMS`; a name has its SHA-256 only when one line
  names it, with 64 hex digits.
- `verify_release(sums, bundle, identity, roots) -> Verified`;
  `Verified::check(name, bytes)`, `Verified::index(text)`;
  `verify::verify_dir(dir, identity, roots) -> Checked`.
- `index::Index`, `index::read(text)`, `Index::check()`,
  `Index::agrees(&sums)`, `Index::to_text()`; `index::schema::schema()`
  (feature `schema`).
- `version::Version` (semver order), `version::valid_tag`,
  `version::tags_of_api`, `version::tags_of_atom`, `version::newest`,
  `version::release_target`, `version::asset`.
- `trial::check(file, &Expect { program, version, limit })`: `--version`
  once, stdin empty, within the limit; its first line `PROGRAM VERSION`.
- `replace::replace(exe, bytes, trial) -> Replaced { said, prev }`;
  `replace::running()`, `replace::shown()`, `replace::remove_aside(exe)`.
- `keep::prev_of(exe)`, `keep::rollback(exe, expect) -> RolledBack`,
  `keep::fallback(exe, expect)`.
- `fetch::Fetcher` (feature `fetch`): a GET through the caller's
  `pod_proxy::Client`, HTTPS only, 5 redirects, a bound of size and time;
  a 404 is `Error::NotFound`; no message holds the query of a URL.
- `update::Source`, `update::newest`, `update::plan`, `update::signed`,
  `update::binary`, `update::install` (feature `fetch`).

## The release index

`release-index.json`, schema `schemas/release-index.v1.schema.json`
(generated from `index::Index` by a test):

| Field | What |
| --- | --- |
| `schema` | 1 |
| `project` | `OWNER/REPO` |
| `version` | the tag |
| `date` | `YYYY-MM-DDTHH:MM:SSZ` |
| `commit` | the commit of the tag (optional) |
| `signature` | `sums` (`SHA256SUMS`), `bundle` (`SHA256SUMS.sigstore.json`), `identity` (the URI of the leaf), `issuer` |
| `files` | each file: `name`, `sha256`, `size`, `target` (optional) |
| `sbom` | a file of `files`, or an `https://` URL (optional) |
| `acceptance` | a file of `files`, or an `https://` URL (optional; T-105 writes it) |

CI writes the index before `SHA256SUMS`, and `SHA256SUMS` holds its line:
the one signature of `SHA256SUMS` signs the index too. A verifier checks
the bundle first, then the index against `SHA256SUMS`
(`Verified::index`). A reader refuses a field that it does not know
(`additionalProperties: false`, as each schema of `schemas/`).

```sh
python3 -I scripts/release-index.py --project OWNER/REPO --version "v$V" \
    --dir dist --date "$SOURCE_DATE_EPOCH" --commit "$(git rev-parse HEAD)" \
    --out dist/release-index.json FILE...
```

## Features

| Feature | What |
| --- | --- |
| (none) | the verifier, the index, the versions, the trial, the replace and the kept binary; no tokio |
| `fetch` | `fetch` and `update`: the files of a release through `pod_proxy::Client` |
| `schema` | `index::schema`: the JSON Schema of the index (schemars); no binary turns it on |

## Settings

The crate reads no environment variable. The consumer gives: the
repository and the release workflow (`Source`, `Identity`; the ruling
"Services: what stays"), the roots (`Roots`), the HTTPS client with its
proxy and its trust (`pod_proxy::Client`), the program name and the
version of the trial (`Expect`), the path of the binary (`replace::running`
gives the running one).

## Failures

`Error`: `Unavailable`, `NotFound`, `Refused` (a signature, an identity, a
SHA-256, a trial run), `CantWrite` (the binary is as it was), `Config`.
`Error::class()` gives the `pod_core::Class`; podssh maps `CantWrite` to
`EX_CANTCREAT` by the variant.

## Parity

| Capability | podssh | podnet | Here |
| --- | --- | --- | --- |
| Rekor's signed entry timestamp and the entry's body | yes | no | yes (podssh) |
| The chain to Fulcio at the time of the entry | yes | yes (webpki) | yes (podssh, pure Rust) |
| Identity and issuer as a setting | repo and tag | repo and tag | the URI and the issuer |
| Canonical JSON of the promise with `preserve_order` in the binary | no | n/a | yes |
| One valid line of a name in `SHA256SUMS` | first line | exactly one | exactly one (podnet) |
| The file checked again after its trial run | no | yes (issue 236) | yes (podnet) |
| No write of others and no set-user-ID on the new file | copies the mode | 0755 | the old mode, masked by 0755 |
| A random temporary name, Windows `.exe`, the rename aside on Windows | yes | no | yes (podssh) |
| The kept binary checked by a trial before a rollback | yes | yes | yes, and checked again after it (podnet) |
| A tag form for URLs (no `/`, `%`, `..`) | semver only | yes (issue 227) | both |
| The list of releases: the API, else the feed | yes | yes | yes |
| A plan of a pinned tag and of `--force` | in its command | `plan` | `update::plan` (podnet) |
| The fetch through the consumer's proxy and TLS | podssh-ws | podnet `fetch` | `pod_proxy::Client` |

## Adoption map

| Consumer | Old | New |
| --- | --- | --- |
| podssh | `update/sigstore.rs` (`verify_with`, `Identity { repo, tag }`, `Roots`) | `pod_release::sigstore::verify_with`, `Identity::github(repo, "release.yml", tag)`, `Roots` |
| podssh | `update/release.rs` (`Version`, `tags_of_api`, `tags_of_atom`, `newest`) | `pod_release::version` (`tags_of_atom` with no roxmltree) |
| podssh | `update/fetch.rs` (`Fetcher`, `Url`) | `pod_release::fetch::Fetcher::new(pod_proxy::Client)` |
| podssh | `update/replace.rs`, `update/keep.rs`, `update/trial.rs` | `pod_release::replace`, `keep` (`rollback` gives `RolledBack`; the lines of `way_back` stay), `trial` (`Expect { program: "podssh", .. }`) |
| podssh | `update/mod.rs` `sum_for`, the steps of `update` | `Sums`, `update::signed`, `update::binary`, `update::install`; the lines, the flags and the exit codes stay |
| podnet | `src/update.rs` `valid_tag`, `atom_tags`, `newest_in_atom`, `version_order`, `choose`, `plan`, `asset_target` | `pod_release::version::{valid_tag, tags_of_atom, newest, Version}`, `update::plan`, `version::release_target` |
| podnet | `src/update/cosign.rs` `verify_bundle`, `sum_of` | `pod_release::sigstore::verify` (adds the check of Rekor), `Sums::get` |
| podnet | `src/update.rs` `trial`, `install`, `rollback`, `same_file` | `trial::check`, `update::install`, `keep::rollback` |
| podnet | `src/sigstore-fulcio.pem` | `Roots::sigstore()` |

## Tests

```sh
cargo test -p pod-release --all-features
python3 -I scripts/release-index.py --self-test
```

- `sigstore::tests`: the published bundle of podcrates v0.76.0
  (`tests/fixtures/v0.76.0/`) verifies; another tag, repository, workflow
  or issuer, a changed file, a changed signature, promise, body or time,
  a changed byte of each checked value of the bundle, a leaf that Fulcio
  did not sign, other roots: each refused.
- `verify::tests`: the directory of the release verifies; one changed
  byte of `podlab-install.sh`, of `SHA256SUMS` or of the bundle, and each
  other identity, fail.
- `update::tests`: a stand-in of GitHub on the loopback serves the release
  (its asset through a redirect to a signed URL); a changed file or sums
  from the server, another workflow, a missing tag; the newest from the
  API, else the feed.
- `replace::tests`, `keep::tests`: the replace, the kept binary, the
  trial, a file swapped after its trial, a rollback and a second rollback.
- `index::tests`: the schema file against the types, the index of the
  script against the schema and `read`, and back to the same bytes.
- The plant `release-no-signature` removes the check of the signature;
  `the_published_bundle_signs_the_published_sums` passes, and the tests of a
  changed signature fail.

## Sources

`SOURCES` lists each file of podssh at `5975bb0` and of podnet at
`613a94b`.
