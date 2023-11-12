use std::sync::Arc;

use log::info;

use karyons_core::{pubsub::Subscription, Executor};

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
///
///
/// # Example
/// ```
/// use std::sync::Arc;
///
/// use easy_parallel::Parallel;
/// use smol::{channel as smol_channel, future, Executor};
///
/// use karyons_p2p::{Backend, Config, PeerID};
///
/// let peer_id = PeerID::random();
///
/// // Create the configuration for the backend.
/// let mut config = Config::default();
///
/// // Create a new Backend
/// let backend = Backend::new(peer_id, config);
///
/// // Create a new Executor
/// let ex = Arc::new(Executor::new());
///
/// let task = async {
///     // Run the backend
///     backend.run(ex.clone()).await.unwrap();
///
///     // ....
///
///     // Shutdown the backend
///     backend.shutdown().await;
/// };
///
/// future::block_on(ex.run(task));
///
/// ```
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
    pub fn new(id: PeerID, config: Config) -> ArcBackend {
        let config = Arc::new(config);
        let monitor = Arc::new(Monitor::new());

        let conn_queue = ConnQueue::new();

        let peer_pool = PeerPool::new(&id, conn_queue.clone(), config.clone(), monitor.clone());
        let discovery = Discovery::new(&id, conn_queue, config.clone(), monitor.clone());

        Arc::new(Self {
            id: id.clone(),
            monitor,
            discovery,
            config,
            peer_pool,
        })
    }

    /// Run the Backend, starting the PeerPool and Discovery instances.
    pub async fn run(self: &Arc<Self>, ex: Executor<'_>) -> Result<()> {
        info!("Run the backend {}", self.id);
        self.peer_pool.start(ex.clone()).await?;
        self.discovery.start(ex.clone()).await?;
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
