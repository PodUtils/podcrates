//! A target of a route on a Unix socket (T-836, PodUtils/podssh#295): the
//! host of a [`Route`] is `unix:PATH`, or on Linux
//! `unix:@NAME` for an abstract name, with port 0. On Windows the socket is
//! an AF_UNIX socket of WinSock (PodUtils/podssh#298). The node's owner names
//! the target; an operator names only a route, and an exit dials its own
//! way, so no operator reaches a socket of the node.
//!
//! The connection goes on as a `TcpStream` of the same descriptor: a read,
//! a write, a shutdown and the move of a session to a new relay
//! (`pod_e2e`'s `move_out`, which takes a `TcpStream`) work on a Unix
//! stream socket as on a TCP one. Its address and the options of TCP are
//! not there.

use std::time::Duration;

use tokio::net::TcpStream;

use super::routes::Route;

/// The prefix of the host of a Unix target.
pub const UNIX: &str = "unix:";

impl Route {
    /// A route to the Unix socket `path` (`@NAME`: an abstract name, on
    /// Linux).
    pub fn unix(name: Option<String>, path: &str) -> Route {
        Route { name, host: format!("{UNIX}{path}"), port: 0 }
    }

    /// The path of a Unix target, if this route has one.
    pub fn unix_path(&self) -> Option<&str> {
        unix_path(&self.host)
    }
}

/// The path of the host `unix:PATH`, if it is one.
pub fn unix_path(host: &str) -> Option<&str> {
    host.strip_prefix(UNIX).filter(|p| !p.is_empty())
}

/// Connect to the Unix socket `path` within `timeout`: on Windows, an
/// AF_UNIX socket of WinSock (Windows 10 1803 and later).
#[cfg(any(unix, windows))]
pub async fn connect(path: &str, timeout: Duration) -> Result<TcpStream, String> {
    let shown = format!("the Unix socket {path}");
    let path = path.to_string();
    let dial = tokio::task::spawn_blocking(move || open_std(&path));
    let tcp = match tokio::time::timeout(timeout, dial).await {
        Ok(Ok(Ok(tcp))) => tcp,
        Ok(Ok(Err(e))) => return Err(format!("{shown}: {}", pod_core::text::io_words(&e))),
        Ok(Err(e)) => return Err(format!("{shown}: {e}")),
        Err(_) => return Err(format!("{shown}: no answer within {} s", timeout.as_secs_f64())),
    };
    tcp.set_nonblocking(true).map_err(|e| format!("{shown}: {}", pod_core::text::io_words(&e)))?;
    TcpStream::from_std(tcp).map_err(|e| format!("{shown}: {}", pod_core::text::io_words(&e)))
}

/// The socket at `path`, connected, as a std `TcpStream` of the same
/// descriptor.
#[cfg(unix)]
fn open_std(path: &str) -> std::io::Result<std::net::TcpStream> {
    let fd: std::os::fd::OwnedFd = connect_std(path)?.into();
    Ok(std::net::TcpStream::from(fd))
}

#[cfg(unix)]
fn connect_std(path: &str) -> std::io::Result<std::os::unix::net::UnixStream> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    if let Some(name) = path.strip_prefix('@') {
        #[cfg(target_os = "android")]
        use std::os::android::net::SocketAddrExt;
        #[cfg(target_os = "linux")]
        use std::os::linux::net::SocketAddrExt;
        let addr = std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes())?;
        return std::os::unix::net::UnixStream::connect_addr(&addr);
    }
    std::os::unix::net::UnixStream::connect(path)
}

/// The socket at `path`, connected by WinSock: the standard library has no
/// AF_UNIX on Windows. A connected stream socket of any family carries bytes
/// as a TCP one does, so it is handed on as one.
#[cfg(windows)]
fn open_std(path: &str) -> std::io::Result<std::net::TcpStream> {
    use std::os::windows::io::FromRawSocket;
    use windows_sys::Win32::Networking::WinSock as ws;
    // SAFETY: an all-zero SOCKADDR_UN is valid.
    let mut addr: ws::SOCKADDR_UN = unsafe { std::mem::zeroed() };
    // The name ends with a NUL inside the address.
    if path.len() >= addr.sun_path.len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("the path is longer than {} bytes", addr.sun_path.len() - 1),
        ));
    }
    addr.sun_family = ws::AF_UNIX;
    for (slot, byte) in addr.sun_path.iter_mut().zip(path.as_bytes()) {
        *slot = *byte as _;
    }
    // WSAStartup counts its callers; the standard library calls it too.
    // SAFETY: an all-zero WSADATA is valid for WSAStartup to fill.
    let mut data: ws::WSADATA = unsafe { std::mem::zeroed() };
    // SAFETY: as above.
    let rc = unsafe { ws::WSAStartup(0x0202, &mut data) };
    if rc != 0 {
        return Err(std::io::Error::from_raw_os_error(rc));
    }
    // SAFETY: a plain socket call; the handle is owned below, or closed.
    let socket = unsafe { ws::socket(ws::AF_UNIX as i32, ws::SOCK_STREAM, 0) };
    if socket == ws::INVALID_SOCKET {
        // SAFETY: a call with no argument.
        return Err(std::io::Error::from_raw_os_error(unsafe { ws::WSAGetLastError() }));
    }
    let len = std::mem::size_of::<ws::SOCKADDR_UN>() as i32;
    // SAFETY: `addr` is a SOCKADDR_UN of `len` bytes.
    let rc = unsafe { ws::connect(socket, (&addr as *const ws::SOCKADDR_UN).cast(), len) };
    if rc != 0 {
        // SAFETY: a call with no argument.
        let e = std::io::Error::from_raw_os_error(unsafe { ws::WSAGetLastError() });
        // SAFETY: the socket was opened above and is closed once.
        unsafe { ws::closesocket(socket) };
        return Err(e);
    }
    // SAFETY: a connected stream socket, owned from here by the stream.
    Ok(unsafe { std::net::TcpStream::from_raw_socket(socket as u64) })
}

/// No Unix socket on this system.
#[cfg(not(any(unix, windows)))]
pub async fn connect(path: &str, _timeout: Duration) -> Result<TcpStream, String> {
    Err(format!("the Unix socket {path}: this system has no Unix socket for a route"))
}
