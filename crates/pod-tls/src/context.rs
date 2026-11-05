//! The TLS of a binary in one value: the provider that the binary chose,
//! its settings, the configuration kept for each trust, the groups that
//! servers took, and the state of ECH. The binary makes one [`Context`] and
//! shares it (`Arc<Context>`); a test makes its own.

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use pod_core::{Class, Failure, Stage};
use rustls::ClientConfig;
use rustls::crypto::CryptoProvider;

use crate::error::Error;
use crate::groups::Kept;
use crate::hint::{Words, handshake_failure};
use crate::kept::Configs;
use crate::trust::{self, TlsRoots, Trust};

/// The settings of a [`Context`], which the consumer fills once: in place of
/// the variables and the files that podssh read itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The names of the consumer in the words of a failure.
    pub words: Words,
    /// A PEM bundle that adds to the default roots; podssh's is
    /// `podssh-ca.pem` next to its binary (found with
    /// `std::env::current_exe`, never `argv[0]`). Unreadable, it is passed
    /// over.
    pub extra_bundle: Option<PathBuf>,
    /// The system bundles of the default trust, most common first: the first
    /// readable one is used. [`trust::SYSTEM_BUNDLES`] by default.
    pub system_bundles: Vec<PathBuf>,
}

impl Settings {
    /// The settings with `words`, no extra bundle and the system bundles of
    /// [`trust::SYSTEM_BUNDLES`].
    pub fn new(words: Words) -> Settings {
        Settings {
            words,
            extra_bundle: None,
            system_bundles: trust::SYSTEM_BUNDLES.iter().map(PathBuf::from).collect(),
        }
    }
}

/// The TLS of a binary: see the module.
pub struct Context {
    provider: Arc<CryptoProvider>,
    settings: Settings,
    configs: Configs,
    groups: Arc<Kept>,
    #[cfg(feature = "ech")]
    pub(crate) ech: crate::ech::Ech,
}

impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context")
            .field("settings", &self.settings)
            .field("groups", &self.groups)
            .finish_non_exhaustive()
    }
}

impl Context {
    /// A context over `provider`, the one provider of the binary. Nothing is
    /// read until a configuration is asked for.
    pub fn new(provider: Arc<CryptoProvider>, settings: Settings) -> Context {
        Context {
            provider,
            settings,
            configs: Configs::default(),
            groups: Arc::new(Kept::new()),
            #[cfg(feature = "ech")]
            ech: crate::ech::Ech::new(),
        }
    }

    pub fn provider(&self) -> &Arc<CryptoProvider> {
        &self.provider
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn words(&self) -> &Words {
        &self.settings.words
    }

    /// The groups that servers took, which each configuration of this
    /// context shares. A consumer gives them their file once
    /// ([`Kept::keep_in`]), and says their note ([`Kept::take_note`]).
    pub fn groups(&self) -> &Arc<Kept> {
        &self.groups
    }

    /// The state of ECH: its mode, its cache of lists, the outcome of each
    /// name.
    #[cfg(feature = "ech")]
    pub fn ech(&self) -> &crate::ech::Ech {
        &self.ech
    }

    /// The default trust store: the compiled-in roots, the extra bundle, the
    /// system's stores and the first readable system bundle.
    pub fn default_roots(&self) -> TlsRoots {
        trust::default_roots(self.settings.extra_bundle.as_deref(), &self.settings.system_bundles)
    }

    /// The trust anchors for `trust`. A caller's configuration holds its own,
    /// which cannot be listed.
    pub fn roots_for(&self, trust: &Trust) -> Result<TlsRoots, Error> {
        match trust {
            Trust::File(path) => trust::roots_from_bundle(path),
            Trust::Default => Ok(self.default_roots()),
            Trust::Caller(_) => {
                Err(Error::Config("the caller's TLS configuration holds its own roots; they cannot be listed".into()))
            }
        }
    }

    /// [`client_config`] over `roots`, with the provider and the groups of
    /// this context.
    pub fn client_config(&self, roots: &TlsRoots) -> Result<Arc<ClientConfig>, Error> {
        client_config(roots, self.provider.clone(), self.groups.clone())
    }

    /// The rustls configuration for `trust`: the caller's as it is, or the
    /// crate's own over the roots of `trust`, made once in this context, so
    /// TLS resumes.
    pub fn config_for(&self, trust: &Trust) -> Result<Arc<ClientConfig>, Error> {
        match trust {
            Trust::Caller(caller) => Ok(caller.config().clone()),
            own => self.configs.config(own, || self.client_config(&self.roots_for(own)?)),
        }
    }

    /// A TLS client over `stream`, a connection that is open already (a relay
    /// session to an IRC server, for example): the certificate is checked
    /// against `server_name` and the roots of `trust`, within `limit`. A
    /// failure is the caller's to say, and never a fall back to plain text.
    pub async fn connect_over<S>(
        &self,
        stream: S,
        server_name: &str,
        trust: &Trust,
        limit: Duration,
    ) -> Result<tokio_rustls::client::TlsStream<S>, Failure>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let config = self.config_for(trust)?;
        let name = crate::wrap::server_name(server_name)?;
        let connector = tokio_rustls::TlsConnector::from(config);
        match tokio::time::timeout(limit, connector.connect(name, stream)).await {
            Ok(Ok(tls)) => Ok(tls),
            // rustls's text tells an unknown issuer from a name mismatch.
            Ok(Err(e)) => {
                let text =
                    format!("the TLS handshake with {server_name} failed: {}", handshake_failure(&e, self.words()));
                Err(Failure::caused(Stage::Tls, Class::of_io(e.kind()), text, e))
            }
            Err(_) => Err(Failure::new(
                Stage::Tls,
                Class::Timeout,
                format!("the TLS handshake with {server_name} did not end within {} s", limit.as_secs()),
            )),
        }
    }
}

/// A client configuration over `roots` with `provider`, whose store of
/// sessions takes the groups that servers took from `groups`.
///
/// **The provider is passed explicitly, never installed as a process
/// default.** `install_default` is global and can only succeed once per
/// process, so a library that did it would take the choice away from the
/// binary that embeds it, and a second call would return an error that reads
/// like a bug in the caller.
pub fn client_config(
    roots: &TlsRoots,
    provider: Arc<CryptoProvider>,
    groups: Arc<Kept>,
) -> Result<Arc<ClientConfig>, Error> {
    let verifier =
        rustls::client::WebPkiServerVerifier::builder_with_provider(Arc::new(roots.roots.clone()), provider.clone())
            .build()
            .map_err(|e| Error::Config(format!("server certificate verifier: {e}")))?;

    let mut config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::Config(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        // **No SNI suppression, no ALPN, no client certificate.** A client
        // that presents a client certificate has nothing to present one for.
        .with_no_client_auth();
    // The tickets stay in this configuration's memory, as rustls's default
    // keeps them; the group that each server took outlives the run. No early
    // data: `enable_early_data` stays off.
    config.resumption = rustls::client::Resumption::store(Arc::new(crate::store::Store::new(groups)));

    Ok(Arc::new(config))
}
