use karyon_net::Endpoint;

use crate::PeerID;

/// Wire-level connection events.
#[derive(Clone, Debug)]
pub enum ConnectionKind {
    Connected(Endpoint),
    ConnectRetried(Endpoint),
    ConnectFailed(Endpoint),
    Accepted(Endpoint),
    AcceptFailed,
    Disconnected(Endpoint),
    Listening(Endpoint),
    ListenFailed(Endpoint),
}

/// Peer-pool lifecycle events.
#[derive(Clone, Debug)]
pub enum PoolEvent {
    NewPeer(PeerID),
    RemovePeer(PeerID),
    HandshakeFailed(Option<PeerID>),
    /// Outbound dial failed. `peer_id` may be `None` for manual peer
    /// endpoints whose id wasn't pre-shared.
    ConnectFailed(Option<PeerID>, Endpoint),
    /// Inbound or outbound connection rejected because a peer with
    /// the same id is already connected.
    PeerAlreadyConnected(PeerID),
}

/// Discovery (Kademlia) events.
#[derive(Clone, Debug)]
pub enum DiscoveryKind {
    LookupStarted(Endpoint),
    LookupFailed(Endpoint),
    LookupSucceeded(Endpoint, usize),
    RefreshStarted,
    /// Refresh sweep completed. Carries the number of entries pinged.
    RefreshSucceeded(usize),
    RefreshFailed,
    /// Routing-table entry evicted after exceeding the failure budget.
    EntryEvicted(PeerID),
}

impl ConnectionKind {
    pub(super) fn get_endpoint(&self) -> Option<&Endpoint> {
        match self {
            ConnectionKind::Connected(endpoint)
            | ConnectionKind::ConnectRetried(endpoint)
            | ConnectionKind::ConnectFailed(endpoint)
            | ConnectionKind::Accepted(endpoint)
            | ConnectionKind::Disconnected(endpoint)
            | ConnectionKind::Listening(endpoint)
            | ConnectionKind::ListenFailed(endpoint) => Some(endpoint),
            ConnectionKind::AcceptFailed => None,
        }
    }

    pub(super) fn variant_name(&self) -> &'static str {
        match self {
            ConnectionKind::Connected(_) => "Connected",
            ConnectionKind::ConnectRetried(_) => "ConnectRetried",
            ConnectionKind::ConnectFailed(_) => "ConnectFailed",
            ConnectionKind::Accepted(_) => "Accepted",
            ConnectionKind::AcceptFailed => "AcceptFailed",
            ConnectionKind::Disconnected(_) => "Disconnected",
            ConnectionKind::Listening(_) => "Listening",
            ConnectionKind::ListenFailed(_) => "ListenFailed",
        }
    }
}

impl PoolEvent {
    pub(super) fn get_peer_id(&self) -> Option<&PeerID> {
        match self {
            PoolEvent::NewPeer(peer_id)
            | PoolEvent::RemovePeer(peer_id)
            | PoolEvent::PeerAlreadyConnected(peer_id) => Some(peer_id),
            PoolEvent::HandshakeFailed(peer_id) | PoolEvent::ConnectFailed(peer_id, _) => {
                peer_id.as_ref()
            }
        }
    }

    pub(super) fn get_endpoint(&self) -> Option<&Endpoint> {
        match self {
            PoolEvent::ConnectFailed(_, endpoint) => Some(endpoint),
            _ => None,
        }
    }

    pub(super) fn variant_name(&self) -> &'static str {
        match self {
            PoolEvent::NewPeer(_) => "NewPeer",
            PoolEvent::RemovePeer(_) => "RemovePeer",
            PoolEvent::HandshakeFailed(_) => "HandshakeFailed",
            PoolEvent::ConnectFailed(_, _) => "ConnectFailed",
            PoolEvent::PeerAlreadyConnected(_) => "PeerAlreadyConnected",
        }
    }
}

impl DiscoveryKind {
    pub(super) fn get_endpoint(&self) -> Option<&Endpoint> {
        match self {
            DiscoveryKind::LookupStarted(endpoint)
            | DiscoveryKind::LookupFailed(endpoint)
            | DiscoveryKind::LookupSucceeded(endpoint, _) => Some(endpoint),
            _ => None,
        }
    }

    pub(super) fn get_size(&self) -> Option<usize> {
        match self {
            DiscoveryKind::LookupSucceeded(_, size) | DiscoveryKind::RefreshSucceeded(size) => {
                Some(*size)
            }
            _ => None,
        }
    }

    pub(super) fn get_peer_id(&self) -> Option<&PeerID> {
        match self {
            DiscoveryKind::EntryEvicted(peer_id) => Some(peer_id),
            _ => None,
        }
    }

    pub(super) fn variant_name(&self) -> &'static str {
        match self {
            DiscoveryKind::LookupStarted(_) => "LookupStarted",
            DiscoveryKind::LookupFailed(_) => "LookupFailed",
            DiscoveryKind::LookupSucceeded(_, _) => "LookupSucceeded",
            DiscoveryKind::RefreshStarted => "RefreshStarted",
            DiscoveryKind::RefreshSucceeded(_) => "RefreshSucceeded",
            DiscoveryKind::RefreshFailed => "RefreshFailed",
            DiscoveryKind::EntryEvicted(_) => "EntryEvicted",
        }
    }
}
