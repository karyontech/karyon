use std::{future::Future, sync::Arc};

use parking_lot::Mutex;

use crate::async_runtime::{global_executor, Executor, Task};

use super::{select, CondWait, Either};

/// TaskGroup A group that contains spawned tasks.
///
/// # Example
///
/// ```
///
/// use std::sync::Arc;
///
/// use karyon_core::async_util::{TaskGroup, sleep};
///
/// async {
///     let group = TaskGroup::new();
///
///     group.spawn(sleep(std::time::Duration::MAX), |_| async {});
///
///     group.cancel().await;
///
/// };
///
/// ```
pub struct TaskGroup {
    tasks: Mutex<Vec<TaskHandler>>,
    stop_signal: Arc<CondWait>,
    executor: Executor,
}

impl TaskGroup {
    /// Creates a new TaskGroup without providing an executor
    ///
    /// This will spawn a task onto a global executor (single-threaded by default).
    pub fn new() -> Self {
        Self {
            tasks: Mutex::new(Vec::new()),
            stop_signal: Arc::new(CondWait::new()),
            executor: global_executor(),
        }
    }

    /// Creates a new TaskGroup by providing an executor
    pub fn with_executor(executor: Executor) -> Self {
        Self {
            tasks: Mutex::new(Vec::new()),
            stop_signal: Arc::new(CondWait::new()),
            executor,
        }
    }

    /// Spawns a new task and calls the callback after it has completed
    /// or been canceled. The callback will have the `TaskResult` as a
    /// parameter, indicating whether the task completed or was canceled.
    pub fn spawn<T, Fut, CallbackF, CallbackFut>(&self, fut: Fut, callback: CallbackF)
    where
        T: Send + Sync + 'static,
        Fut: Future<Output = T> + Send + 'static,
        CallbackF: FnOnce(TaskResult<T>) -> CallbackFut + Send + 'static,
        CallbackFut: Future<Output = ()> + Send + 'static,
    {
        let task = TaskHandler::new(
            self.executor.clone(),
            fut,
            callback,
            self.stop_signal.clone(),
        );
        self.tasks.lock().push(task);
    }

    /// Checks if the TaskGroup is empty.
    pub fn is_empty(&self) -> bool {
        self.tasks.lock().is_empty()
    }

    /// Get the number of the tasks in the group.
    pub fn len(&self) -> usize {
        self.tasks.lock().len()
    }

    /// Cancels all tasks in the group.
    pub async fn cancel(&self) {
        self.stop_signal.broadcast().await;

        loop {
            // XXX BE CAREFUL HERE, it hold synchronous mutex across .await point.
            let task = self.tasks.lock().pop();
            if let Some(t) = task {
                t.cancel().await
            } else {
                break;
            }
        }
    }
}

impl Default for TaskGroup {
    fn default() -> Self {
        Self::new()
    }
}

/// The result of a spawned task.
#[derive(Debug)]
pub enum TaskResult<T> {
    Completed(T),
    Cancelled,
}

impl<T: std::fmt::Debug> std::fmt::Display for TaskResult<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            TaskResult::Cancelled => write!(f, "Task cancelled"),
            TaskResult::Completed(res) => write!(f, "Task completed: {:?}", res),
        }
    }
}

/// TaskHandler
pub struct TaskHandler {
    task: Task<()>,
    cancel_flag: Arc<CondWait>,
}

impl<'a> TaskHandler {
    /// Creates a new task handler
    fn new<T, Fut, CallbackF, CallbackFut>(
        ex: Executor,
        fut: Fut,
        callback: CallbackF,
        stop_signal: Arc<CondWait>,
    ) -> TaskHandler
    where
        T: Send + Sync + 'static,
        Fut: Future<Output = T> + Send + 'static,
        CallbackF: FnOnce(TaskResult<T>) -> CallbackFut + Send + 'static,
        CallbackFut: Future<Output = ()> + Send + 'static,
    {
        let cancel_flag = Arc::new(CondWait::new());
        let cancel_flag_c = cancel_flag.clone();
        let task = ex.spawn(async move {
            // Waits for either the stop signal or the task to complete.
            let result = select(stop_signal.wait(), fut).await;

            let result = match result {
                Either::Left(_) => TaskResult::Cancelled,
                Either::Right(res) => TaskResult::Completed(res),
            };

            // Call the callback
            callback(result).await;

            cancel_flag_c.signal().await;
        });

        TaskHandler { task, cancel_flag }
    }

    /// Cancels the task.
    async fn cancel(self) {
        self.cancel_flag.wait().await;
        self.task.cancel().await;
    }
}

#[cfg(test)]
mod tests {
    use std::{future, sync::Arc};

    use crate::async_runtime::block_on;
    use crate::async_util::sleep;

    use super::*;

    #[cfg(feature = "tokio")]
    #[test]
    fn test_task_group_with_tokio_executor() {
        let ex = Arc::new(tokio::runtime::Runtime::new().unwrap());
        ex.clone().block_on(async move {
            let group = Arc::new(TaskGroup::with_executor(ex.into()));

            group.spawn(future::ready(0), |res| async move {
                assert!(matches!(res, TaskResult::Completed(0)));
            });

            group.spawn(future::pending::<()>(), |res| async move {
                assert!(matches!(res, TaskResult::Cancelled));
            });

            let groupc = group.clone();
            group.spawn(
                async move {
                    groupc.spawn(future::pending::<()>(), |res| async move {
                        assert!(matches!(res, TaskResult::Cancelled));
                    });
                },
                |res| async move {
                    assert!(matches!(res, TaskResult::Completed(_)));
                },
            );

            // Do something
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            group.cancel().await;
        });
    }

    #[cfg(feature = "smol")]
    #[test]
    fn test_task_group_with_smol_executor() {
        let ex = Arc::new(smol::Executor::new());
        smol::block_on(ex.clone().run(async move {
            let group = Arc::new(TaskGroup::with_executor(ex.into()));

            group.spawn(future::ready(0), |res| async move {
                assert!(matches!(res, TaskResult::Completed(0)));
            });

            group.spawn(future::pending::<()>(), |res| async move {
                assert!(matches!(res, TaskResult::Cancelled));
            });

            let groupc = group.clone();
            group.spawn(
                async move {
                    groupc.spawn(future::pending::<()>(), |res| async move {
                        assert!(matches!(res, TaskResult::Cancelled));
                    });
                },
                |res| async move {
                    assert!(matches!(res, TaskResult::Completed(_)));
                },
            );

            // Do something
            smol::Timer::after(std::time::Duration::from_millis(50)).await;
            group.cancel().await;
        }));
    }

    #[test]
    fn test_task_group() {
        block_on(async {
            let group = Arc::new(TaskGroup::new());

            group.spawn(future::ready(0), |res| async move {
                assert!(matches!(res, TaskResult::Completed(0)));
            });

            group.spawn(future::pending::<()>(), |res| async move {
                assert!(matches!(res, TaskResult::Cancelled));
            });

            let groupc = group.clone();
            group.spawn(
                async move {
                    groupc.spawn(future::pending::<()>(), |res| async move {
                        assert!(matches!(res, TaskResult::Cancelled));
                    });
                },
                |res| async move {
                    assert!(matches!(res, TaskResult::Completed(_)));
                },
            );

            // Do something
            sleep(std::time::Duration::from_millis(50)).await;
            group.cancel().await;
        });
    }
}
