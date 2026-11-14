//! The four OCI documents that the client reads, written by hand: an
//! index, a manifest, a descriptor and the configuration of an image. The
//! `oci-spec` crate costs about 40 crates for these four structs.
//!
//! A field that no caller reads is absent, not carried through. `serde`
//! ignores unknown fields, so a registry can send more. A document is never
//! written again from its parse: a store keeps the bytes as received, which
//! are the bytes of the digest.

use serde::Deserialize;

use crate::digest::Digest;
use crate::error::{Class, Refusal, Result, registry};

/// The OS of the client. [`crate::platform`] decides the platform at run
/// time.
pub use crate::platform::OS;

pub const MEDIA_OCI_INDEX: &str = "application/vnd.oci.image.index.v1+json";
pub const MEDIA_OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
pub const MEDIA_DOCKER_LIST: &str = "application/vnd.docker.distribution.manifest.list.v2+json";
pub const MEDIA_DOCKER_MANIFEST: &str = "application/vnd.docker.distribution.manifest.v2+json";

/// What `Accept:` offers, the first choice first. A registry takes the first
/// type that it can serve, so the OCI types come before the docker types.
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
    /// The version of the OS (`10.0.17763.6532`): a Windows index gives one
    /// entry for each version of one `windows/amd64`.
    #[serde(rename = "os.version", default)]
    pub os_version: Option<String>,
}

impl Platform {
    /// A cache with no variant in its key gives a fetched variant to the
    /// next fetch with no variant. A caller names the platform on each fetch
    /// and keeps it with the image; this is the test.
    pub fn is(&self, os: &str, arch: &str) -> bool {
        self.os == os && self.architecture == arch
    }

    pub fn word(&self) -> String {
        match &self.variant {
            Some(v) if !v.is_empty() => format!("{}/{}/{}", self.os, self.architecture, v),
            _ => format!("{}/{}", self.os, self.architecture),
        }
    }

    /// The word with the version of the OS, in the form of containerd's
    /// `platforms.FormatAll`: `windows(10.0.17763.6532)/amd64`. The word of
    /// a platform with no `os.version` is [`Platform::word`].
    pub fn full_word(&self) -> String {
        match self.os_version.as_deref().filter(|v| !v.is_empty()) {
            Some(v) => format!("{}({v}){}", self.os, &self.word()[self.os.len()..]),
            None => self.word(),
        }
    }
}

/// The annotation of an attestation manifest of BuildKit (its value
/// [`ATTESTATION`]): an entry of an index that is no image.
pub const REFERENCE_TYPE: &str = "vnd.docker.reference.type";
pub const ATTESTATION: &str = "attestation-manifest";

#[derive(Debug, Clone, Deserialize)]
pub struct Descriptor {
    #[serde(rename = "mediaType", default)]
    pub media_type: String,
    pub digest: String,
    pub size: u64,
    #[serde(default)]
    pub platform: Option<Platform>,
    /// The annotations: an OCI layout names an image in
    /// [`crate::layout::REF_ANNOTATION`]. A registry descriptor has none.
    #[serde(default)]
    pub annotations: std::collections::HashMap<String, String>,
}

impl Descriptor {
    pub fn parsed_digest(&self) -> Result<Digest> {
        Digest::parse(&self.digest)
    }

    /// Whether this entry of an index is an attestation manifest
    /// ([`REFERENCE_TYPE`] is [`ATTESTATION`]), not an image.
    pub fn is_attestation(&self) -> bool {
        self.annotations.get(REFERENCE_TYPE).is_some_and(|v| v == ATTESTATION)
    }
}

/// An image index or a docker manifest list. The two differ in `mediaType`
/// and in no field that the client reads.
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
    /// The bytes of the configuration and the layers as fetched: the check
    /// of the free space before a pull. These are compressed bytes; the
    /// extracted size is not guessed here.
    pub fn stored_bytes(&self) -> u64 {
        self.config.size + self.layers.iter().map(|l| l.size).sum::<u64>()
    }
}

/// The configuration of an image. Only the fields that callers read.
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
    /// What the image says to run.
    #[serde(default)]
    pub config: RunConfig,
}

/// The `config` object of an image configuration: the fields of an entry.
///
/// The names of the fields are the specification's, with capitals; the
/// docker configuration uses the same names. `serde` renames them, so the
/// words are the words of the specification.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RunConfig {
    /// `None` and `Some(vec![])` are different: an image with an empty
    /// `Entrypoint` array cleared it, and "unset" would run the entry point
    /// of the parent image.
    #[serde(rename = "Entrypoint")]
    pub entrypoint: Option<Vec<String>>,
    #[serde(rename = "Cmd")]
    pub cmd: Option<Vec<String>>,
    #[serde(rename = "Env", default)]
    pub env: Vec<String>,
    #[serde(rename = "WorkingDir", default)]
    pub working_dir: String,
    /// The user of the image. A caller with no root cannot always change to
    /// it.
    #[serde(rename = "User", default)]
    pub user: String,
}

/// What a manifest endpoint gave, before the client knows which of the two
/// it is. The `mediaType` of the document decides, not the header of the
/// answer: a registry can serve either type for a wide `Accept`, and the
/// digest is of the document.
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
        let peek: Peek = serde_json::from_slice(bytes)
            .map_err(|e| registry(Class::Protocol, format!("the registry's manifest document does not parse: {e}")))?;

        if peek.schema_version == Some(1) {
            return Err(registry(
                Class::Unsupported,
                "this is a schema 1 manifest. The client does not read schema 1: it \
                 carries no config descriptor and its digest is not the digest \
                 docker reports, so the digest cannot be the digest of docker",
            ));
        }

        // The document's own type first, the transport's second, the shape last.
        let media = peek.media_type.as_deref().or(header_media_type).unwrap_or("");
        match media {
            MEDIA_OCI_INDEX | MEDIA_DOCKER_LIST => Ok(Fetched::Index(from_slice(bytes)?)),
            MEDIA_OCI_MANIFEST | MEDIA_DOCKER_MANIFEST => Ok(Fetched::Manifest(from_slice(bytes)?)),
            other => {
                // No guess: where no declared type can be used, the shape
                // decides, and an error names it when the shape cannot.
                match (peek.manifests.is_some(), peek.config.is_some()) {
                    (true, false) => Ok(Fetched::Index(from_slice(bytes)?)),
                    (false, true) => Ok(Fetched::Manifest(from_slice(bytes)?)),
                    _ => Err(registry(
                        Class::Unsupported,
                        format!(
                            "manifest media type {other:?} is not one the client reads, and \
                             the document has neither a lone `manifests` array nor a \
                             lone `config` descriptor to decide it by shape"
                        ),
                    )),
                }
            }
        }
    }
}

fn from_slice<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes)
        .map_err(|e| registry(Class::Protocol, format!("the registry's document does not parse: {e}")))
}

/// Pick the entry for one platform out of an index.
///
/// Never "the first one". The registry orders an index, and its head can be
/// an arm64 root file system that then fails with `Exec format error`.
///
/// Two passes, in this order. A match of the variant wins over a match that
/// ignores the variant: `linux/arm/v7` and `linux/arm/v6` are different
/// images. Only when no entry matched with its variant does an entry with no
/// variant match, so that `linux/arm64` finds a `linux/arm64/v8` entry. No
/// match is [`Refusal::NoPlatform`] (`Class::NotFound`): its list names
/// each platform one time, with its `os.version` ([`Platform::full_word`]),
/// and no attestation manifest.
pub fn select_platform<'a>(index: &'a Index, want: &crate::platform::Platform) -> Result<&'a Descriptor> {
    let mut offered: Vec<String> = Vec::new();
    let mut loose: Option<&'a Descriptor> = None;
    for m in &index.manifests {
        // An attestation entry has no platform, or `unknown/unknown` and the
        // annotation. It is not a candidate and the error does not name it.
        if m.is_attestation() {
            continue;
        }
        let Some(p) = &m.platform else { continue };
        let v = p.variant.as_deref().filter(|s| !s.is_empty());
        if want.os == p.os && want.arch == p.architecture {
            match (want.variant.as_deref(), v) {
                (Some(a), Some(b)) if a == b => return Ok(m),
                (Some(_), Some(_)) => {}
                // One side gave no variant: a candidate, not yet the answer.
                // A match of the variant later in the index wins.
                _ => loose = loose.or(Some(m)),
            }
        }
        let word = p.full_word();
        if !offered.contains(&word) {
            offered.push(word);
        }
    }
    if let Some(m) = loose {
        return Ok(m);
    }
    Err(Refusal::NoPlatform {
        want: want.to_string(),
        offered: if offered.is_empty() { "nothing with a platform".to_string() } else { offered.join(", ") },
    }
    .failure())
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
        // amd64 is the second entry and arm64 the first: a choice of the head
        // passes each other assertion here.
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
        let Some(Refusal::NoPlatform { want, offered: _ }) = crate::refusal(&e) else {
            panic!("wrong variant: {e}");
        };
        assert_eq!(e.class, Class::NotFound);
        assert_eq!(want, "linux/riscv64", "{e}");
        let text = format!("{e}");
        assert!(text.contains("linux/arm64/v8"), "{text}");
        assert!(text.contains("linux/amd64"), "{text}");
    }

    /// PodUtils/podvm#346: a Windows index named `windows/amd64` two times
    /// and its attestations as `unknown/unknown`. The list names each
    /// platform one time, with its `os.version`, and no attestation.
    #[test]
    fn the_offered_list_has_no_repeat_no_attestation_and_the_os_version() {
        let entry = |d: char, platform: &str, extra: &str| {
            format!(
                r#"{{"mediaType":"application/vnd.oci.image.manifest.v1+json","digest":"sha256:{}","size":1,
                   "platform":{platform}{extra}}}"#,
                d.to_string().repeat(64)
            )
        };
        let att = r#","annotations":{"vnd.docker.reference.type":"attestation-manifest",
                                     "vnd.docker.reference.digest":"sha256:x"}"#;
        let doc = format!(
            r#"{{"schemaVersion":2,"mediaType":"application/vnd.oci.image.index.v1+json","manifests":[{}]}}"#,
            [
                entry('a', r#"{"architecture":"amd64","os":"windows","os.version":"10.0.17763.6532"}"#, ""),
                entry('b', r#"{"architecture":"amd64","os":"windows","os.version":"10.0.20348.2849"}"#, ""),
                entry('f', r#"{"architecture":"amd64","os":"linux"}"#, att),
                entry('c', r#"{"architecture":"amd64","os":"linux"}"#, ""),
                entry('d', r#"{"architecture":"amd64","os":"linux"}"#, ""),
                entry('e', r#"{"architecture":"unknown","os":"unknown"}"#, att),
            ]
            .join(",")
        );
        let Fetched::Index(index) = Fetched::parse(doc.as_bytes(), None).unwrap() else {
            panic!("parsed as a manifest");
        };
        let e = select_platform(&index, &p("linux/arm64")).unwrap_err();
        let Some(Refusal::NoPlatform { offered, .. }) = crate::refusal(&e) else {
            panic!("wrong variant: {e}");
        };
        assert_eq!(offered, "windows(10.0.17763.6532)/amd64, windows(10.0.20348.2849)/amd64, linux/amd64", "{e}");
        // An attestation with the platform of an image, before the image,
        // is not a candidate either.
        assert!(select_platform(&index, &p("linux/amd64")).unwrap().digest.ends_with("cccc"));
        assert!(index.manifests[5].is_attestation() && !index.manifests[0].is_attestation());
    }

    /// `linux/arm64` and `linux/arm64/v8` are one platform, written two ways,
    /// and registries write both. An ask with no variant finds the entry with
    /// a variant.
    #[test]
    fn an_unqualified_ask_finds_a_variant_qualified_entry() {
        let Fetched::Index(index) = Fetched::parse(INDEX.as_bytes(), None).unwrap() else {
            panic!("parsed as a manifest");
        };
        let picked = select_platform(&index, &p("linux/arm64")).unwrap();
        assert!(picked.digest.ends_with("1111"), "{}", picked.digest);
    }

    /// The order of the two passes: a match of the variant wins over a loose
    /// match earlier in the index.
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
        assert!(picked.digest.starts_with("sha256:bbbb"), "{}", picked.digest);
        // v6 does not take the v7 entry: they are different machines.
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
        let e = Fetched::parse(br#"{"schemaVersion":1,"name":"library/alpine"}"#, None).unwrap_err();
        assert!(format!("{e}").contains("schema 1"), "{e}");
    }
}
