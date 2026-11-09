//! The OCI image layout of a tarball: `oci-layout`, `index.json` and
//! `blobs/sha256/<hex>`. The constants, the filter of the entries that a
//! load takes, and the `index.json` of one image. The save, the load and the
//! import of a store are the caller's.

use crate::digest::Digest;
use crate::error::{Class, Result, registry};

/// The `oci-layout` file.
pub const OCI_LAYOUT: &str = r#"{"imageLayoutVersion":"1.0.0"}"#;
/// The annotation that names an image of the index: `REPOSITORY:TAG`.
pub const REF_ANNOTATION: &str = "org.opencontainers.image.ref.name";
/// The media type of a layer that is a plain tar.
pub const MEDIA_UNTAR: &str = "application/vnd.oci.image.layer.v1.tar";

/// The entries of a tarball that a load takes, and no other. A list of the
/// allowed names, not a check of traversal: a foreign tarball with more
/// paths gets a refusal by name, not a partial read.
pub fn allowed_entry(name: &str) -> bool {
    if name == "oci-layout" || name == "index.json" {
        return true;
    }
    let Some(hex) = name.strip_prefix("blobs/sha256/") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The `index.json` of one image: the manifest of `media_type`, its digest
/// and its size, named `refname` ([`REF_ANNOTATION`]).
pub fn index_json(media_type: &str, manifest: &Digest, size: usize, refname: &str) -> Result<Vec<u8>> {
    let index = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": crate::spec::MEDIA_OCI_INDEX,
        "manifests": [{
            "mediaType": media_type,
            "digest": manifest.to_string(),
            "size": size,
            "annotations": { REF_ANNOTATION: refname },
        }],
    });
    serde_json::to_vec(&index).map_err(|e| registry(Class::Protocol, format!("the save index does not render: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{Fetched, MEDIA_OCI_MANIFEST};

    #[test]
    fn only_the_names_of_a_layout_are_allowed() {
        assert!(allowed_entry("oci-layout") && allowed_entry("index.json"));
        assert!(allowed_entry(&format!("blobs/sha256/{}", "a".repeat(64))));
        for bad in
            ["blobs/sha256/../../etc/passwd", "blobs/sha512/aa", "manifest.json", "blobs/sha256/abc", "./index.json"]
        {
            assert!(!allowed_entry(bad), "{bad} was allowed");
        }
    }

    #[test]
    fn the_index_names_its_image_and_parses_as_an_index() {
        let d = Digest::of(b"manifest");
        let bytes = index_json(MEDIA_OCI_MANIFEST, &d, 8, "docker.io/library/imp:v9").unwrap();
        let Fetched::Index(index) = Fetched::parse(&bytes, None).unwrap() else {
            panic!("the index parsed as a manifest");
        };
        assert_eq!(index.manifests.len(), 1);
        let entry = &index.manifests[0];
        assert_eq!((entry.digest.as_str(), entry.size), (d.to_string().as_str(), 8));
        assert_eq!(entry.annotations[REF_ANNOTATION], "docker.io/library/imp:v9");
    }
}
