/// A set of helper tools and functions.
pub mod util;

/// A module containing async utilities that work with the
/// [`smol`](https://github.com/smol-rs/smol) async runtime.
pub mod async_util;

/// Represents karyons's Core Error.
pub mod error;

/// [`event::EventSys`] implementation.
pub mod event;

/// A simple publish-subscribe system [`Read More`](./pubsub/struct.Publisher.html)
pub mod pubsub;

/// A cryptographic key pair
pub mod key_pair;

use smol::Executor as SmolEx;
use std::sync::Arc;

/// A pointer to an Executor
pub type Executor<'a> = Arc<SmolEx<'a>>;

/// A Global Executor
pub type GlobalExecutor = Arc<SmolEx<'static>>;

use error::Result;
