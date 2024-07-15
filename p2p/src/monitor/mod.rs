mod event;

use std::sync::Arc;

use log::error;

use karyon_core::event::{EventEmitter, EventListener, EventValue, EventValueTopic};

use karyon_net::Endpoint;

pub(crate) use event::{ConnEvent, DiscvEvent, PPEvent};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::{Config, PeerID};

/// Responsible for network and system monitoring.
///
/// It use event emitter to notify the registerd listeners about new events.
///
/// # Example
///
/// ```
/// use std::sync::Arc;
///
/// use smol::Executor;
///
/// use karyon_p2p::{
///     Config, Backend, PeerID, keypair::{KeyPair, KeyPairType}, monitor::ConnectionEvent,
/// };
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
///     let listener = monitor.register::<ConnectionEvent>().await;
///     
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
    pub(crate) async fn notify<E: ToEventStruct>(&self, event: E) {
        if self.config.enable_monitor {
            let event = event.to_struct();
            if let Err(err) = self.event_emitter.emit(&event).await {
                error!("Failed to notify monitor event {:?}: {err}", event);
            }
        }
    }

    /// Registers a new event listener for the provided topic.
    pub async fn register<E>(&self) -> EventListener<MonitorTopic, E>
    where
        E: Clone + EventValue + EventValueTopic<Topic = MonitorTopic>,
    {
        self.event_emitter.register(&E::topic()).await
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum MonitorTopic {
    Connection,
    PeerPool,
    Discovery,
}

pub(super) trait ToEventStruct: Sized {
    type EventStruct: From<Self> + Clone + EventValueTopic<Topic = MonitorTopic> + EventValue;
    fn to_struct(self) -> Self::EventStruct {
        self.into()
    }
}

impl ToEventStruct for ConnEvent {
    type EventStruct = ConnectionEvent;
}

impl ToEventStruct for PPEvent {
    type EventStruct = PeerPoolEvent;
}

impl ToEventStruct for DiscvEvent {
    type EventStruct = DiscoveryEvent;
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ConnectionEvent {
    pub event: String,
    pub date: i64,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub endpoint: Option<Endpoint>,
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PeerPoolEvent {
    pub event: String,
    pub date: i64,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub peer_id: Option<PeerID>,
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DiscoveryEvent {
    pub event: String,
    pub date: i64,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub endpoint: Option<Endpoint>,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub size: Option<usize>,
}

impl From<ConnEvent> for ConnectionEvent {
    fn from(event: ConnEvent) -> Self {
        let endpoint = event.get_endpoint().cloned();
        Self {
            endpoint,
            event: event.variant_name().to_string(),
            date: get_current_timestamp(),
        }
    }
}

impl From<PPEvent> for PeerPoolEvent {
    fn from(event: PPEvent) -> Self {
        let peer_id = event.get_peer_id().cloned();
        Self {
            peer_id,
            event: event.variant_name().to_string(),
            date: get_current_timestamp(),
        }
    }
}

impl From<DiscvEvent> for DiscoveryEvent {
    fn from(event: DiscvEvent) -> Self {
        let (endpoint, size) = event.get_endpoint_and_size();
        Self {
            endpoint: endpoint.cloned(),
            size,
            event: event.variant_name().to_string(),
            date: get_current_timestamp(),
        }
    }
}

impl EventValue for ConnectionEvent {
    fn id() -> &'static str {
        "ConnectionEvent"
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

impl EventValueTopic for ConnectionEvent {
    type Topic = MonitorTopic;
    fn topic() -> Self::Topic {
        MonitorTopic::Connection
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

fn get_current_timestamp() -> i64 {
    chrono::Utc::now().timestamp()
}
