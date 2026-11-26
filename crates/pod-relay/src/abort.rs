//! A task that ends with its owner.

use tokio::task::JoinHandle;

/// A task that is aborted when this value drops: a future that is dropped
/// (its task aborted, or a `select!` that took another arm) stops the tasks
/// that it started, and so frees what they hold.
pub(crate) struct AbortOnDrop(pub(crate) JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}
