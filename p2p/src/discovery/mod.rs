use std::{future::pending, sync::Arc};

use async_trait::async_trait;

use karyon_core::{async_runtime::Executor, crypto::KeyPair};

use crate::{
    PeerID, Result, config::ConfigRef, message::PeerAddr, monitor::MonitorRef,
    protocol::ProtocolFlags,
};

/// What a discovery gets from the node that runs it. Passed to the
/// constructor given to `Node::with_discovery`.
pub struct DiscoveryContext {
    pub key_pair: KeyPair,
    pub peer_id: PeerID,
    pub config: ConfigRef,
    pub monitor: MonitorRef,
    pub executor: Executor,
}

/// `Arc` reference to a `Discovery`.
pub type DiscoveryRef = Arc<dyn Discovery>;

/// A peer the discovery wants the Node to connect to.
pub struct DiscoveredPeer {
    /// `None` if the id is not known before the handshake.
    pub peer_id: Option<PeerID>,
    /// Addresses the peer accepts peer connections on.
    pub addrs: Vec<PeerAddr>,
}

/// Connection lifecycle events the Node forwards to discovery.
#[derive(Debug, Clone)]
pub enum PeerConnectionEvent {
    /// A peer joined the pool, inbound or outbound.
    Connected(PeerID),
    /// A peer left the pool.
    Disconnected(PeerID),
    /// The Node could not dial a peer. The id is `None` for a
    /// `dial_peers` entry whose id is not known.
    DialFailed(Option<PeerID>),
    /// A connection, inbound or outbound, failed the handshake or was
    /// denied by `AccessControl`. The id is `None` if not verified yet.
    HandshakeFailed(Option<PeerID>),
}

/// Trait that any discovery protocol must implement.
///
/// Discovery implementations find peers and yield them via
/// `recv()`. The Node awaits `recv()` in a loop and handles
/// connecting - discovery never connects directly. The Node dials
/// `Config::dial_peers` itself. Only `start` and `recv` are required.
#[async_trait]
pub trait Discovery: Send + Sync {
    /// Start the discovery service. Listen endpoints, if needed by
    /// the implementation, must be provided at construction time.
    async fn start(self: Arc<Self>) -> Result<()>;

    /// Shutdown the discovery service.
    async fn shutdown(&self) {}

    /// Await the next discovered peer. The Node calls this in a
    /// loop; on shutdown the surrounding task is cancelled.
    async fn recv(&self) -> DiscoveredPeer;

    /// Receive a peer connection event from the Node.
    fn on_event(&self, _event: PeerConnectionEvent) {}

    /// Advertise an item (protocol id, swarm key, ...) to other peers.
    /// `REQUIRED` items are required from peers, `PREFERRED` ones are
    /// preferred but not required. Other bits are implementation
    /// defined.
    fn advertise(&self, _item: &[u8], _flags: ProtocolFlags) {}
}

/// Default discovery of `Node::new`: finds no peers.
pub(crate) struct NoDiscovery;

#[async_trait]
impl Discovery for NoDiscovery {
    async fn start(self: Arc<Self>) -> Result<()> {
        Ok(())
    }

    async fn recv(&self) -> DiscoveredPeer {
        pending().await
    }
}
