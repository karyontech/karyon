use std::{collections::HashSet, sync::Arc};

use karyon_net::Endpoint;

use crate::{conn_direction::ConnDirection, peer::PeerID, protocol::ProtocolID};

/// `Arc` reference to a `PeerInfo`.
pub(crate) type PeerInfoRef = Arc<PeerInfo>;

/// What is known about a connected peer once the handshake is done.
/// Never changes; shared by the pool's session, the peer and every
/// protocol handle.
pub(crate) struct PeerInfo {
    pub(crate) id: PeerID,
    pub(crate) direction: ConnDirection,
    pub(crate) endpoint: Endpoint,
    pub(crate) negotiated: HashSet<ProtocolID>,
}

impl PeerInfo {
    pub(crate) fn is_inbound(&self) -> bool {
        matches!(self.direction, ConnDirection::Inbound)
    }
}
