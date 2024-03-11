use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::Mutex,
    task::{Context, Poll, Waker},
};

use smol::lock::MutexGuard;

use crate::util::random_16;

/// CondVar is an async version of <https://doc.rust-lang.org/std/sync/struct.Condvar.html>
///
/// # Example
///
///```
/// use std::sync::Arc;
///
/// use smol::lock::Mutex;
///
/// use karyon_core::async_util::CondVar;
///
///  async {
///     
///     let val = Arc::new(Mutex::new(false));
///     let condvar = Arc::new(CondVar::new());
///
///     let val_cloned = val.clone();
///     let condvar_cloned = condvar.clone();
///     smol::spawn(async move {
///         let mut val = val_cloned.lock().await;
///
///         // While the boolean flag is false, wait for a signal.
///         while !*val {
///             val = condvar_cloned.wait(val).await;
///         }
///
///         // ...
///     });
///
///     let condvar_cloned = condvar.clone();
///     smol::spawn(async move {
///         let mut val = val.lock().await;
///
///         // While the boolean flag is false, wait for a signal.
///         while !*val {
///             val = condvar_cloned.wait(val).await;
///         }
///
///         // ...
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
        let m = MutexGuard::source(&g);

        CondVarAwait::new(self, g).await;

        m.lock().await
    }

    /// Wakes up one blocked task waiting on this condvar.
    pub fn signal(&self) {
        self.inner.lock().unwrap().wake(true);
    }

    /// Wakes up all blocked tasks waiting on this condvar.
    pub fn broadcast(&self) {
        self.inner.lock().unwrap().wake(false);
    }
}

impl Default for CondVar {
    fn default() -> Self {
        Self::new()
    }
}

struct CondVarAwait<'a, T> {
    id: Option<u16>,
    condvar: &'a CondVar,
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

impl<'a, T> Future for CondVarAwait<'a, T> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut inner = self.condvar.inner.lock().unwrap();

        match self.guard.take() {
            Some(_) => {
                // the first pooll will release the Mutexguard
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
                match inner.wakers.get_mut(i).unwrap() {
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

impl<'a, T> Drop for CondVarAwait<'a, T> {
    fn drop(&mut self) {
        if let Some(id) = self.id {
            let mut inner = self.condvar.inner.lock().unwrap();
            if let Some(wk) = inner.wakers.get_mut(&id).unwrap().take() {
                wk.wake()
            }
        }
    }
}

/// Wakers is a helper struct to store the task wakers
struct Wakers {
    wakers: HashMap<u16, Option<Waker>>,
}

impl Wakers {
    fn new() -> Self {
        Self {
            wakers: HashMap::new(),
        }
    }

    fn put(&mut self, waker: Option<Waker>) -> u16 {
        let mut id: u16;

        id = random_16();
        while self.wakers.contains_key(&id) {
            id = random_16();
        }

        self.wakers.insert(id, waker);
        id
    }

    fn delete(&mut self, id: &u16) -> Option<Option<Waker>> {
        self.wakers.remove(id)
    }

    fn wake(&mut self, signal: bool) {
        for (_, wk) in self.wakers.iter_mut() {
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
    use super::*;
    use smol::lock::Mutex;
    use std::{
        collections::VecDeque,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };

    // The tests below demonstrate a solution to a problem in the Wikipedia
    // explanation of condition variables:
    // https://en.wikipedia.org/wiki/Monitor_(synchronization)#Solving_the_bounded_producer/consumer_problem.

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
        smol::block_on(async {
            let number_of_tasks = 30;

            let queue = Arc::new(Mutex::new(Queue::new(5)));
            let condvar_full = Arc::new(CondVar::new());
            let condvar_empty = Arc::new(CondVar::new());

            let queue_cloned = queue.clone();
            let condvar_full_cloned = condvar_full.clone();
            let condvar_empty_cloned = condvar_empty.clone();

            let _producer1 = smol::spawn(async move {
                for i in 1..number_of_tasks {
                    // Lock queue mtuex
                    let mut queue = queue_cloned.lock().await;

                    // Check if the queue is non-full
                    while queue.is_full() {
                        // Release queue mutex and sleep
                        queue = condvar_full_cloned.wait(queue).await;
                    }

                    queue.items.push_back(format!("task {i}"));

                    // Wake up the consumer
                    condvar_empty_cloned.signal();
                }
            });

            let queue_cloned = queue.clone();
            let task_consumed = Arc::new(AtomicUsize::new(0));
            let task_consumed_ = task_consumed.clone();
            let consumer = smol::spawn(async move {
                for _ in 1..number_of_tasks {
                    // Lock queue mtuex
                    let mut queue = queue_cloned.lock().await;

                    // Check if the queue is non-empty
                    while queue.is_empty() {
                        // Release queue mutex and sleep
                        queue = condvar_empty.wait(queue).await;
                    }

                    let _ = queue.items.pop_front().unwrap();

                    task_consumed_.fetch_add(1, Ordering::Relaxed);

                    // Do something

                    // Wake up the producer
                    condvar_full.signal();
                }
            });

            consumer.await;
            assert!(queue.lock().await.is_empty());
            assert_eq!(task_consumed.load(Ordering::Relaxed), 29);
        });
    }

    #[test]
    fn test_condvar_broadcast() {
        smol::block_on(async {
            let tasks = 30;

            let queue = Arc::new(Mutex::new(Queue::new(5)));
            let condvar = Arc::new(CondVar::new());

            let queue_cloned = queue.clone();
            let condvar_cloned = condvar.clone();
            let _producer1 = smol::spawn(async move {
                for i in 1..tasks {
                    // Lock queue mtuex
                    let mut queue = queue_cloned.lock().await;

                    // Check if the queue is non-full
                    while queue.is_full() {
                        // Release queue mutex and sleep
                        queue = condvar_cloned.wait(queue).await;
                    }

                    queue.items.push_back(format!("producer1: task {i}"));

                    // Wake up all producer and consumer tasks
                    condvar_cloned.broadcast();
                }
            });

            let queue_cloned = queue.clone();
            let condvar_cloned = condvar.clone();
            let _producer2 = smol::spawn(async move {
                for i in 1..tasks {
                    // Lock queue mtuex
                    let mut queue = queue_cloned.lock().await;

                    // Check if the queue is non-full
                    while queue.is_full() {
                        // Release queue mutex and sleep
                        queue = condvar_cloned.wait(queue).await;
                    }

                    queue.items.push_back(format!("producer2: task {i}"));

                    // Wake up all producer and consumer tasks
                    condvar_cloned.broadcast();
                }
            });

            let queue_cloned = queue.clone();
            let task_consumed = Arc::new(AtomicUsize::new(0));
            let task_consumed_ = task_consumed.clone();

            let consumer = smol::spawn(async move {
                for _ in 1..((tasks * 2) - 1) {
                    {
                        // Lock queue mutex
                        let mut queue = queue_cloned.lock().await;

                        // Check if the queue is non-empty
                        while queue.is_empty() {
                            // Release queue mutex and sleep
                            queue = condvar.wait(queue).await;
                        }

                        let _ = queue.items.pop_front().unwrap();

                        task_consumed_.fetch_add(1, Ordering::Relaxed);

                        // Do something

                        // Wake up all producer and consumer tasks
                        condvar.broadcast();
                    }
                }
            });

            consumer.await;
            assert!(queue.lock().await.is_empty());
            assert_eq!(task_consumed.load(Ordering::Relaxed), 58);
        });
    }
}
