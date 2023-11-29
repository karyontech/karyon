//! A lightweight, extensible, and customizable peer-to-peer (p2p) network stack.
//!
//! # Example
//! ```
//! use std::sync::Arc;
//!
//! use easy_parallel::Parallel;
//! use smol::{channel as smol_channel, future, Executor};
//!
//! use karyons_core::crypto::{KeyPair, KeyPairType};
//! use karyons_p2p::{Backend, Config, PeerID};
//!
//! let key_pair = KeyPair::generate(&KeyPairType::Ed25519);
//!
//! // Create the configuration for the backend.
//! let mut config = Config::default();
//!
//! // Create a new Executor
//! let ex = Arc::new(Executor::new());
//!
//! // Create a new Backend
//! let backend = Backend::new(&key_pair, config, ex.clone());
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
mod codec;
mod config;
mod connection;
mod connector;
mod discovery;
mod error;
mod listener;
mod message;
mod peer;
mod peer_pool;
mod protocols;
mod routing_table;
mod slots;
mod tls_config;
mod version;

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
pub use version::Version;

use error::{Error, Result};
