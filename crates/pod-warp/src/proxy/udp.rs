//! The UDP of a SOCKS5 client: the association of RFC 1928 (section 7),
//! with a UDP socket of the host on the address of the control connection,
//! and the association in the stream (0x83, PodUtils/podnet issue 84),
//! with no UDP socket of the host. Each datagram goes through a UDP socket
//! of the session ([`Dial::datagrams`]); a name is resolved inside the
//! tunnel once for each association. An association ends when its control
//! connection closes.

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use pod_core::datagrams::{recv_from, send_to};
use pod_core::{Class, Failure, Stage, Target};
use pod_proxy::serve;
use pod_proxy::udp::{read_frame, unwrap, wrap, write_frame};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite};

use super::client::Udp;
use super::names::Cache;

/// The client of a UDP association (PodUtils/podnet#278): the address that
/// the request names (RFC 1928: the address and the port that the client
/// sends from; zero is any), then the first sender that matches it. A
/// datagram of another address is dropped: another local process cannot
/// take the replies, or send through the tunnel with no credentials of the
/// proxy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct UdpClient {
    pub(crate) ip: Option<IpAddr>,
    pub(crate) port: u16,
    pub(crate) fixed: Option<SocketAddr>,
}

impl UdpClient {
    pub(crate) fn of(request: &Target) -> UdpClient {
        let ip = match request {
            Target::Addr(addr) if !addr.ip().is_unspecified() => Some(addr.ip()),
            _ => None,
        };
        UdpClient { ip, port: request.port(), fixed: None }
    }

    /// Whether a datagram of `from` is of the client. The first match fixes
    /// the client.
    pub(crate) fn accept(&mut self, from: SocketAddr) -> bool {
        if let Some(fixed) = self.fixed {
            return fixed == from;
        }
        let ip_ok = self.ip.is_none_or(|ip| ip == from.ip());
        let port_ok = self.port == 0 || self.port == from.port();
        if ip_ok && port_ok {
            self.fixed = Some(from);
        }
        ip_ok && port_ok
    }
}

/// The address of the UDP relay (PodUtils/podnet#278): the local address of
/// the TCP control connection, so that a client of another host can reach
/// it; 127.0.0.1 for a Unix socket. An unspecified address is not sent
/// back.
pub fn relay_bind_ip(local: Option<IpAddr>) -> IpAddr {
    match local {
        Some(ip) if !ip.is_unspecified() => ip,
        _ => IpAddr::from([127, 0, 0, 1]),
    }
}

fn failure(text: &str, e: impl std::error::Error + Send + Sync + 'static) -> Failure {
    Failure::caused(Stage::Proxy, Class::Unavailable, text.to_string(), e)
}

/// Serve one SOCKS5 UDP association on `control`, with a relay socket on
/// `bind` for `client_of`. A failure to make the sockets gets the reply 1
/// (PodUtils/podnet issue 114), not a closed connection.
pub(crate) async fn associate<S>(
    mut control: S,
    udp: Udp<'_>,
    client_of: UdpClient,
    bind: IpAddr,
) -> Result<(), Failure>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let sockets = async {
        let text = "a UDP socket of the host for the relay (a host that refuses it can use the UDP association in the stream, command 0x83)";
        let local = tokio::net::UdpSocket::bind(SocketAddr::new(bind, 0)).await.map_err(|e| failure(text, e))?;
        let tunnel = udp.net.datagrams().await?;
        Ok::<_, Failure>((Arc::new(local), tunnel))
    };
    let (local, tunnel) = match sockets.await {
        Ok(sockets) => sockets,
        Err(e) => {
            let _ = serve::socks5_reply(&mut control, serve::REPLY_GENERAL_FAILURE, None).await;
            return Err(e);
        }
    };
    let relay = local.local_addr().map_err(|e| failure("the address of the relay", e))?;
    serve::socks5_reply(&mut control, serve::REPLY_SUCCEEDED, Some(relay))
        .await
        .map_err(|e| failure("the reply of UDP ASSOCIATE", e))?;
    tracing::debug!(%relay, "a UDP ASSOCIATE");
    let client = Mutex::new(client_of);
    let to_tunnel = async {
        let mut buf = vec![0u8; 65_536];
        let mut cache = Cache::default();
        loop {
            let Ok((n, from)) = local.recv_from(&mut buf).await else { return };
            if !client.lock().unwrap_or_else(|e| e.into_inner()).accept(from) {
                tracing::debug!(%from, "a UDP datagram of another sender is dropped");
                continue;
            }
            let Some((target, payload)) = unwrap(&buf[..n]) else { continue };
            let Some(to) = cache.address(udp.names, udp.net.as_ref(), &target).await else { continue };
            // PodUtils/podnet issue 112: a datagram that the tunnel refuses
            // (a family that the device lacks) does not end the association.
            let _ = send_to(tunnel.as_ref(), payload, to).await;
        }
    };
    let from_tunnel = async {
        let mut buf = vec![0u8; 65_536];
        loop {
            let Ok((n, from)) = recv_from(tunnel.as_ref(), &mut buf).await else { return };
            let target = client.lock().unwrap_or_else(|e| e.into_inner()).fixed;
            if let Some(target) = target {
                let _ = local.send_to(&wrap(&buf[..n], from.ip(), from.port()), target).await;
            }
        }
    };
    let control_closed = async {
        let mut buf = [0u8; 256];
        while let Ok(n) = control.read(&mut buf).await {
            if n == 0 {
                break;
            }
        }
    };
    tokio::select! {
        _ = to_tunnel => {}
        _ = from_tunnel => {}
        _ = control_closed => {}
    }
    Ok(())
}

/// Serve one UDP association in the stream (PodUtils/podnet issue 84,
/// [`pod_proxy::udp::CMD_UDP_IN_STREAM`]): each datagram each way is a
/// frame on the control connection, so no UDP socket of the host is
/// necessary.
pub(crate) async fn in_stream<S>(mut control: S, udp: Udp<'_>) -> Result<(), Failure>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let tunnel = match udp.net.datagrams().await {
        Ok(tunnel) => tunnel,
        Err(e) => {
            let _ = serve::socks5_reply(&mut control, serve::REPLY_GENERAL_FAILURE, None).await;
            return Err(e);
        }
    };
    serve::socks5_reply(&mut control, serve::REPLY_SUCCEEDED, None)
        .await
        .map_err(|e| failure("the reply of the UDP association in the stream", e))?;
    tracing::debug!("a UDP association in the stream (no UDP socket of the host)");
    let (mut read, mut write) = tokio::io::split(control);
    let to_tunnel = async {
        let mut cache = Cache::default();
        while let Ok(Some(datagram)) = read_frame(&mut read).await {
            let Some((target, payload)) = unwrap(&datagram) else { continue };
            let Some(to) = cache.address(udp.names, udp.net.as_ref(), &target).await else { continue };
            let _ = send_to(tunnel.as_ref(), payload, to).await;
        }
    };
    let from_tunnel = async {
        let mut buf = vec![0u8; 65_536];
        loop {
            let Ok((n, from)) = recv_from(tunnel.as_ref(), &mut buf).await else { return };
            if write_frame(&mut write, &wrap(&buf[..n], from.ip(), from.port())).await.is_err() {
                return;
            }
        }
    };
    tokio::select! {
        _ = to_tunnel => {}
        _ = from_tunnel => {}
    }
    Ok(())
}
