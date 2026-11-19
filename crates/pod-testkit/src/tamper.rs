//! A changed copy of a signed or hashed file, for a test that proves that a
//! check refuses one changed byte.

use std::path::Path;

/// One bit of the byte `at` of the file `path` changed. An `at` past the
/// end changes the last byte, so that `usize::MAX` names it.
pub fn flip(path: &Path, at: usize) {
    let mut bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert!(!bytes.is_empty(), "{} is empty", path.display());
    let at = at.min(bytes.len() - 1);
    bytes[at] ^= 1;
    std::fs::write(path, bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}

#[cfg(test)]
mod tests {
    #[test]
    fn one_bit_of_one_byte_changes() {
        let dir = std::env::temp_dir().join(format!("pod-testkit-tamper-{}", std::process::id()));
        std::fs::write(&dir, b"abc").unwrap();
        super::flip(&dir, 0);
        super::flip(&dir, usize::MAX);
        assert_eq!(std::fs::read(&dir).unwrap(), b"`bb");
        std::fs::remove_file(&dir).unwrap();
    }
}
