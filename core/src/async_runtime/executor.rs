use std::{future::Future, panic::catch_unwind, sync::Arc, thread};

use once_cell::sync::OnceCell;

#[cfg(feature = "smol")]
pub use smol::Executor as SmolEx;

#[cfg(feature = "tokio")]
pub use tokio::runtime::Runtime;

use super::Task;

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

/// Returns a single-threaded global executor
pub fn global_executor() -> Executor {
    #[cfg(feature = "smol")]
    fn init_executor() -> Executor {
        let ex = smol::Executor::new();
        thread::Builder::new()
            .name("smol-executor".to_string())
            .spawn(|| loop {
                catch_unwind(|| {
                    smol::block_on(global_executor().inner.run(std::future::pending::<()>()))
                })
                .ok();
            })
            .expect("cannot spawn executor thread");
        // Prevent spawning another thread by running the process driver on this
        // thread. see https://github.com/smol-rs/smol/blob/master/src/spawn.rs
        ex.spawn(async_process::driver()).detach();
        Executor {
            inner: Arc::new(ex),
        }
    }

    #[cfg(feature = "tokio")]
    fn init_executor() -> Executor {
        let ex = Arc::new(tokio::runtime::Runtime::new().expect("cannot build tokio runtime"));
        thread::Builder::new()
            .name("tokio-executor".to_string())
            .spawn({
                let ex = ex.clone();
                move || {
                    catch_unwind(|| ex.block_on(std::future::pending::<()>())).ok();
                }
            })
            .expect("cannot spawn tokio runtime thread");
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
