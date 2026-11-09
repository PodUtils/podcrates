//! The parse of an image reference, by docker's rules of normalisation.
//!
//! The normalisation has a function: `alpine`, `alpine:latest`,
//! `docker.io/library/alpine` and `index.docker.io/library/alpine:latest`
//! are one image. A store that keys them as four images pulls four times.
//!
//! HTTPS only. A reference with an explicit `http://` scheme is carried as
//! [`Reference::plain_http`], and the caller decides with its policy
//! ([`crate::transport::Policy::permits_explicit_http`]).
//!
//! The names of Docker Hub are docker's rules of the reference, so they are
//! constants here. The service file `docker-hub-registry` of `pod-services`
//! holds the same values, and a test compares them.

use std::fmt;

use crate::digest::Digest;
use crate::error::{Result, usage};

/// The domain of a name with no domain. `docker.io` is the name that docker
/// prints, and `registry-1.docker.io` is the host that answers `/v2/`.
pub const DEFAULT_DOMAIN: &str = "docker.io";
pub const DEFAULT_ENDPOINT: &str = "registry-1.docker.io";
/// The other domains of Docker Hub: each one has [`DEFAULT_ENDPOINT`].
pub const HUB_ALIASES: &[&str] = &["index.docker.io"];
/// docker puts this prefix before a name of one component on the hub.
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
    /// The caller wrote `http://`. The parse does not refuse it: the
    /// transport policy decides, and a `Reference` is a name that knows no
    /// policy. The caller refuses it where its policy and its flags are in
    /// scope.
    pub plain_http: bool,
}

impl Reference {
    pub fn parse(input: &str) -> Result<Reference> {
        let s = input.trim();
        if s.is_empty() {
            return Err(usage("invalid reference: the reference is empty"));
        }
        for scheme in ["http://", "https://"] {
            if let Some(rest) = s.strip_prefix(scheme) {
                if scheme == "http://" {
                    // The scheme goes and the host stays, so that the caller
                    // can name the registry to permit.
                    let mut r = Reference::parse(rest)?;
                    r.plain_http = true;
                    return Ok(r);
                }
                // `https://` is accepted and removed: it asks for what the
                // client does in each case.
                return Reference::parse(rest);
            }
        }

        // The digest first: `@` is not in a domain or a tag, so it has one
        // meaning wherever it is.
        let (rest, digest) = match s.split_once('@') {
            Some((rest, d)) => (rest, Some(Digest::parse(d)?)),
            None => (s, None),
        };

        // docker's rule: the first component is a domain when it has a `.` or
        // a `:`, or is `localhost`. Each other name is a path on the hub.
        // With no rule, `myhost:5000/x` and `user/x:5000` are the same.
        let (domain, remainder) = match rest.split_once('/') {
            Some((head, tail)) if head == "localhost" || head.contains('.') || head.contains(':') => {
                // Checked, not only found. `..` has a dot, so the rule above
                // reads `../etc/passwd` as a registry `..` with a repository
                // `etc/passwd`, and the check of the repository passes it.
                if !is_domain(head) {
                    return Err(usage(format!(
                        "invalid reference: {input:?}: {head:?} is where a registry host goes and is \
                         not one"
                    )));
                }
                (head.to_string(), tail)
            }
            _ => (DEFAULT_DOMAIN.to_string(), rest),
        };

        // The tag is after the last colon, and only when no `/` comes after
        // it, so that a port of a domain is not read as a tag.
        let (path, tag) = match remainder.rsplit_once(':') {
            Some((p, t)) if !t.contains('/') => (p, Some(t.to_string())),
            _ => (remainder, None),
        };

        if path.is_empty() {
            return Err(usage(format!("invalid reference: {input:?} names no repository")));
        }
        for component in path.split('/') {
            if component.is_empty() || component == "." || component == ".." {
                return Err(usage(format!(
                    "invalid reference: {input:?} has an empty or relative path component, which cannot \
                     name a repository and cannot be a store directory"
                )));
            }
            if !component.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b)) {
                return Err(usage(format!(
                    "invalid reference: {input:?}: repository component {component:?} is not lowercase \
                     alphanumeric with separators"
                )));
            }
        }
        if let Some(t) = &tag
            && (t.is_empty() || !t.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)))
        {
            return Err(usage(format!("invalid reference: {input:?}: tag {t:?} is not valid")));
        }

        let repository = if domain == DEFAULT_DOMAIN && !path.contains('/') {
            format!("{OFFICIAL_PREFIX}/{path}")
        } else {
            path.to_string()
        };

        // A reference with no tag and no digest means `:latest`. The default
        // is set here, once, so that no caller sets its own.
        let tag = match (&tag, &digest) {
            (None, None) => Some(DEFAULT_TAG.to_string()),
            _ => tag,
        };

        Ok(Reference { domain, repository, tag, digest, plain_http: false })
    }

    /// The host of the `/v2/` requests. It is not [`Reference::domain`] for
    /// the hub alone.
    pub fn endpoint(&self) -> &str {
        if self.domain == DEFAULT_DOMAIN || HUB_ALIASES.contains(&self.domain.as_str()) {
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

    /// How docker prints the repository: with no `library/` prefix of the hub
    /// and no `docker.io/` domain, and with each other part.
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

    /// The canonical `<domain>/<repository>`: the key of a store. Not the
    /// printed form: two printed forms can be one image.
    pub fn canonical_repository(&self) -> String {
        format!("{}/{}", self.domain, self.repository)
    }
}

/// A registry host: labels of letters, digits and hyphens with dots between
/// them, and an optional `:port`. Narrower than a URI authority on purpose:
/// no user information and no path get to a URL.
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
        && host
            .split('.')
            .all(|label| !label.is_empty() && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
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
        let keys: Vec<String> =
            ["alpine", "alpine:latest", "docker.io/library/alpine", "index.docker.io/library/alpine:latest"]
                .iter()
                .map(|s| Reference::parse(s).unwrap().canonical_repository())
                .collect();
        // `index.docker.io` is another domain with the same endpoint. It is
        // kept as written, so that a list prints what the caller asked for.
        assert_eq!(keys[0], keys[1]);
        assert_eq!(keys[1], keys[2]);
        assert_eq!(Reference::parse(&keys[3]).unwrap().endpoint(), DEFAULT_ENDPOINT);
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

    /// The parse records `http://`, and the caller decides with its policy.
    /// The scheme is never guessed: a reference with no scheme is
    /// `plain_http: false` and stays HTTPS.
    #[test]
    fn plain_http_is_recorded_and_never_inferred() {
        let r = Reference::parse("http://registry.example.com/x").unwrap();
        assert!(r.plain_http, "http:// was not carried up to the policy");
        assert_eq!(r.endpoint(), "registry.example.com");

        for bare in ["registry.example.com/x", "https://registry.example.com/x"] {
            assert!(!Reference::parse(bare).unwrap().plain_http, "{bare} was read as plain HTTP");
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
        // The repository becomes a directory of a store.
        for bad in ["../etc/passwd", "a//b", "reg.io/../x", "reg.io/./x", "..", "./x"] {
            assert!(Reference::parse(bad).is_err(), "{bad} parsed");
        }
        // `..` gets to the place of the domain, as it has a dot. The check
        // of the repository does not see it, so the domain has its own.
        assert!(!is_domain(".."));
        assert!(is_domain("localhost:5000") && is_domain("ghcr.io"));
        assert!(!is_domain("reg.io:") && !is_domain("reg.io:http"));
    }

    #[test]
    fn an_uppercase_repository_is_refused_rather_than_lowercased() {
        // A change to lower case would fetch, with no word, an image that
        // the caller did not name.
        assert_eq!(Reference::parse("Alpine").unwrap_err().class, pod_core::Class::Usage);
        assert!(Reference::parse("Alpine").is_err());
    }

    /// The constants of Docker Hub are the data of `pod-services`.
    #[test]
    fn the_hub_constants_are_the_data_of_pod_services() {
        let hub = pod_services::get("docker-hub-registry").expect("the service file of Docker Hub");
        assert_eq!(hub.text("domain"), Some(DEFAULT_DOMAIN));
        assert_eq!(hub.endpoint("registry").map(|e| e.host()), Some(DEFAULT_ENDPOINT));
        assert_eq!(hub.text("official-prefix"), Some(OFFICIAL_PREFIX));
        let aliases: Vec<&str> = hub.texts("aliases").unwrap_or_default().iter().map(|a| a.as_ref()).collect();
        assert_eq!(aliases, HUB_ALIASES);
    }
}
