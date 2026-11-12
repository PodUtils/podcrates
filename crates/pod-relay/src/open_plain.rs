//! The plain wire of the forward opener (feature `plain-ws`): the upgrade
//! over plain TCP to a stand-in relay on the loopback, with the checks of
//! `pod_ws::plain::connect_loopback`, as a session of the relay's own stream
//! type, so that the opener gives it as it gives a session of TLS.

use pod_ws::client::{ConnectError, WRITE_TIMEOUT, WsClientConfig, upgrade};
use pod_ws::session::RelaySession;

use crate::token::{self, MintContext, Token, TokenError};

/// The token of a stand-in relay: the one that the settings give, or that
/// the run or the cache keep. A mint goes over TLS, which a stand-in does
/// not answer, so none is tried.
pub(super) fn token(ctx: &MintContext<'_>) -> Result<Token, TokenError> {
    token::kept(ctx)?.ok_or_else(|| {
        TokenError::Connect(ConnectError::Config(
            "plain ws:// to a relay on the loopback takes the token of the settings: a mint goes over TLS".into(),
        ))
    })
}

/// The upgrade at `config.endpoint` with `token`. A host that is not the
/// loopback is refused before any connection (`pod_ws::plain::loopback_tcp`),
/// so a token never leaves this host in clear; no proxy is used.
pub(super) async fn connect(config: &WsClientConfig, token: &str) -> Result<RelaySession, ConnectError> {
    let at = &config.endpoint;
    // Another host is refused by `loopback_tcp`, with words of its own.
    if pod_proxy::env::is_loopback(&at.host) {
        config.validate().map_err(ConnectError::Config)?;
        if token.contains(['\r', '\n']) {
            return Err(ConnectError::Config("the relay token contains a line break".into()));
        }
    }
    let tcp = pod_ws::plain::loopback_tcp(&at.host, at.port, config.timeout).await?;
    let (peer, local) = (tcp.peer_addr().ok(), tcp.local_addr().ok());
    let host_header = pod_proxy::host::authority(&at.host, at.port);
    let (tcp, pending) = tokio::time::timeout(config.timeout, upgrade(tcp, &host_header, &at.path, token))
        .await
        .map_err(|_| ConnectError::Timeout { step: "the WebSocket upgrade", after: config.timeout })??;
    let connected = pod_core::Connected { io: Box::new(tcp), peer, local };
    Ok(RelaySession::over_cleartext(connected, pending, config.idle_timeout, WRITE_TIMEOUT).with_limits(config.limits))
}
