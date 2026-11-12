//! `websocket-upgrade` (an endpoint upgrades to a WebSocket) and
//! `forward-banner` (a public forwarder carries a session to the SSH
//! server of another service, in each of the three forms of a forwarder's
//! template: the target in the query, in a first text frame, or in a
//! CONNECT packet of the Wisp protocol).

use pod_services::{Endpoint, Service, Watch};
use pod_ws::frame::{OPCODE_BINARY, OPCODE_CLOSE, OPCODE_TEXT};
use pod_ws::session::RelaySession;
use pod_ws::url::{Stream, WsUrl};

use crate::args::Args;
use crate::{Checked, Finding, Net, Outcome};

/// The longest SSH banner line (RFC 4253, 4.2).
const MAX_BANNER: usize = 255;
/// The Wisp packets of version 1: CONNECT, DATA, CONTINUE, CLOSE.
const WISP_CONNECT: u8 = 0x01;
const WISP_DATA: u8 = 0x02;
const WISP_CLOSE: u8 = 0x04;
/// The stream of the one connection of a check.
const WISP_STREAM: u32 = 1;

async fn dial(net: &Net, text: &str) -> Result<RelaySession<Stream>, Finding> {
    dial_offering(net, text, None).await
}

/// [`dial`], the upgrade offering the subprotocol `protocol`, if any.
async fn dial_offering(net: &Net, text: &str, protocol: Option<&str>) -> Result<RelaySession<Stream>, Finding> {
    let url = WsUrl::parse(text).map_err(|e| Finding::failed(format!("the URL of the WebSocket: {e}")))?;
    let way = pod_ws::way::Way::new(net.context.clone(), net.dial.clone(), net.user_agent.clone());
    pod_ws::url::dial_offering(&url, protocol, &way, &net.trust, net.limit, Some(net.limit)).await.map_err(|e| {
        let why = pod_ws::url::failure(&url, &e);
        match &e {
            pod_ws::ConnectError::Dial(dial) if crate::net::silent(dial) => Finding::NoAnswer(why),
            _ => Finding::Failed(why),
        }
    })
}

/// The WebSocket URL of an endpoint: `wss://` or `ws://`, its host, its
/// port and its path.
fn ws_url(endpoint: &Endpoint) -> String {
    let scheme = if endpoint.scheme() == "ws" { "ws" } else { "wss" };
    let host =
        if endpoint.host().contains(':') { format!("[{}]", endpoint.host()) } else { endpoint.host().to_string() };
    format!("{scheme}://{host}:{}{}", endpoint.port(), endpoint.path().unwrap_or("/"))
}

/// `websocket-upgrade`: the endpoint answers the upgrade with 101; with
/// `protocol`, an upgrade that offers that subprotocol (`mqtt` for a
/// broker, which may refuse an upgrade with none).
pub async fn upgrade(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let args = Args::new(service, watch);
    let protocol = match watch.arg("protocol") {
        Some(_) => Some(args.text("protocol")?),
        None => None,
    };
    let endpoint = args.endpoint()?;
    let url = ws_url(endpoint);
    let session = dial_offering(net, &url, protocol).await?;
    let _ = session.send_close(1000, "").await;
    let seen = match protocol {
        Some(protocol) => format!("{url} upgraded, offered the subprotocol {protocol}"),
        None => format!("{url} upgraded"),
    };
    Ok(Outcome { seen: vec![seen], findings: Vec::new(), skipped: None })
}

/// `forward-banner`: through the template of the fact that `template`
/// names, the SSH server of the service `target` sends its banner.
pub async fn forward_banner(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let args = Args::new(service, watch);
    let template = args.fact("template")?;
    let template = template.value().as_text().ok_or_else(|| Finding::failed("the template is no text"))?;
    let target = ssh_endpoint(args.target("target", &net.services)?)?;
    let (host, port) = (target.host().trim_end_matches('.'), target.port());
    let shown = template.split('?').next().unwrap_or(template);
    let banner = tokio::time::timeout(net.limit * 2, async {
        if let Some(rest) = template.strip_prefix("wisp+") {
            wisp(net, rest, host, port).await
        } else if template.contains("{host}") {
            let url = template.replace("{host}", host).replace("{port}", &port.to_string());
            let session = dial(net, &url).await?;
            read_banner(&session, carried).await
        } else {
            let session = dial(net, template).await?;
            let ask = format!("{{\"Host\":{host:?},\"Port\":{port}}}");
            session.send_text(&ask).await.map_err(|e| Finding::failed(format!("{shown}: {e}")))?;
            // The first frame, text or binary, is the answer to the target.
            let mut answered = false;
            read_banner(&session, |payload, opcode| {
                if std::mem::replace(&mut answered, true) {
                    return carried(payload, opcode);
                }
                let word: Option<serde_json::Value> = serde_json::from_slice(payload).ok();
                match word.as_ref().and_then(|v| v.get("status")).and_then(|s| s.as_str()) {
                    Some("ok") => Ok(None),
                    _ => Err(Finding::failed(format!("the forwarder refused: {}", String::from_utf8_lossy(payload)))),
                }
            })
            .await
        }
    })
    .await
    .map_err(|_| Finding::failed(format!("{shown} gave no SSH banner within {:?}", net.limit * 2)))??;
    Ok(Outcome { seen: vec![format!("{shown} to {host}:{port} gave {banner:?}")], findings: Vec::new(), skipped: None })
}

/// The bytes of the target in a frame of a forwarder: a binary frame. A
/// text frame is the forwarder's own word (a Worker's
/// `{"type":"game.connected"}`), and no byte of the target.
fn carried(payload: &[u8], opcode: u8) -> Result<Option<Vec<u8>>, Finding> {
    Ok((opcode != OPCODE_TEXT).then(|| payload.to_vec()))
}

/// The endpoint of `service` with the scheme `ssh`, else its first.
pub fn ssh_endpoint(service: &Service) -> Result<&Endpoint, Finding> {
    let endpoints = service.endpoints();
    endpoints
        .iter()
        .find(|e| e.scheme() == "ssh")
        .or(endpoints.first())
        .ok_or_else(|| Finding::failed(format!("the service {} has no endpoint", service.id())))
}

/// The first line of the bytes that `take` gives from each data frame
/// (none for a frame of the protocol), when it starts with `SSH-`.
async fn read_banner(
    session: &RelaySession<Stream>,
    mut take: impl FnMut(&[u8], u8) -> Result<Option<Vec<u8>>, Finding>,
) -> Result<String, Finding> {
    let mut bytes = Vec::new();
    loop {
        let frame = session.read_frame().await.map_err(|e| Finding::failed(format!("the session ended: {e}")))?;
        match frame.opcode {
            OPCODE_CLOSE => return Err(Finding::failed("the forwarder closed before a banner")),
            OPCODE_TEXT | OPCODE_BINARY => {
                if let Some(data) = take(&frame.payload, frame.opcode)? {
                    bytes.extend_from_slice(&data);
                }
            }
            _ => continue,
        }
        if let Some(line) = banner_line(&bytes)? {
            let _ = session.send_close(1000, "").await;
            return Ok(line);
        }
    }
}

/// The banner in `bytes` when its line is whole: a failure when the bytes
/// are no SSH banner.
pub fn banner_line(bytes: &[u8]) -> Result<Option<String>, Finding> {
    let Some(end) = bytes.iter().position(|b| *b == b'\n') else {
        if bytes.len() > MAX_BANNER {
            return Err(Finding::failed("the server sent no line of a banner"));
        }
        return Ok(None);
    };
    let line = String::from_utf8_lossy(&bytes[..end]).trim_end_matches('\r').to_string();
    if !line.starts_with("SSH-") {
        return Err(Finding::failed(format!(
            "the server sent {:?}, not an SSH banner",
            pod_core::text::one_line(&line)
        )));
    }
    Ok(Some(line))
}

/// The banner through a Wisp server at `url`: one CONNECT of TCP to
/// `host:port` on stream 1, then its DATA.
async fn wisp(net: &Net, url: &str, host: &str, port: u16) -> Result<String, Finding> {
    let session = dial(net, url).await?;
    let mut connect = vec![WISP_CONNECT];
    connect.extend_from_slice(&WISP_STREAM.to_le_bytes());
    connect.push(0x01);
    connect.extend_from_slice(&port.to_le_bytes());
    connect.extend_from_slice(host.as_bytes());
    session.send_binary(&connect).await.map_err(|e| Finding::failed(format!("the Wisp CONNECT: {e}")))?;
    read_banner(&session, |payload, _| {
        if payload.len() < 5 {
            return Err(Finding::failed("a Wisp packet of less than 5 bytes"));
        }
        let stream = u32::from_le_bytes([payload[1], payload[2], payload[3], payload[4]]);
        match payload[0] {
            WISP_DATA if stream == WISP_STREAM => Ok(Some(payload[5..].to_vec())),
            WISP_CLOSE if stream == WISP_STREAM => Err(Finding::failed(format!(
                "the Wisp server closed the stream: reason 0x{:02x}",
                payload.get(5).unwrap_or(&0)
            ))),
            _ => Ok(None),
        }
    })
    .await
}
