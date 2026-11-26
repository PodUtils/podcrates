//! The support of the tests of Trojan: the stand-ins are the crate's own
//! (`pod_egress::standin::trojan`, feature `standin`), here with a
//! certificate of the test CA for `vless_support::NAME`.

#![allow(dead_code)]

use std::sync::Arc;

use tokio::net::TcpStream;

pub use pod_egress::standin::trojan::{PASSWORD, Seen, serve_tls, serve_ws};

/// A stand-in on TLS for `vless_support::NAME`, each connection to `serve`.
pub async fn listen<F, Fut>(serve: F) -> (u16, Arc<Seen>)
where
    F: Fn(tokio_rustls::server::TlsStream<TcpStream>, Arc<Seen>) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let config = crate::vless_support::config();
    pod_egress::standin::trojan::listen(config, serve).await.unwrap().detach()
}
