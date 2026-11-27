//! The codes and the words of the errors of the system that a test makes
//! with `std::io::Error::from_raw_os_error`. The numbers and the words
//! differ on each system: ECONNRESET is 104 on Linux and 54 on macOS, and
//! Windows has its own codes and words.

/// A connection that the peer reset: the code and the words of the system.
#[cfg(unix)]
pub const RESET: (i32, &str) = (libc::ECONNRESET, "Connection reset by peer");
/// A connection that the peer reset (WSAECONNRESET).
#[cfg(windows)]
pub const RESET: (i32, &str) = (10054, "An existing connection was forcibly closed by the remote host.");

/// A file that is not there: the code and the words of the system.
#[cfg(unix)]
pub const MISSING: (i32, &str) = (libc::ENOENT, "No such file or directory");
/// A file that is not there (ERROR_FILE_NOT_FOUND).
#[cfg(windows)]
pub const MISSING: (i32, &str) = (2, "The system cannot find the file specified.");

#[cfg(test)]
mod tests {
    use super::*;

    /// The words are the words of this system for each code.
    #[test]
    fn each_code_has_the_words_of_this_system() {
        for (code, words) in [RESET, MISSING] {
            let shown = std::io::Error::from_raw_os_error(code).to_string();
            assert_eq!(shown, format!("{words} (os error {code})"));
        }
    }
}
