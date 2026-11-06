//! The JSON form of DNS over HTTPS (`application/dns-json`), as Cloudflare
//! and Google serve it: an answer's status and its records, the addresses
//! of A and AAAA records, and SRV records.

use std::net::IpAddr;

use crate::{NXDOMAIN, dns_safe};

/// A JSON answer, as it came (podnet's `DohAnswer`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Answer {
    /// The DNS status (`Status`): 0 for an answer, 3 for NXDOMAIN; `None`
    /// when the answer gives none.
    pub status: Option<u64>,
    /// Each record of `Answer` that has a type, in order.
    pub records: Vec<Record>,
}

/// One record of a JSON answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// The record type (`type`): 1 for A, 28 for AAAA, 33 for SRV, 65 for
    /// HTTPS.
    pub kind: u64,
    /// Its data in the presentation form (`data`), empty when it gives none.
    pub data: String,
    /// Its TTL in seconds (`TTL`), when it gives one.
    pub ttl: Option<u32>,
}

/// The answer of `body`, or why it is no JSON answer of DNS.
pub fn parse(body: &[u8]) -> Result<Answer, String> {
    let doc: serde_json::Value = serde_json::from_slice(body).map_err(|_| "not a DNS JSON answer".to_string())?;
    if !doc.is_object() {
        return Err("not a DNS JSON answer".into());
    }
    let records = doc
        .get("Answer")
        .and_then(|a| a.as_array())
        .into_iter()
        .flatten()
        .filter_map(|record| {
            let kind = record.get("type").and_then(|t| t.as_u64())?;
            let data = record.get("data").and_then(|d| d.as_str()).unwrap_or_default().to_string();
            let ttl = record.get("TTL").and_then(|t| t.as_u64()).and_then(|t| u32::try_from(t).ok());
            Some(Record { kind, data, ttl })
        })
        .collect();
    Ok(Answer { status: doc.get("Status").and_then(|s| s.as_u64()), records })
}

impl Answer {
    /// The records of the type `want`, or why there is none: NXDOMAIN, or
    /// `DNS status N` for another status, `DNS status missing` for none.
    pub fn of(&self, want: u64) -> Result<Vec<&Record>, String> {
        match self.status {
            Some(0) => Ok(self.records.iter().filter(|r| r.kind == want).collect()),
            Some(3) => Err(NXDOMAIN.to_string()),
            other => Err(format!("DNS status {}", other.map(|s| s.to_string()).unwrap_or_else(|| "missing".into()))),
        }
    }
}

/// The addresses in a DNS JSON answer (`Answer[].data` of type 1 or 28).
pub fn parse_answer(body: &[u8], kind: &str) -> Result<Vec<IpAddr>, String> {
    let want: u64 = if kind == "AAAA" { 28 } else { 1 };
    Ok(answers(body, want)?.iter().filter_map(|data| data.parse().ok()).collect())
}

/// The `data` of each answer of the type `want` in a DNS JSON answer, or why
/// there is none.
pub(crate) fn answers(body: &[u8], want: u64) -> Result<Vec<String>, String> {
    Ok(parse(body)?.of(want)?.into_iter().map(|r| r.data.clone()).collect())
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
