use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use async_task::Runnable as TaskRunnable;

/// A boxed future that completes after a delay. See `Runtime::sleep`.
pub type SleepFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

/// A karyon task ready to run. Hides async_task, so it can change without
/// breaking users.
pub struct Runnable(TaskRunnable);

impl Runnable {
    pub(crate) fn new(runnable: TaskRunnable) -> Self {
        Self(runnable)
    }

    /// Polls the task once. Returns `true` if the task woke itself while
    /// running (it yielded); it is then scheduled again.
    pub fn run(self) -> bool {
        self.0.run()
    }
}

/// `Arc` reference to a `Runtime`.
pub type RuntimeRef = Arc<dyn Runtime>;

/// A user supplied runtime: runs karyon's tasks and timers, e.g. a
/// single-threaded executor on WASI. Pass it to karyon as
/// `Executor::from(runtime)`. smol and tokio need none of this; karyon uses
/// their own tasks and timers.
///
/// karyon calls `schedule` each time one of its tasks is woken. Run it with
/// `Runnable::run`.
// TODO: let the user bring their own tasks, with async_task as the default.
pub trait Runtime: Send + Sync {
    /// Runs `runnable`, now or soon.
    fn schedule(&self, runnable: Runnable);

    /// A future that completes after `duration`.
    fn sleep(&self, duration: Duration) -> SleepFuture;
}

#[cfg(test)]
mod tests {
    use std::{
        panic::{AssertUnwindSafe, catch_unwind},
        sync::atomic::{AtomicUsize, Ordering},
    };

    use crate::{
        async_runtime::{Executor, block_on, spawn},
        async_util::{TaskGroup, sleep},
    };

    use super::*;

    /// A user runtime: counts its calls and runs on the built-in one.
    #[derive(Default)]
    struct Counting {
        scheduled: AtomicUsize,
        sleeps: AtomicUsize,
    }

    impl Runtime for Counting {
        fn schedule(&self, runnable: Runnable) {
            self.scheduled.fetch_add(1, Ordering::SeqCst);
            spawn(async move {
                runnable.run();
            })
            .detach();
        }

        fn sleep(&self, duration: Duration) -> SleepFuture {
            self.sleeps.fetch_add(1, Ordering::SeqCst);
            Box::pin(sleep(duration))
        }
    }

    #[test]
    fn user_runtime_runs_tasks_and_timers() {
        block_on(async {
            let rt = Arc::new(Counting::default());
            let runtime: RuntimeRef = rt.clone();
            let ex = Executor::from(runtime.clone());

            assert_eq!(ex.spawn(async { 1 + 1 }).await.unwrap(), 2);

            let slept = runtime.clone();
            ex.spawn(async move { slept.sleep(Duration::from_millis(1)).await })
                .await
                .unwrap();

            // A TaskGroup on the user runtime cancels its tasks.
            let group = TaskGroup::with_executor(ex);
            group.spawn(runtime.sleep(Duration::MAX));
            group.cancel().await;

            assert!(rt.scheduled.load(Ordering::SeqCst) >= 2);
            assert!(rt.sleeps.load(Ordering::SeqCst) >= 1);
        });
    }

    #[test]
    fn user_runtime_task_panic_reaches_the_awaiter() {
        let res = catch_unwind(AssertUnwindSafe(|| {
            block_on(async {
                let runtime: RuntimeRef = Arc::new(Counting::default());
                let ex = Executor::from(runtime);
                let _ = ex.spawn(async { panic!("boom") }).await;
            })
        }));
        assert!(res.is_err());
    }
}
