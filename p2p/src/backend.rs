use std::sync::Arc;

use log::info;

use karyons_core::{pubsub::Subscription, GlobalExecutor};

use crate::{
    config::Config,
    connection::ConnQueue,
    discovery::{ArcDiscovery, Discovery},
    monitor::{Monitor, MonitorEvent},
    peer_pool::PeerPool,
    protocol::{ArcProtocol, Protocol},
    ArcPeer, PeerID, Result,
};

pub type ArcBackend = Arc<Backend>;

/// Backend serves as the central entry point for initiating and managing
/// the P2P network.
pub struct Backend {
    /// The Configuration for the P2P network.
    config: Arc<Config>,

    /// Peer ID.
    id: PeerID,

    /// Responsible for network and system monitoring.
    monitor: Arc<Monitor>,

    /// Discovery instance.
    discovery: ArcDiscovery,

    /// PeerPool instance.
    peer_pool: Arc<PeerPool>,
}

impl Backend {
    /// Creates a new Backend.
    pub fn new(id: PeerID, config: Config, ex: GlobalExecutor) -> ArcBackend {
        let config = Arc::new(config);
        let monitor = Arc::new(Monitor::new());
        let cq = ConnQueue::new();

        let peer_pool = PeerPool::new(&id, cq.clone(), config.clone(), monitor.clone(), ex.clone());

        let discovery = Discovery::new(&id, cq, config.clone(), monitor.clone(), ex);

        Arc::new(Self {
            id: id.clone(),
            monitor,
            discovery,
            config,
            peer_pool,
        })
    }

    /// Run the Backend, starting the PeerPool and Discovery instances.
    pub async fn run(self: &Arc<Self>) -> Result<()> {
        info!("Run the backend {}", self.id);
        self.peer_pool.start().await?;
        self.discovery.start().await?;
        Ok(())
    }

    /// Attach a custom protocol to the network
    pub async fn attach_protocol<P: Protocol>(
        &self,
        c: impl Fn(ArcPeer) -> ArcProtocol + Send + Sync + 'static,
    ) -> Result<()> {
        self.peer_pool.attach_protocol::<P>(Box::new(c)).await
    }

    /// Returns the number of currently connected peers.
    pub async fn peers(&self) -> usize {
        self.peer_pool.peers_len().await
    }

    /// Returns the `Config`.
    pub fn config(&self) -> Arc<Config> {
        self.config.clone()
    }

    /// Returns the number of occupied inbound slots.
    pub fn inbound_slots(&self) -> usize {
        self.discovery.inbound_slots.load()
    }

    /// Returns the number of occupied outbound slots.
    pub fn outbound_slots(&self) -> usize {
        self.discovery.outbound_slots.load()
    }

    /// Subscribes to the monitor to receive network events.
    pub async fn monitor(&self) -> Subscription<MonitorEvent> {
        self.monitor.subscribe().await
    }

    /// Shuts down the Backend.
    pub async fn shutdown(&self) {
        self.discovery.shutdown().await;
        self.peer_pool.shutdown().await;
    }
}
