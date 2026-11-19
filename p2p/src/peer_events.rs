use std::sync::Arc;

use karyon_eventemitter::{EventEmitter, EventEmitterRef, EventListener, EventTopic, EventValue};
use karyon_net::Endpoint;

use crate::{
    PeerID,
    discovery::{DiscoveryRef, PeerConnectionEvent},
    monitor::{ConnectionKind, MonitorRef, PoolEvent},
};

/// Topic key for the peer-lifecycle event emitter.
#[derive(Hash, PartialEq, Eq, Debug, Clone)]
pub enum PeerEventTopic {
    Lifecycle,
}

/// Peer-lifecycle events. Each registered listener receives every
/// event independently.
#[derive(Debug, Clone, EventValue)]
pub enum PeerEvent {
    /// Peer added after a successful handshake.
    Added(PeerID),
    /// Previously-added peer removed.
    Removed(PeerID),
    /// Handshake failed before the peer was added.
    HandshakeFailed(Option<PeerID>),
}

impl EventTopic for PeerEvent {
    type Topic = PeerEventTopic;
    fn topic() -> Self::Topic {
        PeerEventTopic::Lifecycle
    }
}

/// Listener returned by `register_peer_events`.
pub type PeerEventListener = EventListener<PeerEventTopic, PeerEvent>;

/// `Arc` reference to `PeerEvents`.
pub(crate) type PeerEventsRef = Arc<PeerEvents>;

/// Where the PeerPool reports what happens to its peers. One call per
/// event; it goes to the monitor, the peer-event listeners and the
/// discovery.
pub(crate) struct PeerEvents {
    monitor: MonitorRef,
    discovery: DiscoveryRef,
    emitter: EventEmitterRef<PeerEventTopic>,
}

impl PeerEvents {
    pub(crate) fn new(monitor: MonitorRef, discovery: DiscoveryRef) -> PeerEventsRef {
        Arc::new(Self {
            monitor,
            discovery,
            emitter: EventEmitter::new(),
        })
    }

    /// Register a listener for the peer-lifecycle events.
    pub(crate) fn register(&self) -> PeerEventListener {
        self.emitter.register(&PeerEventTopic::Lifecycle)
    }

    pub(crate) async fn added(&self, pid: &PeerID) {
        self.monitor.notify(PoolEvent::NewPeer(pid.clone())).await;
        let _ = self.emitter.emit(&PeerEvent::Added(pid.clone())).await;
        self.discovery
            .on_event(PeerConnectionEvent::Connected(pid.clone()));
    }

    pub(crate) async fn removed(&self, pid: &PeerID) {
        self.monitor
            .notify(PoolEvent::RemovePeer(pid.clone()))
            .await;
        let _ = self.emitter.emit(&PeerEvent::Removed(pid.clone())).await;
        self.discovery
            .on_event(PeerConnectionEvent::Disconnected(pid.clone()));
    }

    pub(crate) async fn handshake_failed(&self, pid: Option<PeerID>) {
        self.monitor
            .notify(PoolEvent::HandshakeFailed(pid.clone()))
            .await;
        let _ = self
            .emitter
            .emit(&PeerEvent::HandshakeFailed(pid.clone()))
            .await;
        self.discovery
            .on_event(PeerConnectionEvent::HandshakeFailed(pid));
    }

    pub(crate) async fn already_connected(&self, pid: &PeerID) {
        self.monitor
            .notify(PoolEvent::PeerAlreadyConnected(pid.clone()))
            .await;
    }

    pub(crate) async fn disconnected(&self, endpoint: &Endpoint) {
        self.monitor
            .notify(ConnectionKind::Disconnected(endpoint.clone()))
            .await;
    }
}
