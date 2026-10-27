use std::{
    collections::HashMap,
    future::Future,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Weak,
    },
    thread::panicking,
};

use parking_lot::Mutex;

use crate::async_runtime::{global_executor, Executor, Task};

use super::{select, CondWait, Either};

/// Identifies a spawned task within a [`TaskGroup`].
pub type TaskID = usize;

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
///     group.spawn(sleep(std::time::Duration::MAX));
///
///     group.cancel().await;
///
/// };
///
/// ```
pub struct TaskGroup {
    inner: Arc<Inner>,
}

/// Shared state of a [`TaskGroup`]. Held behind an `Arc` so each task can
/// keep a `Weak` reference back to the group and remove itself on
/// completion, without forcing callers to wrap the group in an `Arc`.
struct Inner {
    tasks: Mutex<HashMap<TaskID, TaskHandler>>,
    next_id: AtomicUsize,
    executor: Executor,
}

impl Inner {
    /// Removes a task by id, returning its handler if still present.
    fn remove(&self, id: TaskID) -> Option<TaskHandler> {
        self.tasks.lock().remove(&id)
    }
}

impl TaskGroup {
    /// Creates a new TaskGroup without providing an executor
    ///
    /// This will spawn tasks onto the process-wide multi-threaded
    /// global executor.
    pub fn new() -> Self {
        Self::with_inner(global_executor())
    }

    /// Creates a new TaskGroup by providing an executor
    pub fn with_executor(executor: Executor) -> Self {
        Self::with_inner(executor)
    }

    fn with_inner(executor: Executor) -> Self {
        Self {
            inner: Arc::new(Inner {
                tasks: Mutex::new(HashMap::new()),
                next_id: AtomicUsize::new(0),
                executor,
            }),
        }
    }

    /// Spawns a new task and ignores its result.
    ///
    /// Returns the task's [`TaskID`]. The task removes itself from the
    /// group when it finishes, so the group does not grow without bound.
    pub fn spawn<T, Fut>(&self, fut: Fut) -> TaskID
    where
        T: Send + Sync + 'static,
        Fut: Future<Output = T> + Send + 'static,
    {
        self.spawn_then(fut, |_| async {})
    }

    /// Spawns a new task and calls the callback after it has completed
    /// or been canceled. The callback will have the `TaskResult` as a
    /// parameter, indicating whether the task completed or was canceled.
    ///
    /// Returns the task's [`TaskID`].
    pub fn spawn_then<T, Fut, CallbackF, CallbackFut>(
        &self,
        fut: Fut,
        callback: CallbackF,
    ) -> TaskID
    where
        T: Send + Sync + 'static,
        Fut: Future<Output = T> + Send + 'static,
        CallbackF: FnOnce(TaskResult<T>) -> CallbackFut + Send + 'static,
        CallbackFut: Future<Output = ()> + Send + 'static,
    {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        // Hold the lock across spawn and insert so the task cannot try to
        // remove itself before it has been inserted.
        let mut tasks = self.inner.tasks.lock();
        let task = TaskHandler::new(
            self.inner.executor.clone(),
            fut,
            callback,
            Arc::downgrade(&self.inner),
            id,
        );
        tasks.insert(id, task);
        id
    }

    /// Removes a task by id, returning its handler if still present. The
    /// caller takes ownership and may cancel it.
    pub fn remove(&self, id: TaskID) -> Option<TaskHandler> {
        self.inner.remove(id)
    }

    /// Checks if the TaskGroup is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.tasks.lock().is_empty()
    }

    /// Get the number of the tasks in the group.
    pub fn len(&self) -> usize {
        self.inner.tasks.lock().len()
    }

    /// Cancels all tasks in the group.
    pub async fn cancel(&self) {
        // Take all handlers out, then cancel them without holding the lock.
        let handlers: Vec<TaskHandler> = self.inner.tasks.lock().drain().map(|(_, h)| h).collect();
        for handler in handlers {
            handler.cancel().await;
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
            TaskResult::Completed(res) => write!(f, "Task completed: {res:?}"),
        }
    }
}

/// Removes a task from its group. Detach instead of dropping the handler,
/// so the task is not cancelled from within itself. If `cancel` already
/// took it out, this is a no-op.
fn remove_from_group(group: &Weak<Inner>, id: TaskID) {
    if let Some(group) = group.upgrade() {
        if let Some(handler) = group.remove(id) {
            handler.detach();
        }
    }
}

/// Lives inside the spawned task. If the task or its callback panics, it
/// signals `cancel_flag` and removes the task from the group, so `cancel`
/// does not wait forever.
struct PanicGuard {
    ex: Executor,
    cancel_flag: Arc<CondWait>,
    group: Weak<Inner>,
    id: TaskID,
}

impl Drop for PanicGuard {
    fn drop(&mut self) {
        // On a normal exit the task already signaled and removed itself.
        if !panicking() {
            return;
        }
        // Signaling needs an async lock, so do it from a new task.
        let cancel_flag = self.cancel_flag.clone();
        self.ex
            .spawn(async move { cancel_flag.signal().await })
            .detach();
        // The task's own removal at the end never runs after a panic.
        remove_from_group(&self.group, self.id);
    }
}

/// TaskHandler
pub struct TaskHandler {
    task: Task<()>,
    /// Per-task stop signal. Signaling it makes the task stop and run its
    /// callback with `Cancelled`.
    stop_signal: Arc<CondWait>,
    /// Set once the task has finished running its callback.
    cancel_flag: Arc<CondWait>,
}

impl TaskHandler {
    /// Creates a new task handler
    fn new<T, Fut, CallbackF, CallbackFut>(
        ex: Executor,
        fut: Fut,
        callback: CallbackF,
        group: Weak<Inner>,
        id: TaskID,
    ) -> TaskHandler
    where
        T: Send + Sync + 'static,
        Fut: Future<Output = T> + Send + 'static,
        CallbackF: FnOnce(TaskResult<T>) -> CallbackFut + Send + 'static,
        CallbackFut: Future<Output = ()> + Send + 'static,
    {
        let stop_signal = Arc::new(CondWait::new());
        let stop_signal_c = stop_signal.clone();
        let cancel_flag = Arc::new(CondWait::new());
        let cancel_flag_c = cancel_flag.clone();
        let guard = PanicGuard {
            ex: ex.clone(),
            cancel_flag: cancel_flag.clone(),
            group: group.clone(),
            id,
        };
        let task = ex.spawn(async move {
            // Dropped on every exit, but only acts on a panic.
            let _guard = guard;

            // Waits for either the stop signal or the task to complete.
            let result = select(stop_signal_c.wait(), fut).await;

            let result = match result {
                Either::Left(_) => TaskResult::Cancelled,
                Either::Right(res) => TaskResult::Completed(res),
            };

            // Call the callback
            callback(result).await;

            cancel_flag_c.signal().await;

            // Normal exit. On a panic the guard does this instead.
            remove_from_group(&group, id);
        });

        TaskHandler {
            task,
            stop_signal,
            cancel_flag,
        }
    }

    /// Detaches the task, so dropping the handler does not cancel it.
    fn detach(self) {
        self.task.detach();
    }

    /// Cancels the task: tells it to stop and waits for its callback to run.
    async fn cancel(self) {
        self.stop_signal.signal().await;
        self.cancel_flag.wait().await;
        // Nothing is left to run but the self removal. Detach instead of
        // awaiting, since smol re-raises a task panic in the awaiting task.
        self.detach();
    }
}

#[cfg(test)]
mod tests {
    use std::{future, sync::Arc, time::Duration};

    use crate::async_runtime::block_on;
    use crate::async_util::{sleep, timeout};

    use super::*;

    #[cfg(feature = "tokio")]
    #[test]
    fn test_task_group_with_tokio_executor() {
        let ex = Arc::new(tokio::runtime::Runtime::new().unwrap());
        ex.clone().block_on(async move {
            let group = Arc::new(TaskGroup::with_executor(ex.into()));

            group.spawn_then(future::ready(0), |res| async move {
                assert!(matches!(res, TaskResult::Completed(0)));
            });

            group.spawn_then(future::pending::<()>(), |res| async move {
                assert!(matches!(res, TaskResult::Cancelled));
            });

            let groupc = group.clone();
            group.spawn_then(
                async move {
                    groupc.spawn_then(future::pending::<()>(), |res| async move {
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

            group.spawn_then(future::ready(0), |res| async move {
                assert!(matches!(res, TaskResult::Completed(0)));
            });

            group.spawn_then(future::pending::<()>(), |res| async move {
                assert!(matches!(res, TaskResult::Cancelled));
            });

            let groupc = group.clone();
            group.spawn_then(
                async move {
                    groupc.spawn_then(future::pending::<()>(), |res| async move {
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

            group.spawn_then(future::ready(0), |res| async move {
                assert!(matches!(res, TaskResult::Completed(0)));
            });

            group.spawn_then(future::pending::<()>(), |res| async move {
                assert!(matches!(res, TaskResult::Cancelled));
            });

            let groupc = group.clone();
            group.spawn_then(
                async move {
                    groupc.spawn_then(future::pending::<()>(), |res| async move {
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

    #[test]
    fn test_task_group_removes_finished_tasks() {
        block_on(async {
            let group = Arc::new(TaskGroup::new());

            // A finished task removes itself; a pending one stays.
            group.spawn(future::ready(0));
            group.spawn(future::pending::<()>());

            sleep(std::time::Duration::from_millis(50)).await;
            assert_eq!(group.len(), 1);

            group.cancel().await;
            assert!(group.is_empty());
        });
    }

    #[test]
    fn test_task_group_remove_by_id() {
        block_on(async {
            let group = Arc::new(TaskGroup::new());

            let id = group.spawn(future::pending::<()>());
            assert_eq!(group.len(), 1);

            let handler = group.remove(id).expect("task is present");
            assert!(group.is_empty());
            handler.cancel().await;
        });
    }

    #[test]
    fn test_task_group_cancel_after_panic() {
        block_on(async {
            let group = TaskGroup::new();
            group.spawn(async { panic!("task panic") });

            // Must not wait forever on the panicked task.
            let res = timeout(Duration::from_secs(5), group.cancel()).await;
            assert!(res.is_ok());
        });
    }

    #[test]
    fn test_task_group_removes_panicked_tasks() {
        block_on(async {
            let group = TaskGroup::new();
            group.spawn_then(future::ready(0), |_| async { panic!("callback panic") });
            group.spawn(async { panic!("task panic") });

            sleep(Duration::from_millis(50)).await;
            assert!(group.is_empty());
        });
    }
}
