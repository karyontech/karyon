/// A set of helper tools and functions.
pub mod utils;

/// A module containing async utilities that work with the `smol` async runtime.
pub mod async_utils;

/// Represents karyons's Core Error.
pub mod error;

/// [`EventSys`](./event/struct.EventSys.html) Implementation
pub mod event;

/// A simple publish-subscribe system.[`Read More`](./pubsub/struct.Publisher.html)
pub mod pubsub;

use smol::Executor as SmolEx;
use std::sync::Arc;

/// A pointer to an Executor
pub type Executor<'a> = Arc<SmolEx<'a>>;

/// A Global Executor
pub type GlobalExecutor = Arc<SmolEx<'static>>;

use error::Result;
