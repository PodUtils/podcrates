//! The four OCI documents podvm reads, written out by hand.
//!
//! `TODO/deps.md` T-0904 ruled `oci-spec` out against a number: 40 crates and
//! +69,664 bytes for four structs. These are those four.
//!
//! ⚠ Every field podvm does not read is deliberately absent rather than
//! carried as a passthrough. `serde` ignores unknown fields by default, so a
//! registry sending more is fine, and podvm never re-serialises a document it
//! did not fully understand: what goes back into the store is the **bytes as
//! received**, which is also what the digest was computed over.

use serde::Deserialize;

use crate::digest::Digest;
use crate::error::{Error, Result};

/// ⛔ **The platform is not a constant any more.** It was `OS = "linux"` and
/// `ARCH = "amd64"` until 2026-09-09, which made every pull an amd64 pull
/// whatever the machine was. [`crate::platform`] decides it at run time and
/// [`TODO/image.md`](../../../TODO/image.md) T-0212 is the entry.
pub use crate::platform::OS;

pub const MEDIA_OCI_INDEX: &str = "application/vnd.oci.image.index.v1+json";
pub const MEDIA_OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
pub const MEDIA_DOCKER_LIST: &str = "application/vnd.docker.distribution.manifest.list.v2+json";
pub const MEDIA_DOCKER_MANIFEST: &str = "application/vnd.docker.distribution.manifest.v2+json";

/// What `Accept:` offers, in preference order. ⚠ A registry picks the first it
/// can serve, so the OCI types lead and the docker types are the fallback.
pub const ACCEPT: &str = "application/vnd.oci.image.index.v1+json, \
     application/vnd.oci.image.manifest.v1+json, \
     application/vnd.docker.distribution.manifest.list.v2+json, \
     application/vnd.docker.distribution.manifest.v2+json";

#[derive(Debug, Clone, Deserialize)]
pub struct Platform {
    #[serde(default)]
    pub architecture: String,
    #[serde(default)]
    pub os: String,
    #[serde(default)]
    pub variant: Option<String>,
}

impl Platform {
    /// ⛔ `docs/conventions/forbidden-patterns.md`: fetching a variant of
    /// something into a cache keyed without the variant means the next
    /// unqualified fetch gets the variant. podvm names the platform on every
    /// fetch and records it beside the image, and this is the predicate.
    pub fn is(&self, os: &str, arch: &str) -> bool {
        self.os == os && self.architecture == arch
    }

    pub fn word(&self) -> String {
        match &self.variant {
            Some(v) if !v.is_empty() => format!("{}/{}/{}", self.os, self.architecture, v),
            _ => format!("{}/{}", self.os, self.architecture),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Descriptor {
    #[serde(rename = "mediaType", default)]
    pub media_type: String,
    pub digest: String,
    pub size: u64,
    #[serde(default)]
    pub platform: Option<Platform>,
    /// ⭐ TODO/image.md T-1320 reads the ref-name annotation `save` wrote.
    /// Absent on registry descriptors, which never carry it.
    #[serde(default)]
    pub annotations: std::collections::HashMap<String, String>,
}

impl Descriptor {
    pub fn parsed_digest(&self) -> Result<Digest> {
        Digest::parse(&self.digest)
    }
}

/// An image index or a docker manifest list. The two differ in `mediaType` and
/// in nothing podvm reads.
#[derive(Debug, Clone, Deserialize)]
pub struct Index {
    #[serde(rename = "schemaVersion", default)]
    pub schema_version: u32,
    #[serde(rename = "mediaType", default)]
    pub media_type: String,
    #[serde(default)]
    pub manifests: Vec<Descriptor>,
}

/// A single-platform image manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    #[serde(rename = "schemaVersion", default)]
    pub schema_version: u32,
    #[serde(rename = "mediaType", default)]
    pub media_type: String,
    pub config: Descriptor,
    #[serde(default)]
    pub layers: Vec<Descriptor>,
}

impl Manifest {
    /// What the layers cost on disk as fetched, which is what T-0203 has to
    /// precheck. ⚠ Compressed bytes: the extracted cost is M2's to measure and
    /// is not guessed here.
    pub fn stored_bytes(&self) -> u64 {
        self.config.size + self.layers.iter().map(|l| l.size).sum::<u64>()
    }
}

/// The image configuration blob. Only what podvm reports is declared.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub architecture: String,
    #[serde(default)]
    pub os: String,
    /// RFC 3339, as the specification requires. Absent on some old images, and
    /// then reported as absent rather than as an epoch.
    #[serde(default)]
    pub created: Option<String>,
    /// ⭐ What the image says to RUN. Absent until M3, because until M3 nothing
    /// read it and this module's header rules that a field podvm does not read
    /// is deliberately absent rather than carried as a passthrough.
    #[serde(default)]
    pub config: RunConfig,
}

/// The `config` object of an image configuration, restricted to what
/// [`TODO/enter.md`](../../../TODO/enter.md) M3 actually reads.
///
/// ⚠ The field names are the specification's, which are capitalised, and the
/// docker image config uses the same ones. `serde` renames rather than podvm
/// lower-casing them, so a reader comparing this against the spec sees the
/// spec's own words.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RunConfig {
    /// ⛔ `None` and `Some(vec![])` are different and podvm keeps them apart:
    /// an image with an empty `Entrypoint` array has explicitly cleared it, and
    /// treating that as "unset" would run the parent image's entrypoint.
    #[serde(rename = "Entrypoint")]
    pub entrypoint: Option<Vec<String>>,
    #[serde(rename = "Cmd")]
    pub cmd: Option<Vec<String>>,
    #[serde(rename = "Env", default)]
    pub env: Vec<String>,
    #[serde(rename = "WorkingDir", default)]
    pub working_dir: String,
    /// ⚠ Read and **reported**, never applied: podvm cannot `setuid` to an id
    /// this machine does not map, which is the wall the whole project is about.
    #[serde(rename = "User", default)]
    pub user: String,
}

/// What a manifest endpoint returned, before podvm knows which of the two it
/// is. ⭐ Dispatched on `mediaType` **from the document**, not on the response
/// header: a registry may serve either type under a generous `Accept`, and the
/// document is what the digest was computed over.
#[derive(Debug)]
pub enum Fetched {
    Index(Index),
    Manifest(Manifest),
}

impl Fetched {
    pub fn parse(bytes: &[u8], header_media_type: Option<&str>) -> Result<Fetched> {
        #[derive(Deserialize)]
        struct Peek {
            #[serde(rename = "mediaType")]
            media_type: Option<String>,
            #[serde(rename = "schemaVersion")]
            schema_version: Option<u32>,
            manifests: Option<serde_json::Value>,
            config: Option<serde_json::Value>,
        }
        let peek: Peek = serde_json::from_slice(bytes).map_err(|e| {
            Error::Oci(format!(
                "the registry's manifest document does not parse: {e}"
            ))
        })?;

        if peek.schema_version == Some(1) {
            return Err(Error::Oci(
                "this is a schema 1 manifest. podvm does not read schema 1: it \
                 carries no config descriptor and its digest is not the digest \
                 docker reports, so digest parity (TODO/image.md T-0202) cannot \
                 hold for it"
                    .into(),
            ));
        }

        // The document's own type first, the transport's second, the shape last.
        let media = peek
            .media_type
            .as_deref()
            .or(header_media_type)
            .unwrap_or("");
        match media {
            MEDIA_OCI_INDEX | MEDIA_DOCKER_LIST => Ok(Fetched::Index(from_slice(bytes)?)),
            MEDIA_OCI_MANIFEST | MEDIA_DOCKER_MANIFEST => Ok(Fetched::Manifest(from_slice(bytes)?)),
            other => {
                // ⛔ Never a guess dressed as a reading. Where no declared type
                // is usable, the SHAPE decides and the fallback is named in the
                // error if the shape is ambiguous too.
                match (peek.manifests.is_some(), peek.config.is_some()) {
                    (true, false) => Ok(Fetched::Index(from_slice(bytes)?)),
                    (false, true) => Ok(Fetched::Manifest(from_slice(bytes)?)),
                    _ => Err(Error::Oci(format!(
                        "manifest media type {other:?} is not one podvm reads, and \
                         the document has neither a lone `manifests` array nor a \
                         lone `config` descriptor to decide it by shape"
                    ))),
                }
            }
        }
    }
}

fn from_slice<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes)
        .map_err(|e| Error::Oci(format!("the registry's document does not parse: {e}")))
}

/// Pick the entry for one platform out of an index.
///
/// ⛔ Never "the first one". An index is ordered by the registry, and taking
/// the head is how a caller ends up with an arm64 rootfs whose failure reads as
/// `Exec format error` rather than as a wrong pull.
///
/// ⭐ **Two passes, and the order is the point.** An exact variant match wins
/// over a match that ignores the variant, because `linux/arm/v7` and
/// `linux/arm/v6` are different images and the first pass is what keeps them
/// apart. Only when nothing matched exactly does the wildcard reading apply,
/// which is what makes `linux/arm64` find a `linux/arm64/v8` entry.
pub fn select_platform<'a>(
    index: &'a Index,
    want: &crate::platform::Platform,
) -> Result<&'a Descriptor> {
    let mut offered: Vec<String> = Vec::new();
    let mut loose: Option<&'a Descriptor> = None;
    for m in &index.manifests {
        // ⚠ An attestation entry carries no platform, or `unknown/unknown`. It
        // is not a candidate and is not reported as one, which is why this is
        // `if let` and there is no `else`.
        let Some(p) = &m.platform else { continue };
        let v = p.variant.as_deref().filter(|s| !s.is_empty());
        if want.os == p.os && want.arch == p.architecture {
            match (want.variant.as_deref(), v) {
                (Some(a), Some(b)) if a == b => return Ok(m),
                (Some(_), Some(_)) => {}
                // ⚠ One side said nothing about the variant, so this is a
                // candidate but not yet the answer: an exact match later in the
                // index still wins.
                _ => loose = loose.or(Some(m)),
            }
        }
        offered.push(p.word());
    }
    if let Some(m) = loose {
        return Ok(m);
    }
    Err(Error::NoPlatform {
        want: want.to_string(),
        offered: if offered.is_empty() {
            "nothing with a platform".to_string()
        } else {
            offered.join(", ")
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = r#"{
      "schemaVersion": 2,
      "mediaType": "application/vnd.oci.image.index.v1+json",
      "manifests": [
        {"mediaType":"application/vnd.oci.image.manifest.v1+json",
         "digest":"sha256:1111111111111111111111111111111111111111111111111111111111111111",
         "size":10,"platform":{"architecture":"arm64","os":"linux","variant":"v8"}},
        {"mediaType":"application/vnd.oci.image.manifest.v1+json",
         "digest":"sha256:2222222222222222222222222222222222222222222222222222222222222222",
         "size":20,"platform":{"architecture":"amd64","os":"linux"}},
        {"mediaType":"application/vnd.oci.image.manifest.v1+json",
         "digest":"sha256:3333333333333333333333333333333333333333333333333333333333333333",
         "size":30}
      ]}"#;

    fn p(s: &str) -> crate::platform::Platform {
        crate::platform::Platform::parse(s).unwrap()
    }

    #[test]
    fn the_platform_is_selected_by_name_and_never_by_position() {
        let Fetched::Index(index) = Fetched::parse(INDEX.as_bytes(), None).unwrap() else {
            panic!("parsed as a manifest");
        };
        // ⚠ amd64 is the SECOND entry and arm64 is the first, so a selector
        // taking the head would pass every other assertion here.
        let picked = select_platform(&index, &p("linux/amd64")).unwrap();
        assert!(picked.digest.ends_with("2222"));
        let arm = select_platform(&index, &p("linux/arm64")).unwrap();
        assert!(arm.digest.ends_with("1111"));
    }

    #[test]
    fn an_index_without_the_platform_names_what_it_does_offer() {
        let Fetched::Index(index) = Fetched::parse(INDEX.as_bytes(), None).unwrap() else {
            panic!("parsed as a manifest");
        };
        let e = select_platform(&index, &p("linux/riscv64")).unwrap_err();
        let Error::NoPlatform { want, offered: _ } = &e else {
            panic!("wrong variant: {e}");
        };
        assert_eq!(want, "linux/riscv64", "{e}");
        let text = format!("{e}");
        assert!(text.contains("linux/arm64/v8"), "{text}");
        assert!(text.contains("linux/amd64"), "{text}");
    }

    /// ⛔ `linux/arm64` and `linux/arm64/v8` are one platform written two ways,
    /// and registries write both. An unqualified ask has to find the qualified
    /// entry or every arm64 pull fails against half the registries.
    #[test]
    fn an_unqualified_ask_finds_a_variant_qualified_entry() {
        let Fetched::Index(index) = Fetched::parse(INDEX.as_bytes(), None).unwrap() else {
            panic!("parsed as a manifest");
        };
        let picked = select_platform(&index, &p("linux/arm64")).unwrap();
        assert!(picked.digest.ends_with("1111"), "{}", picked.digest);
    }

    /// ⭐ The two-pass order, and the case that needs it: an exact variant
    /// match must beat a loose one that appears EARLIER in the index.
    #[test]
    fn an_exact_variant_beats_a_loose_match_earlier_in_the_index() {
        let doc = br#"{"schemaVersion":2,
          "mediaType":"application/vnd.oci.image.index.v1+json",
          "manifests":[
            {"mediaType":"application/vnd.oci.image.manifest.v1+json",
             "digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
             "size":10,"platform":{"architecture":"arm","os":"linux"}},
            {"mediaType":"application/vnd.oci.image.manifest.v1+json",
             "digest":"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
             "size":20,"platform":{"architecture":"arm","os":"linux","variant":"v7"}}
          ]}"#;
        let Fetched::Index(index) = Fetched::parse(doc, None).unwrap() else {
            panic!("parsed as a manifest");
        };
        let picked = select_platform(&index, &p("linux/arm/v7")).unwrap();
        assert!(
            picked.digest.starts_with("sha256:bbbb"),
            "{}",
            picked.digest
        );
        // ⛔ And v6 must NOT take the v7 entry: they are different machines.
        let six = select_platform(&index, &p("linux/arm/v6")).unwrap();
        assert!(six.digest.starts_with("sha256:aaaa"), "{}", six.digest);
    }

    #[test]
    fn a_manifest_is_recognised_by_its_own_media_type_not_the_header() {
        let doc = br#"{"schemaVersion":2,
          "mediaType":"application/vnd.oci.image.manifest.v1+json",
          "config":{"mediaType":"application/vnd.oci.image.config.v1+json",
                    "digest":"sha256:4444444444444444444444444444444444444444444444444444444444444444",
                    "size":7},
          "layers":[{"mediaType":"application/vnd.oci.image.layer.v1.tar+gzip",
                     "digest":"sha256:5555555555555555555555555555555555555555555555555555555555555555",
                     "size":11}]}"#;
        // The header lies; the document does not.
        let got = Fetched::parse(doc, Some(MEDIA_DOCKER_LIST)).unwrap();
        let Fetched::Manifest(m) = got else {
            panic!("the header overrode the document");
        };
        assert_eq!(m.stored_bytes(), 18);
    }

    #[test]
    fn a_schema_one_manifest_is_refused_by_name() {
        let e =
            Fetched::parse(br#"{"schemaVersion":1,"name":"library/alpine"}"#, None).unwrap_err();
        assert!(format!("{e}").contains("schema 1"), "{e}");
    }
}
