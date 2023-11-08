/// A set of helper tools and functions.
pub mod utils;

/// A module containing async utilities that work with the `smol` async runtime.
pub mod async_utils;

/// Represents Karyons's Core Error.
pub mod error;

/// [`EventSys`](./event/struct.EventSys.html) Implementation
pub mod event;

/// A simple publish-subscribe system.[`Read More`](./pubsub/struct.Publisher.html)
pub mod pubsub;

use error::Result;
use smol::Executor as SmolEx;
use std::sync::Arc;

/// A wrapper for smol::Executor
pub type Executor<'a> = Arc<SmolEx<'a>>;
