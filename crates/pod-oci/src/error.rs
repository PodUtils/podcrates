//! One typed error for the crate, converted at the boundary.
//!
//! `docs/conventions/code.md`: typed errors, never a bare string, and never an
//! error swallowed silently. Every variant here carries enough to name the
//! thing that failed without the caller re-deriving it.

use std::fmt;

// ⛔ **THE CODES ARE `podvm_probe::exit`'S AND ARE NOT DECLARED HERE.**
// [`TODO/cli.md`](../../../TODO/cli.md) T-0802: they were written out in three
// files and two of them had already diverged. That module carries the table, the
// measurement that produced it and the reason 125 and 1 are different answers.
pub use podvm_probe::exit::{
    EXIT_CANNOT_INVOKE, EXIT_CLI_ERROR, EXIT_FLAG_ERROR, EXIT_NOT_FOUND, EXIT_RUNTIME_ERROR,
};

#[derive(Debug)]
pub enum Error {
    /// The caller wrote something this crate cannot act on.
    Usage(String),
    /// A reference that does not parse, with what was wrong.
    Reference(String),
    /// ⛔ `TODO/image.md` T-0201: a registry offering only `http://` is a named
    /// refusal, not a downgrade. tcp/80 egress hangs on the studied runtime, so
    /// a fallback that exists to improve reliability makes the failure untimed.
    PlainHttpRefused(String),
    /// Transport, DNS, TLS, or a status this client will not act on.
    /// ⭐ TODO/image.md T-1420. A transport failure names its host once, in
    /// `detail` as `<host>: <lowercase cause>: <chain>`, with `what` carrying
    /// only the path. The renderer below keeps both on one line until `-v`
    /// exists to hold the chain (T-1416); nothing is dropped to get there.
    Http { what: String, detail: String },
    /// ⛔ `TODO/image.md` T-0202: computed and declared digests differ. Both are
    /// named, because "digest mismatch" alone cannot be acted on.
    DigestMismatch {
        what: String,
        want: String,
        got: String,
    },
    /// ⛔ `TODO/image.md` T-0203: the destination, the free amount, the required
    /// amount and the unit, so the message can be acted on without a second
    /// command. The mistake this exists to avoid is in the corpus at
    /// `references/mhx__dwarfs/tree/src/utility/filesystem_extractor.cpp:544-552`.
    NoSpace(String),
    /// A malformed or unsupported OCI document, naming the media type.
    Oci(String),
    /// An index with no manifest for the wanted platform: the want and the
    /// offered platform words, comma-joined. Typed (not an `Oci` string)
    /// so `pull -a` can skip the tag deliberately while a single-tag pull
    /// still fails on it (TODO/cli.md T-1331).
    NoPlatform { want: String, offered: String },
    /// The store, its layout, or a lock.
    Store(String),
    /// Anything the filesystem refused, with the path.
    Io {
        path: String,
        source: std::io::Error,
    },
    /// A reference that named nothing this store holds.
    NoSuchImage(String),
}

impl Error {
    pub fn io(path: impl Into<String>, source: std::io::Error) -> Error {
        Error::Io {
            path: path.into(),
            source,
        }
    }

    /// The process exit code this error produces.
    ///
    /// ⚠ Every variant here is raised **after** the flags parsed, so the caller
    /// mistakes among them are docker's 1 rather than its 125. A flag error
    /// never reaches this type: it is refused in the parser, which returns
    /// [`EXIT_FLAG_ERROR`] directly.
    pub fn exit_code(&self) -> i32 {
        match self {
            // ⚠ `PlainHttpRefused` is here because it names something the
            // CALLER wrote: `podvm pull http://...`. A registry whose token
            // realm is `http://` is not caller input and is an `Http` error
            // with the same explanation, so the two do not share a code.
            // ⚠ `NoSuchImage` is here because it is a name that resolved to
            // nothing, which docker answers with 1: measured on 2026-09-09,
            // `docker rmi no-such-image` exits 1 where `docker run` on a
            // missing image exits 125. The difference is not the lookup, it is
            // whether the verb was going to RUN something.
            Error::Usage(_)
            | Error::Reference(_)
            | Error::PlainHttpRefused(_)
            | Error::NoSuchImage(_) => EXIT_CLI_ERROR,
            _ => EXIT_RUNTIME_ERROR,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Usage(m) => write!(f, "{m}"),
            Error::Reference(m) => write!(f, "invalid reference: {m}"),
            Error::PlainHttpRefused(m) => write!(
                f,
                "{m}: podvm speaks HTTPS only and will not downgrade. \
                 tcp/80 egress is broken on the runtime podvm targets, so a \
                 plain-HTTP fallback hangs instead of failing (TODO/image.md T-0201)"
            ),
            Error::Http { what, detail } => write!(f, "{what}: {detail}"),
            Error::DigestMismatch { what, want, got } => write!(
                f,
                "{what}: digest mismatch, expected {want} and computed {got}. \
                 The bytes were discarded"
            ),
            Error::NoSpace(m) => write!(f, "{m}"),
            Error::Oci(m) => write!(f, "{m}"),
            Error::NoPlatform { want, offered } => write!(
                f,
                "this index offers no {want} manifest. It offers: {offered}"
            ),
            Error::Store(m) => write!(f, "store: {m}"),
            Error::Io { path, source } => write!(f, "{path}: {source}"),
            Error::NoSuchImage(r) => write!(f, "no such image: {r}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
