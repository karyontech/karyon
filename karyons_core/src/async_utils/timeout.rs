use std::{future::Future, time::Duration};

use smol::Timer;

use super::{select, Either};
use crate::{error::Error, Result};

/// Waits for a future to complete or times out if it exceeds a specified
/// duration.
///
/// # Example
///
/// ```
/// use std::{future, time::Duration};
///
/// use karyons_core::async_utils::timeout;
///
/// async {
///     let fut = future::pending::<()>();
///     assert!(timeout(Duration::from_millis(100), fut).await.is_err());
/// };
///
/// ```
///
pub async fn timeout<T, F>(delay: Duration, future1: F) -> Result<T>
where
    F: Future<Output = T>,
{
    let result = select(Timer::after(delay), future1).await;

    match result {
        Either::Left(_) => Err(Error::Timeout),
        Either::Right(res) => Ok(res),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{future, time::Duration};

    #[test]
    fn test_timeout() {
        smol::block_on(async move {
            let fut = future::pending::<()>();
            assert!(timeout(Duration::from_millis(10), fut).await.is_err());

            let fut = smol::Timer::after(Duration::from_millis(10));
            assert!(timeout(Duration::from_millis(50), fut).await.is_ok())
        });
    }
}
