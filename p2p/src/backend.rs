use std::{collections::HashMap, sync::Arc};

use log::info;

use karyon_core::{async_runtime::Executor, crypto::KeyPair};
use karyon_net::Endpoint;

use crate::{
    config::Config, conn_queue::ConnQueue, discovery::Discovery, monitor::Monitor, peer::Peer,
    peer_pool::PeerPool, protocol::Protocol, PeerID, Result,
};

/// Backend serves as the central entry point for initiating and managing
/// the P2P network.
pub struct Backend {
    /// The Configuration for the P2P network.
    config: Arc<Config>,

    /// Identity Key pair
    key_pair: KeyPair,

    /// Peer ID
    peer_id: PeerID,

    /// Responsible for network and system monitoring.
    monitor: Arc<Monitor>,

    /// Discovery instance.
    discovery: Arc<Discovery>,

    /// PeerPool instance.
    peer_pool: Arc<PeerPool>,
}

impl Backend {
    /// Creates a new Backend.
    pub fn new(key_pair: &KeyPair, config: Config, ex: Executor) -> Arc<Backend> {
        let config = Arc::new(config);
        let monitor = Arc::new(Monitor::new(config.clone()));
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
            peer_id,
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
        c: impl Fn(Arc<Peer>) -> Arc<dyn Protocol> + Send + Sync + 'static,
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

    /// Returns the `PeerID`.
    pub fn peer_id(&self) -> &PeerID {
        &self.peer_id
    }

    /// Returns the `KeyPair`.
    pub fn key_pair(&self) -> &KeyPair {
        &self.key_pair
    }

    /// Returns a map of inbound connected peers with their endpoints.
    pub async fn inbound_peers(&self) -> HashMap<PeerID, Endpoint> {
        self.peer_pool.inbound_peers().await
    }

    /// Returns a map of outbound connected peers with their endpoints.
    pub async fn outbound_peers(&self) -> HashMap<PeerID, Endpoint> {
        self.peer_pool.outbound_peers().await
    }

    /// Returns the monitor to receive system events.
    pub fn monitor(&self) -> Arc<Monitor> {
        self.monitor.clone()
    }

    /// Shuts down the Backend.
    pub async fn shutdown(&self) {
        self.discovery.shutdown().await;
        self.peer_pool.shutdown().await;
    }
}
