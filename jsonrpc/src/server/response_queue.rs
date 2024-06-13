use std::{collections::VecDeque, sync::Arc};

use karyon_core::{async_runtime::lock::Mutex, async_util::CondVar};

/// A queue for handling responses
pub(super) struct ResponseQueue<T> {
    queue: Mutex<VecDeque<T>>,
    condvar: CondVar,
}

impl<T: std::fmt::Debug> ResponseQueue<T> {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            queue: Mutex::new(VecDeque::new()),
            condvar: CondVar::new(),
        })
    }

    /// Wait while the queue is empty, remove and return the item from the queue,
    /// panicking if empty (shouldn't happen)
    pub(super) async fn recv(&self) -> T {
        let mut queue = self.queue.lock().await;

        while queue.is_empty() {
            queue = self.condvar.wait(queue).await;
        }

        match queue.pop_front() {
            Some(v) => v,
            None => unreachable!(),
        }
    }

    /// Push an item into the queue, notify all waiting tasks that the
    /// condvar has changed
    pub(super) async fn push(&self, res: T) {
        self.queue.lock().await.push_back(res);
        self.condvar.signal();
    }
}
