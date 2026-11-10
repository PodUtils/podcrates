//! The watches of an OCI registry: `oci-ping` (`GET /v2/` and its
//! challenge) and `oci-digest` (each pinned image pulls by its digest).

use std::collections::BTreeMap;

use pod_services::{Endpoint, Service, Watch};

use crate::args::{Args, https_url, named, value_texts};
use crate::{Checked, Finding, Net, Outcome};

/// The manifests that a HEAD of a pinned digest accepts.
const ACCEPT: &str = "application/vnd.oci.image.index.v1+json, application/vnd.oci.image.manifest.v1+json, \
     application/vnd.docker.distribution.manifest.list.v2+json, application/vnd.docker.distribution.manifest.v2+json";

/// The challenge of `/v2/`: none for a registry that answers 200, or the
/// realm and the service of its `Bearer` challenge.
async fn challenge(net: &Net, endpoint: &Endpoint) -> Result<Option<(String, Option<String>)>, Finding> {
    let url = https_url(endpoint, Some("/v2/"));
    let answer = net.send(net.request("GET", &url)).await?;
    match answer.status {
        200 => Ok(None),
        401 => {
            let header = answer.header("www-authenticate").unwrap_or_default();
            let params = bearer(header)
                .ok_or_else(|| Finding::failed(format!("{url} answered 401 with no Bearer challenge: {header:?}")))?;
            let realm = params
                .get("realm")
                .cloned()
                .ok_or_else(|| Finding::failed(format!("the challenge of {url} has no realm")))?;
            Ok(Some((realm, params.get("service").cloned())))
        }
        other => Err(Finding::failed(format!("{url} answered HTTP {other}"))),
    }
}

/// The parameters of a `Bearer` challenge.
pub fn bearer(header: &str) -> Option<BTreeMap<String, String>> {
    let rest = header.trim().strip_prefix("Bearer ").or_else(|| header.trim().strip_prefix("bearer "))?;
    let mut out = BTreeMap::new();
    for part in rest.split(',') {
        let (key, value) = part.split_once('=')?;
        out.insert(key.trim().to_ascii_lowercase(), value.trim().trim_matches('"').to_string());
    }
    Some(out)
}

/// `oci-ping`: `GET /v2/` answers 200, or 401 with a `Bearer` challenge
/// whose realm is `https://` and is the fact's, when the watch names one.
pub async fn ping(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let args = Args::new(service, watch);
    let endpoint = args.endpoint()?;
    let fact = args.opt_fact("fact")?;
    let mut out = Outcome::default();
    match (challenge(net, endpoint).await?, fact) {
        (None, None) => out.saw(format!("{} /v2/ answered 200", endpoint.host())),
        (None, Some(fact)) => {
            out.found(Finding::moved(named(fact), fact.value().to_string(), "no challenge: /v2/ answered 200"))
        }
        (Some((realm, _)), _) if !realm.starts_with("https://") => {
            out.found(Finding::failed(format!("the realm {realm:?} of {} is not https://", endpoint.host())))
        }
        (Some((realm, _)), Some(fact)) if fact.value().as_text() != Some(realm.as_str()) => {
            out.found(Finding::moved(named(fact), fact.value().to_string(), realm))
        }
        (Some((realm, _)), _) => out.saw(format!("{} /v2/ asks a token of {realm}", endpoint.host())),
    }
    Ok(out)
}

/// `oci-digest`: each image `REPOSITORY[:TAG]@sha256:DIGEST` of each fact
/// of `facts` answers a HEAD of its manifest by the digest: 404 is a pin
/// that moved, another status a check that failed; a tag that
/// names another digest now is reported, and is no failure: the pin is the
/// digest.
pub async fn digest(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let args = Args::new(service, watch);
    let endpoint = args.endpoint()?;
    let realm = challenge(net, endpoint).await?;
    let mut out = Outcome::default();
    for name in args.texts("facts")? {
        let fact = service.fact(name).ok_or_else(|| Finding::failed(format!("the service has no fact {name:?}")))?;
        for image in value_texts(fact.value()) {
            let Some((reference, digest)) = image.split_once('@') else {
                out.found(Finding::failed(format!("{image:?} of {} has no digest", named(fact))));
                continue;
            };
            let (repository, tag) = split_tag(reference);
            let token = match &realm {
                Some((realm, service)) => Some(token(net, realm, service.as_deref(), repository).await?),
                None => None,
            };
            match head(net, endpoint, repository, digest, token.as_deref()).await? {
                (200, _) => out.saw(format!("{repository}@{digest}")),
                (404, _) => out.found(Finding::moved(named(fact), &image, "HTTP 404 for the digest")),
                // A limit or a fault of the registry tells nothing of the pin.
                (status, _) => out.found(Finding::failed(format!("{repository}@{digest}: HTTP {status}"))),
            }
            if let Some(tag) = tag
                && let (200, Some(now)) = head(net, endpoint, repository, tag, token.as_deref()).await?
                && now != digest
            {
                out.saw(format!("the tag {repository}:{tag} moved to {now}"));
            }
        }
    }
    Ok(out)
}

/// The repository and the tag of `REPOSITORY[:TAG]`: a `:` after the last
/// `/` starts the tag.
fn split_tag(reference: &str) -> (&str, Option<&str>) {
    let slash = reference.rfind('/').map_or(0, |at| at + 1);
    match reference[slash..].find(':') {
        Some(at) => (&reference[..slash + at], Some(&reference[slash + at + 1..])),
        None => (reference, None),
    }
}

/// An anonymous token to pull `repository`.
async fn token(net: &Net, realm: &str, service: Option<&str>, repository: &str) -> Result<String, Finding> {
    let mut url = format!("{realm}?scope=repository:{repository}:pull");
    if let Some(service) = service {
        url.push_str(&format!("&service={service}"));
    }
    let answer = net.json(&url).await?;
    let token = answer.get("token").or_else(|| answer.get("access_token")).and_then(|t| t.as_str());
    token.map(str::to_string).ok_or_else(|| Finding::failed(format!("{realm} gave no token for {repository}")))
}

/// The status and the `Docker-Content-Digest` of a HEAD of the manifest of
/// `reference` (a digest or a tag) of `repository`.
async fn head(
    net: &Net,
    endpoint: &Endpoint,
    repository: &str,
    reference: &str,
    token: Option<&str>,
) -> Result<(u16, Option<String>), Finding> {
    let url = https_url(endpoint, Some(&format!("/v2/{repository}/manifests/{reference}")));
    let mut request = net.request("HEAD", &url).header("Accept", ACCEPT).redirects(5);
    if let Some(token) = token {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    let mut answer = net.send(request.clone()).await?;
    // A registry limits anonymous reads (429): one more try after the wait
    // that it asks, at most RETRY_MOST.
    if answer.status == 429 {
        let wait = answer.header("retry-after").and_then(|w| w.trim().parse().ok()).unwrap_or(5u64);
        tokio::time::sleep(std::time::Duration::from_secs(wait).min(RETRY_MOST)).await;
        answer = net.send(request).await?;
    }
    Ok((answer.status, answer.header("docker-content-digest").map(str::to_string)))
}

/// The longest wait before the second try of a HEAD that a registry
/// limited.
const RETRY_MOST: std::time::Duration = std::time::Duration::from_secs(30);
