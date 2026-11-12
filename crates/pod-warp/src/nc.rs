//! The byte pipe of podnet's `warp nc HOST PORT` (PodUtils/podnet issue
//! 85): stdin and stdout to HOST:PORT, for `ssh -o ProxyCommand` and git
//! over SSH on a network that refuses port 22. The caller gives the stream
//! (through its running proxy, a session of its own, or its relay) and its
//! input and output; the search for a running proxy, the relay, the exit of
//! the process and the SSH configuration stay with the caller.
//!
//! `-u` (issue 115) is a UDP pipe: each read of the input is one datagram,
//! through a session ([`udp_via_net`]) or through a SOCKS5 association in
//! the stream ([`udp_via_stream`]).

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use pod_core::datagrams::{recv_from, send_to};
use pod_core::{BoxedIo, Class, Dial, Failure, Stage};
use pod_proxy::udp::{read_frame, unwrap, wrap, wrap_name, write_frame};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _, BufReader};
use tokio::sync::mpsc;

use crate::proxy::Names;

/// `host:port` for a SOCKS5 request (`[v6]:port` for IPv6).
pub fn target(host: &str, port: u16) -> String {
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V6(v6)) => format!("[{v6}]:{port}"),
        _ => format!("{host}:{port}"),
    }
}

/// When the TCP pipe sends its FIN, and when it ends.
#[derive(Clone, Copy, Debug)]
pub struct PipeEnd {
    /// `-N`: send the FIN at the end of the input, then wait for the far
    /// end to close.
    pub shutdown: bool,
    /// With no `-N`: after the end of the input, end (and send the FIN)
    /// when no data came for this time.
    pub idle: Duration,
}

/// The default idle time of the TCP pipe (PodUtils/podnet issue 109).
pub const TCP_IDLE: Duration = Duration::from_secs(5);

impl Default for PipeEnd {
    fn default() -> PipeEnd {
        PipeEnd { shutdown: false, idle: TCP_IDLE }
    }
}

fn io_failure(what: &str, e: std::io::Error) -> Failure {
    Failure::caused(Stage::Connect, Class::of_io(e.kind()), what.to_string(), e)
}

/// Copy `input` to `io` and `io` to `output`, until the far end closes.
///
/// PodUtils/podnet issue 188: at the end of the input, no FIN goes to the
/// far end (as OpenBSD nc with no -N): some servers drop a connection that
/// is half closed before they answer. The pipe ends when the far end
/// closes, or `end.idle` after the last data. With `end.shutdown` (-N), the
/// FIN goes at the end of the input. An error of the far end is a failure.
pub async fn pipe_with<R, W>(io: BoxedIo, input: R, mut output: W, end: PipeEnd) -> Result<(), Failure>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let (mut from, mut to) = tokio::io::split(io);
    let upload = async move {
        let mut input = BufReader::new(input);
        let sent = tokio::io::copy(&mut input, &mut to).await;
        if end.shutdown {
            let _ = to.shutdown().await;
        }
        (to, sent)
    };
    let mut upload = std::pin::pin!(upload);
    let mut writer = None;
    let mut buf = vec![0u8; 16 * 1024];
    let mut received = 0u64;
    let mut last = tokio::time::Instant::now();
    loop {
        tokio::select! {
            (to, sent) = &mut upload, if writer.is_none() => {
                tracing::debug!(sent = ?sent.as_ref().ok(), "warp nc: the end of the input");
                writer = Some(to);
                last = tokio::time::Instant::now();
            }
            read = from.read(&mut buf) => match read {
                Ok(0) => break,
                Ok(n) => {
                    received += n as u64;
                    last = tokio::time::Instant::now();
                    output.write_all(&buf[..n]).await.map_err(|e| io_failure("writing the output", e))?;
                    output.flush().await.map_err(|e| io_failure("writing the output", e))?;
                }
                Err(e) => {
                    let _ = output.flush().await;
                    tracing::debug!(received, "warp nc: the far end failed");
                    return Err(io_failure("reading from the far end", e));
                }
            },
            _ = tokio::time::sleep_until(last + end.idle), if writer.is_some() && !end.shutdown => {
                tracing::debug!(received, "warp nc: no data after the end of the input");
                if let Some(mut to) = writer.take() {
                    let _ = to.shutdown().await;
                }
                break;
            }
        }
    }
    tracing::debug!(received, "warp nc: the far end closed");
    output.flush().await.map_err(|e| io_failure("writing the output", e))
}

/// The UDP pipe (PodUtils/podnet issue 115): each read of `input` is one
/// datagram to the far end (`out`); each datagram that comes back (`back`)
/// goes to `output`. After the end of the input, the pipe ends when no
/// datagram came for `idle`.
pub async fn udp_pipe_with<R, W>(
    out: mpsc::Sender<Vec<u8>>,
    mut back: mpsc::Receiver<Vec<u8>>,
    mut input: R,
    mut output: W,
    idle: Duration,
) -> std::io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buf = vec![0u8; 65_507];
    let mut input_open = true;
    let mut last = tokio::time::Instant::now();
    loop {
        tokio::select! {
            read = input.read(&mut buf), if input_open => match read {
                Ok(0) | Err(_) => {
                    input_open = false;
                    last = tokio::time::Instant::now();
                }
                Ok(n) => {
                    if out.send(buf[..n].to_vec()).await.is_err() {
                        break;
                    }
                }
            },
            datagram = back.recv() => match datagram {
                Some(d) => {
                    last = tokio::time::Instant::now();
                    output.write_all(&d).await?;
                    output.flush().await?;
                }
                None => break,
            },
            _ = tokio::time::sleep_until(last + idle), if !input_open => break,
        }
    }
    Ok(())
}

/// The two ends of a UDP pipe: the datagrams to send, and those that come
/// back.
pub type UdpEnds = (mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>);

/// The two channels of a UDP pipe to `host:port` through a session: the
/// name is resolved inside the tunnel; a datagram of another sender than
/// the far end is dropped.
pub async fn udp_via_net(net: Arc<dyn Dial>, names: &Names, host: &str, port: u16) -> Result<UdpEnds, Failure> {
    let ip = names.resolve(net.as_ref(), host).await?;
    let socket = net.datagrams().await?;
    let to = SocketAddr::new(ip, port);
    let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(64);
    let (back_tx, back_rx) = mpsc::channel::<Vec<u8>>(64);
    let sender = socket.clone();
    tokio::spawn(async move {
        while let Some(payload) = out_rx.recv().await {
            if send_to(sender.as_ref(), &payload, to).await.is_err() {
                return;
            }
        }
    });
    tokio::spawn(async move {
        let mut buf = vec![0u8; 65_536];
        while let Ok((n, from)) = recv_from(socket.as_ref(), &mut buf).await {
            if from == to && back_tx.send(buf[..n].to_vec()).await.is_err() {
                return;
            }
        }
    });
    Ok((out_tx, back_rx))
}

/// The two channels of a UDP pipe to `host:port` through `stream`, a
/// SOCKS5 association in the stream that the caller opened
/// ([`pod_proxy::udp::open_in_stream`] on its running proxy). A name goes
/// to the proxy, which resolves it; a name of more than 255 bytes is
/// `Usage`.
pub fn udp_via_stream<S>(stream: S, host: &str, port: u16) -> Result<UdpEnds, Failure>
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let ip = host.trim_start_matches('[').trim_end_matches(']').parse::<IpAddr>().ok();
    if ip.is_none() {
        wrap_name(&[], host, port).map_err(|e| Failure::caused(Stage::Proxy, Class::Usage, "the UDP target", e))?;
    }
    let (mut read, mut write) = tokio::io::split(stream);
    let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(64);
    let (back_tx, back_rx) = mpsc::channel::<Vec<u8>>(64);
    let host = host.to_string();
    tokio::spawn(async move {
        while let Some(payload) = out_rx.recv().await {
            let framed = match ip {
                Some(ip) => wrap(&payload, ip, port),
                None => match wrap_name(&payload, &host, port) {
                    Ok(framed) => framed,
                    Err(_) => return,
                },
            };
            if write_frame(&mut write, &framed).await.is_err() {
                return;
            }
        }
    });
    tokio::spawn(async move {
        while let Ok(Some(datagram)) = read_frame(&mut read).await {
            if let Some((_, payload)) = unwrap(&datagram)
                && back_tx.send(payload.to_vec()).await.is_err()
            {
                return;
            }
        }
    });
    Ok((out_tx, back_rx))
}

#[cfg(test)]
mod tests;
