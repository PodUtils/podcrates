//! The stand-in CA checks what a CA checks (feature `standin`): the
//! certificate of TLS-ALPN-01 (RFC 8737, section 3), each JWS (RFC 8555,
//! section 6), the contacts, the owner of each object, the state of an
//! order and the CSR.

mod common;

use std::sync::Arc;

use base64::Engine as _;
use common::{NAME, listener, listener_with, provider};
use pod_acme::certs::{self, Key};
use pod_acme::client::{Client, Http, json_of};
use pod_acme::jws::Account;
use pod_acme::standin::validate::validate;
use pod_acme::standin::{Plan, Standin};
use pod_proxy::http::Response;
use pod_tls::server::{HTTP1, Served};
use serde_json::{Value, json};

fn now() -> i64 {
    pod_acme::order::now_secs()
}

/// A served pair of the certificate PEM `cert` and its key.
fn pair(cert: &str, key: &str) -> Arc<rustls::sign::CertifiedKey> {
    pod_tls::server::certified(cert.as_bytes(), key.as_bytes()).unwrap()
}

/// A certificate of TLS-ALPN-01 as `certs::challenge` makes it, with the
/// names `names` and an acmeIdentifier that is critical or not.
fn challenge_with(names: &[&str], key_authorization: &str, critical: bool) -> Arc<rustls::sign::CertifiedKey> {
    use sha2::Digest as _;
    let key = Key::generate().unwrap();
    let mut params = rcgen::CertificateParams::new(names.iter().map(|n| n.to_string()).collect::<Vec<_>>()).unwrap();
    params.serial_number = Some(rcgen::SerialNumber::from_slice(&[1, 2, 3]));
    let digest = sha2::Sha256::digest(key_authorization.as_bytes());
    let mut extension = rcgen::CustomExtension::new_acme_identifier(&digest[..]);
    extension.set_criticality(critical);
    params.custom_extensions = vec![extension];
    let cert = params.self_signed(&key).unwrap();
    pair(&certs::pem("CERTIFICATE", cert.der()), &key.pem())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_stand_in_refuses_a_certificate_of_tls_alpn_01_that_rfc_8737_does_not_allow() {
    pod_testkit::need_loopback!();
    let served = Served::new(None);
    let at = listener(served.clone()).await;
    let ka = "token-1.thumbprint";
    let check = |served: &Served, chosen: Arc<rustls::sign::CertifiedKey>| {
        served.set_challenge(NAME, chosen);
        validate(provider(), at, NAME, ka)
    };
    let (cert, key) = certs::challenge(NAME, ka, now()).unwrap();
    check(&served, pair(&cert, &key)).await.expect("the certificate of RFC 8737");
    validate(provider(), at, &NAME.to_uppercase(), ka).await.expect("a name in another case");

    let (cert, key) = certs::challenge(NAME, "token-1.another", now()).unwrap();
    let err = check(&served, pair(&cert, &key)).await.unwrap_err();
    assert_eq!(err, "the acmeIdentifier is not the SHA-256 of the key authorization");

    let own = Key::generate().unwrap();
    let err = check(&served, pair(&certs::own(NAME, &own, now()).unwrap(), &own.pem())).await.unwrap_err();
    assert_eq!(err, "the certificate has 0 acmeIdentifier extensions, not one");

    let err = check(&served, challenge_with(&[NAME], ka, false)).await.unwrap_err();
    assert_eq!(err, "the acmeIdentifier extension is not critical");
    check(&served, challenge_with(&[NAME], ka, true)).await.expect("the same, critical");

    let err = check(&served, challenge_with(&[NAME, "www.podacme.test"], ka, true)).await.unwrap_err();
    assert!(
        err.starts_with("the subjectAltName is ") && err.ends_with(", not the one dNSName files.podacme.test"),
        "{err}"
    );
    let err = check(&served, challenge_with(&["other.podacme.test"], ka, true)).await.unwrap_err();
    assert!(err.ends_with("not the one dNSName files.podacme.test"), "{err}");

    // No challenge for the name: the listener gives no certificate.
    let err = validate(provider(), at, "other.podacme.test", ka).await.unwrap_err();
    assert!(err.starts_with("the TLS handshake with "), "{err}");

    // A listener that does not choose acme-tls/1.
    let plain = Served::new(None);
    let (cert, key) = certs::challenge(NAME, ka, now()).unwrap();
    plain.set_challenge(NAME, pair(&cert, &key));
    let no_alpn = listener_with(plain.clone(), &[]).await;
    let err = validate(provider(), no_alpn, NAME, ka).await.unwrap_err();
    assert_eq!(err, format!("{no_alpn} did not choose the protocol acme-tls/1 for {NAME}"));
    let http_only = listener_with(plain, &[HTTP1]).await;
    assert!(validate(provider(), http_only, NAME, ka).await.is_err());
}

/// A raw JWS POST of `account` to `url`, with a nonce of `nonce_url`.
async fn send(
    http: &Http,
    account: &Account,
    kid: Option<&str>,
    url: &str,
    signed_url: &str,
    nonce: &str,
    payload: Option<&Value>,
) -> Response {
    let body = account.sign(kid, nonce, signed_url, payload).unwrap();
    http.send("POST", url, true, "application/json", body.as_bytes()).await.unwrap()
}

async fn nonce(http: &Http, url: &str) -> String {
    let answer = http.send("GET", url, false, "*/*", b"").await.unwrap();
    answer.header("replay-nonce").unwrap().to_string()
}

/// The problem type of an answer, past the prefix of ACME, and its status.
fn problem(answer: &Response) -> (u16, String) {
    let v: Value = serde_json::from_slice(&answer.body).unwrap_or(Value::Null);
    let kind = v["type"].as_str().unwrap_or_default().trim_start_matches("urn:ietf:params:acme:error:");
    (answer.status, kind.to_string())
}

/// What a JWS does wrong, its `kid`, the `url` that it signs, and the
/// status and the problem that it gets.
type Case<'a> = (&'a str, Option<&'a str>, &'a str, (u16, &'a str));

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_stand_in_checks_each_jws_as_a_ca_does() {
    pod_testkit::need_loopback!();
    let served = Served::new(None);
    let standin = Standin::start(provider(), Plan::new(listener(served).await)).await.unwrap();
    let http = standin.http().unwrap();
    let dir: Value =
        json_of(&http.send("GET", &standin.directory, false, "application/json", b"").await.unwrap()).unwrap();
    let url = |key: &str| dir[key].as_str().unwrap().to_string();
    let (new_nonce, new_account, new_order) = (url("newNonce"), url("newAccount"), url("newOrder"));
    let terms = json!({"termsOfServiceAgreed": true});
    let order = json!({"identifiers": [{"type": "dns", "value": NAME}]});

    let alice = Account::generate().unwrap();
    let n = nonce(&http, &new_nonce).await;
    let made = send(&http, &alice, None, &new_account, &new_account, &n, Some(&terms)).await;
    assert_eq!(made.status, 201);
    let kid = made.header("location").unwrap().to_string();
    // The same nonce again.
    let again = send(&http, &alice, None, &new_account, &new_account, &n, Some(&terms)).await;
    assert_eq!(problem(&again), (400, "badNonce".into()));

    // A request of the account that another key signed.
    let mallory = Account::generate().unwrap();
    let n = nonce(&http, &new_nonce).await;
    let forged = send(&http, &mallory, Some(&kid), &new_order, &new_order, &n, Some(&order)).await;
    assert_eq!(problem(&forged), (400, "malformed".into()));
    assert!(String::from_utf8_lossy(&forged.body).contains("the JWS signature does not verify"));

    let cases: [Case<'_>; 4] = [
        ("a jwk after newAccount", None, new_order.as_str(), (400, "malformed")),
        (
            "a kid that names no account",
            Some("https://127.0.0.1:1/acct/9"),
            new_order.as_str(),
            (400, "accountDoesNotExist"),
        ),
        ("the url of another resource", Some(kid.as_str()), new_account.as_str(), (403, "unauthorized")),
        ("a kid at newAccount", Some(kid.as_str()), new_account.as_str(), (400, "malformed")),
    ];
    for (what, kid, signed_url, want) in cases {
        let n = nonce(&http, &new_nonce).await;
        let to = if what == "a kid at newAccount" { &new_account } else { &new_order };
        let answer = send(&http, &alice, kid, to, signed_url, &n, Some(&order)).await;
        assert_eq!(problem(&answer), (want.0, want.1.to_string()), "{what}: {}", String::from_utf8_lossy(&answer.body));
    }
    // Not of the type of JOSE.
    let n = nonce(&http, &new_nonce).await;
    let body = alice.sign(Some(&kid), &n, &new_order, Some(&order)).unwrap();
    let plain = http.send("POST", &new_order, false, "application/json", body.as_bytes()).await.unwrap();
    assert_eq!(problem(&plain), (415, "malformed".into()));

    // The contacts of a new account.
    for (contact, want) in
        [("tel:+12025550100", "unsupportedContact"), ("mailto:a@example.org,b@example.org", "invalidContact")]
    {
        let n = nonce(&http, &new_nonce).await;
        let payload = json!({"termsOfServiceAgreed": true, "contact": [contact]});
        let answer =
            send(&http, &Account::generate().unwrap(), None, &new_account, &new_account, &n, Some(&payload)).await;
        assert_eq!(problem(&answer), (400, want.to_string()), "{contact}");
    }
    let n = nonce(&http, &new_nonce).await;
    let no_terms = send(&http, &mallory, None, &new_account, &new_account, &n, Some(&json!({}))).await;
    assert_eq!(problem(&no_terms), (400, "malformed".into()));

    // An order: not ready for its finalization, and its own account's alone.
    let n = nonce(&http, &new_nonce).await;
    let ordered = send(&http, &alice, Some(&kid), &new_order, &new_order, &n, Some(&order)).await;
    assert_eq!(ordered.status, 201);
    let body = json_of(&ordered).unwrap();
    let finalize = body["finalize"].as_str().unwrap().to_string();
    let n = nonce(&http, &new_nonce).await;
    let early = send(&http, &alice, Some(&kid), &finalize, &finalize, &n, Some(&json!({"csr": "AA"}))).await;
    assert_eq!(problem(&early), (403, "orderNotReady".into()));
    let n = nonce(&http, &new_nonce).await;
    let made = send(&http, &mallory, None, &new_account, &new_account, &n, Some(&terms)).await;
    let mallory_kid = made.header("location").unwrap().to_string();
    let order_url = ordered.header("location").unwrap().to_string();
    let n = nonce(&http, &new_nonce).await;
    let other = send(&http, &mallory, Some(&mallory_kid), &order_url, &order_url, &n, None).await;
    assert_eq!(problem(&other), (403, "unauthorized".into()));
    let wildcard = json!({"identifiers": [{"type": "dns", "value": "*.podacme.test"}]});
    let n = nonce(&http, &new_nonce).await;
    let refused = send(&http, &alice, Some(&kid), &new_order, &new_order, &n, Some(&wildcard)).await;
    assert_eq!(problem(&refused), (400, "rejectedIdentifier".into()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_stand_in_issues_for_a_csr_of_the_names_of_the_order_and_a_key_of_its_own() {
    pod_testkit::need_loopback!();
    let served = Served::new(None);
    let standin = Standin::start(provider(), Plan::new(listener(served.clone()).await)).await.unwrap();
    let account = Account::generate().unwrap();
    let account_key = Key::from_pkcs8(&certs::pem_block(&account.pem(), "PRIVATE KEY").unwrap()).unwrap();
    let mut client = Client::open(standin.http().unwrap(), &standin.directory, account).await.unwrap();
    assert!(client.account().await.unwrap());
    let new_order = client.directory.new_order.clone();
    let order = json!({"identifiers": [{"type": "dns", "value": NAME}]});
    let ordered = client.post(&new_order, Some(&order), "application/json").await.unwrap();
    let order_url = ordered.header("location").unwrap().to_string();
    let body = json_of(&ordered).unwrap();
    let authz_url = body["authorizations"][0].as_str().unwrap().to_string();
    let finalize = body["finalize"].as_str().unwrap().to_string();

    let (authz, _) = client.get_json(&authz_url).await.unwrap();
    let tls = authz["challenges"].as_array().unwrap().iter().find(|c| c["type"] == "tls-alpn-01").unwrap().clone();
    let http01 = authz["challenges"].as_array().unwrap().iter().find(|c| c["type"] == "http-01").unwrap().clone();
    let refused = client.post(http01["url"].as_str().unwrap(), Some(&json!({})), "application/json").await.unwrap();
    assert_eq!(problem(&refused), (400, "malformed".into()), "http-01 is listed, not validated");
    let ka = client.key().key_authorization(tls["token"].as_str().unwrap());
    let (cert, key) = certs::challenge(NAME, &ka, now()).unwrap();
    served.set_challenge(NAME, pair(&cert, &key));
    let answer = client.post(tls["url"].as_str().unwrap(), Some(&json!({})), "application/json").await.unwrap();
    assert_eq!(json_of(&answer).unwrap()["status"], "processing");
    let mut status = String::new();
    for _ in 0..50 {
        let (authz, _) = client.get_json(&authz_url).await.unwrap();
        status = authz["status"].as_str().unwrap().to_string();
        if status != "pending" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(status, "valid");
    let (order_now, _) = client.get_json(&order_url).await.unwrap();
    assert_eq!(order_now["status"], "ready");

    let b64 = |der: Vec<u8>| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(der);
    let own = Key::generate().unwrap();
    let bad_csrs = [
        (b64(certs::csr("other.podacme.test", &own).unwrap()), "names"),
        (b64(certs::csr(NAME, &account_key).unwrap()), "the key of the account"),
        (b64(certs::tunnel_csr(NAME, &own).unwrap()), "names"),
    ];
    for (csr, why) in bad_csrs {
        let answer = client.post(&finalize, Some(&json!({ "csr": csr })), "application/json").await.unwrap();
        assert_eq!(problem(&answer), (400, "badCSR".into()), "{why}");
        assert!(
            String::from_utf8_lossy(&answer.body).contains(why),
            "{why}: {}",
            String::from_utf8_lossy(&answer.body)
        );
    }
    let mut forged = certs::csr(NAME, &own).unwrap();
    let last = forged.len() - 1;
    forged[last] ^= 1;
    let answer = client.post(&finalize, Some(&json!({ "csr": b64(forged) })), "application/json").await.unwrap();
    assert_eq!(problem(&answer), (400, "badCSR".into()), "a CSR whose signature does not verify");

    let answer = client
        .post(&finalize, Some(&json!({ "csr": b64(certs::csr(NAME, &own).unwrap()) })), "application/json")
        .await
        .unwrap();
    let valid = json_of(&answer).unwrap();
    assert_eq!(valid["status"], "valid");
    let chain = client.post(valid["certificate"].as_str().unwrap(), None, pod_acme::client::PEM_CHAIN).await.unwrap();
    let (_, leaf) = certs::load(&chain.body, own.pem().as_bytes()).expect("the chain loads with the key of the CSR");
    assert_eq!(leaf.names, [NAME]);
    assert_eq!(standin.seen().issued, [vec![NAME.to_string()]]);
}
