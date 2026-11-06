//! One question to the resolvers in turn, through a way, each in the first
//! of its forms that answers in its form: the first resolver that answers
//! wins. Through a way whose far end resolves names, each is asked by its
//! name first, then by its address; a name that two addresses share is
//! asked once.

use std::time::Duration;

use tokio::time::Instant;

use crate::doh::{self, Doh, Form};
use crate::get::get;
use crate::miss::{Miss, read, way as by_way};
use crate::way::Way;
use crate::{NXDOMAIN, dns_safe};

/// The answer of the first resolver of `resolvers` that answers the
/// question of `kind` for `name` in a form of `forms` (none: each of its
/// forms), read by `parse`. NXDOMAIN ends the question; a failure of the way
/// to a host goes on to its next host; the resolver's own status goes on to
/// the next resolver; an answer of another form goes on to the next form.
pub(crate) async fn first<T>(
    way: &Way<'_>,
    resolvers: &[Doh],
    name: &str,
    kind: &str,
    forms: Option<Form>,
    parse: impl Fn(Form, &[u8]) -> Result<T, String>,
    deadline: Instant,
) -> Result<T, String> {
    let name = name.trim_end_matches('.');
    if !dns_safe(name) {
        return Err(format!("{name:?} is not a name to ask a resolver for"));
    }
    let mut reasons = Vec::new();
    // The resolvers whose own status came: their other hosts would say it.
    let mut done = vec![false; resolvers.len()];
    for (i, host) in doh::order(resolvers, way.dial.names_inside()) {
        if done[i] {
            continue;
        }
        let resolver = &resolvers[i];
        for form in resolver.forms().into_iter().filter(|f| forms.is_none_or(|only| only == *f)) {
            let left = deadline.saturating_duration_since(Instant::now()).min(way.settings.each);
            if left.is_zero() {
                reasons.push("out of time".to_string());
                return Err(reasons.join("; "));
            }
            match at(way, resolver, &host, form, name, kind, left)
                .await
                .and_then(|body| read(&body, |bytes| parse(form, bytes)))
            {
                Ok(found) => return Ok(found),
                Err(Miss::Said(e)) if e == NXDOMAIN => return Err(format!("{host}: {e}")),
                Err(Miss::Form(e)) => reasons.push(format!("{host} ({}): {e}", form.words())),
                // The way to this host failed: its address may pass where
                // its name did not, as the proxy decides.
                Err(Miss::Way(e, _)) => {
                    reasons.push(format!("{host}: {e}"));
                    break;
                }
                // The resolver's own status, which it gives by either.
                Err(Miss::Said(e)) => {
                    reasons.push(format!("{host}: {e}"));
                    done[i] = true;
                    break;
                }
            }
        }
    }
    Err(if reasons.is_empty() { "no resolver answered".into() } else { reasons.join("; ") })
}

/// One question for the records of `kind` of `name` in `form`, to `host`,
/// the resolver's name or address, within `left`.
async fn at(
    way: &Way<'_>,
    resolver: &Doh,
    host: &str,
    form: Form,
    name: &str,
    kind: &str,
    left: Duration,
) -> Result<pod_proxy::http::Response, Miss> {
    let target = resolver.target(form, name, kind).map_err(by_way)?;
    Ok(get(way, resolver, host, &target, form.accept(), left, Some(left)).await?)
}
