# pod-tls

TLS for the crates of podcrates and their binaries: where the trust
anchors come from (a trust for each connection, or the roots of a
context in podnet's order), the rustls configuration of a client over
them with the provider that the binary gives, the words of a failed
handshake, a TLS server with a P-256 key, the split of a ClientHello
into records, and, behind features, the pure-Rust crypto provider and
Encrypted Client Hello (ECH).

The crate installs no process default provider, reads no variable, keeps
no process state and prints nothing. The binary makes one `Context` and
shares it.

## API

| Item | What |
| --- | --- |
| `Context::new(provider, settings)` | the TLS of a binary: the provider, the `Settings`, the configuration kept for each trust (16 at most, a CA file that changed by time or length is read again), the roots of the context (read one time), the groups that servers took, the state of ECH |
| `Context::config_for(&trust)`, `roots_for`, `default_roots`, `client_config(&roots)` | the configuration of a trust, kept so that TLS resumes; the roots of a trust |
| `Context::tls_over(stream, name, &trust, limit)`, `connect_over(..)` | a verified handshake over an open stream; a `pod_core::Failure` at `Stage::Tls` on failure |
| `client_config(&roots, provider, groups)`, `tls_over(stream, name, config, limit, &words)` | the same with no context |
| `Trust` (`File`, `Default`, `Caller`), `Trust::caller(config)`, `CallerConfig` | where the anchors come from, for each connection; a caller's own configuration, refused with ALPN, used as it is |
| `Context::roots()`, `Context::client_config_with(extra, alpn)`, `roots::load`, `Roots` (`store`, `sources`, `with_extra`), `CaFile` | the roots of the settings in podnet's order: the CA file alone, else the extra bundle, the first system bundle with a usable root and the compiled-in roots; each source named with its counts (`2 roots, 1 skipped`); read one time in the context, a failure too; a new configuration over them with extra roots (PEM texts) and the caller's ALPN |
| `TlsRoots`, `roots_from_bundle`, `roots_from_compiled_set`, `SYSTEM_BUNDLES`, `ROOTS_VERSION`, `ROOTS_PUBLISHED` | the roots and their sources |
| `Words`, `handshake_hint`, `handshake_failure`, `handshake_error`, `clock_refused`, `clock_hint(chain)` | the words of a failed handshake: an unknown issuer gets where a CA goes, a certificate refused by its time gets this host's clock; `clock_hint` reads the text of an error chain |
| `not_after_dates(pem)` | the notAfter (`YYYY-MM-DD`) of each certificate of a PEM text |
| `Error` (`Bundle`, `Config`, `Roots`, `Server`, `Split`) | a trust, the roots, a configuration, the pair of a server or a split that cannot be used; `From<Error> for Failure` (`Class::Config`) |
| `groups` | `GroupFile`, `Groups`, `Kept` (the group that each server took, kept across runs), `Dirs` (the file `tls-groups.txt` in the first of the consumer's directories), `HEADER` |
| `bundle` | `load_bundle`, `parse_bundle`, `pem_certificates`: the PEM reader of `CERTIFICATE` blocks |
| `stores` | `read`, `summary`, `Store`: the roots of the stores of Windows (crypt32) and macOS (Security framework) |
| `fragment` | `Split` (`mode`, `min`, `max`), `Split::parse(mode, size)`, `Mode` (`Record`, `Sni`), `Part` (`accepted`, `what`), `DEFAULT_SIZE`, `wrap(io, split)`, `fragment_with`, `Fragmenting`: the first ClientHello of a stream in records of a random size, or cut in the server name |
| `server` | `acceptor(provider, cert, key, alpn)`, `acceptor_for(provider, served, alpn)`, `certified`, `signing_key`, `certificates`, `Served` (the pair changed while the server runs, the challenges of TLS-ALPN-01), `ACME_TLS_1`, `HTTP1` |
| `crypto` (feature `pure`) | `provider()`, `suites::self_check`, `aead`, `hash`, `hkdf`, `hmac`, `kx`, `quic`, `random`, `sign`, `tls12`, `hpke`, `rsa_sig` (feature `rsa`) |
| `ech` (feature `ech`) | `Ech` (`set_mode`, `mode`, `wanted`, `outcome`, `cache`), `Mode` (`parse`, `word`), `chosen`, `open(&context, &route)`, `open_with`, `Route`, `Records`, `NoRecords`, `Published`, `Outcome`, `Cache`, `Clock`, `Lookup`, `Why`, `READ_WITHIN`, `SEALED_WAIT` |

## Features

| Feature | What |
| --- | --- |
| none (default) | the configurations, the trust, the roots, the stores, the server, the split of a ClientHello (it changes nothing unless the caller gives a split); the binary gives a provider (ring, aws-lc or the pure one) |
| `pure` | the pure-Rust `CryptoProvider`: TLS 1.3 and TLS 1.2 (ECDHE with AES-GCM or ChaCha20-Poly1305), X25519 and P-256, ECDSA and Ed25519, the keys of QUIC; no C compiler |
| `rsa` | `pure` with the RSA verifier (PKCS #1 v1.5 and PSS, 2048 to 4096 bits) and the ECDHE_RSA suites. Verify only: RUSTSEC-2023-0071 concerns the operations of `rsa` 0.9 with a private key (`deny.toml` ignores it) |
| `ech` | ECH, with the HPKE of `pure` (rustls has HPKE only from aws-lc) |

## Settings

| Field | What | podssh | podnet |
| --- | --- | --- | --- |
| `Settings::words.app` | the name of the program in the words | `podssh` | `podnet` |
| `Settings::words.replace_roots` | where a CA that replaces the roots goes | `--ca-file FILE or SSL_CERT_FILE` | `--ca-file FILE or SSL_CERT_FILE` |
| `Settings::words.add_roots` | where a CA that adds to the roots goes | `podssh-ca.pem next to the binary` | `podnet-ca.pem beside the binary` |
| `Settings::words.ech_strict` | how the user asks for strict ECH | `PODSSH_ECH=strict` | the default of `Words::new` (no ECH) |
| `Settings::extra_bundle` | a PEM bundle that adds to the default roots; a missing one is passed over | `podssh-ca.pem` next to the binary (`std::env::current_exe`, never `argv[0]`) | `podnet-ca.pem` beside the binary (`current_exe`); in `Context::roots` a file that is there and cannot be read or parsed is an error |
| `Settings::system_bundles` | the system bundles, first usable one used | `SYSTEM_BUNDLES` | `SYSTEM_BUNDLES` (the same list) |
| `Settings::ca_file` | the CA file of `Context::roots`, used alone | none: podssh gives `Trust::File` for each connection | `CaFile::new("--ca-file", path)`, else `CaFile::new("SSL_CERT_FILE", value)` when the variable is not empty |
| `context.ech().set_mode(mode)` | the mode of ECH | `Mode::parse` of `PODSSH_ECH`, then the settings file's `ECH` (`ech::chosen`) | none |
| `context.groups().keep_in(Box::new(Dirs(dirs)))` | the file of the groups | the cache's directories | none (the groups stay in memory) |
| the `Split` given to `fragment::wrap` | the split of the first ClientHello | none | `Split::parse` of `--fragment-client-hello` and `--fragment-size`, for the MASQUE connection |

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
| The tests: unit (43) and `tests/{bundle,caller_tls,certificate_structure,crypto_vectors,ech_handshake,hostname_verification,live_handshake,no_permissive_verifier,quic_keys,random_source,roots_version,rsa,signatures,system_store,tls12,tls_group,tls_resume}.rs`, with their names | [x] with the podnet tests below: 62 unit, 92 integration, 7 ignored (5 live, 2 steps of the system store); a test of a configuration runs on ring and on the pure provider |
| The doctor tests of `tests/bundle.rs` (4) | they stay with the doctor of `podssh-ws` (`pod-ws`) |
| The test of the bundle path next to the binary | it stays with `default_bundle_path` in podssh |
| `the_websocket_keys_report_a_source_that_gives_no_bytes` | it goes with the WebSocket handshake (`pod-ws`) |
| `the_binary_never_takes_a_callers_configuration` (it scans podssh-cli) | it stays in podssh |
| The fuzz target `pem_bundle` | T-017 |
| The plants `t642-hello-unsealed`, `t642-nonce-unsequenced`, `t642-ttl-floor`, `t664-group-file-open`, `t664-group-not-kept`, `t664-groups-half-read` | [x] `scripts/plants/tls-hello-unsealed`, `tls-nonce-unsequenced`, `tls-ttl-floor`, `tls-group-file-open`, `tls-group-not-kept`, `tls-groups-half-read`: the same defects, caught by the same tests |

| Capability of podnet `src/trust.rs` and `src/tls_fragment.rs` at `b7682c5` | Here |
| --- | --- |
| The order of the roots: a CA file alone (`--ca-file`, else `SSL_CERT_FILE`); else the file beside the binary, the first system bundle with a usable root and the compiled-in roots | [x] `Context::roots`, `Settings::{ca_file, extra_bundle, system_bundles}`; the consumer reads its flag, its variable and the path of its binary. As in podnet, no store of the system is read here (`Trust::Default` reads them). The files are read with the PEM reader of `bundle` (podnet used `rustls_pki_types::pem`): a file that does not parse adds no root |
| Where each root came from, with its counts (`SSL_CERT_FILE /etc/corp.pem (2 roots)`), for doctor, status and preflight | [x] `Roots::sources` |
| A certificate that is not usable as a root is skipped with a warning and counted (`1 roots, 1 skipped`); a CA file with no usable root gives the reason; the words of each error (`--ca-file PATH: the file has no certificate`) | [x] `roots::load`, `Error::Roots`; the warnings go to `tracing` |
| The roots read one time, a failure too (the statics `CA_FILE`, `ROOTS`) | [x] a field of the `Context` |
| A configuration over the roots with extra roots (PEM texts) and ALPN; no process default provider (`init_crypto`) | [x] `Context::client_config_with`, `Roots::with_extra`; the provider of the context |
| The edge roots of Cloudflare (`cf-edge-roots.pem`, `EDGE_ROOTS_FIRST_END`) and their test `the_first_end_of_the_edge_roots` | they wait for `pod-cloudflare`, which gives them as extra roots |
| The hint of the clock for a certificate that its time refuses (expired, or before its start) | [x] `clock_hint(chain)`, with the words of `pod_core::clock::hint` |
| `imf_fixdate`, `civil_from_days` | [x] `pod_core::clock::{http_date, civil_from_days}`; the test `the_dates_of_imf_fixdate` |
| `not_after_dates`, its DER reader | [x] `not_after_dates`; a time that is too short is an error, where podnet's slice panicked |
| `ROOTS_CRATE_VERSION` against Cargo.lock (`the_roots_version_is_the_one_of_cargo_lock`) | [x] `ROOTS_VERSION`, `tests/roots_version.rs`: one webpki-roots in Cargo.lock, at this version, covers both claims |
| `no_other_code_sets_its_own_verifier` (a scan of podnet's tree) | [x] its claims are in `tests/no_permissive_verifier.rs` for each crate of the workspace: only `DANGEROUS` calls `dangerous()`, each with the WebPKI verifier; the plant `tls-dangerous-call`. podnet keeps its own scan of its own tree |
| The split of the ClientHello: the modes `record` and `sni`, the sizes (16 to 4096, `N` or `MIN-MAX`, 64 to 160 by default), the first record only, 16 KiB at most, a stream that takes the record in more than one write; a bad size is an error also with `off` | [x] `fragment` |
| The split of the process (`set`, `active`) | [x] a value of the caller, given to `fragment::wrap(io, split)` |
| The words of a bad split name podnet's flags | [x] `Error::Split { part, value }` names the value and the accepted values (`Part::accepted`); the consumer adds its flag |
| The random sizes of the records | [x] `pod_core::entropy::OsEntropy`; no global state |
| The unit tests of both files, with their names | [x] `tests/roots.rs`: `an_explicit_file_is_used_alone`, `a_skipped_root_is_counted`; `hint.rs`: `the_clock_hint_names_the_host_date`, `the_dates_of_imf_fixdate`; `fragment.rs`: `the_records_carry_the_same_client_hello`, `the_sni_mode_cuts_the_name`, `other_records_and_bad_values`, `the_stream_splits_the_first_record_only`. New: `each_source_is_named_in_its_order`, `a_context_reads_its_roots_one_time`, `the_configuration_offers_the_alpn_of_the_caller`, `a_private_ca_is_trusted_only_when_it_is_given` (loopback, each provider), `a_bad_value_names_its_part_and_the_accepted_values`, `wrap_splits_only_when_the_caller_gives_a_split`, `the_not_after_of_each_certificate`, `a_certificate_that_cannot_be_read_is_an_error` |
| podnet `tests/tls.rs` (`a_private_ca_is_trusted_only_when_it_is_given`, `an_expired_certificate_names_the_host_clock`, `a_reset_in_the_tls_handshake_names_the_network`) | they run the podnet binary: they stay in podnet. `tests/roots.rs` has a test of the library with the name of the first |
| Pins | `src/trust.rs` has none: the pins of names (`--resolve`, `PODNET_RESOLVE`) are in `src/resolve.rs` (`pod-dns`, T-015), the key pin of MASQUE in `src/warp/session.rs` (`pod-warp`) |
| Fuzz targets of these files | none in podnet |

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
| podnet | `crate::init_crypto()` before each configuration | `let context = Arc::new(pod_tls::Context::new(Arc::new(rustls::crypto::ring::default_provider()), settings))`, once in `main`; podnet can keep its process default for its other code |
| podnet | `trust::set_ca_file(path)` | `settings.ca_file = Some(pod_tls::CaFile::new("--ca-file", path))` |
| podnet | `trust::CA_FILE_VARIABLE` (`SSL_CERT_FILE`, read in `trust::roots`) | podnet keeps the constant and reads the variable: with no `--ca-file` and a value that is not empty, `settings.ca_file = Some(CaFile::new("SSL_CERT_FILE", value))` |
| podnet | `trust::BESIDE_BINARY` (`podnet-ca.pem`), the path from `std::env::current_exe` | podnet keeps both: `settings.extra_bundle = current_exe().parent().join(BESIDE_BINARY)` |
| podnet | `trust::SYSTEM_BUNDLES` | `pod_tls::SYSTEM_BUNDLES` (the same list; `Settings::new` fills `system_bundles` with it) |
| podnet | `trust::ROOTS_CRATE_VERSION` | `pod_tls::ROOTS_VERSION` (and `ROOTS_PUBLISHED`) |
| podnet | `trust::EDGE_ROOTS_FIRST_END`, `cloudflare::CF_EDGE_ROOTS_PEM` | podnet keeps them until `pod-cloudflare` |
| podnet | `trust::roots()` (`anyhow::Result<Arc<Roots>>`) | `context.roots()` (`Result<Arc<pod_tls::Roots>, pod_tls::Error>`, the same words); `doctor.rs`, `main.rs` (exit 78 with `Fault::Config` when `--ca-file` cannot be used), `preflight.rs`, `status.rs` |
| podnet | `trust::Roots { store, sources }` | `pod_tls::Roots { store, sources }` |
| podnet | `trust::load(explicit, beside, bundles)` | `pod_tls::roots::load(ca_file.as_ref(), beside, &bundles)` (`bundles: &[PathBuf]`) |
| podnet | `trust::client_config(extra, alpn)` (`anyhow::Result<ClientConfig>`) | `context.client_config_with(extra, alpn)` (`Result<ClientConfig, pod_tls::Error>`); `agent.rs`, `cloudflare/mod.rs`, `cloudflare/quic.rs` (a test CA as extra root), `fetch.rs`, `net.rs`, `util/tls.rs`, `warp/exit.rs`, `warp/proxy.rs`, `warp/quic.rs`, `ws.rs`, `ws_relay.rs` |
| podnet | `net.rs` `verified_connector`: the compiled-in roots when `trust::client_config` fails | `pod_tls::client_config(&pod_tls::roots_from_compiled_set(), context.provider().clone(), context.groups().clone())` |
| podnet | `doctor.rs` `verifier`: `roots.store` and the edge roots, `WebPkiServerVerifier::builder` | `context.roots()?.with_extra(&[CF_EDGE_ROOTS_PEM])?`, `WebPkiServerVerifier::builder_with_provider(store, context.provider().clone())` |
| podnet | `trust::clock_hint(&err)` (`report.rs`) | `pod_tls::clock_hint(&format!("{err:#} {err:?}"))`; the words become `certificates are checked against this host's clock, which reads DATE`, and the test `an_expired_certificate_names_the_host_clock` of `tests/tls.rs` expects them |
| podnet | `trust::imf_fixdate(time)` (`detach.rs`; the tests of `doctor.rs` and `quicktunnel.rs`) | `pod_core::clock::http_date(secs)`, with `secs` the seconds of `time` since the Unix epoch (`i64`) |
| podnet | `trust::not_after_dates(pem)` (`anyhow::Result`) | `pod_tls::not_after_dates(pem)` (`Result<Vec<String>, String>`) |
| podnet | the test `the_roots_version_is_the_one_of_cargo_lock` | it stays in podnet with `pod_tls::ROOTS_VERSION`: podnet's Cargo.lock chooses the webpki-roots of its binary |
| podnet | the test `no_other_code_sets_its_own_verifier` | it stays in podnet for its tree; the scan of `pod-tls` covers the workspace of podcrates |
| podnet | `tls_fragment::{Mode, Split, DEFAULT_SIZE, fragment_with, Fragmenting}` | `pod_tls::fragment::*` |
| podnet | `tls_fragment::Split::parse(mode, size)` (`anyhow`, words with `--fragment-client-hello` and `--fragment-size`) | `pod_tls::fragment::Split::parse(mode, size)`; podnet makes its words from `Error::Split { part, value }`: `Part::Mode` gives `--fragment-client-hello={value:?}: give {part.accepted()}`, `Part::Size` gives `--fragment-size={value}: give {part.accepted()}`, the same text as before, with `Fault::Config` (`main.rs`) |
| podnet | `tls_fragment::set(split)`, its `tracing::info!` line | podnet keeps the split (with its other settings of `warp`) and writes the line |
| podnet | `tls_fragment::active()` | the split that podnet keeps |
| podnet | `tls_fragment::wrap(io)` (`warp/session.rs`, two calls) | `pod_tls::fragment::wrap(io, split)`; `net::BoxedIo` has the bounds of `pod_core::BoxedIo` (`AsyncRead + AsyncWrite + Send + Unpin`): podnet takes `pod_core::BoxedIo`, or boxes its stream again |
| podnet | the tests of `tests/tls.rs` | they stay in podnet; with the adoption they run on the roots of `pod-tls` |

## Sources

`SOURCES` lists the imported files: podssh `crates/podssh-ws/src/crypto/`,
`tls.rs` (now `context.rs`, `trust.rs`, `hint.rs`), `tls/`, `stores/`,
`tls_server.rs` and `tls_server/` (now `server`), `tls_wrap.rs` (now
`wrap.rs`), `bundle.rs`, `ech/`, `crates/podssh-relay/src/tls_groups.rs`
(now `groups/dirs.rs`), and their tests and fixtures; podnet
`src/trust.rs` (now `roots.rs`, `hint.rs`, `validity.rs`,
`tests/roots.rs`) and `src/tls_fragment.rs` (now `fragment.rs`) at
`b7682c5`.
