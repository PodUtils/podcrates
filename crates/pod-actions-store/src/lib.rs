//! The actions of a GitHub Actions workflow by tag, for a runner that runs
//! the workflow on its own host: each `uses: OWNER/REPO[/PATH]@REF` of the
//! workflow and of each composite action that it names, resolved to a
//! commit, kept as a tarball in a store, and pinned in a lock file by its
//! commit and by the SHA-256 of its uncompressed tar.
//!
//! ```no_run
//! use pod_actions_store::{Git, Lock, Mode, Options, Store, resolve_with, workflow_uses};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let text = "on: push\njobs:\n  a:\n    runs-on: x\n    steps:\n      - uses: o/r@v1\n";
//! let workflow = pod_actions_workflow::parse(text)?.workflow;
//! let path = std::path::Path::new("actions.lock");
//! let lock = Lock::read(path)?;
//! let store = Store { dir: "store".into() };
//! let git = Git::new("https://github.com/", "scratch");
//! // The consumer gives `actions/checkout` in its own way: no fetch of it
//! // in the workflow or in a composite action.
//! let except = ["actions/checkout"];
//! let uses = workflow_uses(&workflow, &except);
//! let options = Options { except: &except };
//! let r = resolve_with(&uses, &options, &lock, &store, &git, Mode::Online, "actions.lock")?;
//! if r.changed {
//!     r.lock.write(path)?;
//! }
//! # Ok(())
//! # }
//! ```
//!
//! GitHub matches the owner and the name of an action with no case, and the
//! ref and the path with case; so does each match of this crate. A
//! `uses: $/PATH` of a composite action is the repository of that action
//! at its ref ([`self_uses`]).
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

pub use error::{CheckoutFault, Error, LineFault, SelfFault};
pub use git::Git;
pub use images::{ImageSource, by_tag, workflow_images};
pub use lock::{Lock, Pin, is_commit};
pub use store::{
    Forge, Mode, Options, Resolved, SELF_PREFIX, Store, add_trees, archive_name, checkout_target, digest, resolve,
    resolve_with, self_uses, workflow_trees, workflow_uses,
};
