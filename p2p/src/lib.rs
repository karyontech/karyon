#![doc = include_str!("../README.md")]

mod access_control;
mod admission;
mod codec;
mod config;
mod conn_direction;
mod connector;
mod discovery;
mod error;
mod handshake;
mod listener;
mod message;
mod node;
mod peer;
mod peer_events;
mod peer_pool;
mod protocols;
mod queued_conn;
mod session;
mod sessions;
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

pub use access_control::{
    AccessControl, AccessControlRef, Action, AllowAll, PeerCandidate, Subject,
};
pub use config::{Config, ConfigRef};
pub use conn_direction::ConnDirection;
pub use discovery::{
    DiscoveredPeer, Discovery, DiscoveryContext, DiscoveryRef, PeerConnectionEvent,
};
pub use message::{PeerAddr, Protocol};

// Building blocks for discovery crates.
pub use connector::{Connector, ConnectorRef};
pub use listener::{Listener, ListenerRef};
pub use message::{ShutdownMsg, pick_endpoint};
pub use node::{Node, NodeRef};
pub use peer::PeerID;
pub use peer_events::{PeerEvent, PeerEventListener};
pub use session::{SessionInfo, SessionInfoRef};
pub use version::Version;
pub use version::{VersionInt, version_match};

pub mod endpoint {
    pub use karyon_net::{Addr, Endpoint, Port};
}

pub mod keypair {
    pub use karyon_core::crypto::{KeyPair, KeyPairType, PublicKey, SecretKey};
}

pub use error::{Error, Result};
