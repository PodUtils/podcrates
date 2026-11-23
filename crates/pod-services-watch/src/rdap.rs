//! `rdap`: the registration of a domain does not end soon. The record
//! comes from the RDAP server that IANA's bootstrap names for the
//! top-level domain, else from the fallback server, else from the RDAP
//! server of the registrar that the watch names (`registrar`, its IANA id).

use pod_proxy::http::Response;
use pod_services::{Service, Watch};

use crate::args::Args;
use crate::github::days_since;
use crate::{Checked, Finding, Net, Outcome};

/// The days that a registration must have left, where the watch gives no
/// `ahead-days`. Most domains of the data are of a third party, and a
/// registrar renews a domain in its last 30 to 45 days: a bound of 90 days
/// made a red run each day for two months that podcrates cannot change, and
/// it hid the other findings. A domain of the owner gives `ahead-days = 90`:
/// time for a renewal that waits for a person.
pub const AHEAD_DAYS: i64 = 30;
/// Under this count of days left, the run shows the end of the
/// registration, but it is no finding.
pub const NOTE_DAYS: i64 = 90;

/// `rdap`: the `expiration` event of the record of `domain` is `ahead-days`
/// ([`AHEAD_DAYS`]) or more after the day of the run. Where each server
/// refuses, the server of the registrar `registrar` gives the record.
pub async fn check(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let args = Args::new(service, watch);
    let domain = args.text("domain")?.trim_end_matches('.').to_ascii_lowercase();
    let ahead = args.opt_int("ahead-days")?.unwrap_or(AHEAD_DAYS);
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
    // A registry can refuse the address of a runner (CentralNic for `.xyz`
    // answers 403 to cloud ranges): the registrar's server is a second
    // source of the same registration.
    let registrar = args.opt_int("registrar")?;
    // Each failure, not the last alone: a fault of the registry's own
    // server must show beside the answer of the fallback.
    let mut failed = Vec::new();
    let mut found = None;
    for base in bases {
        found = ask(net, &base, &domain, &mut failed).await;
        if found.is_some() {
            break;
        }
    }
    if found.is_none()
        && let Some(id) = registrar
    {
        match registrar_base(net, id).await {
            Ok(base) => found = ask(net, &base, &domain, &mut failed).await,
            Err(why) => failed.push(why),
        }
    }
    let Some((url, record)) = found else {
        return Err(Finding::failed(format!("no RDAP server has a record of {domain}: {}", failed.join("; "))));
    };
    let record: serde_json::Value =
        serde_json::from_slice(&record.body).map_err(|e| Finding::failed(format!("{url} is not JSON: {e}")))?;
    let ends = record
        .get("events")
        .and_then(|e| e.as_array())
        .into_iter()
        .flatten()
        .filter_map(|e| Some((e.get("eventAction")?.as_str()?, e.get("eventDate")?.as_str()?)))
        // A registrar's server can give only its own event.
        .min_by_key(|(action, _)| EXPIRATIONS.iter().position(|a| a == action).unwrap_or(usize::MAX))
        .filter(|(action, _)| EXPIRATIONS.contains(action))
        .map(|(_, day)| day)
        .ok_or_else(|| Finding::failed(format!("the record of {domain} at {url} has no expiration")))?;
    let left = days_since(ends, net.today).map(|d| -d);
    let left = left.ok_or_else(|| Finding::failed(format!("the expiration {ends:?} of {domain} is no day")))?;
    let day = ends.get(..10).unwrap_or(ends);
    if left < ahead {
        return Err(Finding::failed(format!("the registration of {domain} ends on {day}, in {left} days: renew it")));
    }
    let note = if left < NOTE_DAYS { ": the owner has not renewed it yet" } else { "" };
    // A refusal before the record stays in the line of the run.
    let read = if failed.is_empty() { String::new() } else { format!(" (read at {url}; {})", failed.join("; ")) };
    Ok(Outcome {
        seen: vec![format!("{domain} is registered until {day}, {left} days{note}{read}")],
        findings: Vec::new(),
        skipped: None,
    })
}

/// The answer of the RDAP server at `base` for `domain`, with its URL; a
/// refusal or a fault of the way goes to `failed`.
async fn ask(net: &Net, base: &str, domain: &str, failed: &mut Vec<String>) -> Option<(String, Response)> {
    let url = format!("{}/domain/{domain}", base.trim_end_matches('/'));
    let request = net.request("GET", &url).header("Accept", "application/rdap+json").redirects(5);
    match net.send(request).await {
        Ok(answer) if answer.is_success() => Some((url, answer)),
        Ok(answer) => {
            failed.push(format!("{url} answered HTTP {}", answer.status));
            None
        }
        Err(f) => {
            failed.push(why(f));
            None
        }
    }
}

/// The words of a finding.
fn why(finding: Finding) -> String {
    match finding {
        Finding::Failed(why) | Finding::NoAnswer(why) | Finding::Moved { now: why, .. } => why,
    }
}

/// The events that give the end of a registration, in the order of trust:
/// the registry's, then the registrar's.
const EXPIRATIONS: [&str; 2] = ["expiration", "registrar expiration"];

/// The RDAP base of the registrar with the IANA id `id`, from IANA's list
/// of the registrar ids (CSV: the id, the name, the status, the base).
async fn registrar_base(net: &Net, id: i64) -> Result<String, String> {
    let url = &net.bases.rdap_registrars;
    let list = net.get_ok(url, "text/csv").await.map_err(why)?;
    let list = String::from_utf8_lossy(&list.body);
    let wanted = id.to_string();
    list.lines()
        .find(|line| line.split(',').next().map(|f| f.trim_matches('"')) == Some(wanted.as_str()))
        .and_then(|line| line.rsplit(',').next())
        .map(|base| base.trim().trim_matches('"').to_string())
        .filter(|base| base.starts_with("https://"))
        .ok_or_else(|| format!("{url} has no RDAP server of the registrar {id}"))
}
