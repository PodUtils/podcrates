//! The words of a failure that a limit of the host explains.

/// The words of an error of too many open files (`EMFILE`, `ENFILE`): the
/// limit of the process, and how to raise it. `None` for another error.
pub fn open_files(err: &std::io::Error) -> Option<String> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        if !matches!(err.raw_os_error(), Some(libc::EMFILE) | Some(libc::ENFILE)) {
            return None;
        }
        let mut limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        // SAFETY: getrlimit writes one rlimit, which `limit` is.
        let soft = match unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } {
            0 => limit.rlim_cur.to_string(),
            _ => "?".into(),
        };
        Some(format!("too many open files (the limit is {soft}); raise it with ulimit -n"))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = err;
        None
    }
}

#[cfg(test)]
mod tests {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn the_hint_of_open_files_names_the_limit() {
        let err = std::io::Error::from_raw_os_error(libc::EMFILE);
        let hint = super::open_files(&err).unwrap();
        assert!(hint.contains("ulimit -n") && hint.contains("the limit is "), "{hint}");
        assert!(super::open_files(&std::io::Error::from_raw_os_error(libc::ECONNREFUSED)).is_none());
    }
}
