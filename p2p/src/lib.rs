//! A lightweight, extensible, and customizable peer-to-peer (p2p) network stack.
//!
//! # Example
//! ```
//! use std::sync::Arc;
//!
//! use easy_parallel::Parallel;
//! use smol::{channel as smol_channel, future, Executor};
//!
//! use karyons_p2p::{Backend, Config, PeerID};
//!
//! let peer_id = PeerID::random();
//!
//! // Create the configuration for the backend.
//! let mut config = Config::default();
//!
//!
//! // Create a new Executor
//! let ex = Arc::new(Executor::new());
//!
//! // Create a new Backend
//! let backend = Backend::new(peer_id, config, ex.clone());
//!
//! let task = async {
//!     // Run the backend
//!     backend.run().await.unwrap();
//!
//!     // ....
//!
//!     // Shutdown the backend
//!     backend.shutdown().await;
//! };
//!
//! future::block_on(ex.run(task));
//!
//! ```
//!
mod backend;
mod config;
mod connection;
mod connector;
mod discovery;
mod error;
mod io_codec;
mod listener;
mod message;
mod peer;
mod peer_pool;
mod protocols;
mod routing_table;
mod slots;
mod utils;

/// Responsible for network and system monitoring.
/// [`Read More`](./monitor/struct.Monitor.html)
pub mod monitor;
/// Defines the protocol trait.
/// [`Read More`](./protocol/trait.Protocol.html)
pub mod protocol;

pub use backend::{ArcBackend, Backend};
pub use config::Config;
pub use error::Error as P2pError;
pub use peer::{ArcPeer, PeerID};
pub use utils::Version;

use error::{Error, Result};
