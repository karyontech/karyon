use std::{future::Future, time::Duration};

use crate::{error::Error, Result};

use super::{select, sleep, Either};

/// Waits for a future to complete or times out if it exceeds a specified
/// duration.
///
/// # Example
///
/// ```
/// use std::{future, time::Duration};
///
/// use karyon_core::async_util::timeout;
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
    let result = select(sleep(delay), future1).await;

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
        crate::async_runtime::block_on(async move {
            let fut = future::pending::<()>();
            assert!(timeout(Duration::from_millis(10), fut).await.is_err());

            let fut = sleep(Duration::from_millis(10));
            assert!(timeout(Duration::from_millis(50), fut).await.is_ok())
        });
    }
}
