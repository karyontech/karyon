use std::{future::Future, sync::Arc, time::Duration};

#[cfg(feature = "smol")]
use std::{
    future::pending,
    num::NonZeroUsize,
    panic::{AssertUnwindSafe, catch_unwind},
    thread,
};

#[cfg(feature = "smol")]
use log::error;

use async_task::Builder;
use once_cell::sync::OnceCell;

#[cfg(feature = "smol")]
use smol::Executor as SmolEx;

#[cfg(all(feature = "tokio", target_family = "wasm"))]
use tokio::runtime::Builder as RtBuilder;
#[cfg(feature = "tokio")]
use tokio::{
    runtime::{Handle, Runtime as TokioRt},
    time::sleep as tokio_sleep,
};

#[cfg(feature = "smol")]
use crate::async_util::sleep;
use crate::{
    Error, Result,
    async_util::{Either, select},
};

use super::{Runnable, RuntimeRef, Task};

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
/// Pass `ex.clone().into()` to karyon APIs that take an [`Executor`]. A
/// user runtime (see [`Runtime`](super::Runtime)) works the same way:
/// `Executor::from(runtime_ref)`.
#[derive(Clone)]
pub struct Executor {
    inner: Inner,
}

#[derive(Clone)]
enum Inner {
    #[cfg(feature = "smol")]
    Smol(Arc<SmolEx<'static>>),
    /// A tokio runtime by handle. `_rt` keeps the runtime alive when it was
    /// passed by value or `Arc`; `None` for a borrowed `Handle`.
    #[cfg(feature = "tokio")]
    Tokio {
        handle: Handle,
        _rt: Option<Arc<TokioRt>>,
    },
    Custom(RuntimeRef),
}

impl Executor {
    pub fn spawn<T: Send + 'static>(
        &self,
        future: impl Future<Output = T> + Send + 'static,
    ) -> Task<T> {
        match &self.inner {
            #[cfg(feature = "smol")]
            Inner::Smol(ex) => ex.spawn(future).into(),
            #[cfg(feature = "tokio")]
            Inner::Tokio { handle, .. } => handle.spawn(future).into(),
            Inner::Custom(rt) => {
                let rt = rt.clone();
                // A panic in the task is raised again where it is awaited.
                let (runnable, task) = Builder::new()
                    .propagate_panic(true)
                    .spawn(|_| future, move |r| rt.schedule(Runnable::new(r)));
                runnable.schedule();
                Task::custom(task)
            }
        }
    }

    /// Waits for `duration` on this executor's timers.
    pub async fn sleep(&self, duration: Duration) {
        match &self.inner {
            Inner::Custom(rt) => rt.sleep(duration).await,
            #[cfg(feature = "smol")]
            Inner::Smol(_) => sleep(duration).await,
            #[cfg(feature = "tokio")]
            Inner::Tokio { handle, .. } => {
                // The timer registers with this runtime.
                let timer = {
                    let _guard = handle.enter();
                    tokio_sleep(duration)
                };
                timer.await
            }
        }
    }

    /// Waits for `future`, or fails with `Error::Timeout` after `delay` on
    /// this executor's timers.
    pub async fn timeout<T>(&self, delay: Duration, future: impl Future<Output = T>) -> Result<T> {
        match select(self.sleep(delay), future).await {
            Either::Left(_) => Err(Error::Timeout),
            Either::Right(res) => Ok(res),
        }
    }

    /// The tokio runtime handle; `None` on a user runtime.
    #[cfg(feature = "tokio")]
    pub fn handle(&self) -> Option<&Handle> {
        match &self.inner {
            Inner::Tokio { handle, .. } => Some(handle),
            Inner::Custom(_) => None,
        }
    }
}

impl From<RuntimeRef> for Executor {
    fn from(rt: RuntimeRef) -> Executor {
        Executor {
            inner: Inner::Custom(rt),
        }
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
///
/// On wasm it is a single-threaded tokio runtime that nothing drives, so
/// tasks spawned on it never run. Use `Executor::from(runtime)` there.
pub fn global_executor() -> Executor {
    #[cfg(feature = "smol")]
    fn init_executor() -> Executor {
        let ex = Arc::new(SmolEx::new());
        let num_threads = thread::available_parallelism()
            .map(NonZeroUsize::get)
            .unwrap_or(1);
        for i in 0..num_threads {
            let ex = ex.clone();
            thread::Builder::new()
                .name(format!("smol-executor-{i}"))
                .spawn(move || {
                    loop {
                        // A panicking task unwinds out of block_on; log it
                        // and keep the worker alive.
                        let run = AssertUnwindSafe(|| smol::block_on(ex.run(pending::<()>())));
                        if catch_unwind(run).is_err() {
                            error!("global executor worker recovered from a task panic");
                        }
                    }
                })
                .expect("cannot spawn executor thread");
        }
        // Prevent spawning another thread by running the process driver on this
        // executor. see https://github.com/smol-rs/smol/blob/master/src/spawn.rs
        ex.spawn(async_process::driver()).detach();
        ex.into()
    }

    #[cfg(feature = "tokio")]
    fn init_executor() -> Executor {
        // wasm has no threads: a single-threaded runtime there.
        #[cfg(target_family = "wasm")]
        let rt = RtBuilder::new_current_thread().enable_all().build();
        #[cfg(not(target_family = "wasm"))]
        let rt = TokioRt::new();
        Arc::new(rt.expect("cannot build tokio runtime")).into()
    }

    GLOBAL_EXECUTOR.get_or_init(init_executor).clone()
}

#[cfg(feature = "smol")]
impl From<Arc<SmolEx<'static>>> for Executor {
    fn from(ex: Arc<SmolEx<'static>>) -> Executor {
        Executor {
            inner: Inner::Smol(ex),
        }
    }
}

#[cfg(feature = "tokio")]
impl From<Arc<TokioRt>> for Executor {
    fn from(rt: Arc<TokioRt>) -> Executor {
        Executor {
            inner: Inner::Tokio {
                handle: rt.handle().clone(),
                _rt: Some(rt),
            },
        }
    }
}

/// Shares a tokio runtime the app already runs, e.g. under `#[tokio::main]`:
/// `Executor::from(Handle::current())`. The runtime must outlive karyon's use
/// of it.
#[cfg(feature = "tokio")]
impl From<Handle> for Executor {
    fn from(handle: Handle) -> Executor {
        Executor {
            inner: Inner::Tokio { handle, _rt: None },
        }
    }
}

#[cfg(feature = "smol")]
impl From<SmolEx<'static>> for Executor {
    fn from(ex: SmolEx<'static>) -> Executor {
        Arc::new(ex).into()
    }
}

#[cfg(feature = "tokio")]
impl From<TokioRt> for Executor {
    fn from(rt: TokioRt) -> Executor {
        Arc::new(rt).into()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        pin::pin,
        task::{Context, Poll, Wake, Waker},
        thread::{Thread, current, park},
        time::Instant,
    };

    use super::*;

    /// Wakes the thread that polls.
    struct Unpark(Thread);

    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    /// Polls `future` on this thread, outside any runtime.
    fn block_on_plain<T>(future: impl Future<Output = T>) -> T {
        let waker: Waker = Arc::new(Unpark(current())).into();
        let mut cx = Context::from_waker(&waker);
        let mut future = pin!(future);
        loop {
            if let Poll::Ready(out) = future.as_mut().poll(&mut cx) {
                return out;
            }
            park();
        }
    }

    #[test]
    fn builtin_timers_work_outside_the_runtime() {
        let ex = global_executor();

        let start = Instant::now();
        block_on_plain(ex.sleep(Duration::from_millis(20)));
        assert!(start.elapsed() >= Duration::from_millis(20));

        let slow = ex.sleep(Duration::from_secs(60));
        assert!(block_on_plain(ex.timeout(Duration::from_millis(10), slow)).is_err());
    }

    #[cfg(feature = "tokio")]
    #[test]
    fn tokio_handle_executor_shares_the_runtime() {
        let rt = TokioRt::new().unwrap();
        let ex = Executor::from(rt.handle().clone());
        let out = rt.block_on(async move {
            ex.sleep(Duration::from_millis(1)).await;
            ex.spawn(async { 7 }).await.unwrap()
        });
        assert_eq!(out, 7);
    }
}
