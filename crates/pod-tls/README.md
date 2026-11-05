# pod-tls

TLS for the crates of podcrates and their binaries: where the trust
anchors come from, the rustls configuration of a client over them with
the provider that the binary gives, the words of a failed handshake, a
TLS server with a P-256 key, and, behind features, the pure-Rust crypto
provider and Encrypted Client Hello (ECH).

The crate installs no process default provider, reads no variable, keeps
no process state and prints nothing. The binary makes one `Context` and
shares it.

## API

| Item | What |
| --- | --- |
| `Context::new(provider, settings)` | the TLS of a binary: the provider, the `Settings`, the configuration kept for each trust (16 at most, a CA file that changed by time or length is read again), the groups that servers took, the state of ECH |
| `Context::config_for(&trust)`, `roots_for`, `default_roots`, `client_config(&roots)` | the configuration of a trust, kept so that TLS resumes; the roots of a trust |
| `Context::tls_over(stream, name, &trust, limit)`, `connect_over(..)` | a verified handshake over an open stream; a `pod_core::Failure` at `Stage::Tls` on failure |
| `client_config(&roots, provider, groups)`, `tls_over(stream, name, config, limit, &words)` | the same with no context |
| `Trust` (`File`, `Default`, `Caller`), `Trust::caller(config)`, `CallerConfig` | where the anchors come from; a caller's own configuration, refused with ALPN, used as it is |
| `TlsRoots`, `roots_from_bundle`, `roots_from_compiled_set`, `SYSTEM_BUNDLES`, `ROOTS_VERSION`, `ROOTS_PUBLISHED` | the roots and their sources |
| `Words`, `handshake_hint`, `handshake_failure`, `handshake_error`, `clock_refused` | the words of a failed handshake: an unknown issuer gets where a CA goes, a certificate refused by its time gets this host's clock |
| `Error` (`Bundle`, `Config`, `Server`) | a trust, a configuration or the pair of a server that cannot be used; `From<Error> for Failure` |
| `groups` | `GroupFile`, `Groups`, `Kept` (the group that each server took, kept across runs), `Dirs` (the file `tls-groups.txt` in the first of the consumer's directories), `HEADER` |
| `bundle` | `load_bundle`, `parse_bundle`, `pem_certificates`: the PEM reader of `CERTIFICATE` blocks |
| `stores` | `read`, `summary`, `Store`: the roots of the stores of Windows (crypt32) and macOS (Security framework) |
| `server` | `acceptor(provider, cert, key, alpn)`, `acceptor_for(provider, served, alpn)`, `certified`, `signing_key`, `certificates`, `Served` (the pair changed while the server runs, the challenges of TLS-ALPN-01), `ACME_TLS_1`, `HTTP1` |
| `crypto` (feature `pure`) | `provider()`, `suites::self_check`, `aead`, `hash`, `hkdf`, `hmac`, `kx`, `quic`, `random`, `sign`, `tls12`, `hpke`, `rsa_sig` (feature `rsa`) |
| `ech` (feature `ech`) | `Ech` (`set_mode`, `mode`, `wanted`, `outcome`, `cache`), `Mode` (`parse`, `word`), `chosen`, `open(&context, &route)`, `open_with`, `Route`, `Records`, `NoRecords`, `Published`, `Outcome`, `Cache`, `Clock`, `Lookup`, `Why`, `READ_WITHIN`, `SEALED_WAIT` |

## Features

| Feature | What |
| --- | --- |
| none (default) | the configurations, the trust, the stores, the server; the binary gives a provider (ring, aws-lc or the pure one) |
| `pure` | the pure-Rust `CryptoProvider`: TLS 1.3 and TLS 1.2 (ECDHE with AES-GCM or ChaCha20-Poly1305), X25519 and P-256, ECDSA and Ed25519, the keys of QUIC; no C compiler |
| `rsa` | `pure` with the RSA verifier (PKCS #1 v1.5 and PSS, 2048 to 4096 bits) and the ECDHE_RSA suites. Verify only: RUSTSEC-2023-0071 concerns the operations of `rsa` 0.9 with a private key (`deny.toml` ignores it) |
| `ech` | ECH, with the HPKE of `pure` (rustls has HPKE only from aws-lc) |

## Settings

| Field | What | podssh |
| --- | --- | --- |
| `Settings::words.app` | the name of the program in the words | `podssh` |
| `Settings::words.replace_roots` | where a CA that replaces the roots goes | `--ca-file FILE or SSL_CERT_FILE` |
| `Settings::words.add_roots` | where a CA that adds to the roots goes | `podssh-ca.pem next to the binary` |
| `Settings::words.ech_strict` | how the user asks for strict ECH | `PODSSH_ECH=strict` |
| `Settings::extra_bundle` | a PEM bundle that adds to the default roots | `podssh-ca.pem` next to the binary (`std::env::current_exe`, never `argv[0]`) |
| `Settings::system_bundles` | the system bundles, first readable one used | `SYSTEM_BUNDLES` |
| `context.ech().set_mode(mode)` | the mode of ECH | `Mode::parse` of `PODSSH_ECH`, then the settings file's `ECH` (`ech::chosen`) |
| `context.groups().keep_in(Box::new(Dirs(dirs)))` | the file of the groups | the cache's directories |

## Parity

| Capability of podssh `crates/podssh-ws/src/{crypto,tls.rs,tls,stores,tls_server.rs,tls_server,tls_wrap.rs,bundle.rs,ech}` and `crates/podssh-relay/src/tls_groups.rs` | Here |
| --- | --- |
| The pure provider: suites, key exchange, signatures, HKDF, HMAC, AEAD, QUIC keys, the OS random source, the self check | [x] `crypto` (feature `pure`) |
| RSA verification and the ECDHE_RSA suites | [x] feature `rsa` |
| HPKE for ECH | [x] `crypto::hpke` |
| No permissive verifier; the WebPKI verifier checks the name each time | [x] `client_config`; `tests/no_permissive_verifier.rs` |
| The trusts: a file alone, the default set, a caller's configuration (no ALPN) | [x] `Trust` |
| The default set: compiled-in roots, the bundle next to the binary, the stores of Windows and macOS, the first system bundle | [x] `Context::default_roots`; the bundle's path is a setting |
| The age of the compiled-in roots, checked against Cargo.lock | [x] `ROOTS_VERSION`, `tests/roots_version.rs` |
| One configuration for each trust, so TLS resumes; a CA file that changed is read again; 16 at most | [x] `Context::config_for` |
| The group that each server took, kept across runs in a private file (64 servers, 30 days, written once a day) | [x] `groups`, `groups::Dirs` |
| The hint of an unknown issuer and of a certificate refused by the clock | [x] `Words`, `handshake_hint` |
| A verified handshake over an open stream, the kind of a cut kept | [x] `tls_over`, `connect_over` (the io error is the cause of the `Failure`) |
| The PEM reader (CRLF, whitespace, padding, an empty block, a private key block) | [x] `bundle` |
| The TLS server with a P-256 key (SEC1 and PKCS #8), the pair changed while it serves, TLS-ALPN-01 | [x] `server` (the provider is the caller's) |
| ECH: the modes, the names that it seals, the hellos and their order (sealed, the retry configs once, plain after a refusal but in strict), the cache (FLOOR 30 s, NO_TTL 300 s, CEILING a day, MISS 30 s, COOL 5 min, MOST 256, one read at a time), the outcome of each name (64) | [x] `ech` |
| The tests: unit (43) and `tests/{bundle,caller_tls,certificate_structure,crypto_vectors,ech_handshake,hostname_verification,live_handshake,no_permissive_verifier,quic_keys,random_source,roots_version,rsa,signatures,system_store,tls12,tls_group,tls_resume}.rs`, with their names | [x] 52 unit, 85 integration, 7 ignored (5 live, 2 steps of the system store); a test of a configuration runs on ring and on the pure provider |
| The doctor tests of `tests/bundle.rs` (4) | they stay with the doctor of `podssh-ws` (`pod-ws`) |
| The test of the bundle path next to the binary | it stays with `default_bundle_path` in podssh |
| `the_websocket_keys_report_a_source_that_gives_no_bytes` | it goes with the WebSocket handshake (`pod-ws`) |
| `the_binary_never_takes_a_callers_configuration` (it scans podssh-cli) | it stays in podssh |
| The fuzz target `pem_bundle` | T-017 |

The merge of podnet's `src/trust.rs` (roots, pins, `SSL_CERT_FILE`) and
`src/tls_fragment.rs` comes next in T-012 (`docs/design.md`, section 8,
step 5).

## Adoption map

| Consumer | Old path | New path |
| --- | --- | --- |
| podssh | `podssh_ws::crypto::*` | `pod_tls::crypto::*` (feature `pure`; `rsa_sig` with `rsa`) |
| podssh | `podssh_ws::crypto::provider()`, used inside each configuration | `let context = Arc::new(pod_tls::Context::new(Arc::new(pod_tls::crypto::provider()), settings))`, once |
| podssh | `podssh_ws::Trust`, `tls::{Trust, CallerConfig, TlsRoots, SYSTEM_BUNDLES, ROOTS_VERSION, ROOTS_PUBLISHED, roots_from_bundle, roots_from_compiled_set, clock_refused}` | `pod_tls::*` |
| podssh | `tls::{roots_for, config_for, default_roots}(..)` | `context.{roots_for, config_for, default_roots}(..)` |
| podssh | `tls::client_config(&roots)` | `context.client_config(&roots)` |
| podssh | `tls::client_config_with(&roots, provider)` | `pod_tls::client_config(&roots, Arc::new(provider), context.groups().clone())` |
| podssh | `tls::client_config_keeping(&roots, provider, kept)` | `pod_tls::client_config(&roots, Arc::new(provider), kept)` |
| podssh | `tls::connect_over(s, name, &trust, limit)` (`Err(String)`) | `context.connect_over(s, name, &trust, limit)` (`Err(Failure)`, the same words) |
| podssh | `tls::UNKNOWN_ISSUER_HINT` | `context.words().unknown_issuer_hint()`, with the words of the settings above |
| podssh | `tls::{handshake_hint, handshake_failure}(e)` | `pod_tls::{handshake_hint, handshake_failure}(e, context.words())` |
| podssh | `tls::groups::{GroupFile, Groups, Kept, HEADER, MOST, MAX_AGE_MS}` | `pod_tls::groups::*`; the header is `pod-tls groups 1` (a file of podssh's header is set aside once, with the note) |
| podssh | `tls::groups::{process, keep_in, take_note}` | `context.groups()`, `.keep_in(file)`, `.take_note()` |
| podssh | `podssh_relay::tls_groups::{In, FILE}`, `install()` | `pod_tls::groups::{Dirs, FILE}`; `context.groups().keep_in(Box::new(Dirs(dirs)))` once, before the first dial |
| podssh | `podssh_ws::stores::*` | `pod_tls::stores::*` |
| podssh | `podssh_ws::tls_server::{acceptor, acceptor_for}(..)` (`Err(String)`) | `pod_tls::server::{acceptor, acceptor_for}(provider, ..)` (`Err(pod_tls::Error::Server)`) |
| podssh | `podssh_ws::tls_server::{certified, signing_key, certificates, Served, ACME_TLS_1, HTTP1, TlsAcceptor}` | `pod_tls::server::*` (`certified` gives `pod_tls::Error`) |
| podssh | `podssh_ws::tls_over(s, name, &trust, limit)` (`Err(ConnectError)`) | `context.tls_over(s, name, &trust, limit)` (`Err(Failure)`: `Class::Config`, `Usage` for a bad name, `Timeout`, else the class of the io kind with the io error as the cause; `pod-proxy`'s `Cut::of(kind)` reads the cut) |
| podssh | `cut::TlsFailure::of(&e)` | `pod_tls::handshake_error(e, context.words())` |
| podssh | `WsError::{Bundle, Config}` of the trust | `pod_tls::Error::{Bundle, Config}` |
| podssh | `podssh_ws::bundle::{load_bundle, parse_bundle, pem_certificates}` | `pod_tls::bundle::*` |
| podssh | `bundle::{BUNDLE_FILE_NAME, executable_path, default_bundle_path}`, `podssh-ca.pem` next to the binary | podssh keeps them and gives `Settings::extra_bundle = default_bundle_path().ok()` |
| podssh | `PODSSH_ECH`, `ech::ECH_ENV`, `ech::Mode::from_env` | podssh reads its variable: `ech::Mode::parse(std::env::var("PODSSH_ECH").ok().as_deref())`, its refusal prefixed with `PODSSH_ECH: ` |
| podssh | `ech::{set_mode, mode}` | `context.ech().set_mode(ech::chosen(set, env))`, `context.ech().mode()` |
| podssh | `ech::{wanted, outcome}(..)` | `context.ech().{wanted, outcome}(..)` |
| podssh | `ech::Outcome::words()` | `outcome.words(&context.words().ech_strict)` |
| podssh | `ech::cache::process()` | `context.ech().cache()` |
| podssh | `ech::Dial { host, port, server_name, trust, proxy, limit, alpn }` | `ech::Route { target: &Target::new(host, port), server_name, trust, dial: &the_dialer, records: &the_reader, limit, alpn }` |
| podssh | `ech::open(host, port, name, &trust, &proxy, limit, alpn)` (`RelayStream`, `ConnectError`) | `ech::open(&context, &route)` (`tokio_rustls::client::TlsStream<pod_core::BoxedIo>`, `Failure`) |
| podssh | `ech::open_with(&cache, mode, &dial)` | `ech::open_with(&context, &cache, mode, &route)` |
| podssh | `resolve::https::{Published, doh_https}` as ECH reads them | `ech::Published`; an `ech::Records` that `pod-dns` implements (T-015) |
| podssh | the test switches `PODSSH_TEST_OPENSSL`, `PODSSH_TEST_STORE_ROOT`, `PODSSH_TEST_STORE_HOLDS`, `PODSSH_LIVE` | `POD_TLS_TEST_OPENSSL`, `POD_TLS_TEST_STORE_ROOT`, `POD_TLS_TEST_STORE_HOLDS`, `POD_TLS_LIVE` |

## Sources

`SOURCES` lists the imported files: podssh `crates/podssh-ws/src/crypto/`,
`tls.rs` (now `context.rs`, `trust.rs`, `hint.rs`), `tls/`, `stores/`,
`tls_server.rs` and `tls_server/` (now `server`), `tls_wrap.rs` (now
`wrap.rs`), `bundle.rs`, `ech/`, `crates/podssh-relay/src/tls_groups.rs`
(now `groups/dirs.rs`), and their tests and fixtures.
