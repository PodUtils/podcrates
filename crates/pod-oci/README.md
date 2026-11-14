# pod-oci

An OCI registry client for Linux, with no root and no daemon: the parse
of a reference by docker's rules, digests checked while the bytes are
written, the four OCI documents, the platform of a pull, the logins of
docker and podman, the distribution API over `pod-proxy` and `pod-tls`
(the challenge, the token, the `Basic` answer, a bounded retry), the
policy of insecure registries, the resolve of a reference to the manifest
of one platform, and the pool that fetches the blobs of a manifest into
the store of the caller. Four rules:

1. HTTPS only, with no plain-HTTP fallback of its own. Plain HTTP only
   for a registry that the caller named insecure (`transport::Policy`),
   and the client says so each time (`Policy::disclosure`, the event
   `oci.plain-http`). On hosts that drop outbound tcp/80, a fallback
   hangs and does not fail. No login and no token goes over plain HTTP:
   a 401 there is a refusal (`Policy`).
2. Each blob is checked while it is written. The bytes get their name in
   the store of the caller only when the digest and the size agree; a cut
   transfer starts again from byte zero, never from an offset.
3. The crate builds no TLS configuration that does not check a
   certificate. A caller that allows a registry with no check gives its
   own configuration as a `pod_tls::Trust` (`Client::with_insecure`).
4. No secret in a log or an error. `Credential` has no debug view of its
   password, the body of a token answer never gets to a message, and each
   URL in a message loses its query (`registry::redact`).

The caller gives each setting: the blocking HTTP client of `pod-proxy`
(its proxy settings from the caller's environment, its TLS context with
the caller's provider, its user agent), the trusts, the policy, the
logins (`credentials::Files` at the paths of the caller, or a `Logins`
of its own), the sink of events and the file-size ceiling. The crate
reads no variable, prints nothing and keeps no process state. The crate
is Linux only (`#![cfg(target_os = "linux")]`): on another target it is
empty. It has no `unsafe` and no C of its own.

## API

| Item | What |
| --- | --- |
| `reference` | `Reference::parse` (docker's rules: `alpine` is `docker.io/library/alpine:latest`; `http://` is kept in `plain_http`, not refused; a domain is a DNS name, an IPv4 address or an IPv6 address in brackets, with an optional port from 1 to 65535), `endpoint`, `manifest_selector`, `display_repository`, `canonical_repository`; `DEFAULT_DOMAIN`, `DEFAULT_ENDPOINT`, `HUB_ALIASES`, `OFFICIAL_PREFIX`, `DEFAULT_TAG` |
| `digest` | `Digest` (`parse`, `of`, `algorithm`, `hex`, `short`, `blob_path`), `Verifier` (`new`, `written`, `digest`, `finish(what, want, size)`), `SHA256`, `hex_of` |
| `spec` | `Index`, `Manifest` (`stored_bytes`), `Descriptor` (`parsed_digest`, `is_attestation`), `Config`, `RunConfig`, `Platform` (of a descriptor, with `os_version`; `word`, `full_word`: `windows(10.0.17763.6532)/amd64`), `Fetched::parse(bytes, header)`, `select_platform(&index, &want)` (the list of `NoPlatform` names each platform one time, by `full_word`, and no attestation manifest), `REFERENCE_TYPE`, `ATTESTATION`, `ACCEPT`, `MEDIA_OCI_INDEX`, `MEDIA_OCI_MANIFEST`, `MEDIA_DOCKER_LIST`, `MEDIA_DOCKER_MANIFEST`, `OS` |
| `platform` | `Platform` (`host`, `wanted(flag, defaults)`, `parse`, `matches`, `is_host`), `OS`, `HOST_ARCH`, `HOST_VARIANT` |
| `credentials` | `Credential`, `Logins` (`lookup(host)`), `NoLogins`, `Files` (`new(home, runtime_dir)`, `store_login`, `remove_login`; a `Logins`), `lookup_in`, `store_login_in`, `remove_login_in`, `basic_header`, `parse_auth_json`, `upsert_auth_json`, `delete_auth_json`, `server_host`, `helper_login`, `helper_store`, `helper_erase` |
| `transport` | `Policy` (`resolve(hosts, tls_verify)`, `with_tls_verify`, `with_insecure`, `is_insecure`, `verify`, `first`, `after_connect_failure`, `permits_explicit_http`, `disclosure`, `named`, `http_fallback`), `Decision`, `normalise_host` (the grammar of a domain of a reference; anything else is `Refusal::Host` with the reason), `parse_hosts(text, label)` |
| `trust` | `host_bundle(var)` (the first file of `BUNDLE_VARS`, then of `BUNDLE_PATHS`, with a certificate), `HostBundle { path, announced_by, bytes }`, `roots(bundle)` (the bundle alone, else the compiled-in roots of `pod-tls`, with the reason in `source`) |
| `registry` | `Client` (`new(http, verified)`, `with_policy`, `with_insecure`, `with_logins`, `with_events`, `with_file_ceiling`, `manifest`, `tags`, `blob`, `policy`), `FetchedManifest`, `Restart` (for `Vec<u8>` and `File`), `redact`, `MANIFEST_CEILING`, `TAGS_CEILING` |
| `pull` | `resolve(&mut client, &reference, &platform)` to `Resolved { top, manifest_digest, manifest_bytes, manifest }`; `Blobs` (`stage`, `commit`, `discard`), `Job`, `Step`, `Report`, `fetch_parallel(&blobs, &jobs, workers, fetch)`, `over_ceiling`, `FETCH_WORKERS` |
| `layout` | `OCI_LAYOUT`, `REF_ANNOTATION`, `MEDIA_UNTAR`, `allowed_entry`, `index_json` |
| `error` | `Result`, `Refusal::{Digest, NoPlatform, Host}` (`failure`), `refusal(&failure)` |
| `REDIRECTS` | the redirects of one request (5), which `pod-proxy` follows |

Each failure is a `pod_core::Failure`. A request with no answer keeps the
stage and the class of `pod-proxy` (`Dns`, `Proxy`, `Connect`, `Network`,
`Tls`), and its text names the host once: `GET /v2/...: HOST: connection
failed: ...`. An answer, a document or a digest of a registry is at
`Stage::Registry`; a local file or a credential helper at `Stage::Host`.
The classes: a value of the caller (a reference, a digest, a platform, a
host) `Usage`; a missing manifest or blob (404) and an index with no
manifest of the platform `NotFound`; 401 and 403 `Refused`; 429
`TempFail`; 5xx `Unavailable`; a manifest, a tags listing, a page count
or a blob past its limit, a token realm on `http://` and a 401 over
plain HTTP, `Policy`; a
digest or a document that breaks the protocol `Protocol`; schema 1, an
index in an index or a scheme other than `Bearer` and `Basic`
`Unsupported`; a registry named insecure with no trust for it `Config`.
`Refusal::Digest`, `Refusal::NoPlatform` and `Refusal::Host` (`Usage`) are the typed causes; a
caller reads them with `refusal(&failure)` (a pull of each tag skips a
tag with `NoPlatform`).

`Client::blob` tries a cut transfer again, up to three tries with a
backoff of 500 ms doubled and capped at 30 s, and writes one line for
each new try to the writer of the caller. A wrong digest stops at once.
`Client::get` tries a status of 429, 500, 502, 503 or 504 and a
transport failure again, with the seconds of `Retry-After` under the same
cap.

## Parity

Against podvm `crates/podvm-image` at `6cd3442`:

| Capability of podvm | Here |
| --- | --- |
| [x] `reference` (docker's rules, `http://` kept, the domain check) | the same; the names of Docker Hub are constants, and a test compares them with `docker-hub-registry` of `pod-services` |
| [x] `digest` (`Digest`, `Verifier`, `hex_of`) | the same; `hex_of` is public |
| [x] `oci` (the four documents, `Fetched::parse`, `select_platform`) | `spec`, the same |
| [x] `platform` (`Platform`, `host`, `parse`, `matches`; `wanted` reads `PODVM_DEFAULT_PLATFORM`, `DOCKER_DEFAULT_PLATFORM`) | `wanted(flag, defaults)`: the caller gives the names and the values |
| [x] `credentials` (the files, the helpers, the store and the removal of a login, `default_paths` from `$HOME` and `$XDG_RUNTIME_DIR`) | `Files::new(home, runtime_dir)`; the errors are `Failure`s with the same text; a new file starts with the mode 0600 |
| [x] `transport::Policy` (`resolve` reads `PODVM_INSECURE_REGISTRIES`, `PODVM_CONFIG` and its file) | `Policy::resolve(hosts, tls_verify)` and `parse_hosts`; the caller reads its variables and its file. `disclosure` has no prefix of a program |
| [x] `registry::Client` (`manifest`, `tags`, `blob`, the challenge, the token, `Basic`, the retry, the move to HTTP for a named host) | the same, over the caller's HTTP client and trusts; the line on stderr is the event `oci.plain-http` |
| [x] `net` (the redirects with no `Authorization` to another host, no step to `http://`, the words of a transport failure) | `pod-proxy` follows the redirects (`REDIRECTS`); `net` is private |
| [x] `tls` (the host bundle, `client_config`) | `trust::{host_bundle, roots}`; the caller makes the configuration with its provider |
| [ ] `tls::client_config_unverified`, `AcceptAny` | stay in podvm: the crate builds no configuration with no check |
| [x] the fetch pool of `pull` (`FETCH_WORKERS`, the strides, the cancel, the order of the transcript) | `pull::{Blobs, fetch_parallel, Job, Step, Report}`, with the count of workers from the caller |
| [x] the resolve of `pull` (the index, the platform, no index in an index) | `pull::resolve` |
| [ ] `pull`, `pull_all`, `local`, the record, the provenance, the probe cache, the check of the space | stay in podvm (the store) |
| [x] `layout` (the constants, the filter, the index) | the same; `save`, `load` and `import` stay in podvm |
| [x] `error::Error` | `pod_core::Failure` with `Refusal`; `exit_code` and docker's codes stay in podvm |
| [x] the tests: 78 unit tests of the moved parts, `tests/registry_fixture.rs`, `tests/proxy_pull.rs` | each with its name; 11 new unit tests and `tests/client.rs` (3); `proxy_pull` gives the variables to the client, with no child process |

## Adoption map (podvm)

| Old path | New path |
| --- | --- |
| `podvm_image::{reference, digest, oci}` | `pod_oci::{reference, digest, spec}`, re-exported by `podvm_image` under the old names |
| `podvm_image::platform::Platform::wanted(flag)` | `podvm_image::platform::wanted(flag)` (podvm's variables) over `pod_oci::platform::Platform::wanted(flag, defaults)` |
| `podvm_image::transport::Policy::resolve(flags, tls_verify)` | `podvm_image::transport::resolve(flags, tls_verify)` (`PODVM_INSECURE_REGISTRIES`, `PODVM_CONFIG`) over `pod_oci::transport::{Policy::resolve, parse_hosts}` |
| `Policy::disclosure` (`podvm: ...`) | `podvm: ` + `pod_oci`'s line |
| `podvm_image::credentials::{lookup, store_login, remove_login, default_paths}` | podvm's functions over `credentials::files()` (`$HOME`, `$XDG_RUNTIME_DIR`); the rest is `pod_oci::credentials` |
| `podvm_image::registry::Client::with_policy(policy)` | `podvm_image::registry::client(policy)?`: podvm's dialer, roots, `AcceptAny`, logins, `podvm: ` events and file-size ceiling |
| `podvm_image::tls::{client_config, host_bundle_bytes, HostBundle}` | podvm's, over `pod_oci::trust::{roots, host_bundle}` |
| `podvm_image::Error` | gains `Image(pod_core::Failure)` and `From<Failure>`: `Refusal::NoPlatform` and `Refusal::Digest` keep `NoPlatform` and `DigestMismatch`; `Class::Usage` exits 1, the rest 125 |
| the fetch pool of `pull.rs` | `pod_oci::pull::fetch_parallel` with `Store` as its `Blobs`; podvm keeps `FETCH_WORKERS` with its check of the fork slots |
| the constants and the index of `layout.rs` | `pod_oci::layout` |
| `crates/podvm-image/src/{reference,digest,oci}.rs`, `credentials/tests.rs`, `registry/{http,wire,tests}.rs` | leave podvm |
| each tag of podcrates in podvm (`podvm-image`, `podvm-probe`, `podvm-ssh`, `podvm-supervise`) | `v0.10.0` |

## Tests

`cargo test -p pod-oci`. The unit tests use a temporary directory each
and loopback only; a test that binds starts with `need_loopback!`. The
credential tests run a helper that does not exist
(`pod-oci-no-such-helper-xyz`): no tool of the host. The integration tests
run the registry fixture of `tests/common/registry.rs` (plain HTTP on
loopback) and a stand-in CONNECT proxy; each test gives its own proxy
variables to the client, and no test changes the environment of the
process. No test reaches a registry on the internet.

The grammar of a host (`host::tests`, `transport::tests`) takes a name,
an IPv4 address and an IPv6 address in brackets with a port, and refuses
a space, a `!`, an IPv6 address with no brackets, a port out of its range
and an empty label, each with its reason; a `NoPlatform` list of an
index of two Windows versions, a repeated platform and attestation
manifests names each platform one time, with its `os.version`, and no
attestation (`spec::tests`). The plants `oci-insecure-host-any-text`,
`oci-offered-repeats-a-platform`, `oci-offered-names-an-attestation` and
`oci-offered-drops-the-os-version` prove them.

`tests/distribution.rs` (T-353): the client against the CNCF
distribution registry on 127.0.0.1, with a configuration, certificates
and a made-up login that the test makes (`tests/common/distribution/`).
The test pushes an index of linux/amd64 and linux/arm64 over the
distribution API; the client resolves it for each platform and fetches
the config and the layer by digest, under htpasswd (`Basic`) and under
token auth (`Bearer`, a token server of the test, an ES256 JWT that the
registry checks against the root of the test). A wrong password and a
request with no login get 401; a layer and a manifest changed in the
store are refused by their digests; no login goes over plain HTTP. The
registry is `POD_OCI_TEST_REGISTRY_BIN`, else `registry` on the path; a
host with neither skips, unless `POD_OCI_TEST_REGISTRY=required`.
`scripts/ci/registry-tests.sh` runs it in `ci.yml` with the release of
distribution 3.1.2, checked by its SHA-256. `tests/live.rs` pulls from
ECR Public in `live.yml`.

## Sources

`SOURCES` lists each imported file of podvm with its blob, and the
`check-sources.py` check compares them with podvm's `main`.
