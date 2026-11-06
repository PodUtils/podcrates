//! The redirects of [`super::Client::send`] and
//! [`super::Client::send_streamed`]: a request that asks for them
//! ([`super::Request::redirects`]) follows 301, 302, 303, 307 and 308 for
//! `GET` and `HEAD`. Each hop takes the proxy of its own URL and the trust
//! of the request. `Authorization`, `Proxy-Authorization` and `Cookie` go
//! only to the origin (scheme, host, port) of the first URL. A step from
//! `https://` to `http://` is refused.

use pod_core::{Class, Failure, Stage};

use super::streamed::{Bounds, Streamed};
use super::url::{Target, target, without_query};
use super::{Client, Request};

/// The headers that stay with the origin of the first URL: they can hold a
/// credential.
const PRIVATE: [&str; 3] = ["authorization", "proxy-authorization", "cookie"];

impl Client {
    /// `request` and, when it asks, each redirect after it.
    pub(super) async fn follow(&self, request: &Request, bounds: Bounds) -> Result<Streamed, Failure> {
        let mut answer = self.open(request, bounds).await?;
        if request.redirects == 0 || !matches!(request.method, "GET" | "HEAD") {
            return Ok(answer);
        }
        let first = origin(&request.url)?;
        let mut followed = 0u8;
        loop {
            if !matches!(answer.status, 301 | 302 | 303 | 307 | 308) {
                return Ok(answer);
            }
            let Some(location) = answer.header("location") else { return Ok(answer) };
            let next = join(&answer.url, location).map_err(usage)?;
            if followed == request.redirects {
                let text =
                    format!("more than {} redirects; the last one to {}", request.redirects, without_query(&next));
                return Err(Failure::new(Stage::Connect, Class::Protocol, text));
            }
            if answer.url.starts_with("https://") && !next.starts_with("https://") {
                let text = format!(
                    "a redirect from https:// to http:// is refused: {} to {}",
                    without_query(&answer.url),
                    without_query(&next)
                );
                return Err(Failure::new(Stage::Connect, Class::Protocol, text));
            }
            followed += 1;
            drop(answer);
            let hop = hop(request, next, &first)?;
            answer = self.open(&hop, bounds).await.map_err(|e| {
                let text = format!("redirected to {}: {}", without_query(&hop.url), e.text());
                Failure::caused(e.stage, e.class, text, e)
            })?;
        }
    }
}

/// The request to `url` after a redirect: the private headers stay with
/// the origin `first`.
fn hop(request: &Request, url: String, first: &(bool, String, u16)) -> Result<Request, Failure> {
    let same = origin(&url)? == *first;
    let mut hop = request.clone();
    hop.url = url;
    if !same {
        hop.headers.retain(|(name, _)| !PRIVATE.iter().any(|p| name.eq_ignore_ascii_case(p)));
    }
    Ok(hop)
}

/// The scheme, the host (lower case) and the port of `url`.
fn origin(url: &str) -> Result<(bool, String, u16), Failure> {
    let Target { tls, host, port, .. } = target(url).map_err(usage)?;
    Ok((tls, host.to_ascii_lowercase(), port))
}

fn usage(text: String) -> Failure {
    Failure::new(Stage::Connect, Class::Usage, text)
}

/// The URL that `location` names, from the URL `base` that answered with
/// it: absolute, `//host/path`, `/path`, `path` or `?query`. The fragment
/// goes; `.` and `..` of the path resolve.
pub(super) fn join(base: &str, location: &str) -> Result<String, String> {
    let location = location.trim();
    let location = location.split('#').next().unwrap_or(location);
    if let Some((scheme, rest)) = location.split_once(':')
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        let scheme = scheme.to_ascii_lowercase();
        return match rest.strip_prefix("//") {
            Some(rest) if scheme == "http" || scheme == "https" => Ok(format!("{scheme}://{rest}")),
            _ => {
                Err(format!("the redirect goes to a URL that is not http:// or https://: {}", without_query(location)))
            }
        };
    }
    let (scheme, rest) = base.split_once("://").ok_or("the URL has no scheme")?;
    let rest = rest.split('#').next().unwrap_or(rest);
    if let Some(far) = location.strip_prefix("//") {
        return Ok(format!("{scheme}://{far}"));
    }
    let end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    let path = tail.split('?').next().unwrap_or(tail);
    let path = if path.is_empty() { "/" } else { path };
    let (location_path, query) = match location.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (location, None),
    };
    let merged = if location_path.starts_with('/') {
        location_path.to_string()
    } else if location_path.is_empty() {
        path.to_string()
    } else {
        let dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
        format!("{dir}/{location_path}")
    };
    let mut url = format!("{scheme}://{authority}{}", dot_segments(&merged));
    if let Some(query) = query {
        url.push('?');
        url.push_str(query);
    }
    Ok(url)
}

/// `path` with its `.` and `..` segments resolved (RFC 3986, 5.2.4).
fn dot_segments(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let parts: Vec<&str> = path.split('/').skip(1).collect();
    for (i, part) in parts.iter().enumerate() {
        let last = i + 1 == parts.len();
        match *part {
            "." | ".." => {
                if *part == ".." {
                    out.pop();
                }
                if last {
                    out.push("");
                }
            }
            part => out.push(part),
        }
    }
    format!("/{}", out.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_location_joins_its_base() {
        let base = "https://r.example:5000/v2/a/blobs/sha256:00?x=1#f";
        let cases = [
            ("https://cdn.example/b?sig=1", "https://cdn.example/b?sig=1"),
            ("HTTP://cdn.example/b#frag", "http://cdn.example/b"),
            ("//cdn.example/b", "https://cdn.example/b"),
            ("/v2/b?q=2#f", "https://r.example:5000/v2/b?q=2"),
            ("c", "https://r.example:5000/v2/a/blobs/c"),
            ("../c?y", "https://r.example:5000/v2/a/c?y"),
            ("./", "https://r.example:5000/v2/a/blobs/"),
            ("?y=2", "https://r.example:5000/v2/a/blobs/sha256:00?y=2"),
        ];
        for (location, want) in cases {
            assert_eq!(join(base, location).unwrap(), want, "{location}");
        }
        assert_eq!(join("http://h:8080?x", "c").unwrap(), "http://h:8080/c");
        assert_eq!(join("http://h", "/a/../../b").unwrap(), "http://h/b");
        assert!(join(base, "ftp://cdn.example/b").is_err());
    }
}
