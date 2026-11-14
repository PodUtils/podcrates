//! Why the store refused: the cause of each failure of
//! [`super::store`].

use std::path::PathBuf;

use pod_core::{Class, Failure, Stage};

/// Why the store refused: the cause of the [`Failure`] of
/// [`super::store`]. [`refusal`] finds it in a failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The origin declared more than the ceiling, before the first byte was
    /// written.
    DeclaredTooLarge { declared: u64, ceiling: u64 },
    /// The bytes came past the ceiling: the origin lied, or declared no
    /// size.
    OverCeiling { ceiling: u64 },
    /// The digest is not the pinned one.
    Digest { want: String, got: String },
    /// The body came past the size that the origin declared, or a later
    /// answer declared another size of the object.
    Size { declared: u64, got: u64 },
    /// A `206` whose `Content-Range` does not start at the offset of the
    /// request, does not parse, or disagrees with its `Content-Length`.
    Range { from: u64, got: String },
    /// A status that is not `200`, `206`, `416` or a break.
    Status { status: u16 },
    /// The transfer broke on each try. The partial file stays, so that a
    /// later call resumes it.
    Broken { have: u64, total: Option<u64>, tries: u32, last: String },
    /// Another store holds the lock of the partial file.
    Busy { part: PathBuf },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::DeclaredTooLarge { declared, ceiling } => write!(
                f,
                "the origin declares {declared} bytes, over the {ceiling}-byte ceiling, so nothing was downloaded"
            ),
            Refusal::OverCeiling { ceiling } => write!(
                f,
                "the transfer passed the {ceiling}-byte ceiling, so it was stopped and the partial file removed"
            ),
            Refusal::Digest { want, got } => write!(f, "sha256 mismatch: pinned {want}, downloaded {got}"),
            Refusal::Size { declared, got } => {
                write!(f, "the origin declared {declared} bytes and gave {got}, so the partial file was removed")
            }
            Refusal::Range { from, got } => write!(
                f,
                "the origin answered a range from byte {from} with Content-Range {got:?}, so the partial file was removed"
            ),
            Refusal::Status { status } => write!(f, "the origin answered {status}"),
            Refusal::Broken { have, total, tries, last } => {
                let of = total.map(|t| format!(" of {t}")).unwrap_or_default();
                write!(
                    f,
                    "the transfer broke {tries} times ({last}); {have}{of} bytes stay in the partial file for a resume"
                )
            }
            Refusal::Busy { part } => write!(f, "{}: another store writes this file", part.display()),
        }
    }
}

impl std::error::Error for Refusal {}

impl Refusal {
    /// The failure of this refusal: [`Class::Policy`] for a ceiling,
    /// [`Class::TempFail`] for a break and a busy file, [`Class::NotFound`]
    /// for `404` and `410`, [`Class::Refused`] for `401` and `403`,
    /// [`Class::Protocol`] for the rest.
    pub fn failure(self) -> Failure {
        let class = match self {
            Refusal::DeclaredTooLarge { .. } | Refusal::OverCeiling { .. } => Class::Policy,
            Refusal::Broken { .. } | Refusal::Busy { .. } => Class::TempFail,
            Refusal::Status { status: 404 | 410 } => Class::NotFound,
            Refusal::Status { status: 401 | 403 } => Class::Refused,
            Refusal::Digest { .. } | Refusal::Size { .. } | Refusal::Range { .. } | Refusal::Status { .. } => {
                Class::Protocol
            }
        };
        Failure::caused(Stage::Host, class, self.to_string(), self)
    }

    /// True when the partial file stays after this refusal: its bytes are
    /// good, and a later call resumes them.
    pub fn keeps_part(&self) -> bool {
        matches!(self, Refusal::Status { .. } | Refusal::Broken { .. } | Refusal::Busy { .. })
    }
}

/// The [`Refusal`] in the chain of `f`, when the store refused.
pub fn refusal(f: &Failure) -> Option<&Refusal> {
    crate::cause_of::<Refusal>(f)
}
