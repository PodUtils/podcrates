# pod-release

The release of a product of PodUtils: its index, the check of its
signature and of its files, and the self-update of a running binary.

* T-104 (GitHub #35 A-X-13, GitHub #36 G-11, githome's G-34 on GitHub
  #223): one release index for each release of each product, its writer
  for CI (`scripts/release-index.py`, call C793) and one verifier.
* T-880 (the ruling "Shared code"): the update of a running binary from
  podnet (`src/update.rs`, `src/update/`) and podssh
  (`crates/podssh-cli/src/update/`), from the better of the two copies.
* T-965 (githome's G-34, A-X-24, PodUtils/podrunner@T-562): the verifier as the
  static binary `pod-release` (feature `cli`), an asset of each release of
  podcrates, for a CI with no Rust and for an installer (calls C804 to
  C807).
* T-964: the bundle of Rekor v2: the inclusion proof with its signed
  checkpoint, and the RFC 3161 timestamp of Sigstore's timestamp
  authority (call C820).

## Use

```rust
use pod_release::{Identity, Roots, verify::verify_dir};

// The identity is the product's: its release workflow at the tag.
let who = Identity::github("PodUtils/podnet", "release.yml", "v0.80.0");
let checked = verify_dir(dir, &who, &Roots::sigstore())?;
// checked.files: each file of SHA256SUMS in dir, checked;
// checked.missing: each one that is not there; checked.index: the index.
```

## The command line

`pod-release` is a static musl binary of each release of podcrates:
`pod-release-x86_64-unknown-linux-musl` and
`pod-release-aarch64-unknown-linux-musl`, in its `SHA256SUMS`, and the
image `ghcr.io/podutils/pod-release:TAG`. It reads files only: the caller
downloads them with its own tool. Its help and its manual come from
`pod-manual` (`pod-release man --no-pager`, `man/pod-release.1`).

```sh
# The release: the bundle by the signer, each file that DIR holds, the index.
pod-release verify --repo OWNER/REPO --tag TAG [--workflow release.yml] DIR
pod-release verify --identity URI [--issuer URL] DIR
# Each file of SHA256SUMS must be in DIR.
pod-release verify --repo OWNER/REPO --tag TAG --all DIR
# These files only (an installer), and the index when DIR holds it.
pod-release verify --repo OWNER/REPO --tag TAG DIR FILE...
# A dry run before the signature: each file and the index, no bundle.
pod-release sums [--all] DIR [FILE...]
# The files of an index: NAME TARGET SIZE SHA256, one line for each.
pod-release index [--target TRIPLE] release-index.json
```

`verify` checks in this order and stops at the first failure: the bundle
`SHA256SUMS.sigstore.json` of `SHA256SUMS` by the signer (Sigstore's public
roots, compiled in), the SHA-256 of each file, then `release-index.json`
against `SHA256SUMS`, its schema and the signer (`Index::signed_as`). An
index in DIR that `SHA256SUMS` does not name is refused. On success it
writes `ok NAME` for each file, `missing NAME` for each file of
`SHA256SUMS` that DIR does not hold, and a last line `verified: ...`.

| Exit | Class of `pod_core` | When |
| --- | --- | --- |
| 0 | | each check holds |
| 64 | `Usage` | a usage error of the command line |
| 66 | `NotFound` | `SHA256SUMS`, its bundle, a named file, or a file with `--all`, is not in DIR |
| 69 | `Unavailable` | a file is there and cannot be read |
| 77 | `Refused` | the signature, the signer, a SHA-256 or the index |

A failure writes one line on the error output: `pod-release: CLASS: WHY`.

The release dry run of a product (PodUtils/podrunner@T-562):

```sh
curl -fsSLO "https://github.com/PodUtils/podcrates/releases/download/v$V/pod-release-x86_64-unknown-linux-musl"
echo "$PINNED_SHA256  pod-release-x86_64-unknown-linux-musl" | sha256sum -c
chmod +x pod-release-x86_64-unknown-linux-musl
./pod-release-x86_64-unknown-linux-musl sums --all dist
# On the tag, after cosign sign-blob:
./pod-release-x86_64-unknown-linux-musl verify --repo "$GITHUB_REPOSITORY" --tag "$GITHUB_REF_NAME" --all dist
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
  a file, with no network: the digest, the signature of the leaf, an
  entry of a log of the roots (Rekor v1 or v2) with each proof that it
  holds (Rekor's signed entry timestamp over the canonical entry; the
  inclusion proof of RFC 6962 with its checkpoint, a signed note of the
  log), each RFC 3161 timestamp by the timestamp authority of the roots,
  the chain of the leaf to Fulcio's root at each time that the bundle
  proves, code signing, the URI and the OIDC issuer of the leaf
  (`Identity`), and the entry's body (`hashedrekord` 0.0.1 or 0.0.2)
  against the file, the signature and the leaf. A bundle needs the
  promise or the proof, and a time: the promise's or a timestamp's (call
  C820). Gives `Logged { time, index }`.
- `Identity { uri, issuer }`, `Identity::github(repo, workflow, tag)`.
- `Roots { fulcio, logs, tsa }`, `Roots::sigstore()`: the Fulcio chain,
  the logs (`Log { name, key }`: `rekor.sigstore.dev` and the shard
  `log2025-1.rekor.sigstore.dev` of Rekor v2) and the chain of the
  timestamp authority of Sigstore's public instance, compiled in from its
  trusted root (sigstore/root-signing, `targets/trusted_root.json`, at
  the commit that `SOURCES` names); `Roots::from_pem(fulcio, &[(name,
  key)], tsa)`; `Log::id()` (the log id of a bundle).
- `Sums::parse`, `Sums::get(name)`, `Sums::check(name, bytes)`,
  `Sums::names()`: `SHA256SUMS`; a name has its SHA-256 only when one line
  names it, with 64 hex digits.
- `verify_release(sums, bundle, identity, roots) -> Verified`;
  `Verified::check(name, bytes)`, `Verified::index(text)`;
  `verify::verify_dir(dir, identity, roots) -> Checked`;
  `verify::verify_files(dir, identity, roots, Which) -> Checked` (`Which`:
  `Present`, `All`, `Named(names)`); `verify::check_files(dir, &sums,
  Which) -> Found` (no signature: the check of a dry run);
  `verify::read_file(path)` (a missing file is `Error::NotFound`).
- `index::Index`, `index::read(text)`, `Index::check()`,
  `Index::agrees(&sums)`, `Index::signed_as(&identity)` (the signature,
  the signer, and for GitHub the project and the tag), `Index::to_text()`; `index::schema::schema()`
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
- `cli` (feature `cli`): the exit codes `EXIT_*`, `cli::code(&Error)`,
  `cli::line(&Error)`, `cli::facts()`, `cli::manual(clap::Command)`.

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
| `cli` | the binary `pod-release`: clap, and its help and manual from `pod-manual` |

## Settings

The crate reads no environment variable. The consumer gives: the
repository and the release workflow (`Source`, `Identity`; the ruling
"Services: what stays"), the roots (`Roots`), the HTTPS client with its
proxy and its trust (`pod_proxy::Client`), the program name and the
version of the trial (`Expect`), the path of the binary (`replace::running`
gives the running one).

## Failures

`Error`: `Unavailable`, `NotFound` (also a file that a check of a
directory needs and does not find), `Refused` (a signature, an identity, a
SHA-256, an index, a trial run), `CantWrite` (the binary is as it was),
`Config`.
`Error::class()` gives the `pod_core::Class`; podssh maps `CantWrite` to
`EX_CANTCREAT` by the variant.

## Parity

| Capability | podssh | podnet | Here |
| --- | --- | --- | --- |
| Rekor's signed entry timestamp and the entry's body | yes | no | yes (podssh) |
| The inclusion proof and its signed checkpoint; Rekor v2 | no | no | yes |
| The RFC 3161 timestamp | no | no | yes |
| The chain to Fulcio at the time of the entry | yes | yes (webpki) | yes (podssh, pure Rust), at each proven time |
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
  did not sign, other roots: each refused. `sigstore::tests::parts`: the
  bundles of v0.76.0 and v0.77.0 (cosign v3.1.4) with each part removed:
  the promise alone, and the proof with the timestamp (the form of Rekor
  v2), verify; no promise and no timestamp, or no promise and no proof,
  are refused; a changed hash, root, index or size of the proof, a
  changed origin, size, root or signature of the checkpoint, a changed
  time or signature of the timestamp, and another timestamp authority:
  each refused. `sigstore::tests::rekor2`: two bundles of Rekor v2 of
  sigstore-conformance (`rekor2-happy-path`, and
  `rekor2-checkpoint-origin-not-first` with a witness line first) verify
  by the staging trusted root of their test; with no timestamp, no proof,
  a changed hash, body or checkpoint signature, each refused; the
  compiled roots equal Sigstore's public trusted root. The files of
  Sigstore are in `tests/fixtures/sigstore/` with their licence
  (Apache-2.0, `tests/fixtures/sigstore/LICENSE`); `SOURCES` names their
  commits.
- `verify::tests`: the directory of the release verifies; one changed
  byte of `podlab-install.sh`, of `SHA256SUMS` or of the bundle, and each
  other identity, fail. The index of v0.77.0
  (`tests/fixtures/v0.77.0/`) verifies with its signer; another tag,
  repository, issuer, version or bundle name fails `signed_as`.
- `tests/cli.rs`: the binary as a process over the files of v0.76.0 and
  v0.77.0: each release verifies; one changed byte of `podlab-install.sh`,
  `SHA256SUMS`, the bundle and the index, and another tag, repository,
  workflow or issuer, each give 77 and one line; a missing file with
  `--all` or by name gives 66; an index that `SHA256SUMS` does not name,
  and an index with a field that the schema does not know (`sums`), give
  77; `index`, the usage errors (64), the help, the version and the manual.
- `pod-release` (the binary's own tests): the checks of `pod-manual`: the
  text rules, the parser, `man/pod-release.1` and `man/llms.txt`, the
  variables, the exit codes and the rules of the flags.
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
  changed signature fail. The plant `release-index-unread` takes the index
  by its line alone, and `release-named-file-missing` takes a named file
  that is not there as missing: `tests/cli.rs` catches each.

## Sources

`SOURCES` lists each file of podssh at `5975bb0` and of podnet at
`613a94b`.
