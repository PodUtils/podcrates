//! The user and the password that a client of a proxy that the consumer
//! serves must give: SOCKS5's RFC 1929, or HTTP `Proxy-Authorization:
//! Basic`. The check takes a time that does not depend on where a wrong
//! value differs, and no message shows the password.

use std::path::Path;

use base64::Engine as _;
use pod_core::files::{Others, read_own};

/// A user and a password that a client must give.
#[derive(Clone)]
pub struct Credentials {
    user: String,
    pass: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Credentials({}, <hidden>)", self.user)
    }
}

/// Equal bytes, in a time that does not depend on where they differ.
fn same(a: &[u8], b: &[u8]) -> bool {
    let mut diff = (a.len() != b.len()) as u8;
    for (i, x) in a.iter().enumerate() {
        diff |= x ^ b.get(i).copied().unwrap_or(0);
    }
    diff == 0
}

impl Credentials {
    /// A user and a password of 1 to 255 bytes each (RFC 1929), and a user
    /// with no `:` (HTTP Basic).
    pub fn new(user: &str, pass: &str) -> Result<Credentials, String> {
        if user.is_empty() || user.len() > 255 || pass.is_empty() || pass.len() > 255 {
            return Err("a proxy user and password must each have 1 to 255 bytes (RFC 1929)".into());
        }
        if user.contains(':') {
            return Err("a proxy user has no ':' (HTTP Basic)".into());
        }
        Ok(Credentials { user: user.into(), pass: pass.into() })
    }

    /// The user, and the password from the first line of the file at
    /// `path`. The file must be a regular file of this user that others
    /// cannot read ([`pod_core::files::read_own`]).
    pub fn load(user: &str, path: &Path) -> Result<Credentials, String> {
        let text = read_own(path, Others::NoAccess)?.ok_or_else(|| format!("{}: no such file", path.display()))?;
        let pass = text.lines().next().unwrap_or_default().trim_end_matches('\r');
        Credentials::new(user, pass)
    }

    pub fn user(&self) -> &str {
        &self.user
    }

    /// The value `Basic BASE64(user:pass)` of an Authorization header.
    pub fn basic_header(&self) -> String {
        let token = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", self.user, self.pass));
        format!("Basic {token}")
    }

    /// True for the right user and password.
    pub fn check(&self, user: &str, pass: &str) -> bool {
        same(user.as_bytes(), self.user.as_bytes()) & same(pass.as_bytes(), self.pass.as_bytes())
    }

    /// True for a `Proxy-Authorization` value `Basic BASE64(user:pass)` of
    /// the right user and password.
    pub fn check_basic(&self, value: Option<&str>) -> bool {
        let Some(value) = value else { return false };
        let Some((scheme, token)) = value.trim().split_once(' ') else {
            return false;
        };
        if !scheme.eq_ignore_ascii_case("basic") {
            return false;
        }
        let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(token.trim()) else {
            return false;
        };
        let raw = String::from_utf8_lossy(&raw);
        let Some((user, pass)) = raw.split_once(':') else {
            return false;
        };
        self.check(user, pass)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The check of the credentials of a client.
    #[test]
    fn credentials_are_checked() {
        let c = Credentials::new("alice", "s3cret").unwrap();
        assert!(c.check("alice", "s3cret"));
        assert!(!c.check("alice", "s3cre"));
        assert!(!c.check("bob", "s3cret"));
        assert!(c.check_basic(Some("Basic YWxpY2U6czNjcmV0")));
        assert!(c.check_basic(Some("basic   YWxpY2U6czNjcmV0")));
        assert!(!c.check_basic(Some("Basic YWxpY2U6d3Jvbmc=")));
        assert!(!c.check_basic(Some("Bearer x")));
        assert!(!c.check_basic(None));
        assert!(!format!("{c:?}").contains("s3cret"));
        assert!(Credentials::new("a:b", "p").is_err());
        assert!(Credentials::new("", "p").is_err());
    }

    /// The header of Basic is the check's own form; a file that is not
    /// there is an error with its path.
    #[test]
    fn the_basic_header_and_a_missing_file() {
        let c = Credentials::new("alice", "s3cret").unwrap();
        assert_eq!(c.basic_header(), "Basic YWxpY2U6czNjcmV0");
        assert!(c.check_basic(Some(&c.basic_header())));
        let none = std::env::temp_dir().join(format!("pod-proxy-no-such-file-{}", std::process::id()));
        assert!(Credentials::load("alice", &none).unwrap_err().contains("no such file"));
    }

    #[cfg(unix)]
    #[test]
    fn a_password_file_must_be_private() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = std::env::temp_dir().join(format!("pod-proxy-credentials-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("pass");
        std::fs::write(&file, "s3cret\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Credentials::load("alice", &file).is_err());
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(Credentials::load("alice", &file).unwrap().check("alice", "s3cret"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
