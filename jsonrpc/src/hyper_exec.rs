//! `hyper::rt::Executor` impl shared by the HTTP server and the
//! tokio HTTP/1+2 client. Spawns hyper's internal tasks via the
//! owning Server/Client task_group so they get cancelled on stop.

use std::sync::Arc;

use karyon_core::async_util::{TaskGroup, TaskResult};

#[derive(Clone)]
pub(crate) struct HyperExecutor {
    task_group: Arc<TaskGroup>,
}

impl HyperExecutor {
    pub(crate) fn new(task_group: Arc<TaskGroup>) -> Self {
        Self { task_group }
    }
}

impl<F> hyper::rt::Executor<F> for HyperExecutor
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    fn execute(&self, fut: F) {
        self.task_group.spawn(
            async move {
                let _ = fut.await;
            },
            |_: TaskResult<()>| async {},
        );
    }
}
