//! The support of the tests of VLESS, VMess and Trojan: the stand-ins of a
//! VLESS server are the crate's own (`pod_egress::standin::vless`, feature
//! `standin`), here with a certificate of the test CA for [`NAME`]; the way
//! out of the tests, the root's file and the target.

#![allow(dead_code)]

use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;

use pod_egress::standin::vless as stand;
use pod_proxy::ProxyChoice;
use pod_ws::Way;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

pub mod cert {
    include!("../../../pod-tls/tests/common/cert_for_test.rs");
}

// The target of the tests of a pipe's end of input.
#[path = "../common/talker.rs"]
pub mod talker;
// The way out of the tests.
#[path = "../common/way.rs"]
mod way_of;

// Each test binary that takes this module uses a part of it.
#[allow(unused_imports)]
pub use stand::{CLOSE_AFTER, ID, Seen};

pub const NAME: &str = "vless.test";
pub const BANNER: &[u8] = b"SSH-2.0-stand-in\r\n";

/// The way out of the tests through `proxy`, with [`NAME`] at the
/// loopback.
pub fn way(proxy: ProxyChoice) -> Way {
    let lo: IpAddr = "127.0.0.1".parse().unwrap();
    way_of::through(proxy, &[(NAME, lo)])
}

pub fn root_file(tag: &str) -> Root {
    let dir = std::env::temp_dir().join(format!("pod-egress-vless-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("root.pem");
    let der = cert::encoding::self_signed_inner(&cert::encoding::anchor_name(NAME), true).der;
    std::fs::write(&path, cert::encoding::pem(&der)).unwrap();
    Root(path)
}

/// The root's file, its directory gone when the test ends, failed or not:
/// the gate refuses a test that leaves one in the temporary directory.
pub struct Root(PathBuf);

impl std::ops::Deref for Root {
    type Target = std::path::Path;
    fn deref(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        if let Some(dir) = self.0.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// The target: its version line first when `speaks_first`, then an echo.
pub async fn target(speaks_first: bool) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut tcp, _)) = listener.accept().await {
            tokio::spawn(async move {
                if speaks_first && tcp.write_all(BANNER).await.is_err() {
                    return;
                }
                let (mut r, mut w) = tcp.split();
                let _ = tokio::io::copy(&mut r, &mut w).await;
            });
        }
    });
    port
}

/// The server configuration of a stand-in: a certificate of the test CA
/// for [`NAME`].
pub fn config() -> Arc<rustls::ServerConfig> {
    cert::server_for(NAME).unwrap().0.config().clone()
}

/// A VLESS server over a WebSocket at `/ws`, over TLS for [`NAME`].
pub async fn ws_server() -> (u16, Arc<Seen>) {
    stand::ws_server(config()).await.unwrap().detach()
}

/// A VLESS server over XHTTP at `/xh/`, over TLS for [`NAME`], offering
/// `alpn`.
pub async fn xhttp_server(alpn: &[&[u8]]) -> (u16, Arc<Seen>) {
    stand::xhttp_server(config(), alpn).await.unwrap().detach()
}

/// A VLESS server over XHTTP at `/xh/` that speaks HTTP/1.1 alone: over TLS
/// for [`NAME`], or with no TLS when `tls` is false.
pub async fn xhttp1_server(tls: bool) -> (u16, Arc<Seen>) {
    stand::xhttp1_server(tls.then(config)).await.unwrap().detach()
}
