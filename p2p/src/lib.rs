#![doc = include_str!("../README.md")]

mod access_control;
mod codec;
mod config;
mod conn_direction;
mod conn_queue;
mod connector;
mod discovery;
mod error;
mod handshake;
mod listener;
mod message;
mod node;
mod peer;
mod peer_pool;
mod protocols;
mod tls_config;
mod version;

/// Bincode encode/decode helpers.
pub mod util;

/// Responsible for network and system monitoring.
/// [`Read More`](./monitor/struct.Monitor.html)
pub mod monitor;
/// Defines the protocol trait.
/// [`Read More`](./protocol/trait.Protocol.html)
pub mod protocol;

pub use access_control::{AccessControl, Action, AllowAll, PeerCandidate, Subject};
pub use config::Config;
pub use conn_direction::ConnDirection;
pub use discovery::{DiscoveredPeer, Discovery, DiscoveryContext, PeerConnectionEvent};
pub use message::{PeerAddr, Protocol};

// Building blocks for discovery crates.
pub use connector::Connector;
pub use listener::Listener;
pub use message::{ShutdownMsg, pick_endpoint};
pub use node::Node;
pub use peer::{Peer, PeerID};
pub use peer_pool::{PeerEvent, PeerEventListener, PeerPool};
pub use version::Version;
pub use version::{VersionInt, version_match};

pub mod endpoint {
    pub use karyon_net::{Addr, Endpoint, Port};
}

pub mod keypair {
    pub use karyon_core::crypto::{KeyPair, KeyPairType, PublicKey, SecretKey};
}

pub use error::{Error, Result};
