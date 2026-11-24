//! The room of incusd: a storage pool (`shared/api/storage_pool.go`), the
//! space and the inodes of a pool (`GET /1.0/storage-pools/NAME/resources`,
//! `ResourcesStoragePool` of `shared/api/resource.go`), and an image that
//! incus has (`shared/api/image.go`) with its size. incusd measures the pool
//! in its own mount namespace: a `source` that the caller cannot see is
//! measured too.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::client::Client;
use crate::envelope::nullable;
use crate::error::Result;
use crate::wire::escape;

/// A storage pool (`GET /1.0/storage-pools/NAME`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct StoragePool {
    #[serde(default)]
    pub name: String,
    /// `dir`, `btrfs`, `zfs`, `lvm`, `ceph`, ...
    #[serde(default)]
    pub driver: String,
    #[serde(default)]
    pub description: String,
    /// `Created`, `Pending`, `Errored`, `Unknown`, `Unavailable`.
    #[serde(default)]
    pub status: String,
    /// The keys of the driver: `source` (the path or the device), `size`.
    #[serde(default, deserialize_with = "nullable")]
    pub config: BTreeMap<String, String>,
    #[serde(default, deserialize_with = "nullable")]
    pub used_by: Vec<String>,
}

impl StoragePool {
    /// `config.source`: the path of a `dir` pool, the device or the
    /// dataset of another driver.
    pub fn source(&self) -> Option<&str> {
        self.config.get("source").map(String::as_str)
    }
}

/// The space and the inodes of a storage pool, as incusd measures them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub struct PoolResources {
    #[serde(default)]
    pub space: PoolSpace,
    #[serde(default)]
    pub inodes: PoolInodes,
}

impl PoolResources {
    /// The bytes that are not used: `space.total - space.used`.
    pub fn free_bytes(&self) -> u64 {
        self.space.total.saturating_sub(self.space.used)
    }

    /// The inodes that are not used; `None` where the pool has no count of
    /// inodes (a total of 0).
    pub fn free_inodes(&self) -> Option<u64> {
        (self.inodes.total > 0).then(|| self.inodes.total.saturating_sub(self.inodes.used))
    }
}

/// The space of a storage pool, in bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub struct PoolSpace {
    #[serde(default)]
    pub used: u64,
    #[serde(default)]
    pub total: u64,
}

/// The inodes of a storage pool.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub struct PoolInodes {
    #[serde(default)]
    pub used: u64,
    #[serde(default)]
    pub total: u64,
}

/// An image that incus has (`GET /1.0/images/FINGERPRINT`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Image {
    #[serde(default)]
    pub fingerprint: String,
    /// The bytes of the image files (compressed).
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub architecture: String,
    /// `container` or `virtual-machine`.
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub public: bool,
    #[serde(default, deserialize_with = "nullable")]
    pub properties: BTreeMap<String, String>,
    #[serde(default, deserialize_with = "nullable")]
    pub aliases: Vec<ImageAliasName>,
    #[serde(default)]
    pub uploaded_at: String,
}

/// An alias in the list of an image.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct ImageAliasName {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
}

impl Client {
    /// `GET /1.0/storage-pools/NAME`.
    pub async fn storage_pool(&self, name: &str) -> Result<StoragePool> {
        self.get(&format!("/1.0/storage-pools/{}", escape(name)), &[]).await
    }

    /// `GET /1.0/storage-pools/NAME/resources`: the space and the inodes
    /// of the pool, measured by incusd.
    pub async fn storage_pool_resources(&self, name: &str) -> Result<PoolResources> {
        self.get(&format!("/1.0/storage-pools/{}/resources", escape(name)), &[]).await
    }

    /// `GET /1.0/images/FINGERPRINT`: the image, or `None` when incus has
    /// no such image (404). A unique start of a fingerprint names it too.
    pub async fn image(&self, fingerprint: &str) -> Result<Option<Image>> {
        match self.get(&format!("/1.0/images/{}", escape(fingerprint)), &[]).await {
            Ok(image) => Ok(Some(image)),
            Err(e) if e.status() == Some(404) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_resources_of_a_pool_read_resource_go() {
        let json = serde_json::json!({"space": {"used": 300, "total": 1000}, "inodes": {"used": 40, "total": 100}});
        let room: PoolResources = serde_json::from_value(json).unwrap();
        assert_eq!((room.free_bytes(), room.free_inodes()), (700, Some(60)));
        // incusd leaves out a used of 0, and a driver with no count of inodes.
        let room: PoolResources = serde_json::from_value(serde_json::json!({"space": {"total": 5}})).unwrap();
        assert_eq!((room.free_bytes(), room.free_inodes()), (5, None));
    }
}
