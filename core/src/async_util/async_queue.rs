use std::{collections::VecDeque, sync::Arc};

use crate::async_runtime::lock::Mutex;
use crate::async_util::CondVar;

/// Bounded async queue. Producers `push` (blocks while full),
/// consumers `recv` (blocks while empty).
///
/// The mutex covers only the `VecDeque` push/pop, so slow work the
/// consumer does after `recv` is never inside a lock.
///
/// # Example
///
/// ```no_run
/// use karyon_core::async_util::AsyncQueue;
///
/// async {
///     let q = AsyncQueue::<u32>::new(8);
///     q.push(1).await;
///     let _ = q.recv().await;
/// };
/// ```
pub struct AsyncQueue<T> {
    queue: Mutex<VecDeque<T>>,
    capacity: usize,
    not_empty: CondVar,
    not_full: CondVar,
}

impl<T> AsyncQueue<T> {
    /// Create an `AsyncQueue` with the given capacity. Panics if
    /// `capacity` is zero.
    pub fn new(capacity: usize) -> Arc<Self> {
        assert!(capacity > 0, "AsyncQueue capacity must be > 0");
        Arc::new(Self {
            queue: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
            not_empty: CondVar::new(),
            not_full: CondVar::new(),
        })
    }

    /// Push an item. Awaits while the queue is full.
    pub async fn push(&self, item: T) {
        let mut queue = self.queue.lock().await;
        while queue.len() >= self.capacity {
            queue = self.not_full.wait(queue).await;
        }
        queue.push_back(item);
        self.not_empty.signal();
    }

    /// Pop the next item. Awaits while the queue is empty.
    pub async fn recv(&self) -> T {
        let mut queue = self.queue.lock().await;
        while queue.is_empty() {
            queue = self.not_empty.wait(queue).await;
        }
        let item = queue.pop_front().expect("queue not empty");
        self.not_full.signal();
        item
    }

    /// Current number of items.
    pub async fn len(&self) -> usize {
        self.queue.lock().await.len()
    }

    /// True when the queue has no items.
    pub async fn is_empty(&self) -> bool {
        self.queue.lock().await.is_empty()
    }

    /// Configured capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}
