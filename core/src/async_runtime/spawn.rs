use std::future::Future;

use super::Task;

pub fn spawn<T: Send + 'static>(future: impl Future<Output = T> + Send + 'static) -> Task<T> {
    #[cfg(feature = "smol")]
    let result: Task<T> = smol::spawn(future).into();
    #[cfg(feature = "tokio")]
    let result: Task<T> = tokio::spawn(future).into();

    result
}
