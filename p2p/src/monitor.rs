use std::fmt;

use crate::PeerID;

use karyons_core::pubsub::{ArcPublisher, Publisher, Subscription};

use karyons_net::Endpoint;

/// Responsible for network and system monitoring.
///
/// It use pub-sub pattern to notify the subscribers with new events.
///
/// # Example
///
/// ```
/// use std::sync::Arc;
///
/// use smol::Executor;
///
/// use karyons_core::key_pair::{KeyPair, KeyPairType};
/// use karyons_p2p::{Config, Backend, PeerID};
///
/// async {
///     
///     // Create a new Executor
///     let ex = Arc::new(Executor::new());
///
///     let key_pair = KeyPair::generate(&KeyPairType::Ed25519);
///     let backend = Backend::new(&key_pair, Config::default(), ex);
///
///     // Create a new Subscription
///     let sub =  backend.monitor().await;
///     
///     let event = sub.recv().await;
/// };
/// ```
pub struct Monitor {
    inner: ArcPublisher<MonitorEvent>,
}

impl Monitor {
    /// Creates a new Monitor
    pub(crate) fn new() -> Monitor {
        Self {
            inner: Publisher::new(),
        }
    }

    /// Sends a new monitor event to all subscribers.
    pub async fn notify(&self, event: &MonitorEvent) {
        self.inner.notify(event).await;
    }

    /// Subscribes to listen to new events.
    pub async fn subscribe(&self) -> Subscription<MonitorEvent> {
        self.inner.subscribe().await
    }
}

/// Defines various type of event that can be monitored.
#[derive(Clone, Debug)]
pub enum MonitorEvent {
    Conn(ConnEvent),
    PeerPool(PeerPoolEvent),
    Discovery(DiscoveryEvent),
}

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

/// Defines `PeerPool` events.
#[derive(Clone, Debug)]
pub enum PeerPoolEvent {
    NewPeer(PeerID),
    RemovePeer(PeerID),
}

/// Defines `Discovery` events.
#[derive(Clone, Debug)]
pub enum DiscoveryEvent {
    LookupStarted(Endpoint),
    LookupFailed(Endpoint),
    LookupSucceeded(Endpoint, usize),
    RefreshStarted,
}

impl fmt::Display for MonitorEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let val = match self {
            MonitorEvent::Conn(e) => format!("Connection Event: {e}"),
            MonitorEvent::PeerPool(e) => format!("PeerPool Event: {e}"),
            MonitorEvent::Discovery(e) => format!("Discovery Event: {e}"),
        };
        write!(f, "{}", val)
    }
}

impl fmt::Display for ConnEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let val = match self {
            ConnEvent::Connected(endpoint) => format!("Connected: {endpoint}"),
            ConnEvent::ConnectFailed(endpoint) => format!("ConnectFailed: {endpoint}"),
            ConnEvent::ConnectRetried(endpoint) => format!("ConnectRetried: {endpoint}"),
            ConnEvent::AcceptFailed => "AcceptFailed".to_string(),
            ConnEvent::Accepted(endpoint) => format!("Accepted: {endpoint}"),
            ConnEvent::Disconnected(endpoint) => format!("Disconnected: {endpoint}"),
            ConnEvent::Listening(endpoint) => format!("Listening: {endpoint}"),
            ConnEvent::ListenFailed(endpoint) => format!("ListenFailed: {endpoint}"),
        };
        write!(f, "{}", val)
    }
}

impl fmt::Display for PeerPoolEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let val = match self {
            PeerPoolEvent::NewPeer(pid) => format!("NewPeer: {pid}"),
            PeerPoolEvent::RemovePeer(pid) => format!("RemovePeer: {pid}"),
        };
        write!(f, "{}", val)
    }
}

impl fmt::Display for DiscoveryEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let val = match self {
            DiscoveryEvent::LookupStarted(endpoint) => format!("LookupStarted: {endpoint}"),
            DiscoveryEvent::LookupFailed(endpoint) => format!("LookupFailed: {endpoint}"),
            DiscoveryEvent::LookupSucceeded(endpoint, len) => {
                format!("LookupSucceeded: {endpoint} {len}")
            }
            DiscoveryEvent::RefreshStarted => "RefreshStarted".to_string(),
        };
        write!(f, "{}", val)
    }
}

impl From<ConnEvent> for MonitorEvent {
    fn from(val: ConnEvent) -> Self {
        MonitorEvent::Conn(val)
    }
}

impl From<PeerPoolEvent> for MonitorEvent {
    fn from(val: PeerPoolEvent) -> Self {
        MonitorEvent::PeerPool(val)
    }
}

impl From<DiscoveryEvent> for MonitorEvent {
    fn from(val: DiscoveryEvent) -> Self {
        MonitorEvent::Discovery(val)
    }
}
