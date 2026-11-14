//! The actions of a GitHub Actions workflow by tag, for a runner that runs
//! the workflow on its own host: each `uses: OWNER/REPO[/PATH]@REF` of the
//! workflow and of each composite action that it names, resolved to a
//! commit, kept as a tarball in a store, and pinned in a lock file by its
//! commit and by the SHA-256 of its uncompressed tar.
//!
//! ```no_run
//! use pod_actions_store::{Git, Lock, Mode, Store, resolve, workflow_uses};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let text = "on: push\njobs:\n  a:\n    runs-on: x\n    steps:\n      - uses: o/r@v1\n";
//! let workflow = pod_actions_workflow::parse(text)?.workflow;
//! let path = std::path::Path::new("actions.lock");
//! let lock = Lock::read(path)?;
//! let store = Store { dir: "store".into() };
//! let git = Git::new("https://github.com/", "scratch");
//! let uses = workflow_uses(&workflow, &["actions/checkout"]);
//! let r = resolve(&uses, &lock, &store, &git, Mode::Online, "actions.lock")?;
//! if r.changed {
//!     r.lock.write(path)?;
//! }
//! # Ok(())
//! # }
//! ```
//!
//! The same lock pins each checkout of another repository
//! ([`workflow_trees`], [`add_trees`]) and each image by tag
//! ([`images::workflow_images`], [`images::resolve`]).
//!
//! The crate reads no environment variable and chooses no directory: the
//! consumer names the lock, the store and the scratch directory of git.

mod error;
mod git;
pub mod images;
pub mod lock;
pub mod store;

pub use error::{CheckoutFault, Error, LineFault};
pub use git::Git;
pub use images::{ImageSource, by_tag, workflow_images};
pub use lock::{Lock, Pin, is_commit};
pub use store::{
    Forge, Mode, Resolved, Store, add_trees, archive_name, checkout_target, digest, resolve, workflow_trees,
    workflow_uses,
};
