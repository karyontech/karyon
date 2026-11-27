use karyon_net::Endpoint;

use karyon_p2p::{
    PeerID,
    monitor::{DiscoveryEvent, ToEvent},
};

/// Kademlia events, posted to the node's monitor as `DiscoveryEvent`.
#[derive(Clone, Debug)]
pub(crate) enum DiscoveryKind {
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

impl DiscoveryKind {
    fn get_endpoint(&self) -> Option<&Endpoint> {
        match self {
            DiscoveryKind::LookupStarted(endpoint)
            | DiscoveryKind::LookupFailed(endpoint)
            | DiscoveryKind::LookupSucceeded(endpoint, _) => Some(endpoint),
            _ => None,
        }
    }

    fn get_size(&self) -> Option<usize> {
        match self {
            DiscoveryKind::LookupSucceeded(_, size) | DiscoveryKind::RefreshSucceeded(size) => {
                Some(*size)
            }
            _ => None,
        }
    }

    fn get_peer_id(&self) -> Option<&PeerID> {
        match self {
            DiscoveryKind::EntryEvicted(peer_id) => Some(peer_id),
            _ => None,
        }
    }

    fn variant_name(&self) -> &'static str {
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

impl ToEvent for DiscoveryKind {
    type Event = DiscoveryEvent;
}

impl From<DiscoveryKind> for DiscoveryEvent {
    fn from(event: DiscoveryKind) -> Self {
        Self {
            endpoint: event.get_endpoint().cloned(),
            size: event.get_size(),
            peer_id: event.get_peer_id().cloned(),
            event: event.variant_name().to_string(),
            date: crate::unix_timestamp(),
        }
    }
}
