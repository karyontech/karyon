use std::{future::Future, sync::Arc, time::Duration};

use crate::async_runtime::Executor;
use crate::async_util::timeout;

/// Run a multi-threaded executor.
#[cfg(feature = "smol")]
pub fn run_executor<T>(main_future: impl Future<Output = T>, ex: Arc<smol::Executor<'_>>) {
    let (signal, shutdown) = async_channel::unbounded::<()>();

    let num_threads = available_parallelism();

    easy_parallel::Parallel::new()
        .each(0..(num_threads), |_| {
            smol::future::block_on(ex.run(shutdown.recv()))
        })
        .finish(|| {
            smol::future::block_on(async {
                main_future.await;
                drop(signal);
            })
        });
}

/// Returns an estimate of the default amount of parallelism a program should use.
/// see `std::thread::available_parallelism`
pub fn available_parallelism() -> usize {
    std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1)
}

/// Run an async test that receives an [`Executor`], with a timeout.
///
/// Initializes env_logger, creates a multi-threaded executor passed to `f`,
/// and panics if the returned future does not complete within `timeout_secs`.
pub fn run_test_with_executor<F, Fut>(timeout_secs: u64, f: F)
where
    F: FnOnce(Executor) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    let _ = env_logger::try_init();

    #[cfg(feature = "smol")]
    {
        let ex = Arc::new(smol::Executor::new());
        let executor: Executor = ex.clone().into();
        let (signal, shutdown) = async_channel::unbounded::<()>();

        let test_future = f(executor);

        easy_parallel::Parallel::new()
            .each(0..available_parallelism(), |_| {
                smol::future::block_on(ex.run(shutdown.recv()))
            })
            .finish(|| {
                smol::future::block_on(async {
                    timeout(Duration::from_secs(timeout_secs), test_future)
                        .await
                        .unwrap_or_else(|_| panic!("Test timed out after {timeout_secs}s"));
                    drop(signal);
                })
            });
    }

    #[cfg(feature = "tokio")]
    {
        let rt = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("build tokio runtime"),
        );
        let executor: Executor = rt.clone().into();
        let test_future = f(executor);

        rt.block_on(async {
            timeout(Duration::from_secs(timeout_secs), test_future)
                .await
                .unwrap_or_else(|_| panic!("Test timed out after {timeout_secs}s"));
        });
    }
}

/// Run an async test with a timeout.
///
/// Initializes env_logger, creates a multi-threaded executor, and panics if
/// the future does not complete within `timeout_secs`.
pub fn run_test<F: Future<Output = ()> + Send + 'static>(timeout_secs: u64, f: F) {
    run_test_with_executor(timeout_secs, |_| f)
}
