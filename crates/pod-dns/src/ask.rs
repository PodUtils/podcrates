//! One resolver of DNS over HTTPS asked for a name's addresses, at its
//! address or at its name, as the lookup of a resolver and a doctor ask it,
//! with how its connection ended beside the words of a failure: a doctor
//! counts a resolver that the egress reset as it counts a relay host, by the
//! kind of the error, never by the system's words.

use std::net::IpAddr;

use pod_proxy::{Cut, Missed};
use tokio::time::Instant;

use crate::doh::{Doh, Form};
use crate::get::get;
use crate::json::parse_answer;
use crate::miss::{Miss, read, way as by_way};
use crate::way::Way;
use crate::{NXDOMAIN, wire};

/// One resolver at `at`, its address or its name: A, then AAAA when A gave
/// none (or always, with [`crate::Settings::both`]), each in the first of its
/// forms that answers in its form, within [`crate::Settings::each`] and
/// `deadline`. The addresses (each once) and the form that gave them, or why
/// none came, each reason after `at`, with the cut of the failure that ended
/// the asking, where no answer came before it. A resolver that answered once
/// completed a handshake, so a later cut says nothing of the way to it.
pub(crate) async fn ask_kept(
    way: &Way<'_>,
    resolver: &Doh,
    at: &str,
    host: &str,
    deadline: Instant,
) -> Result<(Vec<IpAddr>, Form), Missed> {
    let each = (Instant::now() + way.settings.each).min(deadline);
    let mut forms = resolver.forms();
    let (mut found, mut by, mut reasons) = (Vec::new(), None, Vec::new());
    // The cut that ended the asking, and whether an answer came before it.
    let (mut cut, mut answered) = (None, false);
    'kinds: for kind in ["A", "AAAA"] {
        while let Some(&form) = forms.first() {
            match tokio::time::timeout_at(each, ask_one(way, resolver, at, form, host, kind)).await {
                Ok(Ok(ips)) => {
                    for ip in ips {
                        if !found.contains(&ip) {
                            found.push(ip);
                        }
                    }
                    by = Some(form);
                    answered = true;
                    break;
                }
                Ok(Err(Miss::Said(e))) if e == NXDOMAIN => return Err(Missed::words(format!("{at}: {e}"))),
                // The other form would say the same.
                Ok(Err(Miss::Said(e))) => {
                    reasons.push(format!("{at}: {e}"));
                    answered = true;
                    break;
                }
                // Not served here, or not as this crate reads it: the next
                // form, for this question and the next.
                Ok(Err(Miss::Form(e))) => {
                    reasons.push(format!("{at} ({}): {e}", form.words()));
                    answered = true;
                    forms.remove(0);
                }
                Ok(Err(Miss::Way(e, ended))) => {
                    reasons.push(format!("{at}: {e}"));
                    cut = ended;
                    break 'kinds;
                }
                Err(_) => {
                    reasons.push(format!("{at}: timed out"));
                    cut = Some(Cut::TimedOut);
                    break 'kinds;
                }
            }
        }
        if !found.is_empty() && !way.settings.both {
            break;
        }
    }
    match by {
        Some(form) if !found.is_empty() => Ok((found, form)),
        _ if reasons.is_empty() => Err(Missed::words(format!("{at}: no addresses"))),
        _ => Err(Missed { why: reasons.join("; "), cut: if answered { None } else { cut } }),
    }
}

/// One question over a connection of its own.
async fn ask_one(
    way: &Way<'_>,
    resolver: &Doh,
    at: &str,
    form: Form,
    host: &str,
    kind: &str,
) -> Result<Vec<IpAddr>, Miss> {
    let target = resolver.target(form, host, kind).map_err(by_way)?;
    let response = get(way, resolver, at, &target, form.accept(), way.settings.connect, None).await?;
    read(&response, |body| match (form, wire::code(kind)) {
        (Form::Json, _) => parse_answer(body, kind),
        (Form::Wire, Some(code)) => wire::addrs(body, host, code),
        (Form::Wire, None) => Err(format!("no record type {kind}")),
    })
}
