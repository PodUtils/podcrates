//! The user name and the password of Wisp v2's password extension
//! (MercuryWorkshop/wisp-protocol:protocol.md, branch v2, "0x02 - Password
//! Authentication"). The client's metadata is the length of the user name
//! (one byte), the user name, then the password to the end, each UTF-8.
//! The password goes into that metadata alone: no `Debug` and no line holds
//! it, and it is zeroed on drop.

use zeroize::Zeroizing;

/// The longest user name: its length is one byte.
pub const MAX_USER: usize = 255;

/// A user name and a password for a server that asks for them.
#[derive(Clone, PartialEq, Eq)]
pub struct Login {
    user: String,
    password: Zeroizing<String>,
}

impl std::fmt::Debug for Login {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Login").field("user", &self.user).field("password", &"<redacted>").finish()
    }
}

impl Login {
    /// A login: a user name of 255 bytes at most, and a password that is
    /// not empty. No message quotes the password.
    pub fn new(user: &str, password: &str) -> Result<Login, String> {
        if user.len() > MAX_USER {
            return Err(format!("the Wisp user name has {} bytes, more than {MAX_USER}", user.len()));
        }
        if password.is_empty() {
            return Err("the Wisp password is empty".into());
        }
        Ok(Login { user: user.to_string(), password: Zeroizing::new(password.to_string()) })
    }

    /// `USER:PASSWORD`, as one line of a private file gives it: the user
    /// name ends at the first colon, and one line end at the end is
    /// dropped. No message quotes the text.
    pub fn parse(text: &str) -> Result<Login, String> {
        let text = text.strip_suffix('\n').unwrap_or(text);
        let text = text.strip_suffix('\r').unwrap_or(text);
        if text.contains(['\n', '\r']) {
            return Err("the Wisp login is one line, USER:PASSWORD".into());
        }
        let (user, password) = text.split_once(':').ok_or("the Wisp login is USER:PASSWORD, with a colon")?;
        Login::new(user, password)
    }

    /// The user name.
    pub fn user(&self) -> &str {
        &self.user
    }

    /// The client's metadata of the extension, zeroed on drop.
    pub(super) fn metadata(&self) -> Zeroizing<Vec<u8>> {
        let mut out = Zeroizing::new(Vec::with_capacity(1 + self.user.len() + self.password.len()));
        // Login::new keeps the length within one byte.
        out.push(self.user.len() as u8);
        out.extend_from_slice(self.user.as_bytes());
        out.extend_from_slice(self.password.as_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_metadata_is_the_length_the_user_and_the_password() {
        let login = Login::new("ann", "pw").unwrap();
        assert_eq!(login.metadata().as_slice(), b"\x03annpw");
        assert_eq!(Login::new("", "pw").unwrap().metadata().as_slice(), b"\x00pw", "an empty user name");
        assert_eq!(Login::parse("ann:p:w\r\n").unwrap().metadata().as_slice(), b"\x03annp:w");
    }

    #[test]
    fn a_bad_login_is_refused_and_no_message_or_debug_holds_the_password() {
        assert!(Login::new(&"u".repeat(256), "pw").unwrap_err().contains("more than 255"));
        assert!(Login::new("ann", "").unwrap_err().contains("empty"));
        for bad in ["hunter2", "ann:hunter2\nmore"] {
            let why = Login::parse(bad).unwrap_err();
            assert!(!why.contains("hunter2"), "{why}");
        }
        let shown = format!("{:?}", Login::new("ann", "hunter2").unwrap());
        assert!(shown.contains("ann") && !shown.contains("hunter2"), "{shown}");
    }
}
