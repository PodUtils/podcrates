//! The client's SSH session over the relay WebSocket, as the vendor's Rust
//! client opens it: the host key must be one of the endpoint's host keys,
//! the user is the fact `ssh-user` with no authentication (`none`; the
//! relay checked the connect token at the upgrade), and no compression.

use std::sync::{Arc, Mutex};

use pod_core::{Class, Failure};
use russh::client::{self, AuthResult, Handle};
use russh::keys::{PublicKey, PublicKeyOrCertificate};
use russh::{Preferred, SshId};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::failure::{Step, fail, ssh};
use crate::service;
use crate::settings::Settings;

/// The text of a host key as `hostPublicKeys` holds it: the base64 of its
/// SSH wire form, the second word of its OpenSSH line.
pub fn key_text(key: &PublicKey) -> Option<String> {
    let line = key.to_openssh().ok()?;
    line.split_whitespace().nth(1).map(str::to_string)
}

/// The client side of the session: it takes a host key of the tunnel and
/// no other, and refuses each channel that the host opens.
pub(crate) struct Host {
    keys: Vec<String>,
    /// Why the host key was refused, for the failure.
    refused: Arc<Mutex<Option<&'static str>>>,
}

impl client::Handler for Host {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let why = match key {
            PublicKeyOrCertificate::Certificate(_) => Some("the host showed a certificate, not one of its keys"),
            PublicKeyOrCertificate::PublicKey { .. } if self.keys.is_empty() => {
                Some("the connect information gives no host key to check")
            }
            PublicKeyOrCertificate::PublicKey { key, .. } => match key_text(key) {
                Some(text) if self.keys.contains(&text) => None,
                _ => Some("the host key is not one of the tunnel's host keys"),
            },
        };
        if let Some(why) = why {
            *self.refused.lock().unwrap_or_else(|e| e.into_inner()) = Some(why);
        }
        Ok(why.is_none())
    }
}

/// The configuration of the client: the settings' identification and
/// keepalive, a window of 1 MiB as the vendor's client, and no
/// compression.
fn config(settings: &Settings) -> client::Config {
    client::Config {
        client_id: SshId::Standard(settings.ssh_id.clone().into()),
        window_size: 1024 * 1024,
        preferred: Preferred {
            compression: std::borrow::Cow::Borrowed(&[russh::compression::NONE]),
            ..Preferred::DEFAULT
        },
        keepalive_interval: settings.keepalive,
        keepalive_max: settings.keepalive_max,
        nodelay: true,
        ..client::Config::default()
    }
}

/// The SSH session over `io`, authenticated: the handshake with a host key
/// of `keys`, then the user with no authentication, each within the
/// timeout of `settings`.
pub(crate) async fn start<S>(io: S, keys: Vec<String>, settings: &Settings) -> Result<Handle<Host>, Failure>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let refused = Arc::new(Mutex::new(None));
    let host = Host { keys, refused: refused.clone() };
    let late = |what: &str| fail(Step::Ssh, Class::Timeout, format!("no end of {what} within {:?}", settings.timeout));
    let handshake = client::connect_stream(Arc::new(config(settings)), io, host);
    let mut handle = match tokio::time::timeout(settings.timeout, handshake).await {
        Err(_) => return Err(late("the handshake")),
        Ok(Ok(handle)) => handle,
        Ok(Err(e)) => {
            let why = *refused.lock().unwrap_or_else(|e| e.into_inner());
            return Err(match why {
                Some(why) => fail(Step::Ssh, Class::Refused, why),
                None => ssh(Step::Ssh, "the handshake", &e),
            });
        }
    };
    let user = service::ssh_user();
    match tokio::time::timeout(settings.timeout, handle.authenticate_none(user)).await {
        Err(_) => Err(late("the authentication")),
        Ok(Err(e)) => Err(ssh(Step::Ssh, "the authentication", &e)),
        Ok(Ok(AuthResult::Success)) => Ok(handle),
        Ok(Ok(AuthResult::Failure { remaining_methods, .. })) => {
            let methods: Vec<&str> = remaining_methods.iter().map(<&str>::from).collect();
            Err(fail(
                Step::Ssh,
                Class::Refused,
                format!("the host refused the user {user} with no authentication; it offers: {}", methods.join(", ")),
            ))
        }
    }
}
