//! The failures of the crate: each is at the stage of the host.

use pod_core::{Class, Failure, Stage};

/// A failure of the host with the text `what`.
pub(crate) fn host(class: Class, what: impl Into<String>) -> Failure {
    Failure::new(Stage::Host, class, what)
}

/// The errno `errno` of the call `what`.
pub(crate) fn errno(class: Class, what: &str, errno: i32) -> Failure {
    Failure::caused(Stage::Host, class, what.to_string(), std::io::Error::from_raw_os_error(errno))
}

/// The errno of the last call of this thread, for the call `what`, with
/// the class of its kind.
pub(crate) fn last_os(what: &str) -> Failure {
    let e = std::io::Error::last_os_error();
    Failure::caused(Stage::Host, Class::of_io(e.kind()), what.to_string(), e)
}
