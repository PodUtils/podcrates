//! The `rootfs` object of an image configuration, and the check that it
//! agrees with the manifest. A configuration is untrusted: the count of
//! layers is bounded before it is used.

use serde::Deserialize;

use crate::digest::Digest;
use crate::error::{Class, Result, registry};
use crate::spec::{Config, Manifest};

/// The most layers that the check accepts, in a manifest or a configuration.
/// Docker stops at 127 layers; this bound is wide and still keeps a hostile
/// document from asking for an unbounded loop of a consumer.
pub const MAX_LAYERS: usize = 4096;

/// The only `rootfs.type` that the specification defines.
pub const ROOTFS_LAYERS: &str = "layers";

/// The `rootfs` of an image configuration.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RootFs {
    /// `layers` in each image that the specification defines.
    #[serde(rename = "type", default)]
    pub kind: String,
    /// The digest of the uncompressed bytes of each layer, lowest first. A
    /// string, as in [`crate::spec::Descriptor`]: [`RootFs::parsed_diff_ids`]
    /// gives [`Digest`] values.
    #[serde(default)]
    pub diff_ids: Vec<String>,
}

impl RootFs {
    /// Each `diff_id` as a [`Digest`]. A value that is not a digest is an
    /// error.
    pub fn parsed_diff_ids(&self) -> Result<Vec<Digest>> {
        self.diff_ids.iter().map(|d| Digest::parse(d)).collect()
    }
}

/// Check that the `rootfs` of `config` agrees with the layers of `manifest`:
/// the type is `layers`, and the count of `diff_ids` is the count of layers.
/// A configuration with no `rootfs` (an old image) passes. The error names
/// both counts. It does not compare a `diff_id` with a layer.
pub fn check_layers(manifest: &Manifest, config: &Config) -> Result<()> {
    let Some(rootfs) = &config.rootfs else { return Ok(()) };
    let (layers, ids) = (manifest.layers.len(), rootfs.diff_ids.len());
    if layers > MAX_LAYERS || ids > MAX_LAYERS {
        return Err(registry(
            Class::Policy,
            format!(
                "the manifest names {layers} layer(s) and the configuration has {ids} diff_id(s); \
                 the limit is {MAX_LAYERS}"
            ),
        ));
    }
    if rootfs.kind != ROOTFS_LAYERS {
        return Err(registry(
            Class::Protocol,
            format!(
                "the rootfs type of the configuration is {:?}, not {ROOTFS_LAYERS:?} (the manifest names \
                 {layers} layer(s) and the configuration has {ids} diff_id(s))",
                rootfs.kind
            ),
        ));
    }
    if layers != ids {
        return Err(registry(
            Class::Protocol,
            format!("the manifest names {layers} layer(s) and the configuration has {ids} diff_id(s)"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(n: usize) -> Manifest {
        let layer = |i: usize| {
            format!(r#"{{"mediaType":"application/vnd.oci.image.layer.v1.tar","digest":"sha256:{i:064}","size":1}}"#)
        };
        let layers: Vec<String> = (0..n).map(layer).collect();
        serde_json::from_str(&format!(
            r#"{{"schemaVersion":2,"config":{{"digest":"sha256:{:064}","size":1}},"layers":[{}]}}"#,
            99,
            layers.join(",")
        ))
        .expect("a manifest")
    }

    fn config(kind: &str, n: usize) -> Config {
        let ids: Vec<String> = (0..n).map(|i| format!(r#""sha256:{i:064}""#)).collect();
        serde_json::from_str(&format!(
            r#"{{"architecture":"amd64","os":"linux","rootfs":{{"type":"{kind}","diff_ids":[{}]}}}}"#,
            ids.join(",")
        ))
        .expect("a config")
    }

    #[test]
    fn matching_counts_pass() {
        assert!(check_layers(&manifest(2), &config("layers", 2)).is_ok());
        assert!(check_layers(&manifest(0), &config("layers", 0)).is_ok());
    }

    #[test]
    fn two_layers_and_one_diff_id_fail_with_both_counts() {
        let e = check_layers(&manifest(2), &config("layers", 1)).unwrap_err();
        assert_eq!(e.class, Class::Protocol);
        let text = e.to_string();
        assert!(text.contains("2 layer(s)") && text.contains("1 diff_id(s)"), "{text}");
        assert!(check_layers(&manifest(1), &config("layers", 2)).is_err());
    }

    #[test]
    fn a_config_with_no_rootfs_passes() {
        let c: Config = serde_json::from_str(r#"{"architecture":"amd64","os":"linux"}"#).unwrap();
        assert!(c.rootfs.is_none());
        assert!(check_layers(&manifest(3), &c).is_ok());
    }

    #[test]
    fn a_type_that_is_not_layers_fails() {
        let e = check_layers(&manifest(1), &config("squashed", 1)).unwrap_err();
        assert!(e.to_string().contains("\"squashed\""), "{e}");
    }

    #[test]
    fn a_count_past_the_bound_is_refused() {
        let e = check_layers(&manifest(MAX_LAYERS + 1), &config("layers", MAX_LAYERS + 1)).unwrap_err();
        assert_eq!(e.class, Class::Policy, "{e}");
    }

    #[test]
    fn the_parse_of_a_real_config_gives_the_diff_ids() {
        let c: Config = serde_json::from_str(
            r#"{"architecture":"amd64","os":"linux","config":{"Cmd":["sh"]},
                "rootfs":{"type":"layers","diff_ids":[
                  "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                  "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"]},
                "history":[{}]}"#,
        )
        .unwrap();
        let r = c.rootfs.as_ref().unwrap();
        assert_eq!(r.kind, "layers");
        let ids = r.parsed_diff_ids().unwrap();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[1].hex(), "b".repeat(64));
        assert!(RootFs { kind: "layers".into(), diff_ids: vec!["x".into()] }.parsed_diff_ids().is_err());
    }
}
