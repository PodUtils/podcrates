//! The Unix socket: the bind (a live socket is kept, a stale one is
//! replaced), the accept thread with the check of the peer user, and the
//! connect of a client.

use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;

use crate::catalogue::Catalogue;
use crate::host::Peer;
use crate::server::{self, Incoming, Settings, Shared, Snapshot};
use crate::wire::{Code, Fault};

/// Bind the socket at `path` with `mode`, from the first instant
/// (`pod_core::files::bind_staged`). A stale socket (no one answers,
/// `pod_core::files::socket_answers`) goes first; a live socket is `AddrInUse`, and a path that is not a socket is
/// `AlreadyExists`: neither is removed.
pub fn bind(path: &Path, mode: u32) -> io::Result<UnixListener> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
        Ok(m) if m.file_type().is_socket() => {
            if pod_core::files::socket_answers(path) {
                return Err(io::Error::new(io::ErrorKind::AddrInUse, "a live socket"));
            }
            std::fs::remove_file(path)?;
        }
        Ok(_) => return Err(io::Error::new(io::ErrorKind::AlreadyExists, "not a socket")),
    }
    pod_core::files::bind_staged(path, mode)
}

/// A running server. Its drop stops the accept of new connections and
/// removes its socket, when the path still holds it.
pub struct Server {
    shared: Arc<Shared>,
    path: PathBuf,
    ino: Option<(u64, u64)>,
}

impl Server {
    /// The snapshot of `status` that the host keeps.
    pub fn snapshot(&self) -> Snapshot {
        self.shared.snapshot.clone()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The connections that hold a place now. A connection gives its place
    /// back when the server ends it, after the client reads its last
    /// answer: a caller that counts the places waits for this count.
    pub fn active(&self) -> usize {
        self.shared.active.load(Ordering::SeqCst)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::SeqCst);
        // Wake the accept thread, so that it sees the flag and ends.
        let _ = UnixStream::connect(&self.path);
        let same = std::fs::symlink_metadata(&self.path).ok().map(|m| (m.dev(), m.ino()));
        if same.is_some() && same == self.ino {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Bind `path` with [`Settings::mode`] and serve the control there. The
/// host answers the delegated requests from [`Incoming`].
pub fn serve(path: &Path, settings: Settings, catalogue: Catalogue) -> io::Result<(Server, Incoming)> {
    let listener = bind(path, settings.mode)?;
    Ok(serve_listener(listener, path, settings, catalogue))
}

/// Serve the control on a listener that the caller bound at `path`.
pub fn serve_listener(
    listener: UnixListener,
    path: &Path,
    settings: Settings,
    catalogue: Catalogue,
) -> (Server, Incoming) {
    let ino = std::fs::symlink_metadata(path).ok().map(|m| (m.dev(), m.ino()));
    let shared = Arc::new(Shared {
        settings,
        catalogue,
        snapshot: Snapshot::default(),
        active: AtomicUsize::new(0),
        closed: AtomicBool::new(false),
    });
    let (tx, rx) = mpsc::channel();
    let accept = shared.clone();
    std::thread::spawn(move || {
        let own = pod_core::files::euid();
        for stream in listener.incoming() {
            if accept.closed.load(Ordering::SeqCst) {
                return;
            }
            let Ok(stream) = stream else { continue };
            let ids = pod_core::files::peer_ids(&stream).ok();
            let peer = Peer { uid: ids.map(|i| i.uid), pid: ids.and_then(|i| i.pid), sid: None };
            let Some(tier) = server::admit(&accept.settings, &peer, Some(own), None) else {
                server::refuse(stream, Fault::new(Code::Refused, "the peer is another user"), &accept.settings);
                continue;
            };
            let Some(slot) = server::take_slot(&accept) else {
                server::refuse(stream, Fault::new(Code::Busy, "too many clients"), &accept.settings);
                continue;
            };
            let (shared, calls) = (accept.clone(), tx.clone());
            std::thread::spawn(move || {
                let _slot = slot;
                // An error of I/O ends the connection; the peer sees the close.
                let _ = server::serve_connection(stream, peer, tier, &shared, &calls);
            });
        }
    });
    (Server { shared, path: path.to_path_buf(), ino }, Incoming(rx))
}

/// Connect to the server at `path`.
pub fn connect(path: &Path) -> io::Result<UnixStream> {
    UnixStream::connect(path)
}
