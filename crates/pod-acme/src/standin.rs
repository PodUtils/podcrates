//! A stand-in of an ACME CA on the loopback (feature `standin`), for the
//! tests of this crate and of a consumer: one full order with no CA of the
//! internet. A consumer turns the feature on in `[dev-dependencies]` only.
//!
//! * [`ca`]: a test CA, made at the start: it signs the certificate of the
//!   stand-in's HTTPS and each certificate that an order issues.
//! * [`serve`]: HTTPS on the loopback, HTTP/1.1, with a connection kept for
//!   each next request.
//! * [`signed`]: the check of each JWS POST (RFC 8555, section 6): ES256,
//!   the signature, a nonce that the stand-in gave and that no request used,
//!   the `url` of the request, `jwk` for newAccount alone and `kid` of a
//!   known account for each other request.
//! * [`flow`]: the directory, the nonces, the account (its contacts
//!   checked, an update), the order, its authorization, the finalization
//!   with a CSR and the certificate, each object of its own account alone.
//! * [`validate`]: TLS-ALPN-01 (RFC 8737, section 3): a connection to the
//!   caller's listener with ALPN `acme-tls/1` alone and the name as SNI,
//!   and the check of its certificate.
//!
//! The stand-in checks what a CA checks for each step that it implements,
//! and refuses with the problem types of RFC 8555, section 6.7. It offers
//! TLS-ALPN-01 alone (an `http-01` that it lists is refused), takes a P-256
//! key alone in a CSR, issues at once (no state `processing`), and gives no
//! revocation, no key change, no pre-authorization and no external account.
//! Its TLS takes the provider of the caller.

pub mod ca;
pub mod flow;
pub mod serve;
pub mod signed;
pub mod validate;

use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use rustls::crypto::CryptoProvider;
use tokio::task::JoinHandle;

use crate::client::Http;

/// What the stand-in does.
#[derive(Clone, Debug)]
pub struct Plan {
    /// Where the stand-in connects for TLS-ALPN-01 of each name: the
    /// caller's listener, in place of port 443 of the name's address.
    pub validate_at: SocketAddr,
    /// The contacts that the stand-in refuses with `invalidContact`, as a
    /// CA refuses an address that it does not take.
    pub refused_contacts: Vec<String>,
    /// The first nonces of good requests that the stand-in refuses with
    /// `badNonce`, as Pebble refuses some on purpose.
    pub bad_nonces: usize,
    /// The life of an issued certificate, in days.
    pub days: i64,
}

impl Plan {
    /// A plan that validates at `validate_at`, refuses no contact and no
    /// good nonce, and issues for 90 days.
    pub fn new(validate_at: SocketAddr) -> Plan {
        Plan { validate_at, refused_contacts: Vec::new(), bad_nonces: 0, days: 90 }
    }
}

/// What the stand-in saw.
#[derive(Clone, Debug, Default)]
pub struct Seen {
    /// The contacts of each account, by its number from 1, as they are now.
    pub accounts: Vec<Vec<String>>,
    /// Each refusal, as `TYPE: DETAIL` (the type past the prefix of ACME).
    pub refusals: Vec<String>,
    /// The result of each TLS-ALPN-01 validation, with the name.
    pub validations: Vec<(String, Result<(), String>)>,
    /// The names of each certificate issued.
    pub issued: Vec<Vec<String>>,
    /// The good nonces refused by [`Plan::bad_nonces`].
    pub bad_nonces: usize,
}

/// A stand-in CA that runs on the loopback until it is dropped.
pub struct Standin {
    /// The URL of the directory: `https://127.0.0.1:PORT/dir`.
    pub directory: String,
    /// The certificate of the test CA, as PEM: the root of the stand-in's
    /// HTTPS and of each issued chain.
    pub ca_pem: String,
    provider: Arc<CryptoProvider>,
    state: Arc<Mutex<flow::State>>,
    task: JoinHandle<()>,
}

impl Standin {
    /// Start a stand-in on the runtime of the caller, its TLS on
    /// `provider`, as `plan` says.
    pub async fn start(provider: Arc<CryptoProvider>, plan: Plan) -> io::Result<Standin> {
        let ca = ca::Ca::new().map_err(io::Error::other)?;
        let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
        let base = format!("https://{}", listener.local_addr()?);
        let acceptor = ca.acceptor(provider.clone()).map_err(io::Error::other)?;
        let ca_pem = ca.pem.clone();
        let state = Arc::new(Mutex::new(flow::State::new(base.clone(), plan, ca)));
        let task = tokio::spawn(serve::run(listener, acceptor, state.clone(), provider.clone()));
        Ok(Standin { directory: format!("{base}{}", flow::DIRECTORY), ca_pem, provider, state, task })
    }

    /// The trust of the stand-in's HTTPS: its test CA alone, on the
    /// provider of the stand-in.
    pub fn trust(&self) -> Result<pod_tls::Trust, String> {
        let der = crate::certs::pem_block(&self.ca_pem, "CERTIFICATE").ok_or("the test CA has no PEM block")?;
        let mut roots = rustls::RootCertStore::empty();
        roots.add(rustls_pki_types::CertificateDer::from(der.to_vec())).map_err(|e| format!("the test CA: {e}"))?;
        let config = rustls::ClientConfig::builder_with_provider(self.provider.clone())
            .with_safe_default_protocol_versions()
            .map_err(|e| format!("the TLS versions: {e}"))?
            .with_root_certificates(roots)
            .with_no_client_auth();
        pod_tls::Trust::caller(Arc::new(config)).map_err(|e| e.to_string())
    }

    /// The way of a client to the stand-in: a direct dial, the system
    /// resolver, the trust of [`Standin::trust`], and [`crate::REQUEST`].
    pub fn http(&self) -> Result<Http, String> {
        let words = pod_tls::Words::new("pod-acme");
        let context = Arc::new(pod_tls::Context::new(self.provider.clone(), pod_tls::Settings::new(words)));
        let settings = pod_proxy::Settings::new("pod-acme", pod_proxy::ProxyChoice::Direct);
        let dial = Arc::new(pod_proxy::Dialer::new(settings, Arc::new(pod_proxy::resolve::System::new())));
        let client = pod_proxy::Client::new(context, dial);
        Ok(Http { client, trust: self.trust()?, timeout: crate::REQUEST })
    }

    /// What the stand-in saw until now.
    pub fn seen(&self) -> Seen {
        lock(&self.state).seen.clone()
    }
}

impl Drop for Standin {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// The state, also after a task that held it panicked.
pub(crate) fn lock(state: &Mutex<flow::State>) -> MutexGuard<'_, flow::State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}
