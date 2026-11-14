//! The sweep of the uploads that no run finalizes: the cache stays across
//! runs, and a run that crashes leaves its upload under `.pending`.

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use super::CacheStore;

/// The age after which an upload that no store holds goes: an upload in
/// progress of another store (another run over one directory) changes its
/// files much more often.
pub const PENDING_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// The newest time of change of an upload: its directory, its blocks and
/// its blob. `None` when no time can be read.
fn changed(dir: &Path) -> Option<SystemTime> {
    [dir.to_path_buf(), dir.join("blocks"), dir.join("blob")]
        .iter()
        .filter_map(|p| fs::metadata(p).and_then(|m| m.modified()).ok())
        .max()
}

impl CacheStore {
    /// Remove each upload under `.pending` that this store does not hold
    /// and that did not change for `PENDING_AGE` before `now`. It gives
    /// the number of uploads removed. `new` calls it with the time of the
    /// system.
    pub fn sweep(&self, now: SystemTime) -> usize {
        let held: Vec<String> = self.lock().pending.values().map(|p| p.dir.clone()).collect();
        let Some(limit) = now.checked_sub(PENDING_AGE) else { return 0 };
        let mut removed = 0;
        for e in fs::read_dir(self.dir.join(".pending")).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if held.contains(&name) {
                continue;
            }
            let path = e.path();
            if changed(&path).is_some_and(|t| t < limit) && fs::remove_dir_all(&path).is_ok() {
                removed += 1;
            }
        }
        removed
    }
}
