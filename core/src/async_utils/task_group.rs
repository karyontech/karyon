use std::{future::Future, sync::Arc, sync::Mutex};

use smol::Task;

use crate::Executor;

use super::{select, CondWait, Either};

/// TaskGroup is a group of spawned tasks.
///
/// # Example
///
/// ```
///
/// use std::sync::Arc;
///
/// use karyons_core::async_utils::TaskGroup;
///
/// async {
///
///     let ex = Arc::new(smol::Executor::new());
///     let group = TaskGroup::new();
///
///     group.spawn(ex.clone(), smol::Timer::never(), |_| async {});
///
///     group.cancel().await;
///
/// };
///
/// ```
///
pub struct TaskGroup {
    tasks: Mutex<Vec<TaskHandler>>,
    stop_signal: Arc<CondWait>,
}

impl<'a> TaskGroup {
    /// Creates a new task group
    pub fn new() -> Self {
        Self {
            tasks: Mutex::new(Vec::new()),
            stop_signal: Arc::new(CondWait::new()),
        }
    }

    /// Spawns a new task and calls the callback after it has completed
    /// or been canceled. The callback will have the `TaskResult` as a
    /// parameter, indicating whether the task completed or was canceled.
    pub fn spawn<T, Fut, CallbackF, CallbackFut>(
        &self,
        executor: Executor<'a>,
        fut: Fut,
        callback: CallbackF,
    ) where
        T: Send + Sync + 'a,
        Fut: Future<Output = T> + Send + 'a,
        CallbackF: FnOnce(TaskResult<T>) -> CallbackFut + Send + 'a,
        CallbackFut: Future<Output = ()> + Send + 'a,
    {
        let task = TaskHandler::new(executor.clone(), fut, callback, self.stop_signal.clone());
        self.tasks.lock().unwrap().push(task);
    }

    /// Checks if the task group is empty.
    pub fn is_empty(&self) -> bool {
        self.tasks.lock().unwrap().is_empty()
    }

    /// Get the number of the tasks in the group.
    pub fn len(&self) -> usize {
        self.tasks.lock().unwrap().len()
    }

    /// Cancels all tasks in the group.
    pub async fn cancel(&self) {
        self.stop_signal.broadcast().await;

        loop {
            let task = self.tasks.lock().unwrap().pop();
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
    /// Creates a new task handle
    fn new<T, Fut, CallbackF, CallbackFut>(
        ex: Executor<'a>,
        fut: Fut,
        callback: CallbackF,
        stop_signal: Arc<CondWait>,
    ) -> TaskHandler
    where
        T: Send + Sync + 'a,
        Fut: Future<Output = T> + Send + 'a,
        CallbackF: FnOnce(TaskResult<T>) -> CallbackFut + Send + 'a,
        CallbackFut: Future<Output = ()> + Send + 'a,
    {
        let cancel_flag = Arc::new(CondWait::new());
        let cancel_flag_c = cancel_flag.clone();
        let task = ex.spawn(async move {
            //start_signal.signal().await;
            // Waits for either the stop signal or the task to complete.
            let result = select(stop_signal.wait(), fut).await;

            let result = match result {
                Either::Left(_) => TaskResult::Cancelled,
                Either::Right(res) => TaskResult::Completed(res),
            };

            // Call the callback with the result.
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
    use super::*;
    use std::{future, sync::Arc};

    #[test]
    fn test_task_group() {
        let ex = Arc::new(smol::Executor::new());
        smol::block_on(ex.clone().run(async move {
            let group = Arc::new(TaskGroup::new());

            group.spawn(ex.clone(), future::ready(0), |res| async move {
                assert!(matches!(res, TaskResult::Completed(0)));
            });

            group.spawn(ex.clone(), future::pending::<()>(), |res| async move {
                assert!(matches!(res, TaskResult::Cancelled));
            });

            let groupc = group.clone();
            let exc = ex.clone();
            group.spawn(
                ex.clone(),
                async move {
                    groupc.spawn(exc.clone(), future::pending::<()>(), |res| async move {
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
}
