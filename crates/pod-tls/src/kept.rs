//! One TLS configuration for each trust in a [`crate::Context`]. rustls
//! keeps its cache of sessions in the configuration, so a configuration
//! made for each connection lets no connection resume TLS: each connection
//! of a run then pays a whole handshake, and reads a CA file again.
//!
//! A configuration is kept by its trust, so that a session that one trust
//! verified is never resumed under another: a resumed handshake checks no
//! certificate. A CA file that changed, by its time or its length, gets a
//! new configuration, read again, as a long-lived node is given a new CA.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::SystemTime;

use rustls::ClientConfig;

use crate::error::Error;
use crate::trust::Trust;

/// The most trusts kept: a run names one or two; a process that is given
/// many CA files keeps the newest.
const MOST: usize = 16;

/// What tells that a CA file changed: its time and its length; `None` for a
/// trust with no file, or a file that cannot be read now.
pub(crate) type Stamp = Option<(Option<SystemTime>, u64)>;

struct Kept {
    trust: Trust,
    stamp: Stamp,
    config: Arc<ClientConfig>,
}

/// The configuration of each trust.
#[derive(Default)]
pub(crate) struct Configs {
    kept: Mutex<Vec<Kept>>,
}

impl Configs {
    /// The configuration of `trust`, made by `make` the first time, and again
    /// when its CA file changed. A failure is not kept: the next call tries
    /// again.
    pub(crate) fn config<F>(&self, trust: &Trust, make: F) -> Result<Arc<ClientConfig>, Error>
    where
        F: FnOnce() -> Result<Arc<ClientConfig>, Error>,
    {
        let stamp = stamp(trust);
        let found = find(&self.lock(), trust, &stamp);
        if let Some(config) = found {
            return Ok(config);
        }
        // Made outside the lock: a CA file or a system store can be slow to read.
        let made = make()?;
        let mut kept = self.lock();
        // Two threads that missed at once keep the first one's, so that each
        // connection of a trust shares one cache of sessions.
        if let Some(config) = find(&kept, trust, &stamp) {
            return Ok(config);
        }
        kept.retain(|k| k.trust != *trust);
        if kept.len() >= MOST {
            kept.remove(0);
        }
        kept.push(Kept { trust: trust.clone(), stamp, config: made.clone() });
        Ok(made)
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Kept>> {
        self.kept.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn find(kept: &[Kept], trust: &Trust, stamp: &Stamp) -> Option<Arc<ClientConfig>> {
    kept.iter().find(|k| k.trust == *trust && k.stamp == *stamp).map(|k| k.config.clone())
}

/// The stamp of the CA file of `trust`.
pub(crate) fn stamp(trust: &Trust) -> Stamp {
    match trust {
        Trust::File(path) => file_stamp(path),
        Trust::Default | Trust::Caller(_) => None,
    }
}

fn file_stamp(path: &Path) -> Stamp {
    std::fs::metadata(path).ok().map(|m| (m.modified().ok(), m.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Past [`MOST`] trusts, the oldest goes; a failure is not kept.
    #[test]
    fn the_trusts_kept_are_bounded_and_a_failure_is_not_kept() {
        let configs = Configs::default();
        let made = || -> Result<Arc<ClientConfig>, Error> {
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let config = ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .map_err(|e| Error::Config(e.to_string()))?
                .with_root_certificates(rustls::RootCertStore::empty())
                .with_no_client_auth();
            Ok(Arc::new(config))
        };
        let first = configs.config(&Trust::File("/nonexistent/0.pem".into()), made).unwrap();
        for at in 1..=MOST {
            configs.config(&Trust::File(format!("/nonexistent/{at}.pem").into()), made).unwrap();
        }
        assert_eq!(configs.lock().len(), MOST);
        let again = configs.config(&Trust::File("/nonexistent/0.pem".into()), made).unwrap();
        assert!(!Arc::ptr_eq(&first, &again), "the oldest went");
        let failed = configs.config(&Trust::Default, || Err(Error::Config("planted".into())));
        assert!(failed.is_err());
        assert!(configs.config(&Trust::Default, made).is_ok(), "the failure was not kept");
    }
}
