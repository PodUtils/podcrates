//! The client against the CNCF distribution registry on 127.0.0.1, a
//! registry that this crate did not write (T-353, call C151): the test
//! pushes an index of linux/amd64 and linux/arm64 over the distribution
//! API, and the client resolves it for each platform and fetches the config
//! and the layer by digest. Under htpasswd (a `Basic` challenge) and under
//! token auth (a `Bearer` challenge, a token server of the test, a JWT that
//! the registry checks against the root of the test). A request with no
//! login gets 401; a blob or a manifest that does not match its digest is
//! refused; no login goes over plain HTTP.
//!
//! The registry is `POD_OCI_TEST_REGISTRY_BIN`, else `registry` on the
//! path; with neither, each test skips, unless
//! `POD_OCI_TEST_REGISTRY=required` (`scripts/ci/registry-tests.sh`).
#![cfg(target_os = "linux")]

mod common;
#[allow(dead_code)]
#[path = "common/distribution/mod.rs"]
mod distribution;

use std::sync::Arc;

use distribution::push::{Content, Pusher};
use distribution::server::{Auth, Registry};
use distribution::tls::Pki;
use distribution::token::{TokenServer, mint};
use pod_core::Class;
use pod_oci::credentials::{Credential, Logins};
use pod_oci::transport::Policy;
use pod_oci::{Client, Digest, Platform, Reference, Refusal, pull, refusal};

const REPOSITORY: &str = "pod-oci/test";
const TAG: &str = "two-platforms";

/// The made-up login for each host in `hosts`, and none for another host.
struct Login {
    hosts: Vec<String>,
    password: String,
}

impl Logins for Login {
    fn lookup(&self, host: &str) -> pod_oci::Result<Option<Credential>> {
        let cred = Credential { username: distribution::USER.into(), password: self.password.clone() };
        Ok(self.hosts.iter().any(|h| h == host).then_some(cred))
    }
}

/// A client with no proxy that trusts the root of the test alone, with the
/// made-up login for `hosts`.
fn client(registry: &Registry, hosts: &[&str]) -> Client {
    client_with(registry, hosts, distribution::PASSWORD, Policy::default())
}

fn client_with(registry: &Registry, hosts: &[&str], password: &str, policy: Policy) -> Client {
    let login = Login { hosts: hosts.iter().map(|h| h.to_string()).collect(), password: password.into() };
    let trust = pod_tls::Trust::File(registry.anchor());
    common::client::client_trusting(common::client::environment(&[]), &[], trust)
        .with_policy(policy)
        .with_logins(Arc::new(login))
}

/// The pusher of the test over TLS with `authorization`.
fn pusher(registry: &Registry, pki: &Pki, authorization: String) -> Pusher {
    Pusher { port: registry.port, tls: Some(pki.client_config()), authorization: Some(authorization) }
}

/// The bytes of the blob `digest` of `reference`, fetched by the client.
fn fetch(client: &mut Client, reference: &Reference, digest: &str, size: usize) -> pod_oci::Result<Vec<u8>> {
    let digest = Digest::parse(digest).unwrap();
    client.blob(reference.endpoint(), &reference.repository, &digest, Some(size as u64), Vec::new(), &mut Vec::new())
}

/// For each platform of `content`: the resolve of its tag gives the index
/// and the manifest of the platform, and the config and the layer of the
/// manifest come by digest with the bytes that the test pushed.
fn resolves_each_platform(client: &mut Client, registry: &Registry, content: &Content) {
    let reference = Reference::parse(&content.reference(&registry.endpoint)).unwrap();
    for image in &content.images {
        let platform = Platform::parse(&format!("linux/{}", image.arch)).unwrap();
        let resolved = pull::resolve(client, &reference, &platform)
            .unwrap_or_else(|e| panic!("linux/{}: {e}\n{}", image.arch, registry.said()));
        assert_eq!(resolved.top.digest.to_string(), content.index_digest, "the top document is the index");
        assert_eq!(resolved.top.media_type, distribution::push::INDEX);
        assert_eq!(resolved.top.bytes, content.index);
        assert_eq!(resolved.manifest_digest.to_string(), image.manifest_digest, "linux/{}", image.arch);
        assert_eq!(resolved.manifest_bytes, image.manifest);
        assert_eq!(resolved.manifest.config.digest, image.config_digest);
        let config = fetch(client, &reference, &image.config_digest, image.config.len()).unwrap();
        assert_eq!(config, image.config, "the config of linux/{}", image.arch);
        let config: serde_json::Value = serde_json::from_slice(&config).unwrap();
        assert_eq!(config["architecture"], image.arch);
        let layer = &resolved.manifest.layers[0];
        assert_eq!(layer.digest, image.layer_digest);
        let bytes = fetch(client, &reference, &image.layer_digest, image.layer.len()).unwrap();
        assert_eq!(bytes, image.layer, "the layer of linux/{}", image.arch);
    }
}

#[test]
fn an_index_of_two_platforms_resolves_through_htpasswd() {
    pod_testkit::need_loopback!();
    let pki = Pki::new();
    let Some(registry) = Registry::start("htpasswd", Auth::Htpasswd, true, &pki) else { return };
    let content = Content::new(REPOSITORY, TAG);
    content.push(&pusher(&registry, &pki, distribution::basic(distribution::USER, distribution::PASSWORD)));
    let mut client = client(&registry, &[&registry.endpoint]);
    resolves_each_platform(&mut client, &registry, &content);
    println!("distribution: an index of two platforms resolved through htpasswd");

    // A wrong password: the registry refuses the Basic answer.
    let mut wrong = client_with(&registry, &[&registry.endpoint], "another-made-up-password", Policy::default());
    let reference = Reference::parse(&content.reference(&registry.endpoint)).unwrap();
    let e = pull::resolve(&mut wrong, &reference, &Platform::parse("linux/amd64").unwrap()).err().expect("refused");
    assert_eq!(e.class, Class::Refused, "{e}");
    assert!(e.to_string().contains("HTTP 401"), "{e}");
    println!("distribution: a wrong password got 401");
}

#[test]
fn a_request_with_no_login_gets_401() {
    pod_testkit::need_loopback!();
    let pki = Pki::new();
    let Some(registry) = Registry::start("no-login", Auth::Htpasswd, true, &pki) else { return };
    let content = Content::new(REPOSITORY, TAG);
    content.push(&pusher(&registry, &pki, distribution::basic(distribution::USER, distribution::PASSWORD)));
    // The answer of the registry itself, to a request with no login.
    let bare = Pusher { port: registry.port, tls: Some(pki.client_config()), authorization: None };
    let answer = bare.send("GET", &format!("/v2/{REPOSITORY}/manifests/{TAG}"), &[], &[]);
    assert_eq!(answer.status, 401);
    let challenge = answer.header("WWW-Authenticate").expect("a challenge");
    assert!(challenge.starts_with("Basic "), "{challenge}");
    // The client with no login refuses by name, with no second ask.
    let mut client = client(&registry, &[]);
    let reference = Reference::parse(&content.reference(&registry.endpoint)).unwrap();
    let e = pull::resolve(&mut client, &reference, &Platform::parse("linux/arm64").unwrap()).err().expect("refused");
    assert_eq!(e.class, Class::Refused, "{e}");
    assert!(e.to_string().contains(&format!("no login is stored for {}", registry.endpoint)), "{e}");
    println!("distribution: a request with no login got 401");
}

#[test]
fn an_index_of_two_platforms_resolves_through_a_bearer_token() {
    pod_testkit::need_loopback!();
    let pki = Pki::new();
    let tokens = TokenServer::start(&pki);
    let auth = Auth::Token { realm: tokens.realm() };
    let Some(registry) = Registry::start("token", auth, true, &pki) else { return };
    let content = Content::new(REPOSITORY, TAG);
    let grant = [(REPOSITORY.to_string(), vec!["pull".to_string(), "push".to_string()])];
    let push_token = mint(&pki.key, &pki.leaf_der, &grant);
    content.push(&pusher(&registry, &pki, format!("Bearer {push_token}")));
    let token_host = format!("{}:{}", distribution::AT, tokens.port);
    let mut client = client(&registry, &[&token_host]);
    resolves_each_platform(&mut client, &registry, &content);
    let asked = tokens.asked();
    assert!(!asked.is_empty(), "the client asked for no token");
    for ask in &asked {
        assert!(ask.login, "an ask with no login: {}", ask.path);
        assert_eq!(distribution::query_values(&ask.path, "scope"), [format!("repository:{REPOSITORY}:pull")]);
        assert_eq!(distribution::query_values(&ask.path, "service"), [distribution::SERVICE]);
    }
    // One token for the scope: the client keeps it for each later request.
    assert_eq!(asked.len(), 1, "{asked:?}");
    println!("distribution: an index of two platforms resolved through a bearer token");

    // With no login, the token server refuses, and so does the client.
    let mut bare = client_with(&registry, &[], distribution::PASSWORD, Policy::default());
    let reference = Reference::parse(&content.reference(&registry.endpoint)).unwrap();
    let e = pull::resolve(&mut bare, &reference, &Platform::parse("linux/amd64").unwrap()).err().expect("refused");
    assert_eq!(e.class, Class::Refused, "{e}");
    assert!(e.to_string().contains("HTTP 401"), "{e}");
    println!("distribution: a token ask with no login got 401");
}

#[test]
fn a_blob_and_a_manifest_that_do_not_match_their_digests_are_refused() {
    pod_testkit::need_loopback!();
    let pki = Pki::new();
    let Some(registry) = Registry::start("digest", Auth::Htpasswd, true, &pki) else { return };
    let content = Content::new(REPOSITORY, TAG);
    content.push(&pusher(&registry, &pki, distribution::basic(distribution::USER, distribution::PASSWORD)));
    // Other bytes of the same length in the store: the registry serves
    // them under the digest of the pushed bytes. The registry parses a
    // manifest that it serves, so the change keeps it a manifest: one hex
    // digit of the digest of its config.
    let spoil = |digest: &str, at: usize| {
        let file = registry.blob_file(digest);
        let mut bytes = std::fs::read(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        bytes[at] = if bytes[at] == b'0' { b'1' } else { b'0' };
        std::fs::write(&file, bytes).unwrap();
    };
    let (amd64, arm64) = (&content.images[0], &content.images[1]);
    spoil(&arm64.layer_digest, 0);
    let hex = amd64.manifest.windows(7).position(|w| w == b"sha256:").expect("a digest in the manifest") + 7;
    spoil(&amd64.manifest_digest, hex);
    let mut client = client(&registry, &[&registry.endpoint]);
    let reference = Reference::parse(&content.reference(&registry.endpoint)).unwrap();
    let e = fetch(&mut client, &reference, &arm64.layer_digest, arm64.layer.len()).expect_err("a spoiled layer");
    match refusal(&e) {
        Some(Refusal::Digest { want, got, .. }) => {
            assert_eq!(want, &arm64.layer_digest);
            assert_ne!(got, &arm64.layer_digest);
        }
        other => panic!("not a refusal of the digest: {other:?}: {e}"),
    }
    let platform = Platform::parse("linux/amd64").unwrap();
    let e = pull::resolve(&mut client, &reference, &platform).err().expect("a spoiled manifest");
    assert!(matches!(refusal(&e), Some(Refusal::Digest { want, .. }) if *want == amd64.manifest_digest), "{e}");
    // The rest of the store is whole.
    let resolved = pull::resolve(&mut client, &reference, &Platform::parse("linux/arm64").unwrap()).unwrap();
    assert_eq!(resolved.manifest_digest.to_string(), arm64.manifest_digest);
    println!("distribution: a blob and a manifest that do not match their digests were refused");
}

#[test]
fn no_login_goes_over_plain_http() {
    pod_testkit::need_loopback!();
    let pki = Pki::new();
    let Some(registry) = Registry::start("plain", Auth::Htpasswd, false, &pki) else { return };
    let content = Content::new(REPOSITORY, TAG);
    let auth = distribution::basic(distribution::USER, distribution::PASSWORD);
    content.push(&Pusher { port: registry.port, tls: None, authorization: Some(auth) });
    let policy = Policy::with_insecure(&[&registry.endpoint]);
    let mut client = client_with(&registry, &[&registry.endpoint], distribution::PASSWORD, policy);
    let reference = Reference::parse(&content.reference(&registry.endpoint)).unwrap();
    let e = pull::resolve(&mut client, &reference, &Platform::parse("linux/amd64").unwrap()).err().expect("refused");
    assert_eq!(e.class, Class::Policy, "{e}");
    assert!(e.to_string().contains("sends no login over plain HTTP"), "{e}");
    println!("distribution: no login went over plain HTTP");
}
