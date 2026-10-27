use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

use parking_lot::Mutex;

use crate::async_runtime::lock::MutexGuard;

/// Id of a task waiting on a CondVar.
type WaiterId = u64;

/// CondVar is an async version of <https://doc.rust-lang.org/std/sync/struct.Condvar.html>
///
/// # Example
///
///```
/// use std::sync::Arc;
///
/// use karyon_core::async_util::CondVar;
/// use karyon_core::async_runtime::{spawn, lock::Mutex};
///
///  async {
///     
///     let val = Arc::new(Mutex::new(false));
///     let condvar = Arc::new(CondVar::new());
///
///     spawn({
///         let val = val.clone();
///         let condvar = condvar.clone();
///         async move {
///             let mut val = val.lock().await;
///
///             // While the boolean flag is false, wait for a signal.
///             while !*val {
///                 val = condvar.wait(val).await;
///             }
///
///             // ...
///         }
///     });
///
///     spawn({
///         let condvar = condvar.clone();
///         async move {
///             let mut val = val.lock().await;
///
///             // While the boolean flag is false, wait for a signal.
///             while !*val {
///                 val = condvar.wait(val).await;
///             }
///
///             // ...
///         }
///     });
///     
///     // Wake up all waiting tasks on this condvar
///     condvar.broadcast();
///  };
///
/// ```
pub struct CondVar {
    inner: Mutex<Wakers>,
}

impl CondVar {
    /// Creates a new CondVar
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Wakers::new()),
        }
    }

    /// Blocks the current task until this condition variable receives a notification.
    pub async fn wait<'a, T>(&self, g: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
        #[cfg(feature = "smol")]
        let m = MutexGuard::source(&g);
        #[cfg(feature = "tokio")]
        let m = MutexGuard::mutex(&g);

        CondVarAwait::new(self, g).await;

        m.lock().await
    }

    /// Wakes up one blocked task waiting on this condvar.
    pub fn signal(&self) {
        self.inner.lock().wake(true);
    }

    /// Wakes up all blocked tasks waiting on this condvar.
    pub fn broadcast(&self) {
        self.inner.lock().wake(false);
    }
}

impl Default for CondVar {
    fn default() -> Self {
        Self::new()
    }
}

/// Future returned by `CondVar::wait`, resolves once the task is signaled.
struct CondVarAwait<'a, T> {
    /// Waiter id, set while the task is in the waker list.
    id: Option<WaiterId>,
    condvar: &'a CondVar,
    /// Held until the first poll, then released.
    guard: Option<MutexGuard<'a, T>>,
}

impl<'a, T> CondVarAwait<'a, T> {
    fn new(condvar: &'a CondVar, guard: MutexGuard<'a, T>) -> Self {
        Self {
            condvar,
            guard: Some(guard),
            id: None,
        }
    }
}

impl<T> Future for CondVarAwait<'_, T> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut inner = self.condvar.inner.lock();

        match self.guard.take() {
            Some(_) => {
                // The first poll registers the waker and releases the
                // MutexGuard.
                self.id = Some(inner.put(Some(cx.waker().clone())));
                Poll::Pending
            }
            None => {
                // Return Ready if it has already been polled and removed
                // from the waker list.
                if self.id.is_none() {
                    return Poll::Ready(());
                }

                let i = self.id.as_ref().unwrap();
                let waker_op = match inner.wakers.get_mut(i) {
                    Some(wk) => wk,
                    None => {
                        self.id = None;
                        return Poll::Ready(());
                    }
                };

                // `None` means the waker was taken by signal or broadcast.
                match waker_op {
                    Some(wk) => {
                        // This will prevent cloning again
                        if !wk.will_wake(cx.waker()) {
                            wk.clone_from(cx.waker());
                        }
                        Poll::Pending
                    }
                    None => {
                        inner.delete(i);
                        self.id = None;
                        Poll::Ready(())
                    }
                }
            }
        }
    }
}

impl<T> Drop for CondVarAwait<'_, T> {
    fn drop(&mut self) {
        if let Some(id) = self.id {
            let mut inner = self.condvar.inner.lock();
            // Already signaled but cancelled before waking up, so pass the
            // signal on to another waiter.
            if let Some(None) = inner.wakers.remove(&id) {
                inner.wake(true);
            }
        }
    }
}

/// Wakers is a helper struct to store the task wakers
struct Wakers {
    /// Waiting tasks. `None` means signaled but not woken up yet.
    wakers: HashMap<WaiterId, Option<Waker>>,
    /// Next waiter id, only grows so ids never collide.
    next_id: WaiterId,
}

impl Wakers {
    fn new() -> Self {
        Self {
            wakers: HashMap::new(),
            next_id: 0,
        }
    }

    /// Adds a waker and returns its id.
    /// Called with the CondVar lock held, so ids are never handed out twice.
    /// A u64 does not overflow in practice.
    fn put(&mut self, waker: Option<Waker>) -> WaiterId {
        let id = self.next_id;
        self.next_id += 1;
        self.wakers.insert(id, waker);
        id
    }

    fn delete(&mut self, id: &WaiterId) -> Option<Option<Waker>> {
        self.wakers.remove(id)
    }

    /// Wakes one waiter if `signal` is true, otherwise all of them.
    /// Waiters already signaled are skipped.
    fn wake(&mut self, signal: bool) {
        for wk in self.wakers.values_mut() {
            match wk.take() {
                Some(w) => {
                    w.wake();
                    if signal {
                        break;
                    }
                }
                None => continue,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        },
        task::Wake,
    };

    use crate::async_runtime::{block_on, lock::Mutex, spawn};

    use super::*;

    // The tests below demonstrate a solution to a problem in the Wikipedia
    // explanation of condition variables:
    // https://en.wikipedia.org/wiki/Monitor_(synchronization)#Solving_the_bounded_producer/consumer_problem.

    struct Flag(AtomicBool);

    impl Wake for Flag {
        fn wake(self: Arc<Self>) {
            self.0.store(true, Ordering::Relaxed);
        }
    }

    struct Queue {
        items: VecDeque<String>,
        max_len: usize,
    }
    impl Queue {
        fn new(max_len: usize) -> Self {
            Self {
                items: VecDeque::new(),
                max_len,
            }
        }

        fn is_full(&self) -> bool {
            self.items.len() == self.max_len
        }

        fn is_empty(&self) -> bool {
            self.items.is_empty()
        }
    }

    #[test]
    fn test_condvar_signal() {
        block_on(async {
            let number_of_tasks = 30;

            let queue = Arc::new(Mutex::new(Queue::new(5)));
            let condvar_full = Arc::new(CondVar::new());
            let condvar_empty = Arc::new(CondVar::new());

            let _producer1 = spawn({
                let queue = queue.clone();
                let condvar_full = condvar_full.clone();
                let condvar_empty = condvar_empty.clone();
                async move {
                    for i in 1..number_of_tasks {
                        // Lock queue mtuex
                        let mut queue = queue.lock().await;

                        // Check if the queue is non-full
                        while queue.is_full() {
                            // Release queue mutex and sleep
                            queue = condvar_full.wait(queue).await;
                        }

                        queue.items.push_back(format!("task {i}"));

                        // Wake up the consumer
                        condvar_empty.signal();
                    }
                }
            });

            let task_consumed = Arc::new(AtomicUsize::new(0));

            let consumer = spawn({
                let queue = queue.clone();
                let task_consumed = task_consumed.clone();
                async move {
                    for _ in 1..number_of_tasks {
                        // Lock queue mtuex
                        let mut queue = queue.lock().await;

                        // Check if the queue is non-empty
                        while queue.is_empty() {
                            // Release queue mutex and sleep
                            queue = condvar_empty.wait(queue).await;
                        }

                        let _ = queue.items.pop_front().unwrap();

                        task_consumed.fetch_add(1, Ordering::Relaxed);

                        // Do something

                        // Wake up the producer
                        condvar_full.signal();
                    }
                }
            });

            let _ = consumer.await;
            assert!(queue.lock().await.is_empty());
            assert_eq!(task_consumed.load(Ordering::Relaxed), 29);
        });
    }

    #[test]
    fn test_condvar_broadcast() {
        block_on(async {
            let tasks = 30;

            let queue = Arc::new(Mutex::new(Queue::new(5)));
            let condvar = Arc::new(CondVar::new());

            let _producer1 = spawn({
                let queue = queue.clone();
                let condvar = condvar.clone();
                async move {
                    for i in 1..tasks {
                        // Lock queue mtuex
                        let mut queue = queue.lock().await;

                        // Check if the queue is non-full
                        while queue.is_full() {
                            // Release queue mutex and sleep
                            queue = condvar.wait(queue).await;
                        }

                        queue.items.push_back(format!("producer1: task {i}"));

                        // Wake up all producer and consumer tasks
                        condvar.broadcast();
                    }
                }
            });

            let _producer2 = spawn({
                let queue = queue.clone();
                let condvar = condvar.clone();
                async move {
                    for i in 1..tasks {
                        // Lock queue mtuex
                        let mut queue = queue.lock().await;

                        // Check if the queue is non-full
                        while queue.is_full() {
                            // Release queue mutex and sleep
                            queue = condvar.wait(queue).await;
                        }

                        queue.items.push_back(format!("producer2: task {i}"));

                        // Wake up all producer and consumer tasks
                        condvar.broadcast();
                    }
                }
            });

            let task_consumed = Arc::new(AtomicUsize::new(0));

            let consumer = spawn({
                let queue = queue.clone();
                let task_consumed = task_consumed.clone();
                async move {
                    for _ in 1..((tasks * 2) - 1) {
                        {
                            // Lock queue mutex
                            let mut queue = queue.lock().await;

                            // Check if the queue is non-empty
                            while queue.is_empty() {
                                // Release queue mutex and sleep
                                queue = condvar.wait(queue).await;
                            }

                            let _ = queue.items.pop_front().unwrap();

                            task_consumed.fetch_add(1, Ordering::Relaxed);

                            // Do something

                            // Wake up all producer and consumer tasks
                            condvar.broadcast();
                        }
                    }
                }
            });

            let _ = consumer.await;
            assert!(queue.lock().await.is_empty());
            assert_eq!(task_consumed.load(Ordering::Relaxed), 58);
        });
    }

    #[test]
    fn test_condvar_drop_signaled_waiter() {
        block_on(async {
            let m1 = Mutex::new(());
            let m2 = Mutex::new(());
            let condvar = CondVar::new();

            let f1 = Arc::new(Flag(AtomicBool::new(false)));
            let f2 = Arc::new(Flag(AtomicBool::new(false)));
            let w1 = Waker::from(f1.clone());
            let w2 = Waker::from(f2.clone());

            let mut a1 = Box::pin(CondVarAwait::new(&condvar, m1.lock().await));
            let mut a2 = Box::pin(CondVarAwait::new(&condvar, m2.lock().await));
            assert!(a1.as_mut().poll(&mut Context::from_waker(&w1)).is_pending());
            assert!(a2.as_mut().poll(&mut Context::from_waker(&w2)).is_pending());

            // Wake exactly one waiter.
            condvar.signal();
            let first = f1.0.load(Ordering::Relaxed);
            assert_ne!(first, f2.0.load(Ordering::Relaxed));

            // Cancel the chosen waiter, the other one must wake up.
            if first {
                drop(a1);
                assert!(f2.0.load(Ordering::Relaxed));
                assert!(a2.as_mut().poll(&mut Context::from_waker(&w2)).is_ready());
            } else {
                drop(a2);
                assert!(f1.0.load(Ordering::Relaxed));
                assert!(a1.as_mut().poll(&mut Context::from_waker(&w1)).is_ready());
            }
        });
    }
}
