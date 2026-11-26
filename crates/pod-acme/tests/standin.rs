//! Whole orders of `Acme::obtain` against the stand-in CA (feature
//! `standin`): the account with its contacts, the order, TLS-ALPN-01 on the
//! caller's listener, the finalization and a chain of the test CA; a
//! contact that the CA refuses; the contacts of an account that exists.

mod common;

use std::sync::Arc;

use common::{NAME, events, listener, places, provider};
use pod_acme::standin::{Plan, Standin};
use pod_acme::{Acme, Contact, Failed};
use pod_tls::server::Served;
use rustls::client::danger::ServerCertVerifier as _;
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};

fn contact(text: &str) -> Contact {
    Contact::parse(text).unwrap()
}

/// The certificates of a PEM file, in order.
fn chain(pem: &str) -> Vec<CertificateDer<'static>> {
    x509_parser::pem::Pem::iter_from_buffer(pem.as_bytes())
        .filter_map(Result::ok)
        .filter(|block| block.label == "CERTIFICATE")
        .map(|block| CertificateDer::from(block.contents))
        .collect()
}

/// The chain verifies for `name` against the test CA alone, as a client of
/// the server checks it.
fn verifies(standin: &Standin, pem: &str, name: &str) -> Result<(), String> {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(chain(&standin.ca_pem).remove(0)).unwrap();
    let verifier =
        rustls::client::WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider()).build().unwrap();
    let certs = chain(pem);
    let name = ServerName::try_from(name.to_string()).unwrap();
    verifier
        .verify_server_cert(&certs[0], &certs[1..], &name, &[], UnixTime::now())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_whole_order_gives_a_chain_of_the_ca_and_the_account_has_its_contact() {
    pod_testkit::need_loopback!();
    let served = Served::new(None);
    let mut plan = Plan::new(listener(served.clone()).await);
    plan.bad_nonces = 2;
    let standin = Standin::start(provider(), plan).await.unwrap();
    let (sink, lines) = events();
    let acme = Acme::new(NAME, Some(&standin.directory), standin.http().unwrap(), &places("order"), sink)
        .unwrap()
        .with_contacts(vec![contact("mailto:ops@example.org")]);

    let leaf = acme.obtain(&served).await.expect("a certificate from the stand-in");
    assert_eq!(leaf.names, [NAME]);
    assert!(leaf.not_after > pod_acme::order::now_secs() + 80 * 86_400, "a life of 90 days");
    let current = served.current().expect("the certificate is served");
    let (_, held) = acme.held(pod_acme::order::now_secs()).unwrap().expect("the certificate is kept");
    assert_eq!(held, leaf);
    assert_eq!(current.cert.len(), 2, "the leaf and the CA");
    let kept = std::fs::read_to_string(acme.store.pem_path()).unwrap();
    verifies(&standin, &kept, NAME).expect("the chain verifies against the test CA");
    assert!(verifies(&standin, &kept, "other.podacme.test").is_err(), "for its name alone");

    let seen = standin.seen();
    assert_eq!(seen.accounts, [vec!["mailto:ops@example.org".to_string()]]);
    assert_eq!(seen.validations, [(NAME.to_string(), Ok(()))]);
    assert_eq!(seen.issued, [vec![NAME.to_string()]]);
    assert_eq!(seen.bad_nonces, 2, "the client sent each refused nonce again");
    assert_eq!(seen.refusals.len(), 2, "{:?}", seen.refusals);
    let host = standin.directory.trim_start_matches("https://").split('/').next().unwrap().to_string();
    let lines = lines.lock().unwrap().clone();
    assert_eq!(
        lines,
        [format!(
            "acme.account: a new account at 127.0.0.1, under its terms (https://{host}/terms), contact mailto:ops@example.org"
        )]
    );

    // A renewal finds the account by its key and orders again.
    acme.obtain(&served).await.expect("a second certificate");
    let seen = standin.seen();
    assert_eq!(seen.accounts.len(), 1, "the account is made once");
    assert_eq!(seen.validations.len(), 2);
    assert_eq!(seen.issued.len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_contact_that_the_ca_refuses_ends_the_order_with_a_failure_that_names_it() {
    pod_testkit::need_loopback!();
    let served = Served::new(None);
    let mut plan = Plan::new(listener(served.clone()).await);
    plan.refused_contacts = vec!["mailto:nobody@example.org".into()];
    let standin = Standin::start(provider(), plan).await.unwrap();
    let (sink, lines) = events();
    let contacts = vec![contact("mailto:ops@example.org"), contact("mailto:nobody@example.org")];
    let acme = Acme::new(NAME, Some(&standin.directory), standin.http().unwrap(), &places("refused"), sink)
        .unwrap()
        .with_contacts(contacts);

    let failed = acme.obtain(&served).await.unwrap_err();
    let Failed::Ca(text) = &failed else { panic!("not a failure of the CA: {failed:?}") };
    assert_eq!(
        text,
        "the account: the CA refuses the contact mailto:nobody@example.org: invalidContact: the contact \
         \"mailto:nobody@example.org\": the CA refuses this address"
    );
    let seen = standin.seen();
    assert!(seen.accounts.is_empty() && seen.validations.is_empty() && seen.issued.is_empty(), "{seen:?}");
    assert!(lines.lock().unwrap().is_empty());
    assert!(served.current().is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_account_that_exists_takes_the_contacts_that_the_user_adds_later() {
    pod_testkit::need_loopback!();
    let served = Served::new(None);
    let standin = Standin::start(provider(), Plan::new(listener(served.clone()).await)).await.unwrap();
    let places = places("update");
    let (sink, _) = events();
    let first = Acme::new(NAME, Some(&standin.directory), standin.http().unwrap(), &places, sink.clone()).unwrap();
    first.obtain(&served).await.expect("a certificate with no contact");
    assert_eq!(standin.seen().accounts, [Vec::<String>::new()]);

    let later = Acme::new(NAME, Some(&standin.directory), standin.http().unwrap(), &places, sink)
        .unwrap()
        .with_contacts(vec![contact("mailto:ops@example.org")]);
    later.obtain(&served).await.expect("a certificate with the contact");
    assert_eq!(standin.seen().accounts, [vec!["mailto:ops@example.org".to_string()]], "one account, updated");

    // The same contacts again: no update, and no refusal.
    later.obtain(&served).await.unwrap();
    let seen = standin.seen();
    assert_eq!(seen.accounts.len(), 1);
    assert!(seen.refusals.is_empty(), "{:?}", seen.refusals);
}
