//! The import of an image from a stream (`POST /1.0/images` with the
//! image as the body, `shared/api/image.go` and the handler of the upload):
//! a unified tarball as `application/octet-stream`, or the metadata tarball
//! and the root file system as `multipart/form-data` (the parts `metadata`
//! and `rootfs`, or `rootfs.img` for a VM). The headers `X-Incus-public`,
//! `X-Incus-filename`, `X-Incus-properties` and `X-Incus-fingerprint` go
//! with it. incus answers with an operation whose metadata gives the
//! `fingerprint` and the `size`. The aliases are made after it with
//! `POST /1.0/images/aliases`: the header `X-Incus-aliases` needs the
//! extension `image_import_alias`, which incus 6.0 does not have.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::Client;
use crate::error::{Error, Result};
use crate::upload::{Payload, Upload, system_boundary};
use crate::wire::{Body, escape};

/// The bytes of an image. incus refuses a `metadata.yaml` whose
/// `creation_date` is 0 ("Missing creation date").
#[derive(Debug)]
pub enum ImageData {
    /// One tarball with `metadata.yaml`, `templates/` and `rootfs/`.
    Unified(Payload),
    /// The metadata tarball, and the root file system apart: a tarball, or
    /// the disk of a VM (`vm`).
    Split { metadata: Payload, rootfs: Payload, vm: bool },
}

/// An import of an image.
#[derive(Debug)]
pub struct ImageImport {
    pub data: ImageData,
    /// The aliases to make for the image.
    pub aliases: Vec<String>,
    /// The image is open to callers that the server does not trust.
    pub public: bool,
    /// The name of the file of the image (of the root file system of a split
    /// image); empty: none.
    pub filename: String,
    /// Properties over those of `metadata.yaml`.
    pub properties: BTreeMap<String, String>,
    /// The fingerprint (SHA-256 of the bytes) that incus must compute, when
    /// the caller knows it; incus refuses another.
    pub fingerprint: Option<String>,
}

impl ImageImport {
    /// An import of `data` with no alias, no property and no fingerprint.
    pub fn new(data: ImageData) -> ImageImport {
        ImageImport {
            data,
            aliases: Vec::new(),
            public: false,
            filename: String::new(),
            properties: BTreeMap::new(),
            fingerprint: None,
        }
    }
}

/// An image that incus took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedImage {
    pub fingerprint: String,
    /// The bytes that incus read.
    pub size: u64,
}

/// An alias of an image (`ImageAliasesEntry` of `shared/api/image.go`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ImageAlias {
    pub name: String,
    /// The fingerprint of the image.
    pub target: String,
    #[serde(default)]
    pub description: String,
    /// `container` or `virtual-machine`.
    #[serde(default, rename = "type")]
    pub kind: String,
}

#[derive(Serialize)]
struct AliasPost<'a> {
    name: &'a str,
    target: &'a str,
    description: &'a str,
}

impl Client {
    /// `POST /1.0/images` with the bytes of the image, the wait for its
    /// operation, then `POST /1.0/images/aliases` for each alias.
    pub async fn import_image(&self, import: ImageImport) -> Result<ImportedImage> {
        let call = "POST /1.0/images";
        let mut headers = Vec::new();
        if import.public {
            headers.push(("X-Incus-public".to_string(), "true".to_string()));
        }
        if !import.properties.is_empty() {
            let pairs: Vec<String> =
                import.properties.iter().map(|(k, v)| format!("{}={}", escape(k), escape(v))).collect();
            headers.push(("X-Incus-properties".to_string(), pairs.join("&")));
        }
        if let Some(fingerprint) = &import.fingerprint {
            headers.push(("X-Incus-fingerprint".to_string(), fingerprint.clone()));
        }
        let upload = match import.data {
            ImageData::Unified(payload) => {
                if !import.filename.is_empty() {
                    headers.push(("X-Incus-filename".to_string(), import.filename.clone()));
                }
                Upload::single("application/octet-stream", payload)
            }
            ImageData::Split { metadata, rootfs, vm } => {
                let boundary = system_boundary()
                    .ok_or_else(|| Error::usage(call, "no random bytes for the boundary of the multipart body"))?;
                let rootfs_name = if vm { "rootfs.img" } else { "rootfs" };
                let fields =
                    vec![("metadata", "metadata.tar", metadata), (rootfs_name, import.filename.as_str(), rootfs)];
                Upload::multipart(&boundary, fields)
            }
        };
        let operation = self.run("POST", "/1.0/images", headers, Body::Stream(upload)).await?;
        let fingerprint = operation.text("fingerprint").unwrap_or_default().to_string();
        if fingerprint.is_empty() {
            return Err(Error::protocol(call, format!("the operation {} names no fingerprint", operation.id)));
        }
        let size = match operation.metadata.get("size") {
            Some(Value::String(text)) => text.parse().ok(),
            Some(value) => value.as_u64(),
            None => None,
        };
        let size =
            size.ok_or_else(|| Error::protocol(call, format!("the operation {} gives no size", operation.id)))?;
        for alias in &import.aliases {
            self.create_alias(alias, &fingerprint, "").await?;
        }
        Ok(ImportedImage { fingerprint, size })
    }

    /// `GET /1.0/images/aliases/NAME`: the alias `name`, or `None` when
    /// incus has no such alias (404).
    pub async fn alias(&self, name: &str) -> Result<Option<ImageAlias>> {
        match self.get(&format!("/1.0/images/aliases/{}", escape(name)), &[]).await {
            Ok(alias) => Ok(Some(alias)),
            Err(e) if e.status() == Some(404) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// `POST /1.0/images/aliases`: the alias `name` of the image
    /// `fingerprint`.
    pub async fn create_alias(&self, name: &str, fingerprint: &str, description: &str) -> Result<()> {
        let body = AliasPost { name, target: fingerprint, description };
        self.send_json::<Value>("POST", "/1.0/images/aliases", &body).await.map(drop)
    }
}
