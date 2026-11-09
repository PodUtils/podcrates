//! The names that the caller gives: the variables that the payload and the
//! interposer read, the files on disk, and the word at the start of a line
//! on the error writer. The crate reads none of these variables. It only
//! writes them into the environment of a payload, removes them from it,
//! or names them in a refusal.

/// The names of a caller. Each field is `&'static str`, so a caller can
/// keep its names in a `const`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Names {
    /// The name of the product at the start of a line on the error writer
    /// (`NAME: ...`).
    pub product: &'static str,
    /// The variable of a request for a launch rung. The caller reads it
    /// before it builds the plan. The entry removes it from the
    /// environment of each payload, so a nested run starts with no
    /// request.
    pub mode_request: &'static str,
    /// The variable that names the rung that the entry entered. The entry
    /// sets it beside each exec and removes each older value.
    pub mode_active: &'static str,
    /// The variable that allows the persistent cache rung. A refusal of
    /// the ladder names it.
    pub cache: &'static str,
    /// The variable of the table of path maps of the entry with no chroot,
    /// which the interposer reads.
    pub maps: &'static str,
    /// The variable of the device list of a container record.
    pub device_spec: &'static str,
    /// The variable of the device table that the interposer serves.
    pub device_fds: &'static str,
    /// The marker file of a complete cache directory.
    pub cache_marker: &'static str,
    /// The name of the memfd of a payload (`/memfd:NAME` in
    /// `/proc/PID/exe`).
    pub memfd_label: &'static str,
}

impl Names {
    /// The names of this crate, for a caller that has no names of its own.
    pub const DEFAULT: Names = Names {
        product: "pod-enter",
        mode_request: "POD_ENTER_MODE",
        mode_active: "POD_ENTER_ACTIVE_MODE",
        cache: "POD_ENTER_CACHE",
        maps: "POD_ENTER_MAPS",
        device_spec: "POD_ENTER_DEVICE_SPEC",
        device_fds: "POD_ENTER_DEVICE_FDS",
        cache_marker: ".pod-enter-cache-complete",
        memfd_label: "pod-enter-payload",
    };
}

impl Default for Names {
    fn default() -> Names {
        Names::DEFAULT
    }
}

/// The name of an environment entry `NAME=VALUE`: the part before the
/// first `=`, or the whole entry where it has none.
pub(crate) fn var_name(entry: &str) -> &str {
    entry.split('=').next().unwrap_or("")
}
