//! The HTTP/2 server on the connection of the edge: the control stream, the
//! streams of the visitors, the configuration pushes, the PINGs and the
//! drain.

use super::*;

/// Serve the HTTP/2 connection `io` of the edge (the client is the HTTP/2 server).
/// One task accepts the streams, gives the control stream to the caller,
/// and sends a PING each `keepalive.interval`. When `budget` is spent, the
/// connection drains as for a GOAWAY of the edge.
pub async fn serve_connection<T>(
    io: T,
    runtime: Arc<Runtime>,
    ingress: Arc<Ingress>,
    keepalive: Keepalive,
    budget: Option<Budget>,
) -> Result<Served>
where
    T: AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let host = runtime.host().clone();
    let goaway = Arc::new(tokio::sync::Notify::new());
    let io = GoawayScan { inner: io, scan: FrameScan::new(), seen: Some(goaway.clone()) };
    // TLS has its own time limit; a silent edge after TLS must not hold
    // the dial.
    let mut conn = tokio::time::timeout(DIAL_TIMEOUT, server_builder().handshake::<_, Bytes>(io))
        .await
        .map_err(|_| {
            let text = format!("the HTTP/2 handshake with the edge timed out in {} s", DIAL_TIMEOUT.as_secs());
            crate::fail::new(Stage::EdgeStream, Class::Timeout, text)
        })?
        .or_fail(Stage::EdgeStream, Class::Protocol, "HTTP/2 handshake with the edge")?;
    let ping = conn.ping_pong();
    let (control_tx, control_rx) = tokio::sync::oneshot::channel();
    let (drain_tx, drain_rx) = tokio::sync::oneshot::channel::<String>();
    let (lost_tx, mut lost_rx) = tokio::sync::oneshot::channel::<String>();
    let mut pinger = None;
    if let Some(mut ping) = ping {
        pinger = Some(tokio::spawn(async move {
            loop {
                tokio::time::sleep(keepalive.interval).await;
                match tokio::time::timeout(keepalive.timeout, ping.ping(h2::Ping::opaque())).await {
                    Ok(Ok(_)) => {}
                    Ok(Err(err)) => {
                        let _ = lost_tx.send(format!("the PING failed: {err}"));
                        return;
                    }
                    Err(_) => {
                        let _ = lost_tx
                            .send(format!("the edge did not answer a PING in {} s", keepalive.timeout.as_secs()));
                        return;
                    }
                }
            }
        }));
    }
    let serve = tokio::spawn(async move {
        // The PING task ends with this task, also when it is aborted.
        let _pinger = pinger.map(AbortOnDrop::new);
        let mut control_tx = Some(control_tx);
        let mut drain_tx = Some(drain_tx);
        let spent = async move {
            let Some(budget) = budget else {
                return std::future::pending().await;
            };
            tokio::select! {
                _ = budget.spent.notified() => "the connection has carried its byte budget".to_string(),
                _ = tokio::time::sleep(budget.age) => format!(
                    "the connection is {} min old",
                    budget.age.as_secs() / 60
                ),
            }
        };
        tokio::pin!(spent);
        let reason = loop {
            let next = tokio::select! {
                next = conn.accept() => next,
                reason = &mut spent, if drain_tx.is_some() => {
                    // A planned replacement: the open streams go on.
                    let text = format!("the HTTP/2 edge connection is replaced before a limit of the relay: {reason}");
                    host.event(pod_core::Event::info("tunnel.reconnect", text));
                    if let Some(tx) = drain_tx.take() {
                        let _ = tx.send(reason);
                    }
                    continue;
                }
                _ = goaway.notified(), if drain_tx.is_some() => {
                    // No new stream comes; the open streams go on.
                    debug!("the edge sent GOAWAY");
                    if let Some(tx) = drain_tx.take() {
                        let _ = tx.send("the edge sent GOAWAY".to_string());
                    }
                    continue;
                }
                lost = &mut lost_rx => {
                    // A lost connection has no streams to finish.
                    let reason = lost.unwrap_or_else(|_| "the keepalive ended".into());
                    if let Some(tx) = drain_tx.take() {
                        let _ = tx.send(reason);
                    }
                    return;
                }
            };
            let (request, respond) = match next {
                Some(Ok(stream)) => stream,
                Some(Err(err)) => break format!("the HTTP/2 connection failed: {err}"),
                None => break "the edge ended the HTTP/2 connection (GOAWAY)".to_string(),
            };
            let kind = stream_type(request.headers());
            if kind == StreamType::Control {
                match control_tx.take() {
                    Some(tx) => {
                        let _ = tx.send((request, respond));
                    }
                    None => warn!("the edge opened a second control stream"),
                }
                continue;
            }
            let runtime = runtime.clone();
            let ingress = ingress.clone();
            tokio::spawn(async move {
                if let Err(err) = serve_stream(kind, request, respond, runtime, ingress).await {
                    debug!(error = %err.chain(), "HTTP/2 stream failed");
                }
            });
        };
        debug!(%reason, "the HTTP/2 edge connection takes no new stream");
        if let Some(tx) = drain_tx.take() {
            let _ = tx.send(reason);
        }
        // The open streams go on while the connection is polled.
        let closed = std::future::poll_fn(|cx| conn.poll_closed(cx));
        let _ = tokio::time::timeout(DRAIN_LIMIT, closed).await;
    });
    Ok(Served { control: control_rx, serve, draining: drain_rx })
}

/// A task that is aborted when this value is dropped. A failed attempt
/// stops the tasks that it started.
pub(super) struct AbortOnDrop(Option<JoinHandle<()>>);

impl AbortOnDrop {
    pub(super) fn new(task: JoinHandle<()>) -> Self {
        Self(Some(task))
    }

    /// The task, which then goes on when the handle is dropped.
    pub(super) fn into_inner(mut self) -> JoinHandle<()> {
        self.0.take().expect("the task is held until here")
    }
}

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if let Some(task) = self.0.take() {
            task.abort();
        }
    }
}

/// Serve one stream that is not the control stream.
pub(super) async fn serve_stream(
    kind: StreamType,
    request: http::Request<h2::RecvStream>,
    mut respond: h2::server::SendResponse<Bytes>,
    runtime: Arc<Runtime>,
    ingress: Arc<Ingress>,
) -> Result<()> {
    let (parts, body) = request.into_parts();
    if kind == StreamType::Configuration {
        return answer_configuration(body, respond).await;
    }
    let connect = connect_request(&parts, kind);
    debug!(dest = %connect.dest, ?kind, "HTTP/2 stream");

    // The service layer reads the request body and writes the framed
    // response into pipes. Two tasks move the bytes to and from HTTP/2.
    let reader = body_reader(body);
    let (to_edge, from_service) = tokio::io::duplex(PIPE_BUFFER);
    let service = tokio::spawn(crate::proxy::serve_decoded(
        runtime,
        ingress,
        connect,
        Box::pin(reader.compat()),
        Box::pin(to_edge.compat_write()),
    ));

    let mut from_service = from_service.compat();
    let (error, metadata) = match crate::cloudflare::stream::read_connect_response(&mut from_service).await {
        Ok(response) => response,
        Err(err) => (err.chain(), Vec::new()),
    };
    let (status, headers) = edge_response(&error, &metadata);
    let mut response = http::Response::builder().status(status);
    for (name, value) in &headers {
        response = response.header(name.as_str(), value.as_str());
    }
    let response = response.body(()).or_fail(Stage::EdgeStream, Class::Protocol, "building the edge response")?;
    let send = respond.send_response(response, false).or_fail(
        Stage::EdgeStream,
        Class::Unavailable,
        "sending the response head",
    )?;
    pump_to_h2(from_service.into_inner(), send).await?;
    let _ = service.await;
    Ok(())
}

/// Answer a configuration push with the version it carries.
pub(super) async fn answer_configuration(
    body: h2::RecvStream,
    mut respond: h2::server::SendResponse<Bytes>,
) -> Result<()> {
    let reply = match read_configuration(body_reader(body), REGISTER_TIMEOUT).await {
        Ok(text) => {
            let version = serde_json::from_slice::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v.get("version").and_then(serde_json::Value::as_i64))
                .unwrap_or(-1);
            format!(r#"{{"lastAppliedVersion":{version},"err":null}}"#)
        }
        Err(err) => {
            debug!(error = %err.chain(), "the configuration push is refused");
            serde_json::json!({ "lastAppliedVersion": -1, "err": err.chain() }).to_string()
        }
    };
    let mut send = respond.send_response(http::Response::new(()), false).or_fail(
        Stage::EdgeStream,
        Class::Unavailable,
        "answering a configuration push",
    )?;
    send.send_data(Bytes::from(reply), true).or_fail(
        Stage::EdgeStream,
        Class::Unavailable,
        "answering a configuration push",
    )?;
    Ok(())
}

/// The largest configuration push that the client reads. The client does not
/// apply a remote configuration: it reads only the version.
pub const MAX_CONFIGURATION: usize = 64 * 1024;

/// Read a configuration push of at most [`MAX_CONFIGURATION`] bytes in
/// `limit`. The cap and the time limit stop an edge that sends with no end.
pub(super) async fn read_configuration<R: AsyncRead + Unpin>(reader: R, limit: Duration) -> Result<Vec<u8>> {
    let mut text = Vec::new();
    let mut reader = reader.take(MAX_CONFIGURATION as u64 + 1);
    tokio::time::timeout(limit, reader.read_to_end(&mut text))
        .await
        .map_err(|_| {
            crate::fail::new(
                Stage::EdgeStream,
                Class::Timeout,
                format!("the configuration push did not end in {} s", limit.as_secs()),
            )
        })?
        .io_fail(Stage::EdgeStream, "reading a configuration push")?;
    if text.len() > MAX_CONFIGURATION {
        bail!(
            Stage::EdgeStream,
            Class::Protocol,
            "the configuration push is larger than {} KiB",
            MAX_CONFIGURATION / 1024
        );
    }
    Ok(text)
}

/// A tokio reader for an HTTP/2 request body. A task moves the data frames
/// into a pipe and gives the flow-control capacity back to the edge.
pub fn body_reader(mut body: h2::RecvStream) -> tokio::io::DuplexStream {
    let (mut writer, reader) = tokio::io::duplex(PIPE_BUFFER);
    tokio::spawn(async move {
        while let Some(frame) = body.data().await {
            let Ok(data) = frame else {
                break;
            };
            let len = data.len();
            match tokio::time::timeout(WRITE_STALL, writer.write_all(&data)).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => break,
                Err(_) => {
                    debug!("the origin read nothing for {} s; the stream fails", WRITE_STALL.as_secs());
                    break;
                }
            }
            let _ = body.flow_control().release_capacity(len);
        }
        let _ = writer.shutdown().await;
    });
    reader
}

/// A tokio writer for an HTTP/2 response body. A task moves the bytes from a
/// pipe into data frames, in the limits of the flow-control window.
pub fn body_writer(send: h2::SendStream<Bytes>) -> tokio::io::DuplexStream {
    let (writer, reader) = tokio::io::duplex(PIPE_BUFFER);
    tokio::spawn(async move {
        if let Err(err) = pump_to_h2(reader, send).await {
            debug!(error = %err.chain(), "HTTP/2 response body ended");
        }
    });
    writer
}

/// Copy `source` to the HTTP/2 stream until end of file, then end the stream.
pub(super) async fn pump_to_h2<R>(mut source: R, mut send: h2::SendStream<Bytes>) -> Result<()>
where
    R: AsyncRead + Unpin,
{
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let n = source.read(&mut buf).await.io_fail(Stage::EdgeStream, "reading the answer of the service")?;
        if n == 0 {
            send.send_data(Bytes::new(), true).or_fail(Stage::EdgeStream, Class::Unavailable, "ending the stream")?;
            return Ok(());
        }
        let mut offset = 0;
        while offset < n {
            send.reserve_capacity(n - offset);
            let granted = match tokio::time::timeout(WRITE_STALL, std::future::poll_fn(|cx| send.poll_capacity(cx)))
                .await
            {
                Ok(granted) => granted
                    .or_fail(Stage::EdgeStream, Class::Unavailable, "the edge closed the stream")?
                    .or_fail(Stage::EdgeStream, Class::Unavailable, "the edge closed the stream")?,
                Err(_) => {
                    send.send_reset(h2::Reason::CANCEL);
                    bail!(Stage::EdgeStream, Class::Timeout, "the edge read nothing for {} s", WRITE_STALL.as_secs());
                }
            };
            let take = granted.min(n - offset);
            if take == 0 {
                continue;
            }
            send.send_data(Bytes::copy_from_slice(&buf[offset..offset + take]), false).or_fail(
                Stage::EdgeStream,
                Class::Unavailable,
                "sending to the edge",
            )?;
            offset += take;
        }
    }
}
