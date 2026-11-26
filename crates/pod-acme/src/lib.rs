//! An ACME client (RFC 8555): a certificate for a name from a CA (Let's
//! Encrypt by default), proven by TLS-ALPN-01 on the caller's own listener
//! (RFC 8737), so that nothing else listens and port 443 alone is needed;
//! kept in a cache directory of the caller's and renewed before its end.
//!
//! **The caller gives each part of the way.** The HTTPS client of
//! `pod-proxy` (the way out, the TLS context, the user agent), the trust of
//! the CA, the places of the state, the served certificates of `pod-tls`'s
//! server and the sink of the lines for the user. The crate reads no variable of the environment,
//! keeps no process state and prints nothing. The lines are
//! `pod_core::Event`s of the kinds `acme.account`, `acme.issued` and
//! `acme.renewal`.
//!
//! **Typed failures.** [`Failed`] keeps the words of each failure and is a
//! `pod_core::Failure` with its stage and its class.

pub mod certs;
pub mod client;
pub mod contact;
mod failed;
pub mod jws;
pub mod key;
pub mod order;
#[cfg(feature = "standin")]
pub mod standin;
pub mod store;
pub mod url;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use pod_core::{Event, Events};
use pod_tls::server::Served;
use rustls::sign::CertifiedKey;

use certs::Leaf;
use client::{Client, Http};
use store::Store;
use url::Url;

pub use contact::Contact;
pub use failed::Failed;

/// Let's Encrypt's directory, the default CA: the endpoint `directory` of
/// `letsencrypt-acme` (`pod-services`).
pub fn lets_encrypt() -> String {
    let service = pod_services::get("letsencrypt-acme").expect("pod-services holds the service letsencrypt-acme");
    service.endpoint("directory").map(|e| e.url()).expect("letsencrypt-acme of pod-services has the endpoint directory")
}

/// A certificate is renewed this long before its end, or at a third of its
/// life left when that is shorter, as Let's Encrypt advises.
pub const RENEW_BEFORE: i64 = 30 * 86_400;
/// The first wait after a renewal failed, doubled at each failure after it.
pub const FIRST_RETRY: Duration = Duration::from_secs(300);
pub const MOST_RETRY: Duration = Duration::from_secs(6 * 3600);
/// The limit of one request to the CA.
pub const REQUEST: Duration = Duration::from_secs(30);
/// The limit of one whole order, its challenges and polls included.
const ORDER: Duration = Duration::from_secs(600);
/// A sleep until a renewal wakes this often, so that a clock that jumped,
/// or a host that slept, is seen.
const NAP: Duration = Duration::from_secs(3600);

/// A name's certificate at one CA.
#[derive(Clone)]
pub struct Acme {
    pub name: String,
    pub directory: String,
    pub http: Http,
    pub store: Store,
    /// The contacts of the account (RFC 8555, section 7.3), where the CA
    /// sends the warnings of expiry and of revocation. None by default.
    pub contacts: Vec<Contact>,
    /// Where the lines for the user go: a new account, a certificate, a
    /// renewal that failed.
    pub events: Arc<dyn Events>,
}

impl std::fmt::Debug for Acme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Acme")
            .field("name", &self.name)
            .field("directory", &self.directory)
            .field("http", &self.http)
            .field("store", &self.store)
            .field("contacts", &self.contacts)
            .finish_non_exhaustive()
    }
}

impl Acme {
    /// The client for `name` at the CA of `directory` (Let's Encrypt when
    /// `None`), through `http`, its state in the first of the caller's cache
    /// directories `places` that holds it or can hold it, its lines to
    /// `events`.
    pub fn new(
        name: &str,
        directory: Option<&str>,
        http: Http,
        places: &[PathBuf],
        events: Arc<dyn Events>,
    ) -> Result<Acme, Failed> {
        let directory = directory.map_or_else(lets_encrypt, str::to_string);
        Url::parse(&directory).map_err(|why| Failed::Directory(format!("{directory}: {why}")))?;
        let store = Store::open(&directory, name, places).map_err(Failed::Store)?;
        Ok(Acme { name: name.to_string(), directory, http, store, contacts: Vec::new(), events })
    }

    /// The same client, its account with `contacts`: sent at newAccount,
    /// and set on an account that exists when its own differ. Each contact
    /// is checked when it is made ([`Contact::parse`], [`Contact::mailto`]).
    pub fn with_contacts(mut self, contacts: Vec<Contact>) -> Acme {
        self.contacts = contacts;
        self
    }

    /// The CA's host, for a line.
    pub fn host(&self) -> String {
        Url::parse(&self.directory).map(|u| u.host).unwrap_or_default()
    }

    /// The certificate kept for the name, while it holds: served at once.
    pub fn held(&self, now_secs: i64) -> Result<Option<(Arc<CertifiedKey>, Leaf)>, String> {
        let Some(text) = self.store.held()? else { return Ok(None) };
        let (pair, leaf) = certs::load(text.as_bytes(), text.as_bytes())
            .map_err(|why| format!("{}: {why}", self.store.pem_path().display()))?;
        if !leaf.names.contains(&self.name) || leaf.not_after <= now_secs {
            return Ok(None);
        }
        Ok(Some((pair, leaf)))
    }

    /// Order a certificate, keep it, and serve it from the next handshake.
    pub async fn obtain(&self, served: &Served) -> Result<Leaf, Failed> {
        let account = self.store.account().map_err(Failed::Store)?;
        let mut client = Client::open(self.http.clone(), &self.directory, account).await.map_err(Failed::Ca)?;
        client.contacts = self.contacts.clone();
        if client.account().await.map_err(Failed::Ca)? {
            let terms = client.directory.terms.clone().map(|t| format!(", under its terms ({t})")).unwrap_or_default();
            let contacts: Vec<&str> = self.contacts.iter().map(Contact::as_str).collect();
            let contacts =
                if contacts.is_empty() { String::new() } else { format!(", contact {}", contacts.join(", ")) };
            self.events
                .event(Event::info("acme.account", format!("a new account at {}{terms}{contacts}", self.host())));
        }
        let ordered = order::order(&mut client, &self.name, served);
        let issued = match tokio::time::timeout(ORDER, ordered).await {
            Ok(issued) => issued.map_err(Failed::Ca)?,
            Err(_) => return Err(Failed::Late(format!("no certificate within {} s", ORDER.as_secs()))),
        };
        let path = self.store.keep(&issued.key, &issued.chain).map_err(Failed::Store)?;
        // What is served is what was kept: the file, read again.
        let text = self.store.held().map_err(Failed::Store)?;
        let text = text.ok_or_else(|| Failed::Store(format!("{} is gone", path.display())))?;
        let (pair, leaf) = certs::load(text.as_bytes(), text.as_bytes()).map_err(Failed::Store)?;
        served.set(pair);
        Ok(leaf)
    }
}

/// When a certificate of this life is renewed: 30 days before its end, or
/// at a third of its life left for a shorter one.
pub fn renew_at(leaf: &Leaf) -> i64 {
    let life = leaf.not_after.saturating_sub(leaf.not_before).max(0);
    leaf.not_after - (life / 3).min(RENEW_BEFORE)
}

/// The wait after a renewal that failed, when the wait before it was `last`.
pub fn next_retry(last: Duration) -> Duration {
    (last * 2).min(MOST_RETRY)
}

/// "for NAME, until DATE", of a leaf.
pub fn about(leaf: &Leaf) -> String {
    let names = if leaf.names.is_empty() { "no DNS name".to_string() } else { leaf.names.join(", ") };
    format!("for {names}, until {}", utc(leaf.not_after))
}

/// A time of `secs` since the epoch as RFC 3339 in UTC, to the second.
pub fn utc(secs: i64) -> String {
    let at = std::time::UNIX_EPOCH + Duration::from_secs(u64::try_from(secs).unwrap_or(0));
    pod_core::clock::rfc3339(at)
}

/// Keep the name's certificate for as long as the caller runs: the first
/// one now when `held` is none, then each renewal at its time. A renewal
/// that fails is said and asked again later, and the certificate of before
/// stays. Ends only when there is no certificate to serve: the first order
/// failed, whose failure names the CA and the name.
pub async fn keep(acme: Acme, served: Arc<Served>, mut held: Option<Leaf>) -> Failed {
    let mut wait = FIRST_RETRY;
    loop {
        if let Some(leaf) = &held {
            sleep_until(renew_at(leaf)).await;
        }
        match acme.obtain(&served).await {
            Ok(leaf) => {
                let (from, renewed) = (acme.host(), utc(renew_at(&leaf)));
                let line = format!("a certificate from {from} {}; renewed from {renewed}", about(&leaf));
                acme.events.event(Event::info("acme.issued", line));
                held = Some(leaf);
                wait = FIRST_RETRY;
            }
            Err(why) => match &held {
                None => {
                    return why.within(
                        format!("ACME at {}: ", acme.host()),
                        format!("; no certificate for {} to serve", acme.name),
                    );
                }
                Some(leaf) => {
                    acme.events.event(Event::warn(
                        "acme.renewal",
                        format!(
                            "the renewal of the certificate {} failed: {why}; it stays, and ACME is asked again in {} min",
                            about(leaf),
                            wait.as_secs() / 60
                        ),
                    ));
                    tokio::time::sleep(wait).await;
                    wait = next_retry(wait);
                }
            },
        }
    }
}

/// Sleep until `at`, seconds since the epoch, by the wall clock.
async fn sleep_until(at: i64) {
    loop {
        let left = at.saturating_sub(order::now_secs());
        if left <= 0 {
            return;
        }
        tokio::time::sleep(NAP.min(Duration::from_secs(left as u64))).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(not_before: i64, days: i64) -> Leaf {
        Leaf { names: vec!["relay.podssh.test".into()], not_before, not_after: not_before + days * 86_400 }
    }

    #[test]
    fn a_certificate_is_renewed_30_days_before_its_end_or_at_a_third_of_a_short_life() {
        // Let's Encrypt's 90 days: day 60.
        assert_eq!(renew_at(&leaf(0, 90)), 60 * 86_400);
        // A life of a year: still 30 days before.
        assert_eq!(renew_at(&leaf(0, 365)), 335 * 86_400);
        // Six days (Let's Encrypt's short-lived profile): two days before.
        assert_eq!(renew_at(&leaf(0, 6)), 4 * 86_400);
        // Pebble's 180 s of the harness: 60 s before.
        let short = Leaf { names: vec![], not_before: 1000, not_after: 1180 };
        assert_eq!(renew_at(&short), 1120);
    }

    #[test]
    fn a_failed_renewal_is_asked_again_later_each_time_up_to_6_hours() {
        let mut wait = FIRST_RETRY;
        let mut waits = vec![wait.as_secs()];
        for _ in 0..8 {
            wait = next_retry(wait);
            waits.push(wait.as_secs());
        }
        assert_eq!(waits, [300, 600, 1200, 2400, 4800, 9600, 19200, 21600, 21600]);
    }

    #[test]
    fn a_leaf_is_said_with_its_names_and_its_end() {
        assert_eq!(about(&leaf(1_793_620_800, 90)), "for relay.podssh.test, until 2027-01-31T12:00:00Z");
    }
}
