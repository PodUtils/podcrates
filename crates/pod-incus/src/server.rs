//! `GET /1.0`: the facts of the server (`shared/api/server.go`): the API
//! version, the API extensions, the version of incus, the driver and the
//! kernel. [`Server::check`] compares the extensions with those that the
//! calls of the crate need ([`NEEDED`]).

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use crate::client::Client;
use crate::envelope::nullable;
use crate::error::{Error, ErrorKind, Result};

/// The API extensions that the calls of the crate need. incus 6.0 has each.
pub const NEEDED: [&str; 7] = [
    // The paths `/1.0/instances`.
    "instances",
    // `record-output` of exec, and `logs/exec-output`.
    "container_exec_recording",
    // `user`, `group` and `cwd` of exec.
    "container_exec_user_group_cwd",
    // `GET /1.0/instances/NAME/console`.
    "console",
    // `X-Incus-type: directory` of a file post.
    "directory_manipulation",
    // `X-Incus-type: symlink` of a file post.
    "file_symlinks",
    // `X-Incus-write: append` of a file post.
    "file_append",
];

/// The facts of the server.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Server {
    #[serde(default, deserialize_with = "nullable")]
    pub api_extensions: Vec<String>,
    /// `stable`, or `development`.
    #[serde(default)]
    pub api_status: String,
    /// `1.0`.
    #[serde(default)]
    pub api_version: String,
    /// `trusted` for a caller that the socket trusts.
    #[serde(default)]
    pub auth: String,
    #[serde(default)]
    pub public: bool,
    #[serde(default, deserialize_with = "nullable")]
    pub auth_methods: Vec<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub config: BTreeMap<String, Value>,
    #[serde(default)]
    pub environment: Environment,
}

/// The environment of the server. The certificate is not kept.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Environment {
    #[serde(default, deserialize_with = "nullable")]
    pub addresses: Vec<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub architectures: Vec<String>,
    /// The drivers of the instances: `lxc | qemu`.
    #[serde(default)]
    pub driver: String,
    #[serde(default)]
    pub driver_version: String,
    #[serde(default)]
    pub firewall: String,
    #[serde(default)]
    pub kernel: String,
    #[serde(default)]
    pub kernel_architecture: String,
    #[serde(default)]
    pub kernel_version: String,
    #[serde(default)]
    pub os_name: String,
    #[serde(default)]
    pub os_version: String,
    #[serde(default)]
    pub project: String,
    /// `incus`.
    #[serde(default)]
    pub server: String,
    #[serde(default)]
    pub server_clustered: bool,
    #[serde(default)]
    pub server_name: String,
    #[serde(default)]
    pub server_pid: i64,
    /// The version of incus: `6.0.0`.
    #[serde(default)]
    pub server_version: String,
    #[serde(default)]
    pub storage: String,
    #[serde(default)]
    pub storage_version: String,
}

impl Server {
    /// The server has the API extension `name`.
    pub fn has(&self, name: &str) -> bool {
        self.api_extensions.iter().any(|e| e == name)
    }

    /// The API version is 1.0 and the server has each extension of
    /// [`NEEDED`]; else the error names the first one that is not there.
    pub fn check(&self) -> Result<()> {
        let call = "GET /1.0";
        if self.api_version != "1.0" {
            let what = format!("the API version 1.0 (it has {:?})", self.api_version);
            return Err(Error::new(call, ErrorKind::Unsupported { what }));
        }
        match NEEDED.iter().find(|name| !self.has(name)) {
            Some(name) => Err(Error::new(call, ErrorKind::Unsupported { what: format!("the API extension {name}") })),
            None => Ok(()),
        }
    }
}

impl Client {
    /// `GET /1.0`: the facts of the server.
    pub async fn server(&self) -> Result<Server> {
        self.get("/1.0", &[]).await
    }
}
