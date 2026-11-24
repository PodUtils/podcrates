//! The sidecar rows of the directories that the walk makes for an entry
//! below them (a file `usr/bin/x` with no entry for `usr/bin`).
//!
//! A whiteout or a replacement removes a path and leaves its rows: the last
//! row of a path holds. A directory that the walk makes again at that path
//! needs a row of its own, or a reader of the sidecar gives it the ids and
//! the mode of the removed entry (T-1253).

use super::{Ids, resolve_ids};
use crate::error::{self, Result};
use crate::safety::MADE_MODE;
use crate::sidecar::{Meta, Sidecar};

/// One row for each directory of `parts` from the index `first` to the
/// parent of the entry. The image means owner 0 and mode 0755 for each, as
/// the stream of `crate::flatten` writes it.
pub(super) fn rows(sidecar: &mut Sidecar, parts: &[&str], first: Option<usize>, ids: Ids) -> Result<()> {
    let Some(first) = first else {
        return Ok(());
    };
    let (applied_uid, applied_gid, reason) = resolve_ids(0, 0, ids);
    for end in first + 1..parts.len() {
        let row = Meta {
            path: parts[..end].join("/"),
            uid: 0,
            gid: 0,
            mode: MADE_MODE,
            applied_uid,
            applied_gid,
            reason: reason.clone(),
            applied_mode: None,
            xattrs: Vec::new(),
        };
        sidecar.push_dir(&row).map_err(error::io)?;
    }
    Ok(())
}
