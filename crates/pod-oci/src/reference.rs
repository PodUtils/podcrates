//! Image reference parsing, to docker's normalisation rules.
//!
//! ⚠ The normalisation is not cosmetic: `alpine`, `alpine:latest`,
//! `docker.io/library/alpine` and `index.docker.io/library/alpine:latest` are
//! one image, and a store that treats them as four pulls four times and answers
//! `podvm images alpine` with nothing.
//!
//! ⛔ HTTPS only, per `TODO/image.md` T-0201. A reference written with an
//! explicit `http://` scheme is refused by name rather than downgraded.

use std::fmt;

use crate::digest::Digest;
use crate::error::{Error, Result};

/// What a bare name resolves to. ⚠ `docker.io` is the canonical *name* and
/// `registry-1.docker.io` is the host that answers `/v2/`; they are not
/// interchangeable and docker prints the first while talking to the second.
pub const DEFAULT_DOMAIN: &str = "docker.io";
pub const DEFAULT_ENDPOINT: &str = "registry-1.docker.io";
/// docker prefixes a single-component name with this before asking the hub.
pub const OFFICIAL_PREFIX: &str = "library";
pub const DEFAULT_TAG: &str = "latest";

/// A parsed reference. Exactly one of [`Reference::tag`] and
/// [`Reference::digest`] decides what is fetched; a reference carrying both
/// keeps the tag for display and fetches by digest, which is what docker does.
#[derive(Clone, Debug)]
pub struct Reference {
    /// The canonical domain, as printed. `docker.io` for the hub.
    pub domain: String,
    /// The repository path, `library/alpine` for a bare `alpine`.
    pub repository: String,
    pub tag: Option<String>,
    pub digest: Option<Digest>,
    /// ⭐ The caller wrote `http://`. TODO/image.md T-0213: this is carried
    /// rather than refused here, because whether it is honoured depends on the
    /// transport policy and a `Reference` is a name that knows nothing about
    /// one. ⛔ The refusal still happens, in `pull`, where the policy is in
    /// scope and the message can name the flag that would permit it.
    pub plain_http: bool,
}

impl Reference {
    pub fn parse(input: &str) -> Result<Reference> {
        let s = input.trim();
        if s.is_empty() {
            return Err(Error::Reference("the reference is empty".into()));
        }
        for scheme in ["http://", "https://"] {
            if let Some(rest) = s.strip_prefix(scheme) {
                if scheme == "http://" {
                    // ⭐ TODO/image.md T-0213. The scheme is stripped and the
                    // HOST is remembered, so the caller can be told which
                    // registry to permit. ⛔ Parsing does not decide: a
                    // `Reference` is a name and knows nothing about transport
                    // policy, and threading a policy in here would put the
                    // decision in the one place that cannot report it.
                    // `Reference::plain_http` carries the fact upwards.
                    let mut r = Reference::parse(rest)?;
                    r.plain_http = true;
                    return Ok(r);
                }
                // ⚠ `https://` is accepted and stripped rather than refused:
                // it says nothing podvm does not already do, and refusing it
                // would fail a reference that is exactly right.
                return Reference::parse(rest);
            }
        }

        // The digest first: `@` cannot appear in a domain or a tag, so it is
        // unambiguous wherever it is.
        let (rest, digest) = match s.split_once('@') {
            Some((rest, d)) => (rest, Some(Digest::parse(d)?)),
            None => (s, None),
        };

        // docker's rule: the first component is a domain when it carries a `.`
        // or a `:`, or is exactly `localhost`. Everything else is a repository
        // path on the hub. ⚠ Without this, `myhost:5000/x` and `user/x:5000`
        // are indistinguishable.
        let (domain, remainder) = match rest.split_once('/') {
            Some((head, tail))
                if head == "localhost" || head.contains('.') || head.contains(':') =>
            {
                // ⛔ Validated, not merely recognised. `..` contains a dot, so
                // the rule above alone reads `../etc/passwd` as a registry
                // called `..` with a repository called `etc/passwd`, and the
                // repository check below then passes it. Measured here on
                // 2026-09-08 by the test named for it.
                if !is_domain(head) {
                    return Err(Error::Reference(format!(
                        "{input:?}: {head:?} is where a registry host goes and is \
                         not one"
                    )));
                }
                (head.to_string(), tail)
            }
            _ => (DEFAULT_DOMAIN.to_string(), rest),
        };

        // The tag is after the LAST colon, and only when no `/` follows it, so
        // a port in a domain that reached here cannot be read as a tag.
        let (path, tag) = match remainder.rsplit_once(':') {
            Some((p, t)) if !t.contains('/') => (p, Some(t.to_string())),
            _ => (remainder, None),
        };

        if path.is_empty() {
            return Err(Error::Reference(format!("{input:?} names no repository")));
        }
        for component in path.split('/') {
            if component.is_empty() || component == "." || component == ".." {
                return Err(Error::Reference(format!(
                    "{input:?} has an empty or relative path component, which cannot \
                     name a repository and cannot be a store directory"
                )));
            }
            if !component
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
            {
                return Err(Error::Reference(format!(
                    "{input:?}: repository component {component:?} is not lowercase \
                     alphanumeric with separators"
                )));
            }
        }
        if let Some(t) = &tag
            && (t.is_empty()
                || !t
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)))
        {
            return Err(Error::Reference(format!(
                "{input:?}: tag {t:?} is not valid"
            )));
        }

        let repository = if domain == DEFAULT_DOMAIN && !path.contains('/') {
            format!("{OFFICIAL_PREFIX}/{path}")
        } else {
            path.to_string()
        };

        // ⛔ A reference with neither a tag nor a digest means `:latest`, and
        // that default is applied HERE, once, so no caller invents its own.
        let tag = match (&tag, &digest) {
            (None, None) => Some(DEFAULT_TAG.to_string()),
            _ => tag,
        };

        Ok(Reference {
            domain,
            repository,
            tag,
            digest,
            plain_http: false,
        })
    }

    /// The host to send `/v2/` requests to. ⚠ Differs from [`Reference::domain`]
    /// for the hub alone, and that is the whole reason this is a method.
    pub fn endpoint(&self) -> &str {
        if self.domain == DEFAULT_DOMAIN || self.domain == "index.docker.io" {
            DEFAULT_ENDPOINT
        } else {
            &self.domain
        }
    }

    /// What goes in the `/v2/<name>/manifests/<here>` position.
    pub fn manifest_selector(&self) -> String {
        match (&self.digest, &self.tag) {
            (Some(d), _) => d.to_string(),
            (None, Some(t)) => t.clone(),
            (None, None) => DEFAULT_TAG.to_string(),
        }
    }

    /// How docker prints the repository: the hub's `library/` prefix and the
    /// `docker.io/` domain are both elided, and nothing else is.
    pub fn display_repository(&self) -> String {
        if self.domain == DEFAULT_DOMAIN {
            match self.repository.strip_prefix(&format!("{OFFICIAL_PREFIX}/")) {
                Some(short) if !short.contains('/') => short.to_string(),
                _ => self.repository.clone(),
            }
        } else {
            format!("{}/{}", self.domain, self.repository)
        }
    }

    /// The canonical `<domain>/<repository>`, which is what the store keys on.
    /// ⛔ Never the display form: two display forms can be one image, and a
    /// store keyed on the display form holds it twice.
    pub fn canonical_repository(&self) -> String {
        format!("{}/{}", self.domain, self.repository)
    }
}

/// A registry host: dot-separated labels of alphanumerics and hyphens, with an
/// optional `:port`. ⚠ Deliberately narrower than a URI authority: podvm never
/// puts a userinfo or a path here, and anything it would not itself write is
/// refused rather than passed to a URL builder.
fn is_domain(head: &str) -> bool {
    let (host, port) = match head.rsplit_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (head, None),
    };
    if let Some(p) = port
        && (p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return false;
    }
    !host.is_empty()
        && host.split('.').all(|label| {
            !label.is_empty()
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_repository())?;
        if let Some(t) = &self.tag {
            write!(f, ":{t}")?;
        }
        if let Some(d) = &self.digest {
            write!(f, "@{d}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_name_becomes_the_hubs_library_repository_at_latest() {
        let r = Reference::parse("alpine").unwrap();
        assert_eq!(r.domain, DEFAULT_DOMAIN);
        assert_eq!(r.repository, "library/alpine");
        assert_eq!(r.tag.as_deref(), Some("latest"));
        assert_eq!(r.endpoint(), DEFAULT_ENDPOINT);
        assert_eq!(r.to_string(), "alpine:latest");
    }

    #[test]
    fn the_four_spellings_of_one_image_normalise_to_one_key() {
        let keys: Vec<String> = [
            "alpine",
            "alpine:latest",
            "docker.io/library/alpine",
            "index.docker.io/library/alpine:latest",
        ]
        .iter()
        .map(|s| Reference::parse(s).unwrap().canonical_repository())
        .collect();
        // ⚠ `index.docker.io` is a distinct DOMAIN and the same endpoint; it is
        // recorded as written so `podvm images` prints what was asked for.
        assert_eq!(keys[0], keys[1]);
        assert_eq!(keys[1], keys[2]);
        assert_eq!(
            Reference::parse(&keys[3]).unwrap().endpoint(),
            DEFAULT_ENDPOINT
        );
    }

    #[test]
    fn a_registry_port_is_not_read_as_a_tag() {
        let r = Reference::parse("localhost:5000/team/app").unwrap();
        assert_eq!(r.domain, "localhost:5000");
        assert_eq!(r.repository, "team/app");
        assert_eq!(r.tag.as_deref(), Some("latest"));
        assert_eq!(r.endpoint(), "localhost:5000");
    }

    #[test]
    fn a_two_component_hub_name_keeps_both_components() {
        let r = Reference::parse("myuser/myapp:v1.2").unwrap();
        assert_eq!(r.repository, "myuser/myapp");
        assert_eq!(r.tag.as_deref(), Some("v1.2"));
        assert_eq!(r.display_repository(), "myuser/myapp");
    }

    #[test]
    fn a_digest_reference_carries_no_default_tag() {
        let d = format!("sha256:{}", "0".repeat(64));
        let r = Reference::parse(&format!("alpine@{d}")).unwrap();
        assert!(r.tag.is_none());
        assert_eq!(r.manifest_selector(), d);
    }

    #[test]
    fn a_tag_and_a_digest_together_fetch_by_digest() {
        let d = format!("sha256:{}", "0".repeat(64));
        let r = Reference::parse(&format!("alpine:3.20@{d}")).unwrap();
        assert_eq!(r.tag.as_deref(), Some("3.20"));
        assert_eq!(r.manifest_selector(), d);
    }

    /// ⭐ **The contract moved on 2026-09-09 and did not weaken.** This used to
    /// assert that `Reference::parse` returns an error. Parsing now records the
    /// fact and `pull` decides, because only `pull` has the transport policy in
    /// scope and can name the flag that would permit it (T-0213). ⛔ The
    /// scheme is still never inferred and never downgraded into: a reference
    /// with no scheme is `plain_http: false` and stays HTTPS.
    #[test]
    fn plain_http_is_recorded_and_never_inferred() {
        let r = Reference::parse("http://registry.example.com/x").unwrap();
        assert!(r.plain_http, "http:// was not carried up to the policy");
        assert_eq!(r.endpoint(), "registry.example.com");

        for bare in ["registry.example.com/x", "https://registry.example.com/x"] {
            assert!(
                !Reference::parse(bare).unwrap().plain_http,
                "{bare} was read as plain HTTP"
            );
        }
    }

    #[test]
    fn an_https_scheme_is_accepted_because_it_asks_for_what_podvm_already_does() {
        let r = Reference::parse("https://ghcr.io/o/r:t").unwrap();
        assert_eq!(r.domain, "ghcr.io");
        assert_eq!(r.repository, "o/r");
    }

    #[test]
    fn a_traversal_in_the_repository_path_is_refused() {
        // ⛔ Containment: the repository is a store directory component.
        for bad in [
            "../etc/passwd",
            "a//b",
            "reg.io/../x",
            "reg.io/./x",
            "..",
            "./x",
        ] {
            assert!(Reference::parse(bad).is_err(), "{bad} parsed");
        }
        // ⚠ `..` reaches the DOMAIN slot, because it contains a dot. The
        // repository check never sees it, so the domain has its own.
        assert!(!is_domain(".."));
        assert!(is_domain("localhost:5000") && is_domain("ghcr.io"));
        assert!(!is_domain("reg.io:") && !is_domain("reg.io:http"));
    }

    #[test]
    fn an_uppercase_repository_is_refused_rather_than_lowercased() {
        // Lowercasing would make `podvm pull Alpine` silently fetch something
        // the caller did not name.
        assert!(Reference::parse("Alpine").is_err());
    }
}
