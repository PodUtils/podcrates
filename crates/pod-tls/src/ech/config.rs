//! The TLS configuration of a sealed hello: the context's provider and the
//! crate's verifier over the roots of the trust, as
//! [`crate::Context::client_config`] makes them; TLS 1.3 alone, which ECH
//! needs; and one name's list, sealed with the HPKE of the pure provider
//! (`crypto::hpke`). One for each trust, name, list and ALPN, kept in the
//! context, so that a second connection resumes TLS inside its sealed
//! hello; a CA file that changed gets a new one.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use rustls::ClientConfig;
use rustls::client::{EchConfig, EchMode};
use rustls_pki_types::EchConfigListBytes;

use crate::context::Context;
use crate::kept::{Stamp, stamp};
use crate::trust::Trust;

/// The configurations kept; past it, the oldest goes.
const MOST: usize = 16;

struct Kept {
    trust: Trust,
    stamp: Stamp,
    name: String,
    list: Vec<u8>,
    alpn: Vec<Vec<u8>>,
    config: Arc<ClientConfig>,
}

/// The sealed configurations of a context.
#[derive(Default)]
pub(super) struct Configs {
    kept: Mutex<Vec<Kept>>,
}

impl Configs {
    fn lock(&self) -> MutexGuard<'_, Vec<Kept>> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The configuration that seals `name` with `list` under `trust`, offering
/// `alpn`; why where the list holds no config that the crate can seal with.
pub(super) fn sealed(
    context: &Context,
    trust: &Trust,
    name: &str,
    list: &[u8],
    alpn: &[&[u8]],
) -> Result<Arc<ClientConfig>, String> {
    let configs = &context.ech.configs;
    let (name, stamp) = (super::key(name), stamp(trust));
    let alpn: Vec<Vec<u8>> = alpn.iter().map(|p| p.to_vec()).collect();
    let same = |k: &&Kept| k.trust == *trust && k.stamp == stamp && k.name == name && k.list == list && k.alpn == alpn;
    if let Some(found) = configs.lock().iter().find(same) {
        return Ok(found.config.clone());
    }
    // Made outside the lock: the roots of the default trust are read from
    // the system's stores.
    let config = make(context, trust, list, &alpn)?;
    let mut kept = configs.lock();
    if let Some(found) = kept.iter().find(same) {
        return Ok(found.config.clone());
    }
    if kept.len() >= MOST {
        kept.remove(0);
    }
    kept.push(Kept { trust: trust.clone(), stamp, name, list: list.to_vec(), alpn, config: config.clone() });
    Ok(config)
}

/// A configuration that seals with `list`, of the context's provider over
/// the roots of `trust`.
fn make(context: &Context, trust: &Trust, list: &[u8], alpn: &[Vec<u8>]) -> Result<Arc<ClientConfig>, String> {
    let ech = EchConfig::new(EchConfigListBytes::from(list.to_vec()), crate::crypto::hpke::SUITES)
        .map_err(|e| format!("the list holds no config that this client can seal with ({e})"))?;
    let roots = context.roots_for(trust).map_err(|e| e.to_string())?;
    let provider = context.provider().clone();
    let verifier = rustls::client::WebPkiServerVerifier::builder_with_provider(Arc::new(roots.roots), provider.clone())
        .build()
        .map_err(|e| format!("server certificate verifier: {e}"))?;
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_ech(EchMode::from(ech))
        .map_err(|e| format!("a configuration with ECH: {e}"))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        // No client certificate, as the plain configuration offers none.
        .with_no_client_auth();
    config.alpn_protocols = alpn.to_vec();
    Ok(Arc::new(config))
}
