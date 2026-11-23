//! UPnP IGD SSDP discovery datagrams and pure-Rust XML/SOAP read-only `GetExternalIPAddress`.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};

/// Standard SSDP UDP port.
pub const SSDP_PORT: u16 = 1900;
/// Standard SSDP multicast address (`239.255.255.250:1900`).
pub const SSDP_MULTICAST: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(239, 255, 255, 250), SSDP_PORT));

/// SSDP M-SEARCH for `ssdp:all` (sent unicast to the gateway first, then multicast).
pub const MSEARCH_ALL: &[u8] = b"M-SEARCH * HTTP/1.1\r\n\
HOST: 239.255.255.250:1900\r\n\
ST: ssdp:all\r\n\
MAN: \"ssdp:discover\"\r\n\
MX: 2\r\n\
\r\n";

/// SSDP M-SEARCH specifically for `InternetGatewayDevice:1` (some devices omit it from `ssdp:all`).
pub const MSEARCH_IGD1: &[u8] = b"M-SEARCH * HTTP/1.1\r\n\
HOST: 239.255.255.250:1900\r\n\
ST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\
MAN: \"ssdp:discover\"\r\n\
MX: 2\r\n\
\r\n";

/// Parsed IGD advertisement extracted from an SSDP reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SsdpIgd {
    pub location: String,
    pub st_or_usn: String,
}

/// Parse an SSDP response datagram. Accepts replies from any source port (some gateways reply
/// from an ephemeral port rather than 1900) and matches `:InternetGatewayDevice:` in `ST` or `USN`.
pub fn parse_ssdp_reply(bytes: &[u8]) -> Option<SsdpIgd> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.lines();
    let status = lines.next()?;
    if !status.starts_with("HTTP/1.") || status.split_whitespace().nth(1) != Some("200") {
        return None;
    }
    let mut location = None;
    let mut igd_tag = None;
    for line in lines {
        let Some((name, val)) = line.split_once(':') else {
            continue;
        };
        let key = name.trim();
        let val = val.trim();
        if key.eq_ignore_ascii_case("LOCATION") && !val.is_empty() {
            location = Some(val.to_string());
        } else if (key.eq_ignore_ascii_case("ST") || key.eq_ignore_ascii_case("USN"))
            && val.contains(":InternetGatewayDevice:")
        {
            igd_tag = Some(val.to_string());
        }
    }
    Some(SsdpIgd { location: location?, st_or_usn: igd_tag? })
}

/// Verify that `url` uses plain `http://` and addresses `expected_host` (preventing SSRF from
/// untrusted SSDP `LOCATION` or XML `URLBase`/`controlURL` redirects).
pub fn url_matches_host(url: &str, expected_host: IpAddr) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or("");
    let host = if let Some(after_bracket) = authority.strip_prefix('[') {
        after_bracket.split(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    host.parse::<IpAddr>().ok() == Some(expected_host)
}

/// WAN connection service located in an IGD device description XML document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WanService {
    pub service_type: String,
    pub control_url: String,
}

const WAN_SERVICES: [&str; 3] = [
    "urn:schemas-upnp-org:service:WANIPConnection:1",
    "urn:schemas-upnp-org:service:WANIPConnection:2",
    "urn:schemas-upnp-org:service:WANPPPConnection:1",
];

/// Scan a UPnP device description XML document for a WAN connection service and resolve its
/// `controlURL` relative to `URLBase` or `location_url`. Pure Rust, zero XML dependencies.
pub fn find_wan_service(xml: &str, location_url: &str) -> Option<WanService> {
    let base = extract_tag(xml, "URLBase").filter(|s| !s.is_empty()).unwrap_or(location_url);
    let mut rest = xml;
    while let Some(start) = rest.find("<service") {
        let after_lt = &rest[start + "<service".len()..];
        let Some(gt) = after_lt.find('>') else { break };
        if !matches!(after_lt.as_bytes().first(), Some(b'>' | b' ' | b'\t' | b'\r' | b'\n')) {
            rest = &after_lt[gt + 1..];
            continue;
        }
        let after = &after_lt[gt + 1..];
        let Some(end) = after.find("</service>") else { break };
        let block = &after[..end];
        if let Some(st) = extract_tag(block, "serviceType")
            && WAN_SERVICES.contains(&st)
            && let Some(ctrl) = extract_tag(block, "controlURL")
        {
            let resolved = resolve_url(base, ctrl);
            return Some(WanService { service_type: st.to_string(), control_url: resolved });
        }
        rest = &after[end + "</service>".len()..];
    }
    None
}

/// Build the SOAP body and `SOAPAction` header value for read-only `GetExternalIPAddress`.
pub fn build_get_external_ip_soap(service_type: &str) -> (String, String) {
    let action = format!("\"{service_type}#GetExternalIPAddress\"");
    let body = format!(
        "<?xml version=\"1.0\"?>\
<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\
<s:Body><u:GetExternalIPAddress xmlns:u=\"{service_type}\"/></s:Body></s:Envelope>"
    );
    (action, body)
}

/// Extract `NewExternalIPAddress` from a UPnP SOAP response body. Returns `None` on `0.0.0.0`,
/// missing tag, or parse failure.
pub fn parse_external_ip_soap(xml: &str) -> Option<Ipv4Addr> {
    let text = extract_tag(xml, "NewExternalIPAddress")?;
    let ip: Ipv4Addr = text.trim().parse().ok()?;
    (!ip.is_unspecified()).then_some(ip)
}

/// Extract a UPnP SOAP fault `errorCode` when present.
pub fn parse_soap_fault_code(xml: &str) -> Option<u32> {
    extract_tag(xml, "errorCode")?.trim().parse().ok()
}

fn extract_tag<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let mut pos = 0usize;
    while let Some(lt) = xml[pos..].find('<') {
        let i = pos + lt;
        let rest = &xml[i + 1..];
        if rest.starts_with('/') {
            pos = i + 1;
            continue;
        }
        let Some(gt) = rest.find('>') else { break };
        let open_inner = &rest[..gt];
        let name_part = open_inner.split_whitespace().next().unwrap_or("");
        let local_name = name_part.rsplit_once(':').map_or(name_part, |(_, l)| l);
        if local_name == tag && !open_inner.ends_with('/') {
            let content_start = i + 1 + gt + 1;
            let close_needle = format!("</{name_part}>");
            if let Some(end) = xml[content_start..].find(&close_needle) {
                return Some(xml[content_start..content_start + end].trim());
            }
            return None;
        }
        pos = i + 1 + gt + 1;
    }
    None
}

fn resolve_url(base: &str, relative: &str) -> String {
    if relative.starts_with("http://") || relative.starts_with("https://") {
        return relative.to_string();
    }
    let Some(scheme_end) = base.find("://") else {
        return relative.to_string();
    };
    let after_scheme = &base[scheme_end + 3..];
    let origin_len = after_scheme.find('/').map_or(base.len(), |i| scheme_end + 3 + i);
    let origin = &base[..origin_len];
    if relative.starts_with('/') {
        format!("{origin}{relative}")
    } else {
        let base_dir = match base[origin_len..].rfind('/') {
            Some(slash) => &base[..origin_len + slash + 1],
            None => origin,
        };
        if base_dir.ends_with('/') { format!("{base_dir}{relative}") } else { format!("{base_dir}/{relative}") }
    }
}
