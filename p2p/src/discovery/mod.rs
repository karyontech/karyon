pub mod kademlia;

use std::sync::Arc;

use async_trait::async_trait;

use crate::{message::PeerAddr, PeerID, Result};

/// Represents a discovered peer that the discovery protocol wants the
/// Node to connect to.
pub struct DiscoveredPeer {
    pub peer_id: Option<PeerID>,
    pub addrs: Vec<PeerAddr>,
    pub discovery_addrs: Vec<PeerAddr>,
}

/// Connection lifecycle events the Node forwards to discovery.
#[derive(Debug, Clone)]
pub enum PeerConnectionEvent {
    /// Outbound connection completed handshake successfully.
    Connected(PeerID),
    /// Previously-connected peer has disconnected.
    Disconnected(PeerID),
    /// Connection attempt failed. Peer id may be unknown for
    /// manual peer endpoints whose id wasn't pre-shared.
    ConnectFailed(Option<PeerID>),
}

/// Trait that any discovery protocol must implement.
///
/// Discovery implementations find peers and yield them via
/// `recv()`. The Node awaits `recv()` in a loop and handles
/// connecting - discovery never connects directly.
#[async_trait]
pub trait Discovery: Send + Sync {
    /// Start the discovery service. Listen endpoints, if needed by
    /// the implementation, must be provided at construction time.
    async fn start(self: Arc<Self>) -> Result<()>;

    /// Shutdown the discovery service.
    async fn shutdown(&self);

    /// Await the next discovered peer. The Node calls this in a
    /// loop; on shutdown the surrounding task is cancelled.
    async fn recv(&self) -> DiscoveredPeer;

    /// Receive a peer connection event from the Node.
    fn on_event(&self, event: PeerConnectionEvent);

    /// Return peers in the routing table whose advertised bloom may
    /// contain `item`. Used by application layers (e.g. Swarm) to find
    /// candidates for protocol- or swarm-targeted dials. Implementations
    /// without a routing table may return an empty vector.
    fn find_peers_with(&self, item: &[u8]) -> Vec<DiscoveredPeer>;
}
