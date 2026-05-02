mod event;

use std::sync::Arc;

use log::error;

use karyon_eventemitter::{AsEventValue, EventEmitter, EventListener, EventTopic, EventValue};

use karyon_net::Endpoint;

pub(crate) use event::{ConnectionKind, DiscoveryKind, PoolEvent};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::{Config, PeerID};

/// Responsible for network and system monitoring.
///
/// It use event emitter to notify the registerd listeners about new events.
///
/// # Example
///
/// ```no_run
/// use karyon_core::async_runtime::global_executor;
/// use karyon_p2p::{
///     Config, Node, keypair::{KeyPair, KeyPairType}, monitor::ConnectionEvent,
/// };
///
/// async {
///     let key_pair = KeyPair::generate(&KeyPairType::Ed25519);
///     let node = Node::new(&key_pair, Config::default(), global_executor());
///
///     // Subscribe to a monitor event.
///     let monitor = node.monitor();
///     let listener = monitor.register::<ConnectionEvent>();
///     let new_event = listener.recv().await;
/// };
/// ```
pub struct Monitor {
    event_emitter: Arc<EventEmitter<MonitorTopic>>,
    config: Arc<Config>,
}

impl Monitor {
    /// Creates a new Monitor
    pub(crate) fn new(config: Arc<Config>) -> Self {
        Self {
            event_emitter: EventEmitter::new(),
            config,
        }
    }

    /// Sends a new monitor event to subscribers.
    pub(crate) async fn notify<E: ToEvent>(&self, event: E) {
        if !self.config.enable_monitor {
            return;
        }

        let topic = E::Event::topic();
        if !self.event_emitter.has_listeners(&topic) {
            return;
        }

        let event = event.to_event();
        if let Err(err) = self.event_emitter.emit(&event).await {
            error!("Failed to notify monitor event {event:?}: {err}");
        }
    }

    /// Registers a new event listener for the provided topic.
    pub fn register<E>(&self) -> EventListener<MonitorTopic, E>
    where
        E: Clone + AsEventValue + EventTopic<Topic = MonitorTopic>,
    {
        self.event_emitter.register(&E::topic())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum MonitorTopic {
    Connection,
    PeerPool,
    Discovery,
}

pub(super) trait ToEvent: Sized {
    type Event: From<Self>
        + Clone
        + EventTopic<Topic = MonitorTopic>
        + AsEventValue
        + std::fmt::Debug;
    fn to_event(self) -> Self::Event {
        self.into()
    }
}

impl ToEvent for ConnectionKind {
    type Event = ConnectionEvent;
}

impl ToEvent for PoolEvent {
    type Event = PeerPoolEvent;
}

impl ToEvent for DiscoveryKind {
    type Event = DiscoveryEvent;
}

#[derive(Clone, Debug, EventValue)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ConnectionEvent {
    pub event: String,
    pub date: i64,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub endpoint: Option<Endpoint>,
}

#[derive(Clone, Debug, EventValue)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PeerPoolEvent {
    pub event: String,
    pub date: i64,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub peer_id: Option<PeerID>,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub endpoint: Option<Endpoint>,
}

#[derive(Clone, Debug, EventValue)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DiscoveryEvent {
    pub event: String,
    pub date: i64,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub endpoint: Option<Endpoint>,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub size: Option<usize>,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub peer_id: Option<PeerID>,
}

impl From<ConnectionKind> for ConnectionEvent {
    fn from(event: ConnectionKind) -> Self {
        let endpoint = event.get_endpoint().cloned();
        Self {
            endpoint,
            event: event.variant_name().to_string(),
            date: get_current_timestamp(),
        }
    }
}

impl From<PoolEvent> for PeerPoolEvent {
    fn from(event: PoolEvent) -> Self {
        let peer_id = event.get_peer_id().cloned();
        let endpoint = event.get_endpoint().cloned();
        Self {
            peer_id,
            endpoint,
            event: event.variant_name().to_string(),
            date: get_current_timestamp(),
        }
    }
}

impl From<DiscoveryKind> for DiscoveryEvent {
    fn from(event: DiscoveryKind) -> Self {
        Self {
            endpoint: event.get_endpoint().cloned(),
            size: event.get_size(),
            peer_id: event.get_peer_id().cloned(),
            event: event.variant_name().to_string(),
            date: get_current_timestamp(),
        }
    }
}

impl EventTopic for ConnectionEvent {
    type Topic = MonitorTopic;
    fn topic() -> Self::Topic {
        MonitorTopic::Connection
    }
}

impl EventTopic for PeerPoolEvent {
    type Topic = MonitorTopic;
    fn topic() -> Self::Topic {
        MonitorTopic::PeerPool
    }
}

impl EventTopic for DiscoveryEvent {
    type Topic = MonitorTopic;
    fn topic() -> Self::Topic {
        MonitorTopic::Discovery
    }
}

fn get_current_timestamp() -> i64 {
    chrono::Utc::now().timestamp()
}
