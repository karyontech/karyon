use std::sync::Arc;

use log::info;

use karyons_core::{crypto::KeyPair, pubsub::Subscription, GlobalExecutor};

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

    /// Identity Key pair
    key_pair: KeyPair,

    /// Responsible for network and system monitoring.
    monitor: Arc<Monitor>,

    /// Discovery instance.
    discovery: ArcDiscovery,

    /// PeerPool instance.
    peer_pool: Arc<PeerPool>,
}

impl Backend {
    /// Creates a new Backend.
    pub fn new(key_pair: &KeyPair, config: Config, ex: GlobalExecutor) -> ArcBackend {
        let config = Arc::new(config);
        let monitor = Arc::new(Monitor::new());
        let conn_queue = ConnQueue::new();

        let peer_id = PeerID::try_from(key_pair.public())
            .expect("Derive a peer id from the provided key pair.");
        info!("PeerID: {}", peer_id);

        let peer_pool = PeerPool::new(
            &peer_id,
            conn_queue.clone(),
            config.clone(),
            monitor.clone(),
            ex.clone(),
        );

        let discovery = Discovery::new(
            key_pair,
            &peer_id,
            conn_queue,
            config.clone(),
            monitor.clone(),
            ex,
        );

        Arc::new(Self {
            key_pair: key_pair.clone(),
            monitor,
            discovery,
            config,
            peer_pool,
        })
    }

    /// Run the Backend, starting the PeerPool and Discovery instances.
    pub async fn run(self: &Arc<Self>) -> Result<()> {
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

    /// Returns the `KeyPair`.
    pub async fn key_pair(&self) -> &KeyPair {
        &self.key_pair
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
