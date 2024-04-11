#[cfg(feature = "smol")]
pub use smol::lock::{Mutex, MutexGuard, OnceCell, RwLock};

#[cfg(feature = "tokio")]
pub use tokio::sync::{Mutex, MutexGuard, OnceCell, RwLock};
