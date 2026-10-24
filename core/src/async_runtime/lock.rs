use std::sync::Arc;

#[cfg(feature = "smol")]
pub use smol::lock::{Mutex, MutexGuard, OnceCell, RwLock};

#[cfg(feature = "tokio")]
pub use tokio::sync::{Mutex, MutexGuard, OnceCell, RwLock};

#[cfg(feature = "smol")]
use smol::lock::{Semaphore as RtSemaphore, SemaphoreGuardArc};
#[cfg(feature = "tokio")]
use tokio::sync::{OwnedSemaphorePermit, Semaphore as RtSemaphore};

use crate::Result;

/// A permit from [`Semaphore`]. The permit is released on drop.
#[cfg(feature = "smol")]
pub type SemaphoreGuard = SemaphoreGuardArc;
/// A permit from [`Semaphore`]. The permit is released on drop.
#[cfg(feature = "tokio")]
pub type SemaphoreGuard = OwnedSemaphorePermit;

/// An async counting semaphore that works on both smol and tokio.
///
/// # Example
///
///```
/// use karyon_core::async_runtime::lock::Semaphore;
///
///  async {
///     let sem = Semaphore::new(2);
///     let guard = sem.acquire().await.unwrap();
///     // ...
///     drop(guard);
///  };
///
/// ```
///
pub struct Semaphore(Arc<RtSemaphore>);

impl Semaphore {
    /// Creates a new Semaphore with `permits` permits.
    pub fn new(permits: usize) -> Self {
        Self(Arc::new(RtSemaphore::new(permits)))
    }

    /// Waits for a permit. The returned guard can be moved into other tasks.
    pub async fn acquire(&self) -> Result<SemaphoreGuard> {
        #[cfg(feature = "smol")]
        let guard = self.0.acquire_arc().await;
        #[cfg(feature = "tokio")]
        let guard = self.0.clone().acquire_owned().await?;

        Ok(guard)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::async_runtime::{block_on, spawn};

    use super::*;

    #[test]
    fn test_semaphore() {
        block_on(async {
            let sem = Arc::new(Semaphore::new(1));
            let guard = sem.acquire().await.unwrap();

            let task = spawn({
                let sem = sem.clone();
                async move {
                    let _guard = sem.acquire().await.unwrap();
                }
            });

            // Release the permit so the task can take it.
            drop(guard);
            let _ = task.await;

            // The task dropped its guard, so the permit is free again.
            let _guard = sem.acquire().await.unwrap();
        });
    }
}
