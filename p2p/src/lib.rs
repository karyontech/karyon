//! A lightweight, extensible, and customizable peer-to-peer (p2p) network stack.
//!
//! # Example
//! ```
//! use std::sync::Arc;
//!
//! use easy_parallel::Parallel;
//! use smol::{future, Executor};
//!
//! use karyon_p2p::{Backend, Config, PeerID, keypair::{KeyPair, KeyPairType}};
//!
//! // Generate a new keypair for the peer
//! let key_pair = KeyPair::generate(&KeyPairType::Ed25519);
//!
//! // Create the configuration for the backend.
//! let mut config = Config::default();
//!
//! // Create a new Executor
//! let ex = Arc::new(Executor::new());
//!
//! // Create a new Backend
//! let backend = Backend::new(&key_pair, config, ex.clone().into());
//!
//! let task = async {
//!     // Run the backend
//!     backend.run()
//!         .await
//!         .expect("start the backend");
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
mod conn_queue;
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

pub use backend::Backend;
pub use config::Config;
pub use peer::{Peer, PeerID};
pub use version::Version;

pub mod endpoint {
    pub use karyon_net::{Addr, Endpoint, Port};
}

pub mod keypair {
    pub use karyon_core::crypto::{KeyPair, KeyPairType, PublicKey, SecretKey};
}

pub use error::{Error, Result};
