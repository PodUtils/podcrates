//! The credential of the reply `credential` to `mint`: the files that the
//! runner reads at its start. The bytes go on the wire and into files of
//! mode 0600; they never go into argv, the environment, a log or a report.
//! The writer of the files and the checks of their names stay in podrunner;
//! this is the form of the wire alone.

use std::fmt;

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

/// A secret text. `Debug` prints its length and no byte of it, and the drop
/// wipes it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct Hidden(String);

impl Hidden {
    pub fn new(s: impl Into<String>) -> Hidden {
        Hidden(s.into())
    }

    /// The text, for the one writer of the credential file.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Hidden {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hidden(<{} bytes>)", self.0.len())
    }
}

impl Drop for Hidden {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// One file of a credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MintedFile {
    /// A plain file name; podrunner refuses a name with `/` or `..`.
    pub name: String,
    pub content: Hidden,
}

/// The files that the runner needs for one job.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Minted {
    pub files: Vec<MintedFile>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_prints_no_secret() {
        let c = Minted { files: vec![MintedFile { name: ".runner".into(), content: Hidden::new("CANARY-77") }] };
        let shown = format!("{c:?}");
        assert!(!shown.contains("CANARY-77"), "{shown}");
        assert!(shown.contains("9 bytes"), "{shown}");
        assert_eq!(c.files[0].content.expose(), "CANARY-77");
        let line = serde_json::to_string(&c).unwrap();
        assert_eq!(line, r#"{"files":[{"name":".runner","content":"CANARY-77"}]}"#);
    }
}
