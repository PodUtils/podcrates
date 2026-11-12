//! pod-link: two ends of a link over the memory carrier carry one stream,
//! and the unix carrier checks the host, so that the driver of a link, the
//! frame codec and both carriers link in.

use pod_link::{Carrier, Link, Role, Settings};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub fn link() -> String {
    let unix = unix();
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(e) => return format!("pod-link: no runtime: {e}"),
    };
    let echoed = runtime.block_on(async {
        let (a, b) = pod_link::memory::pair(pod_link::memory::MIN_RING);
        let dialer = Link::new(a, Role::Dialer, Settings::default())?;
        let listener = Link::new(b, Role::Listener, Settings::default())?;
        let mut out = dialer.open()?;
        out.write_all(b"probe").await?;
        out.shutdown().await?;
        let mut back = listener.accept().await?;
        let mut got = Vec::new();
        back.read_to_end(&mut got).await?;
        dialer.close().await;
        Ok::<_, std::io::Error>(got.len())
    });
    format!(
        "pod-link: {} bytes over the memory carrier, the unix carrier {unix}",
        echoed.map_or_else(|e| e.to_string(), |n| n.to_string())
    )
}

#[cfg(unix)]
fn unix() -> String {
    pod_link::unix::Unix::default().available().map_or_else(|e| e.to_string(), |()| "available".into())
}

#[cfg(not(unix))]
fn unix() -> String {
    "not built".into()
}
