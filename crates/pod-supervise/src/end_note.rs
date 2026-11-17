//! The end of a run that the launcher could not write into the table
//! (PodUtils/podvm#568): a file `end.json` in the directory of the
//! container, which [`crate::table::reconcile`] reads in place of `Dead`.

use serde::{Deserialize, Serialize};

use crate::Settings;
use crate::end::End;

/// The name of the file in the directory of a container.
pub const END_FILE: &str = "end.json";

/// What the file holds: the exit code and the end of the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub code: i32,
    pub end: Option<End>,
}

/// Write the end of the run of `id` beside its record. Mode 0600, a new
/// file and a rename, as the table.
pub fn write(s: &Settings, id: &str, note: &Note) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let dir = crate::table::dir(s, id);
    let tmp = dir.join(format!("{END_FILE}.{}.partial", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let bytes = serde_json::to_vec(note).map_err(std::io::Error::other)?;
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
    std::io::Write::write_all(&mut file, &bytes).and_then(|()| file.sync_all())?;
    std::fs::rename(&tmp, dir.join(END_FILE))
}

/// The end of the run of `id`, where its launcher left one; the file goes.
pub fn take(s: &Settings, id: &str) -> Option<Note> {
    let path = crate::table::dir(s, id).join(END_FILE);
    let note = serde_json::from_slice(&std::fs::read(&path).ok()?).ok();
    let _ = std::fs::remove_file(&path);
    note
}
