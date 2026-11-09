//! The log of a container: what its payload wrote to stdout and stderr.

use std::io::Write;
use std::path::Path;

use serde::{Serialize, de::DeserializeOwned};

use crate::table::{self, State};
use crate::{Result, Settings, get, io_at};

/// The captured output of a container. A container that wrote nothing has
/// no log file, and that is an empty log, not an error.
pub fn logs<X>(s: &Settings, want: &str) -> Result<Vec<u8>>
where
    X: Clone + Serialize + DeserializeOwned,
{
    let c = get::<X>(s, want)?;
    let path = table::log_path(s, &c.id);
    match std::fs::read(&path) {
        Ok(v) => Ok(v),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(io_at(&path, e)),
    }
}

/// Follow the log of a container: write what is there, then poll and
/// append until the container is in a terminal state, with one bounded last
/// read, so that a write in the same moment as the change of state still
/// shows.
///
/// A poll with a bound (100 ms), never inotify: the same behaviour on each
/// host. A file that becomes shorter sets the offset back, not an error. A
/// container that never ends is followed with no end, as docker does.
pub fn follow<X>(s: &Settings, want: &str, out: &mut dyn Write) -> Result<()>
where
    X: Clone + Serialize + DeserializeOwned,
{
    follow_from::<X>(s, want, out, 0)
}

/// Follow the log of a container from a byte offset: write what is past
/// it, then poll and append as [`follow`] does. The bytes past the offset
/// are written one time, and the poll continues past them, so that no line
/// is lost or written two times.
pub fn follow_from<X>(s: &Settings, want: &str, out: &mut dyn Write, start: u64) -> Result<()>
where
    X: Clone + Serialize + DeserializeOwned,
{
    const POLL: std::time::Duration = std::time::Duration::from_millis(100);
    const FINAL_DRAIN: std::time::Duration = std::time::Duration::from_millis(200);
    let c = get::<X>(s, want)?;
    let path = table::log_path(s, &c.id);
    let mut offset = append_from(&path, start, out)?;
    loop {
        std::thread::sleep(POLL);
        let (next, done) = follow_once::<X>(s, want, &path, offset, out)?;
        offset = next;
        if done {
            std::thread::sleep(FINAL_DRAIN);
            append_from(&path, offset, out)?;
            return Ok(());
        }
    }
}

/// The byte offset where the last `n` lines of `bytes` start.
///
/// A line ends at `\n`. One `\n` at the end ends the last line and does not
/// start an empty one, so `a\n` and `a` are each one line. Zero lines is
/// the end of the buffer, and more lines than the buffer holds is its
/// start: a count alone never fails.
pub fn tail_offset(bytes: &[u8], n: u64) -> u64 {
    if n == 0 {
        return bytes.len() as u64;
    }
    let body = if bytes.last() == Some(&b'\n') { &bytes[..bytes.len() - 1] } else { bytes };
    let mut seen = 0u64;
    let mut i = body.len();
    while i > 0 {
        i -= 1;
        if body[i] == b'\n' {
            seen += 1;
            if seen == n {
                return (i + 1) as u64;
            }
        }
    }
    0
}

/// Follow the log of a container from its last `n` lines: write the tail,
/// then follow what comes. The tail comes from one read, and the poll
/// continues past the length of that read, so that a line that comes
/// between the two is followed, never lost and never written two times.
pub fn follow_tail<X>(s: &Settings, want: &str, out: &mut dyn Write, n: u64) -> Result<()>
where
    X: Clone + Serialize + DeserializeOwned,
{
    let bytes = logs::<X>(s, want)?;
    let start = tail_offset(&bytes, n);
    let end = bytes.len() as u64;
    out.write_all(&bytes[start as usize..]).map_err(writing)?;
    let _ = out.flush();
    follow_from::<X>(s, want, out, end)
}

fn writing(e: std::io::Error) -> pod_core::Failure {
    pod_core::Failure::caused(
        pod_core::Stage::Host,
        pod_core::Class::of_io(e.kind()),
        format!("writing the log: {e}"),
        e,
    )
}

/// One step of the poll: append what came, and whether the container
/// ended. The last read stays in [`follow_from`]: a write in the same
/// moment as the change of state comes between this read and that one.
///
/// A step, not a loop, so that a test drives the arrivals one by one: the
/// crate starts no thread, also not in a test (`launcher`, the test
/// `nothing_on_the_spawn_path_can_spawn_a_thread`).
pub(crate) fn follow_once<X>(
    s: &Settings,
    want: &str,
    path: &Path,
    offset: u64,
    out: &mut dyn Write,
) -> Result<(u64, bool)>
where
    X: Clone + Serialize + DeserializeOwned,
{
    let offset = append_from(path, offset, out)?;
    Ok((offset, terminal(get::<X>(s, want)?.state)))
}

/// A state at which `follow` stops: the run ended, watched or not.
fn terminal(state: State) -> bool {
    matches!(state, State::Exited | State::Dead)
}

/// Write the bytes past `offset` and return the new offset. A missing file
/// is an empty log (see [`logs`]); a shorter file sets the offset back.
fn append_from(path: &Path, offset: u64, out: &mut dyn Write) -> Result<u64> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(offset),
        Err(e) => return Err(io_at(path, e)),
    };
    let len = f.metadata().map(|m| m.len()).map_err(|e| io_at(path, e))?;
    let offset = offset.min(len);
    f.seek(SeekFrom::Start(offset)).map_err(|e| io_at(path, e))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).map_err(|e| io_at(path, e))?;
    out.write_all(&buf).map_err(writing)?;
    let _ = out.flush();
    Ok(offset + buf.len() as u64)
}
