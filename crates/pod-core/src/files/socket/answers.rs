//! The probe of a Unix socket: does a process serve it?

use std::path::Path;
use std::time::Duration;

/// The connects of [`socket_answers`].
const PROBES: u32 = 3;
/// The time between two connects of [`socket_answers`].
const PROBE_GAP: Duration = Duration::from_millis(50);

/// Whether a process serves the Unix socket at `path`. A child that another
/// thread forks keeps a copy of each listener of its parent until its exec.
/// Thus a socket that its server closed can take a connect for that short
/// time. The socket answers only when each of 3 connects, 50 ms apart,
/// works. A socket that does not answer costs one connect; a socket that
/// answers costs 100 ms, on the thread of the caller.
pub fn socket_answers(path: &Path) -> bool {
    for probe in 0..PROBES {
        if probe > 0 {
            std::thread::sleep(PROBE_GAP);
        }
        if std::os::unix::net::UnixStream::connect(path).is_err() {
            return false;
        }
    }
    true
}
