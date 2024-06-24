use karyon_net::Endpoint;

use crate::PeerID;

/// Defines connection-related events.
#[derive(Clone, Debug)]
pub enum ConnEvent {
    Connected(Endpoint),
    ConnectRetried(Endpoint),
    ConnectFailed(Endpoint),
    Accepted(Endpoint),
    AcceptFailed,
    Disconnected(Endpoint),
    Listening(Endpoint),
    ListenFailed(Endpoint),
}

/// Defines `PP` events.
#[derive(Clone, Debug)]
pub enum PPEvent {
    NewPeer(PeerID),
    RemovePeer(PeerID),
}

/// Defines `Discovery` events.
#[derive(Clone, Debug)]
pub enum DiscvEvent {
    LookupStarted(Endpoint),
    LookupFailed(Endpoint),
    LookupSucceeded(Endpoint, usize),
    RefreshStarted,
}

impl ConnEvent {
    pub(super) fn get_endpoint(&self) -> Option<&Endpoint> {
        match self {
            ConnEvent::Connected(endpoint)
            | ConnEvent::ConnectRetried(endpoint)
            | ConnEvent::ConnectFailed(endpoint)
            | ConnEvent::Accepted(endpoint)
            | ConnEvent::Disconnected(endpoint)
            | ConnEvent::Listening(endpoint)
            | ConnEvent::ListenFailed(endpoint) => Some(endpoint),
            ConnEvent::AcceptFailed => None,
        }
    }

    pub(super) fn variant_name(&self) -> &'static str {
        match self {
            ConnEvent::Connected(_) => "Connected",
            ConnEvent::ConnectRetried(_) => "ConnectRetried",
            ConnEvent::ConnectFailed(_) => "ConnectFailed",
            ConnEvent::Accepted(_) => "Accepted",
            ConnEvent::AcceptFailed => "AcceptFailed",
            ConnEvent::Disconnected(_) => "Disconnected",
            ConnEvent::Listening(_) => "Listening",
            ConnEvent::ListenFailed(_) => "ListenFailed",
        }
    }
}

impl PPEvent {
    pub(super) fn get_peer_id(&self) -> Option<&PeerID> {
        match self {
            PPEvent::NewPeer(peer_id) | PPEvent::RemovePeer(peer_id) => Some(peer_id),
        }
    }
    pub(super) fn variant_name(&self) -> &'static str {
        match self {
            PPEvent::NewPeer(_) => "NewPeer",
            PPEvent::RemovePeer(_) => "RemovePeer",
        }
    }
}

impl DiscvEvent {
    pub(super) fn get_endpoint_and_size(&self) -> (Option<&Endpoint>, Option<usize>) {
        match self {
            DiscvEvent::LookupStarted(endpoint) | DiscvEvent::LookupFailed(endpoint) => {
                (Some(endpoint), None)
            }
            DiscvEvent::LookupSucceeded(endpoint, size) => (Some(endpoint), Some(*size)),
            DiscvEvent::RefreshStarted => (None, None),
        }
    }

    pub(super) fn variant_name(&self) -> &'static str {
        match self {
            DiscvEvent::LookupStarted(_) => "LookupStarted",
            DiscvEvent::LookupFailed(_) => "LookupFailed",
            DiscvEvent::LookupSucceeded(_, _) => "LookupSucceeded",
            DiscvEvent::RefreshStarted => "RefreshStarted",
        }
    }
}
