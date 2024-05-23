use std::{fmt, sync::Arc};

use karyon_core::event::{ArcEventSys, EventListener, EventSys, EventValue, EventValueTopic};

use karyon_net::Endpoint;

use crate::{Config, PeerID};

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
/// use karyon_p2p::{Config, Backend, PeerID, keypair::{KeyPair, KeyPairType}};
///
/// async {
///     
///     // Create a new Executor
///     let ex = Arc::new(Executor::new());
///
///     let key_pair = KeyPair::generate(&KeyPairType::Ed25519);
///     let backend = Backend::new(&key_pair, Config::default(), ex.into());
///
///     // Create a new Subscription
///     let monitor =  backend.monitor();
///     
///     let listener = monitor.conn_events().await;
///     
///     let new_event = listener.recv().await;
/// };
/// ```
pub struct Monitor {
    event_sys: ArcEventSys<MonitorTopic>,

    config: Arc<Config>,
}

impl Monitor {
    /// Creates a new Monitor
    pub(crate) fn new(config: Arc<Config>) -> Self {
        Self {
            event_sys: EventSys::new(),
            config,
        }
    }

    /// Sends a new monitor event to subscribers.
    pub(crate) async fn notify<E>(&self, event: E)
    where
        E: EventValue + Clone + EventValueTopic<Topic = MonitorTopic>,
    {
        if self.config.enable_monitor {
            self.event_sys.emit(&event).await
        }
    }

    /// Registers a new event listener for connection events.
    pub async fn conn_events(&self) -> EventListener<MonitorTopic, ConnEvent> {
        self.event_sys.register(&MonitorTopic::Conn).await
    }

    /// Registers a new event listener for peer pool events.
    pub async fn peer_pool_events(&self) -> EventListener<MonitorTopic, PeerPoolEvent> {
        self.event_sys.register(&MonitorTopic::PeerPool).await
    }

    /// Registers a new event listener for discovery events.
    pub async fn discovery_events(&self) -> EventListener<MonitorTopic, DiscoveryEvent> {
        self.event_sys.register(&MonitorTopic::Discovery).await
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum MonitorTopic {
    Conn,
    PeerPool,
    Discovery,
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

impl EventValue for ConnEvent {
    fn id() -> &'static str {
        "ConnEvent"
    }
}

impl EventValue for PeerPoolEvent {
    fn id() -> &'static str {
        "PeerPoolEvent"
    }
}

impl EventValue for DiscoveryEvent {
    fn id() -> &'static str {
        "DiscoveryEvent"
    }
}

impl EventValueTopic for ConnEvent {
    type Topic = MonitorTopic;
    fn topic() -> Self::Topic {
        MonitorTopic::Conn
    }
}

impl EventValueTopic for PeerPoolEvent {
    type Topic = MonitorTopic;
    fn topic() -> Self::Topic {
        MonitorTopic::PeerPool
    }
}

impl EventValueTopic for DiscoveryEvent {
    type Topic = MonitorTopic;
    fn topic() -> Self::Topic {
        MonitorTopic::Discovery
    }
}
