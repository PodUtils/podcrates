//! The support of the tests of Wisp: the stand-in Wisp server is the
//! crate's own (`pod_egress::standin::wisp`, feature `standin`), written from
//! the protocol's document with its own reading of each byte; `reference`
//! runs wisp-server-python, Mercury Workshop's own server; the targets.

#![allow(dead_code)]

pub mod reference;

use std::net::Ipv4Addr;

use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

pub use pod_egress::standin::wisp::{Speaks, Stand as StandIn};

/// An echo server on the loopback: its port.
pub async fn echo() -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut conn, _)) = listener.accept().await {
            tokio::spawn(async move {
                let (mut r, mut w) = conn.split();
                let _ = tokio::io::copy(&mut r, &mut w).await;
            });
        }
    });
    port
}

/// A server on the loopback that writes `said` to each client and closes.
pub async fn greeter(said: &'static [u8]) -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut conn, _)) = listener.accept().await {
            let _ = conn.write_all(said).await;
            let _ = conn.shutdown().await;
        }
    });
    port
}

/// A port of the loopback where nothing listens.
pub fn closed_port() -> u16 {
    let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.local_addr().unwrap().port()
}
