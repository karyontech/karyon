use std::{collections::HashSet, sync::Arc};

use karyon_net::Endpoint;

use crate::{conn_direction::ConnDirection, peer::PeerID, protocol::ProtocolID};

/// `Arc` reference to a `PeerInfo`.
pub type PeerInfoRef = Arc<PeerInfo>;

/// What is known about a connected peer once the handshake is done.
/// Never changes; shared by the pool's session, the peer and every
/// protocol's `PeerConn`.
#[derive(Debug)]
pub struct PeerInfo {
    /// The remote peer's id.
    pub id: PeerID,
    /// Whether the remote dialed us or we dialed it.
    pub direction: ConnDirection,
    /// The remote peer's endpoint.
    pub endpoint: Endpoint,
    /// The protocols both sides agreed on in the handshake.
    pub negotiated: HashSet<ProtocolID>,
}

impl PeerInfo {
    /// True if the remote dialed us.
    pub fn is_inbound(&self) -> bool {
        matches!(self.direction, ConnDirection::Inbound)
    }
}
