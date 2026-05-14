#![doc = include_str!("../README.md")]

mod bloom;
mod codec;
mod config;
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
mod slots;
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

pub use bloom::{Bloom, BloomRef};
pub use config::Config;
pub use discovery::{kademlia::KademliaDiscovery, DiscoveredPeer, Discovery};
pub use message::{PeerAddr, Protocol};
pub use node::Node;
pub use peer::{Peer, PeerID};
pub use peer_pool::{PeerEvent, PeerPool};
pub use version::Version;

pub mod endpoint {
    pub use karyon_net::{Addr, Endpoint, Port};
}

pub mod keypair {
    pub use karyon_core::crypto::{KeyPair, KeyPairType, PublicKey, SecretKey};
}

pub use error::{Error, Result};
