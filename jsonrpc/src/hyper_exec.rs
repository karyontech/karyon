//! `hyper::rt::Executor` impl shared by the HTTP server and the
//! tokio HTTP/1+2 client. Spawns hyper's internal tasks via the
//! owning Server/Client task_group so they get cancelled on stop.

use karyon_core::async_util::TaskGroupRef;

#[derive(Clone)]
pub(crate) struct HyperExecutor {
    task_group: TaskGroupRef,
}

impl HyperExecutor {
    pub(crate) fn new(task_group: TaskGroupRef) -> Self {
        Self { task_group }
    }
}

impl<F> hyper::rt::Executor<F> for HyperExecutor
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    fn execute(&self, fut: F) {
        self.task_group.spawn(async move {
            let _ = fut.await;
        });
    }
}
