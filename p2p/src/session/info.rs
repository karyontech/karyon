use std::{collections::HashSet, sync::Arc};

use karyon_net::Endpoint;

use crate::{PeerID, conn_direction::ConnDirection, protocol::ProtocolID};

/// `Arc` reference to a `SessionInfo`.
pub type SessionInfoRef = Arc<SessionInfo>;

/// What is known about one session with a peer, from the handshake on.
/// Never changes; a peer that reconnects gets a new one. Shared by the
/// session and every protocol's `PeerConn`.
#[derive(Debug)]
pub struct SessionInfo {
    /// The remote peer's id.
    pub id: PeerID,
    /// Whether the remote dialed us or we dialed it.
    pub direction: ConnDirection,
    /// The remote peer's endpoint.
    pub endpoint: Endpoint,
    /// The protocols both sides agreed on in the handshake.
    pub negotiated: HashSet<ProtocolID>,
}

impl SessionInfo {
    /// True if the remote dialed us.
    pub fn is_inbound(&self) -> bool {
        matches!(self.direction, ConnDirection::Inbound)
    }
}
