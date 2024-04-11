#[cfg(all(feature = "smol", feature = "tokio"))]
compile_error!("Only one async runtime feature should be enabled");

#[cfg(not(any(feature = "smol", feature = "tokio")))]
compile_error!("At least one async runtime feature must be enabled for this crate.");

/// A set of helper tools and functions.
pub mod util;

/// A set of async utilities.
pub mod async_util;

/// Represents karyon's Core Error.
pub mod error;

/// [`event::EventSys`] implementation.
pub mod event;

/// A simple publish-subscribe system [`Read More`](./pubsub/struct.Publisher.html)
pub mod pubsub;

/// A cross-compatible async runtime
pub mod async_runtime;

#[cfg(feature = "crypto")]

/// Collects common cryptographic tools
pub mod crypto;

pub use error::{Error, Result};
