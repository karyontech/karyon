use std::{future::Future, sync::Arc};

#[cfg(feature = "smol")]
use std::{
    future::pending,
    num::NonZeroUsize,
    panic::{catch_unwind, AssertUnwindSafe},
    thread,
};

#[cfg(feature = "smol")]
use log::error;

use once_cell::sync::OnceCell;

#[cfg(feature = "smol")]
pub use smol::Executor as SmolEx;

#[cfg(feature = "tokio")]
pub use tokio::runtime::Runtime;

use super::Task;

/// A handle to a multi-threaded async executor.
///
/// karyon does not run the executor for you. The caller owns the runtime and
/// is responsible for driving it. On `tokio` this is just a `Runtime`. On
/// `smol` you must spin up worker threads yourself, e.g. via
/// [`easy_parallel`](https://docs.rs/easy-parallel):
///
/// ```ignore
/// use std::sync::Arc;
/// use async_channel::unbounded;
/// use easy_parallel::Parallel;
/// use smol::{future, Executor as SmolEx};
///
/// let ex = Arc::new(SmolEx::new());
/// let (signal, shutdown) = unbounded::<()>();
///
/// let num_threads = std::thread::available_parallelism()
///     .map(|n| n.get())
///     .unwrap_or(1);
///
/// Parallel::new()
///     .each(0..num_threads, |_| future::block_on(ex.run(shutdown.recv())))
///     .finish(|| future::block_on(async {
///         // your async main here
///         drop(signal);
///     }));
/// ```
///
/// Pass `ex.clone().into()` to karyon APIs that take an [`Executor`].
#[derive(Clone)]
pub struct Executor {
    #[cfg(feature = "smol")]
    inner: Arc<SmolEx<'static>>,
    #[cfg(feature = "tokio")]
    inner: Arc<Runtime>,
}

impl Executor {
    pub fn spawn<T: Send + 'static>(
        &self,
        future: impl Future<Output = T> + Send + 'static,
    ) -> Task<T> {
        self.inner.spawn(future).into()
    }

    #[cfg(feature = "tokio")]
    pub fn handle(&self) -> &tokio::runtime::Handle {
        self.inner.handle()
    }
}

static GLOBAL_EXECUTOR: OnceCell<Executor> = OnceCell::new();

/// Returns the process-wide global executor. Multi-threaded on both
/// runtimes: `smol` runs one worker thread per core, `tokio` uses its
/// own worker threads.
///
/// Note: this is the convenience path. Resource-conscious users should
/// create and drive their own executor and pass it to karyon APIs; no
/// worker threads are spawned until the first call to this function.
pub fn global_executor() -> Executor {
    #[cfg(feature = "smol")]
    fn init_executor() -> Executor {
        let ex = Arc::new(smol::Executor::new());
        let num_threads = thread::available_parallelism()
            .map(NonZeroUsize::get)
            .unwrap_or(1);
        for i in 0..num_threads {
            let ex = ex.clone();
            thread::Builder::new()
                .name(format!("smol-executor-{i}"))
                .spawn(move || loop {
                    // A panicking task unwinds out of block_on; log it
                    // and keep the worker alive.
                    let run = AssertUnwindSafe(|| smol::block_on(ex.run(pending::<()>())));
                    if catch_unwind(run).is_err() {
                        error!("global executor worker recovered from a task panic");
                    }
                })
                .expect("cannot spawn executor thread");
        }
        // Prevent spawning another thread by running the process driver on this
        // executor. see https://github.com/smol-rs/smol/blob/master/src/spawn.rs
        ex.spawn(async_process::driver()).detach();
        Executor { inner: ex }
    }

    #[cfg(feature = "tokio")]
    fn init_executor() -> Executor {
        let ex = Arc::new(tokio::runtime::Runtime::new().expect("cannot build tokio runtime"));
        Executor { inner: ex }
    }

    GLOBAL_EXECUTOR.get_or_init(init_executor).clone()
}

#[cfg(feature = "smol")]
impl From<Arc<smol::Executor<'static>>> for Executor {
    fn from(ex: Arc<smol::Executor<'static>>) -> Executor {
        Executor { inner: ex }
    }
}

#[cfg(feature = "tokio")]
impl From<Arc<tokio::runtime::Runtime>> for Executor {
    fn from(rt: Arc<tokio::runtime::Runtime>) -> Executor {
        Executor { inner: rt }
    }
}

#[cfg(feature = "smol")]
impl From<smol::Executor<'static>> for Executor {
    fn from(ex: smol::Executor<'static>) -> Executor {
        Executor {
            inner: Arc::new(ex),
        }
    }
}

#[cfg(feature = "tokio")]
impl From<tokio::runtime::Runtime> for Executor {
    fn from(rt: tokio::runtime::Runtime) -> Executor {
        Executor {
            inner: Arc::new(rt),
        }
    }
}
