//! A task that ends with its owner.

use tokio::task::JoinHandle;

/// A task that is aborted when this value drops. A future that is dropped (its
/// task aborted, a `select!` that took another arm, a failed attempt) stops
/// the tasks that it started, and so frees what they hold.
#[derive(Debug)]
pub struct AbortOnDrop<T = ()>(Option<JoinHandle<T>>);

impl<T> AbortOnDrop<T> {
    pub fn new(task: JoinHandle<T>) -> Self {
        Self(Some(task))
    }

    /// The task, which then goes on when the handle is dropped.
    pub fn into_inner(mut self) -> JoinHandle<T> {
        self.0.take().expect("the task is held until here")
    }
}

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        if let Some(task) = self.0.take() {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A task that holds `token` until it ends.
    fn held(token: Arc<()>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let _token = token;
            std::future::pending::<()>().await;
        })
    }

    #[tokio::test]
    async fn a_dropped_guard_aborts_its_task() {
        let token = Arc::new(());
        let guard = AbortOnDrop::new(held(token.clone()));
        tokio::task::yield_now().await;
        assert_eq!(Arc::strong_count(&token), 2, "the task runs");
        drop(guard);
        for _ in 0..100 {
            if Arc::strong_count(&token) == 1 {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("the task still holds its value after the guard dropped");
    }

    #[tokio::test]
    async fn into_inner_lets_the_task_go_on() {
        let token = Arc::new(());
        let task = AbortOnDrop::new(held(token.clone())).into_inner();
        tokio::task::yield_now().await;
        assert!(!task.is_finished());
        assert_eq!(Arc::strong_count(&token), 2, "the task holds its value");
        task.abort();
    }
}
