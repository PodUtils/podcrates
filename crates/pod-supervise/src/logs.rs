//! The log of a container: what its payload wrote to stdout and stderr.

use std::io::{Read, Write};
use std::path::Path;

use serde::{Serialize, de::DeserializeOwned};

use crate::table::{self, Container, State};
use crate::{Result, Settings, get, io_at, stream};

/// The captured output of a container. A container that wrote nothing has
/// no log file, and that is an empty log, not an error. A link at the log
/// is refused, as in each read of a log.
///
/// The whole log is in the `Vec`: a caller that writes the log somewhere
/// copies it with [`crate::stream::open_log`] and
/// [`crate::stream::copy_tail`], one piece at a time.
pub fn logs<X>(s: &Settings, want: &str) -> Result<Vec<u8>>
where
    X: Clone + Serialize + DeserializeOwned,
{
    let c = get::<X>(s, want)?;
    let path = table::log_path(s, &c.id);
    let Some(mut f) = stream::open(&path).map_err(|e| io_at(&path, e))? else {
        return Ok(Vec::new());
    };
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).map_err(|e| io_at(&path, e))?;
    Ok(bytes)
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
        let (next, ended) = follow_once::<X>(s, want, &path, offset, out)?;
        offset = next;
        if let Some(ended) = ended {
            std::thread::sleep(FINAL_DRAIN);
            offset = append_from(&path, offset, out)?;
            // A restart passes through `Exited` between its stop and its
            // start: the follow goes on where the run started again
            // (PodUtils/podvm#569).
            if ended_for_good(&ended, &get::<X>(s, want)?) {
                return Ok(());
            }
        }
    }
}

/// Whether a run that read as ended at `seen` is still the same ended run in
/// `now`: an end state and the same start. A new start, or a state that is
/// not an end, is a restart.
pub(crate) fn ended_for_good<X>(seen: &Container<X>, now: &Container<X>) -> bool {
    terminal(now.state) && now.started_at == seen.started_at && now.finished_at == seen.finished_at
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
/// then follow what comes. The tail is found from the end of the length at
/// the open ([`crate::stream::tail_start`]), and the poll continues past
/// that length, so that a line that comes between the two is followed,
/// never lost and never written two times. No read holds more than one
/// piece of [`crate::stream::CHUNK`] bytes.
pub fn follow_tail<X>(s: &Settings, want: &str, out: &mut dyn Write, n: u64) -> Result<()>
where
    X: Clone + Serialize + DeserializeOwned,
{
    let c = get::<X>(s, want)?;
    let path = table::log_path(s, &c.id);
    let end = match stream::open_sized(&path)? {
        Some((mut f, len)) => {
            stream::copy_tail(&mut f, len, Some(n), None, out).map_err(|e| fault(&path, e))?;
            len
        }
        None => 0,
    };
    follow_from::<X>(s, want, out, end)
}

/// [`follow_from`] with at most `limit` bytes written: the follow ends at
/// the bound with `Ok`, also while the container runs and writes nothing
/// more. Gives the count of bytes written.
pub fn follow_from_limit<X>(s: &Settings, want: &str, out: &mut dyn Write, start: u64, limit: u64) -> Result<u64>
where
    X: Clone + Serialize + DeserializeOwned,
{
    let mut cap = stream::Capped::new(out, limit);
    if cap.full() {
        return Ok(0);
    }
    match follow_from::<X>(s, want, &mut cap, start) {
        Err(_) if cap.full() => Ok(limit),
        Err(e) => Err(e),
        Ok(()) => Ok(limit - cap.left()),
    }
}

/// The failure of a copy of the log at `path`.
fn fault(path: &Path, e: stream::Fault) -> pod_core::Failure {
    match e {
        stream::Fault::Read(e) => io_at(path, e),
        stream::Fault::Write(e) => writing(e),
    }
}

fn writing(e: std::io::Error) -> pod_core::Failure {
    pod_core::Failure::caused(
        pod_core::Stage::Host,
        pod_core::Class::of_io(e.kind()),
        format!("writing the log: {e}"),
        e,
    )
}

/// One step of the poll: append what came, and the record where the
/// container ended. The last read stays in [`follow_from`]: a write in the same
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
) -> Result<(u64, Option<Container<X>>)>
where
    X: Clone + Serialize + DeserializeOwned,
{
    let offset = append_from(path, offset, out)?;
    let c = get::<X>(s, want)?;
    Ok((offset, terminal(c.state).then_some(c)))
}

/// A state at which `follow` stops: the run ended, watched or not.
fn terminal(state: State) -> bool {
    matches!(state, State::Exited | State::Dead)
}

/// Write the bytes past `offset` and return the new offset. A missing file
/// is an empty log (see [`logs`]); a shorter file sets the offset back. The
/// copy goes to the length at the open, one piece at a time: a byte that
/// comes during the copy is for the next poll.
fn append_from(path: &Path, offset: u64, out: &mut dyn Write) -> Result<u64> {
    // A link at the log is not read: it could name any file of the user.
    let Some((mut f, len)) = stream::open_sized(path)? else {
        return Ok(offset);
    };
    let offset = offset.min(len);
    let n = stream::copy_range(&mut f, offset, len, None, out).map_err(|e| fault(path, e))?;
    Ok(offset + n)
}

#[cfg(test)]
mod tests;
