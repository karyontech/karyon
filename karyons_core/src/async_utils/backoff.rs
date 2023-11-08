use std::{
    cmp::min,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    time::Duration,
};

use smol::Timer;

/// Exponential backoff
/// <https://en.wikipedia.org/wiki/Exponential_backoff>
///
/// # Examples
///
/// ```
/// use karyons_core::async_utils::Backoff;
///
///  async {
///     let backoff = Backoff::new(300, 3000);
///
///     loop {
///         backoff.sleep().await;
///     
///         // do something
///         break;
///     }
///
///     backoff.reset();
///
///     // ....
///  };
///
/// ```
///
pub struct Backoff {
    /// The base delay in milliseconds for the initial retry.
    base_delay: u64,
    /// The max delay in milliseconds allowed for a retry.
    max_delay: u64,
    /// Atomic counter
    retries: AtomicU32,
    /// Stop flag
    stop: AtomicBool,
}

impl Backoff {
    /// Creates a new Backoff.
    pub fn new(base_delay: u64, max_delay: u64) -> Self {
        Self {
            base_delay,
            max_delay,
            retries: AtomicU32::new(0),
            stop: AtomicBool::new(false),
        }
    }

    /// Sleep based on the current retry count and delay values.
    /// Retruns the delay value.
    pub async fn sleep(&self) -> u64 {
        if self.stop.load(Ordering::SeqCst) {
            Timer::after(Duration::from_millis(self.max_delay)).await;
            return self.max_delay;
        }

        let retries = self.retries.load(Ordering::SeqCst);
        let delay = self.base_delay * (2_u64).pow(retries);
        let delay = min(delay, self.max_delay);

        if delay == self.max_delay {
            self.stop.store(true, Ordering::SeqCst);
        }

        self.retries.store(retries + 1, Ordering::SeqCst);

        Timer::after(Duration::from_millis(delay)).await;
        delay
    }

    /// Reset the retry counter to 0.
    pub fn reset(&self) {
        self.retries.store(0, Ordering::SeqCst);
        self.stop.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_backoff() {
        smol::block_on(async move {
            let backoff = Arc::new(Backoff::new(5, 15));
            let backoff_c = backoff.clone();
            smol::spawn(async move {
                let delay = backoff_c.sleep().await;
                assert_eq!(delay, 5);

                let delay = backoff_c.sleep().await;
                assert_eq!(delay, 10);

                let delay = backoff_c.sleep().await;
                assert_eq!(delay, 15);
            })
            .await;

            smol::spawn(async move {
                backoff.reset();
                let delay = backoff.sleep().await;
                assert_eq!(delay, 5);
            })
            .await;
        });
    }
}
