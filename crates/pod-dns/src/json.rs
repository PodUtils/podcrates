//! The JSON form of DNS over HTTPS (`application/dns-json`), as Cloudflare
//! and Google serve it: the addresses of A and AAAA records, and SRV
//! records.

use std::net::IpAddr;

use crate::{NXDOMAIN, dns_safe};

/// The addresses in a DNS JSON answer (`Answer[].data` of type 1 or 28).
pub fn parse_answer(body: &[u8], kind: &str) -> Result<Vec<IpAddr>, String> {
    let want: u64 = if kind == "AAAA" { 28 } else { 1 };
    Ok(answers(body, want)?.iter().filter_map(|data| data.parse().ok()).collect())
}

/// The `data` of each answer of the type `want` in a DNS JSON answer, or why
/// there is none.
pub(crate) fn answers(body: &[u8], want: u64) -> Result<Vec<String>, String> {
    let doc: serde_json::Value = serde_json::from_slice(body).map_err(|_| "not a DNS JSON answer".to_string())?;
    match doc.get("Status").and_then(|s| s.as_u64()) {
        Some(0) => {}
        Some(3) => return Err(NXDOMAIN.to_string()),
        other => {
            return Err(format!("DNS status {}", other.map(|s| s.to_string()).unwrap_or_else(|| "missing".into())));
        }
    }
    Ok(doc
        .get("Answer")
        .and_then(|a| a.as_array())
        .map(|answers| {
            answers
                .iter()
                .filter(|a| a.get("type").and_then(|t| t.as_u64()) == Some(want))
                .filter_map(|a| a.get("data").and_then(|d| d.as_str()).map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

/// One SRV record (RFC 2782): where a service is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Srv {
    pub priority: u16,
    pub weight: u16,
    pub port: u16,
    /// The target's name, with no final dot.
    pub target: String,
}

/// The SRV records in a DNS JSON answer: each `data` of type 33 is
/// `PRIORITY WEIGHT PORT TARGET`; one that does not read so is left out.
pub fn parse_srv(body: &[u8]) -> Result<Vec<Srv>, String> {
    Ok(answers(body, 33)?.iter().filter_map(|data| srv_of(data)).collect())
}

/// One SRV record of its presentation form, or none.
pub fn srv_of(data: &str) -> Option<Srv> {
    let mut field = data.split_whitespace();
    let (priority, weight, port) = (field.next()?.parse().ok()?, field.next()?.parse().ok()?, field.next()?);
    let target = field.next()?.trim_end_matches('.').to_ascii_lowercase();
    (field.next().is_none() && dns_safe(&target)).then_some(Srv { priority, weight, port: port.parse().ok()?, target })
}
