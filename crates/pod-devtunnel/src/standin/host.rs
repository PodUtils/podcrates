//! The SSH side of the stand-in: the host of the tunnel, which takes the
//! user of the fact `ssh-user` with no authentication and serves a
//! `direct-tcpip` channel to each port of its plan.

use std::net::SocketAddr;
use std::sync::Arc;

use russh::server::{self, Auth, ChannelOpenHandle, Msg, Session};
use russh::{Channel, ChannelMsg, ChannelOpenFailure};

use super::State;
use crate::service;

/// What a port of the stand-in's host does with a channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Port {
    /// It writes back each byte, and ends the channel after the client's
    /// EOF.
    Echo,
    /// It writes back the first bytes, then ends the channel itself: a far
    /// end that closes.
    EchoOnce,
    /// It carries the channel to a TCP listener of the test (an SSH server
    /// of a consumer's test, as the host of a real tunnel does).
    Forward(SocketAddr),
}

/// The handler of one SSH session of the stand-in.
pub(super) struct Host {
    pub state: Arc<State>,
}

impl server::Handler for Host {
    type Error = russh::Error;

    async fn auth_none(&mut self, user: &str) -> Result<Auth, Self::Error> {
        self.state.seen().users.push(user.to_string());
        Ok(if user == service::ssh_user() { Auth::Accept } else { Auth::reject() })
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host: &str,
        port: u32,
        origin: &str,
        origin_port: u32,
        reply: ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        self.state.seen().channels.push((host.to_string(), port, origin.to_string(), origin_port));
        let plan = self.state.plan.ports.iter().find(|(p, _)| u32::from(*p) == port).map(|(_, kind)| *kind);
        match plan {
            Some(kind) if host == service::connect_host() => {
                reply.accept().await;
                tokio::spawn(serve(channel, kind));
            }
            _ => reply.reject(ChannelOpenFailure::ConnectFailed).await,
        }
        Ok(())
    }
}

/// Serve one channel by the kind of its port.
async fn serve(mut channel: Channel<Msg>, kind: Port) {
    match kind {
        Port::Echo | Port::EchoOnce => {
            while let Some(message) = channel.wait().await {
                match message {
                    ChannelMsg::Data { data } => {
                        if channel.data(&data[..]).await.is_err() {
                            return;
                        }
                        if kind == Port::EchoOnce {
                            break;
                        }
                    }
                    ChannelMsg::Eof | ChannelMsg::Close => break,
                    _ => {}
                }
            }
            let _ = channel.eof().await;
            let _ = channel.close().await;
        }
        Port::Forward(to) => {
            let Ok(mut tcp) = tokio::net::TcpStream::connect(to).await else {
                let _ = channel.close().await;
                return;
            };
            let mut stream = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut stream, &mut tcp).await;
        }
    }
}
