mod executor;
pub mod io;
pub mod lock;
#[cfg(not(target_family = "wasm"))]
pub mod net;
mod runtime;
mod spawn;
mod task;

pub use executor::{Executor, global_executor};
pub use runtime::{Runnable, Runtime, RuntimeRef, SleepFuture};
pub use spawn::spawn;
pub use task::Task;

#[cfg(test)]
pub fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    #[cfg(feature = "smol")]
    let result = smol::block_on(future);
    #[cfg(feature = "tokio")]
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future);

    result
}
