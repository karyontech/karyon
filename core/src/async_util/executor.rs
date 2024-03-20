use std::{panic::catch_unwind, sync::Arc, thread};

use async_lock::OnceCell;
use smol::Executor as SmolEx;

static GLOBAL_EXECUTOR: OnceCell<Arc<smol::Executor<'_>>> = OnceCell::new();

/// A pointer to an Executor
pub type Executor<'a> = Arc<SmolEx<'a>>;

/// Returns a single-threaded global executor
pub(crate) fn global_executor() -> Executor<'static> {
    fn init_executor() -> Executor<'static> {
        let ex = smol::Executor::new();
        thread::Builder::new()
            .spawn(|| loop {
                catch_unwind(|| {
                    smol::block_on(global_executor().run(smol::future::pending::<()>()))
                })
                .ok();
            })
            .expect("cannot spawn executor thread");
        // Prevent spawning another thread by running the process driver on this
        // thread. see https://github.com/smol-rs/smol/blob/master/src/spawn.rs
        ex.spawn(async_process::driver()).detach();
        Arc::new(ex)
    }

    GLOBAL_EXECUTOR.get_or_init_blocking(init_executor).clone()
}
