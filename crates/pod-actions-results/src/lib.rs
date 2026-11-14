//! The artifacts of a run: the ArtifactService of the results API of
//! GitHub Actions that `actions/upload-artifact@v4` and
//! `actions/download-artifact@v4` call at `ACTIONS_RESULTS_URL` (twirp
//! JSON), and the blob routes of the signed URLs (the block blob calls of
//! the Azure client).
//!
//! One [`ArtifactStore`] serves each run, so a job reads the artifact of an
//! earlier job. The store is outside each unit: a unit writes it only
//! through the server that mounts it. The crate has no HTTP server: the
//! caller gives each request to [`ArtifactStore::answer`] (or
//! [`ArtifactStore::route`]) and writes the [`Answer`].
//!
//! A name is unique in a run, as on GitHub. Each signed URL carries a
//! random signature, compared in constant time. The finalize checks the
//! size and the SHA-256 that the client sends. The bytes of a run have a
//! limit. A wrapper of protobuf (`google.protobuf.StringValue`) is a plain
//! string in its JSON; a request is read in both forms.

mod blob;
mod service;
mod store;
pub mod token;
mod wire;

pub use store::ArtifactStore;
pub use token::{BackendIds, backend_ids, results_scope};
pub use wire::{Answer, CONTENT_BLOB, CONTENT_JSON, CONTENT_TEXT, Payload};

/// The path of each call of the service: `SERVICE` and the method.
pub const SERVICE: &str = "/twirp/github.actions.results.api.v1.ArtifactService/";
/// The path of each blob of a signed URL: `BLOBS`, `upload/ID` or
/// `download/ID`, and the query `sig=`.
pub const BLOBS: &str = "/_artifacts/";

/// The default limit of the bytes of one run.
pub const LIMIT: u64 = 4 << 30;

#[cfg(test)]
mod tests;
