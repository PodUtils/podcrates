//! The rename of a replace on Windows. `MoveFileExW` gives
//! `ERROR_ACCESS_DENIED` when another rename or an open handle holds the
//! target, so two writers of one name at once fail where Unix replaces. A
//! rename with POSIX semantics replaces the name as on Unix; a file system
//! that does not take it gets `MoveFileExW`. A target that stays held gets
//! a retry within a bound.

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED, ERROR_SHARING_VIOLATION,
    GENERIC_WRITE,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_FLAG_OPEN_REPARSE_POINT, FILE_RENAME_INFO, FileRenameInfoEx, MOVEFILE_REPLACE_EXISTING,
    MOVEFILE_WRITE_THROUGH, MoveFileExW, SetFileInformationByHandle,
};

// The values of winbase.h: windows-sys has them only in a feature that this
// crate does not use.
const FILE_RENAME_FLAG_REPLACE_IF_EXISTS: u32 = 1;
const FILE_RENAME_FLAG_POSIX_SEMANTICS: u32 = 2;

/// The time that a rename tries again while another holds its target.
const BOUND: Duration = Duration::from_secs(2);
/// The longest pause between two tries.
const LONGEST_PAUSE: Duration = Duration::from_millis(50);

/// Rename `from` over `to`, and return when the rename is on disk.
pub(super) fn rename(from: &Path, to: &Path) -> io::Result<()> {
    let start = Instant::now();
    let mut pause = Duration::from_millis(1);
    let mut tries = 0u32;
    loop {
        tries += 1;
        let e = match rename_once(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => e,
        };
        // A directory at `to` also gives ERROR_ACCESS_DENIED, and stays.
        if !held(&e) || to.is_dir() {
            return Err(e);
        }
        if start.elapsed() >= BOUND {
            let text = format!(
                "the rename onto {} failed {tries} times in {:?}: {}",
                to.display(),
                start.elapsed(),
                crate::text::io_words(&e)
            );
            return Err(io::Error::new(e.kind(), text));
        }
        std::thread::sleep(pause);
        pause = (pause * 2).min(LONGEST_PAUSE);
    }
}

/// Another rename or an open handle holds the target.
fn held(e: &io::Error) -> bool {
    e.raw_os_error().is_some_and(|code| [ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION].contains(&(code as u32)))
}

/// The file system does not take a rename with POSIX semantics (FAT, or a
/// Windows older than 10 1709).
fn unsupported(e: &io::Error) -> bool {
    e.raw_os_error().is_some_and(|code| {
        [ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED, ERROR_INVALID_FUNCTION].contains(&(code as u32))
    })
}

fn rename_once(from: &Path, to: &Path) -> io::Result<()> {
    let file = std::fs::OpenOptions::new()
        .access_mode(DELETE | GENERIC_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(from)?;
    match posix_rename(&file, to) {
        // The flush of the handle after the rename, as MOVEFILE_WRITE_THROUGH
        // does: it returns when the rename is on disk.
        Ok(()) => file.sync_all(),
        Err(e) if unsupported(&e) => {
            drop(file);
            move_file(from, to)
        }
        Err(e) => Err(e),
    }
}

/// The rename of the open `file` to `to`, with the replace of an existing
/// name and POSIX semantics (`FileRenameInfoEx`).
fn posix_rename(file: &std::fs::File, to: &Path) -> io::Result<()> {
    let name: Vec<u16> = to.as_os_str().encode_wide().collect();
    let bytes = name.len() * 2;
    let length =
        u32::try_from(bytes).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "the path is too long"))?;
    // The struct ends with the name and its NUL.
    let size = std::mem::offset_of!(FILE_RENAME_INFO, FileName) + bytes + 2;
    // Words of 8 bytes: the alignment of the handle in the struct. The zeros
    // give the NUL.
    let mut buf = vec![0u64; size.div_ceil(8)];
    let info = buf.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: `buf` is zeroed, aligned for the struct, and holds the struct
    // and the name with its NUL.
    unsafe {
        (*info).Anonymous.Flags = FILE_RENAME_FLAG_REPLACE_IF_EXISTS | FILE_RENAME_FLAG_POSIX_SEMANTICS;
        (*info).FileNameLength = length;
        name.as_ptr().copy_to_nonoverlapping((&raw mut (*info).FileName).cast::<u16>(), name.len());
    }
    let size = u32::try_from(size).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "the path is too long"))?;
    // SAFETY: a handle of `file`, open for DELETE; `info` and `size` describe
    // `buf`, which lives through the call.
    let done = unsafe { SetFileInformationByHandle(file.as_raw_handle(), FileRenameInfoEx, info.cast(), size) };
    if done == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

/// One move that replaces and writes through (`MOVEFILE_WRITE_THROUGH`):
/// it returns when the move is on disk.
fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    let wide = |p: &Path| p.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<u16>>();
    let (from, to) = (wide(from), wide(to));
    // SAFETY: both are NUL-terminated wide strings that live for the call.
    let done = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) };
    if done == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}
