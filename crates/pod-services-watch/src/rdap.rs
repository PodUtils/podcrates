//! `rdap`: the registration of a domain does not end soon. The record
//! comes from the RDAP server that IANA's bootstrap names for the
//! top-level domain, else from the fallback server.

use pod_services::{Service, Watch};

use crate::args::Args;
use crate::github::days_since;
use crate::{Checked, Finding, Net, Outcome};

/// The days that a registration must have left: time for a renewal that
/// waits for a person.
pub const AHEAD_DAYS: i64 = 90;

/// `rdap`: the `expiration` event of the record of `domain` is
/// [`AHEAD_DAYS`] or more after the day of the run.
pub async fn check(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let domain = Args::new(service, watch).text("domain")?.trim_end_matches('.').to_ascii_lowercase();
    let tld = domain.rsplit('.').next().unwrap_or_default().to_string();
    let bootstrap = net.json(&net.bases.rdap_bootstrap).await?;
    let mut bases: Vec<String> = bootstrap
        .get("services")
        .and_then(|s| s.as_array())
        .into_iter()
        .flatten()
        .filter(|row| row.get(0).and_then(|t| t.as_array()).is_some_and(|t| t.iter().any(|t| t.as_str() == Some(&tld))))
        .filter_map(|row| row.get(1).and_then(|u| u.as_array()).cloned())
        .flatten()
        .filter_map(|u| u.as_str().map(str::to_string))
        .filter(|u| u.starts_with("https://"))
        .collect();
    bases.push(net.bases.rdap_fallback.clone());
    let mut last = Finding::failed(format!("no RDAP server has a record of {domain}"));
    for base in bases {
        let url = format!("{}/domain/{domain}", base.trim_end_matches('/'));
        let request = net.request("GET", &url).header("Accept", "application/rdap+json").redirects(5);
        let record = match net.send(request).await {
            Ok(answer) if answer.is_success() => answer,
            Ok(answer) => {
                last = Finding::failed(format!("{url} answered HTTP {}", answer.status));
                continue;
            }
            Err(finding) => {
                last = finding;
                continue;
            }
        };
        let record: serde_json::Value =
            serde_json::from_slice(&record.body).map_err(|e| Finding::failed(format!("{url} is not JSON: {e}")))?;
        let ends = record
            .get("events")
            .and_then(|e| e.as_array())
            .into_iter()
            .flatten()
            .find(|e| e.get("eventAction").and_then(|a| a.as_str()) == Some("expiration"))
            .and_then(|e| e.get("eventDate").and_then(|d| d.as_str()))
            .ok_or_else(|| Finding::failed(format!("the record of {domain} at {url} has no expiration")))?;
        let left = days_since(ends, net.today).map(|d| -d);
        let left = left.ok_or_else(|| Finding::failed(format!("the expiration {ends:?} of {domain} is no day")))?;
        let day = ends.get(..10).unwrap_or(ends);
        if left < AHEAD_DAYS {
            return Err(Finding::failed(format!(
                "the registration of {domain} ends on {day}, in {left} days: renew it"
            )));
        }
        return Ok(Outcome {
            seen: vec![format!("{domain} is registered until {day}, {left} days")],
            findings: Vec::new(),
            skipped: None,
        });
    }
    Err(last)
}
