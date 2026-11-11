//! A handler that offers a UDP service (podssh T-168): each session of the relay
//! is one flow, a UDP socket from the node to TARGET, with each datagram
//! after its 16-bit length in the session's bytes (`crate::udp`). The
//! socket is made when the session opens, as a TCP target is dialled then.

use std::sync::Arc;
use std::time::Duration;

use tokio::io::DuplexStream;

use super::framing::SessionId;
use super::node::{Handler, Opening};
use pod_session::udp::flow;

/// Each direction of the pipe between a session and its flow.
const PIPE: usize = 256 * 1024;

/// Where a handler's lines go: one for each flow that did not end plainly.
pub type Say = Arc<dyn Fn(String) + Send + Sync>;

/// Each session opens a flow to `host:port`, which ends after `idle` with
/// no datagram either way.
#[derive(Clone)]
pub struct UdpHandler {
    pub host: String,
    pub port: u16,
    /// Bound on each lookup of the name, under the node's limit for opening
    /// a session.
    pub timeout: Duration,
    pub idle: Duration,
    pub say: Say,
    /// The resolver of the name of the target.
    pub resolve: Arc<dyn pod_core::Resolve>,
}

impl UdpHandler {
    /// A handler with the idle limit of `flow::IDLE`, no lines, and the
    /// names of `resolve`.
    pub fn new(host: String, port: u16, timeout: Duration, resolve: Arc<dyn pod_core::Resolve>) -> UdpHandler {
        UdpHandler { host, port, timeout, idle: flow::IDLE, say: Arc::new(|_: String| {}), resolve }
    }
}

impl Handler for UdpHandler {
    type Stream = DuplexStream;

    fn open(&self, _id: SessionId) -> Opening<DuplexStream> {
        let (host, port, timeout, idle) = (self.host.clone(), self.port, self.timeout, self.idle);
        let (say, resolve) = (self.say.clone(), self.resolve.clone());
        Box::pin(async move {
            let socket = flow::connect(&*resolve, &host, port, timeout).await.map_err(|e| e.to_string())?;
            let (ours, theirs) = tokio::io::duplex(PIPE);
            tokio::spawn(async move {
                let carried = flow::carry(theirs, socket, idle).await;
                if let Some(note) = carried.note() {
                    say(format!("a flow to udp:{}: {note}", pod_proxy::host::authority(&host, port)));
                }
            });
            Ok::<DuplexStream, String>(ours)
        })
    }
}
