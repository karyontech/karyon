use std::{num::NonZeroUsize, sync::Arc, thread};

use easy_parallel::Parallel;
use smol::{channel, future, future::Future, Executor};

/// Returns an estimate of the default amount of parallelism a program should use.
/// see `std::thread::available_parallelism`
fn available_parallelism() -> usize {
    thread::available_parallelism()
        .map(NonZeroUsize::get)
        .unwrap_or(1)
}

/// Run a multi-threaded executor
pub fn run_executor<T>(main_future: impl Future<Output = T>, ex: Arc<Executor<'_>>) {
    let (signal, shutdown) = channel::unbounded::<()>();

    let num_threads = available_parallelism();

    Parallel::new()
        .each(0..(num_threads), |_| {
            future::block_on(ex.run(shutdown.recv()))
        })
        // Run the main future on the current thread.
        .finish(|| {
            future::block_on(async {
                main_future.await;
                drop(signal);
            })
        });
}
