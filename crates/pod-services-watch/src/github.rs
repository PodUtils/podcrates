//! The watches of a repository on GitHub: `github-release` (the newest
//! release and the age of the pin), `github-files` (the SHA-256 of pinned
//! files at the pin and at the newest release), `github-file` (a file of
//! the default branch holds values) and `tls-roots` (the certificates of a
//! file at the pin).

use std::collections::BTreeSet;

use pod_core::clock::days_from_civil;
use pod_services::{Date, Service, Watch};
use sha2::{Digest, Sha256};

use crate::args::{Args, hex, named, value_texts};
use crate::{Checked, Finding, Net, Outcome};

/// The tag and the day of the newest release of `repo`.
async fn newest(net: &Net, repo: &str) -> Result<(String, String), Finding> {
    let release = net.json(&format!("{}/repos/{repo}/releases/latest", net.bases.github_api)).await?;
    let tag = release.get("tag_name").and_then(|t| t.as_str());
    let tag = tag.ok_or_else(|| Finding::failed(format!("the newest release of {repo} has no tag")))?;
    let day = release.get("published_at").and_then(|t| t.as_str()).unwrap_or_default();
    Ok((tag.to_string(), day.get(..10).unwrap_or_default().to_string()))
}

/// The text of `path` of `repo` at `tag` (a tag, a branch, or `HEAD`).
pub async fn raw(net: &Net, repo: &str, tag: &str, path: &str) -> Result<Vec<u8>, Finding> {
    Ok(net.get_ok(&format!("{}/{repo}/{tag}/{path}", net.bases.github_raw), "*/*").await?.body)
}

/// The days from `day` (`YYYY-MM-DD`) to `today`.
pub fn days_since(day: &str, today: Date) -> Option<i64> {
    let mut parts = day.get(..10)?.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    let now = days_from_civil(i64::from(today.year()), i64::from(today.month()), i64::from(today.day()));
    Some(now - days_from_civil(y, m, d))
}

/// The text of the fact that the argument `name` names.
fn text_of<'a>(args: &Args<'a>, name: &str) -> Result<&'a str, Finding> {
    let fact = args.fact(name)?;
    fact.value().as_text().ok_or_else(|| Finding::failed(format!("the fact {:?} is no text", fact.name())))
}

/// `github-release`: the release of the pinned tag of `fact` is younger
/// than `max-age-days`; the newest release is reported.
pub async fn release(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let args = Args::new(service, watch);
    let (repo, max) = (args.text("repo")?, args.int("max-age-days")?);
    let pinned = text_of(&args, "fact")?;
    let (tag, _) = newest(net, repo).await?;
    let at = net.json(&format!("{}/repos/{repo}/releases/tags/{pinned}", net.bases.github_api)).await?;
    let day = at.get("published_at").and_then(|t| t.as_str()).unwrap_or_default();
    let age = days_since(day, net.today).ok_or_else(|| Finding::failed(format!("the release {pinned} has no day")))?;
    let mut out = Outcome::default();
    out.saw(format!("the newest release of {repo} is {tag}; {pinned} is {age} days old"));
    if age > max {
        let fact = args.fact("fact")?;
        let now = format!("a release of {age} days, past {max}; the newest is {tag}");
        out.found(Finding::moved(named(fact), pinned, now));
    }
    Ok(out)
}

/// `github-files`: each line `KIND PATH SHA256` of `fact` holds at the tag
/// of the fact that `tag` names, and at the newest release of `repo`.
pub async fn files(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let args = Args::new(service, watch);
    let (repo, fact) = (args.text("repo")?, args.fact("fact")?);
    let pinned = text_of(&args, "tag")?;
    let (latest, _) = newest(net, repo).await?;
    let mut tags = vec![pinned.to_string()];
    if latest != pinned {
        tags.push(latest);
    }
    let mut out = Outcome::default();
    for line in value_texts(fact.value()) {
        let mut words = line.split_whitespace();
        let (Some(_), Some(path), Some(sum)) = (words.next(), words.next(), words.next()) else {
            out.found(Finding::failed(format!("{line:?} of {} is not KIND PATH SHA256", named(fact))));
            continue;
        };
        for tag in &tags {
            let now = hex(&Sha256::digest(raw(net, repo, tag, path).await?));
            if now.eq_ignore_ascii_case(sum) {
                out.saw(format!("{path} at {tag}"));
            } else {
                out.found(Finding::moved(format!("{} {path}", named(fact)), sum, format!("{now} at {tag}")));
            }
        }
    }
    Ok(out)
}

/// `github-file`: the file `path` of the default branch of `repo` holds the
/// addresses of each endpoint (of `endpoint` or `role`), or its host when
/// it has no address, or each value of each fact of `facts`.
pub async fn file(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let args = Args::new(service, watch);
    let (repo, path) = (args.text("repo")?, args.text("path")?);
    let text = String::from_utf8_lossy(&raw(net, repo, "HEAD", path).await?).into_owned();
    let mut out = Outcome::default();
    let mut look = |what: String, value: &str| {
        if text.contains(value) {
            out.saw(format!("{repo}/{path} holds {value:?}"));
        } else {
            out.found(Finding::moved(what, value, format!("no {value:?} in {repo}/{path}")));
        }
    };
    match args.texts("facts") {
        Ok(facts) => {
            for name in facts {
                let fact =
                    service.fact(name).ok_or_else(|| Finding::failed(format!("the service has no fact {name:?}")))?;
                for value in value_texts(fact.value()) {
                    look(named(fact), &value);
                }
            }
        }
        Err(_) => {
            // An endpoint dialled by its addresses: the file holds them;
            // else the file holds its host.
            for endpoint in args.endpoints()? {
                let what = format!("endpoint {}", endpoint.name());
                match endpoint.addresses() {
                    [] => look(what, endpoint.host().trim_end_matches('.')),
                    addresses => addresses.iter().for_each(|a| look(what.clone(), a)),
                }
            }
        }
    }
    Ok(out)
}

/// The base64 of each PEM certificate in `text`, with no white space: a Go
/// file holds them in a string.
pub fn certificates(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = text;
    while let Some(start) = rest.find("-----BEGIN CERTIFICATE-----") {
        let body = &rest[start + 27..];
        let Some(end) = body.find("-----END CERTIFICATE-----") else { break };
        let clean: String = body[..end].chars().filter(|c| c.is_ascii_alphanumeric() || "+/=".contains(*c)).collect();
        out.insert(clean);
        rest = &body[end..];
    }
    out
}

/// `tls-roots`: the certificates of the file `path` of `repo` at the tag of
/// the fact that `tag` names are the certificates of `fact`.
pub async fn roots(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let args = Args::new(service, watch);
    let (repo, path, fact) = (args.text("repo")?, args.text("path")?, args.fact("fact")?);
    let tag = text_of(&args, "tag")?;
    let pem = fact.value().as_text().ok_or_else(|| Finding::failed("the fact of the roots is no text"))?;
    let file = certificates(pem);
    let live = certificates(&String::from_utf8_lossy(&raw(net, repo, tag, path).await?));
    let mut out = Outcome::default();
    if file.is_empty() {
        return Err(Finding::failed(format!("{} holds no certificate", named(fact))));
    }
    if live == file {
        out.saw(format!("{repo}/{path} at {tag} holds the {} certificates", file.len()));
    } else {
        let now = format!(
            "{} certificates in {repo}/{path} at {tag}, {} of them not in the fact",
            live.len(),
            live.difference(&file).count()
        );
        out.found(Finding::moved(named(fact), format!("{} certificates", file.len()), now));
    }
    Ok(out)
}
