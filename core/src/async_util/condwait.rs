use super::CondVar;
use crate::async_runtime::lock::Mutex;

/// CondWait is a wrapper struct for CondVar with a Mutex boolean flag.
///
/// # Example
///
///```
/// use std::sync::Arc;
///
/// use karyon_core::async_util::CondWait;
/// use karyon_core::async_runtime::spawn;
///
///  async {
///     let cond_wait = Arc::new(CondWait::new());
///     let cond_wait_cloned = cond_wait.clone();
///     let task = spawn(async move {
///         cond_wait_cloned.wait().await;
///         // ...
///     });
///
///     cond_wait.signal().await;
///  };
///
/// ```
///
pub struct CondWait {
    /// The CondVar
    condvar: CondVar,
    /// Boolean flag
    w: Mutex<bool>,
}

impl CondWait {
    /// Creates a new CondWait.
    pub fn new() -> Self {
        Self {
            condvar: CondVar::new(),
            w: Mutex::new(false),
        }
    }

    /// Waits for a signal or broadcast.
    pub async fn wait(&self) {
        let mut w = self.w.lock().await;

        // While the boolean flag is false, wait for a signal.
        while !*w {
            w = self.condvar.wait(w).await;
        }
    }

    /// Signal a waiting task.
    pub async fn signal(&self) {
        *self.w.lock().await = true;
        self.condvar.signal();
    }

    /// Signal all waiting tasks.
    pub async fn broadcast(&self) {
        *self.w.lock().await = true;
        self.condvar.broadcast();
    }

    /// Reset the boolean flag value to false.
    pub async fn reset(&self) {
        *self.w.lock().await = false;
    }
}

impl Default for CondWait {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use crate::async_runtime::{block_on, spawn};

    use super::*;

    #[test]
    fn test_cond_wait() {
        block_on(async {
            let cond_wait = Arc::new(CondWait::new());
            let count = Arc::new(AtomicUsize::new(0));

            let cond_wait_cloned = cond_wait.clone();
            let count_cloned = count.clone();
            let task = spawn(async move {
                cond_wait_cloned.wait().await;
                count_cloned.fetch_add(1, Ordering::Relaxed);
                // do something
            });

            // Send a signal to the waiting task
            cond_wait.signal().await;

            let _ = task.await;

            // Reset the boolean flag
            cond_wait.reset().await;

            assert_eq!(count.load(Ordering::Relaxed), 1);

            let cond_wait_cloned = cond_wait.clone();
            let count_cloned = count.clone();
            let task1 = spawn(async move {
                cond_wait_cloned.wait().await;
                count_cloned.fetch_add(1, Ordering::Relaxed);
                // do something
            });

            let cond_wait_cloned = cond_wait.clone();
            let count_cloned = count.clone();
            let task2 = spawn(async move {
                cond_wait_cloned.wait().await;
                count_cloned.fetch_add(1, Ordering::Relaxed);
                // do something
            });

            // Broadcast a signal to all waiting tasks
            cond_wait.broadcast().await;

            let _ = task1.await;
            let _ = task2.await;
            assert_eq!(count.load(Ordering::Relaxed), 3);
        });
    }
}
