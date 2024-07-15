use std::{collections::HashMap, sync::Arc};

use bincode::Encode;
use log::{error, info, warn};

use karyon_core::{
    async_runtime::{lock::RwLock, Executor},
    async_util::{TaskGroup, TaskResult},
};

use karyon_net::Endpoint;

use crate::{
    config::Config,
    conn_queue::ConnQueue,
    connection::Connection,
    monitor::{Monitor, PPEvent},
    peer::Peer,
    protocol::{Protocol, ProtocolConstructor, ProtocolID},
    protocols::PingProtocol,
    version::Version,
    Error, PeerID, Result,
};

pub struct PeerPool {
    /// Peer's ID
    pub id: PeerID,

    /// Connection queue
    conn_queue: Arc<ConnQueue>,

    /// Holds the running peers.
    peers: RwLock<HashMap<PeerID, Arc<Peer>>>,

    /// Hashmap contains protocol constructors.
    pub(crate) protocols: RwLock<HashMap<ProtocolID, Box<ProtocolConstructor>>>,

    /// Hashmap contains protocols with their versions
    pub(crate) protocol_versions: RwLock<HashMap<ProtocolID, Version>>,

    /// Managing spawned tasks.
    task_group: TaskGroup,

    /// A global Executor
    executor: Executor,

    /// The Configuration for the P2P network.
    config: Arc<Config>,

    /// Responsible for network and system monitoring.
    monitor: Arc<Monitor>,
}

impl PeerPool {
    /// Creates a new PeerPool
    pub fn new(
        id: &PeerID,
        conn_queue: Arc<ConnQueue>,
        config: Arc<Config>,
        monitor: Arc<Monitor>,
        executor: Executor,
    ) -> Arc<Self> {
        Arc::new(Self {
            id: id.clone(),
            conn_queue,
            peers: RwLock::new(HashMap::new()),
            protocols: RwLock::new(HashMap::new()),
            protocol_versions: RwLock::new(HashMap::new()),
            task_group: TaskGroup::with_executor(executor.clone()),
            executor,
            monitor,
            config,
        })
    }

    /// Starts the [`PeerPool`]
    pub async fn start(self: &Arc<Self>) -> Result<()> {
        self.setup_core_protocols().await?;
        self.task_group.spawn(self.clone().run(), |_| async {});
        Ok(())
    }

    /// Shuts down
    pub async fn shutdown(&self) {
        for (_, peer) in self.peers.read().await.iter() {
            let _ = peer.shutdown().await;
        }

        self.task_group.cancel().await;
    }

    /// Attach a custom protocol to the network
    pub async fn attach_protocol<P: Protocol>(&self, c: Box<ProtocolConstructor>) -> Result<()> {
        self.protocols.write().await.insert(P::id(), c);
        self.protocol_versions
            .write()
            .await
            .insert(P::id(), P::version()?);
        Ok(())
    }

    /// Broadcast a message to all connected peers using the specified protocol.
    pub async fn broadcast<T: Encode>(&self, proto_id: &ProtocolID, msg: &T) {
        for (pid, peer) in self.peers.read().await.iter() {
            if let Err(err) = peer.conn.send(proto_id.to_string(), msg).await {
                error!("failed to send msg to {pid}: {err}");
                continue;
            }
        }
    }

    /// Checks if the peer list contains a peer with the given peer id
    pub async fn contains_peer(&self, pid: &PeerID) -> bool {
        self.peers.read().await.contains_key(pid)
    }

    /// Returns the number of currently connected peers.
    pub async fn peers_len(&self) -> usize {
        self.peers.read().await.len()
    }

    /// Returns a map of inbound peers with their endpoints.
    pub async fn inbound_peers(&self) -> HashMap<PeerID, Endpoint> {
        let mut peers = HashMap::new();
        for (id, peer) in self.peers.read().await.iter() {
            if peer.is_inbound() {
                peers.insert(id.clone(), peer.remote_endpoint().clone());
            }
        }
        peers
    }

    /// Returns a map of outbound peers with their endpoints.
    pub async fn outbound_peers(&self) -> HashMap<PeerID, Endpoint> {
        let mut peers = HashMap::new();
        for (id, peer) in self.peers.read().await.iter() {
            if !peer.is_inbound() {
                peers.insert(id.clone(), peer.remote_endpoint().clone());
            }
        }
        peers
    }

    async fn run(self: Arc<Self>) {
        loop {
            let mut conn = self.conn_queue.next().await;

            for protocol_id in self.protocols.read().await.keys() {
                conn.register_protocol(protocol_id.to_string()).await;
            }

            let conn = Arc::new(conn);

            let result = self.new_peer(conn.clone()).await;

            // Disconnect if there is an error when adding a peer.
            if result.is_err() {
                let _ = conn.disconnect(result).await;
            }
        }
    }

    /// Add a new peer to the peer list.
    async fn new_peer(self: &Arc<Self>, conn: Arc<Connection>) -> Result<()> {
        // Create a new peer
        let peer = Peer::new(
            self.id.clone(),
            Arc::downgrade(self),
            conn.clone(),
            self.config.clone(),
            self.executor.clone(),
        );
        peer.init().await?;
        let pid = peer.id().expect("Get peer id after peer initialization");

        // TODO: Consider restricting the subnet for inbound connections
        if self.contains_peer(&pid).await {
            return Err(Error::PeerAlreadyConnected);
        }

        // Insert the new peer
        self.peers.write().await.insert(pid.clone(), peer.clone());

        let on_disconnect = {
            let this = self.clone();
            let pid = pid.clone();
            |result| async move {
                if let TaskResult::Completed(_) = result {
                    if let Err(err) = this.remove_peer(&pid).await {
                        error!("Failed to remove peer {pid}: {err}");
                    }
                }
            }
        };

        self.task_group.spawn(peer.run(), on_disconnect);

        info!("Add new peer {pid}");
        self.monitor.notify(PPEvent::NewPeer(pid)).await;

        Ok(())
    }

    /// Shuts down the peer and remove it from the peer list.
    async fn remove_peer(&self, pid: &PeerID) -> Result<()> {
        let result = self.peers.write().await.remove(pid);

        let peer = match result {
            Some(p) => p,
            None => return Ok(()),
        };

        let _ = peer.shutdown().await;

        self.monitor.notify(PPEvent::RemovePeer(pid.clone())).await;

        warn!("Peer {pid} removed",);
        Ok(())
    }

    /// Attach the core protocols.
    async fn setup_core_protocols(&self) -> Result<()> {
        let executor = self.executor.clone();
        let ping_interval = self.config.ping_interval;
        let ping_timeout = self.config.ping_timeout;
        let c = move |peer| PingProtocol::new(peer, ping_interval, ping_timeout, executor.clone());
        self.attach_protocol::<PingProtocol>(Box::new(c)).await
    }
}
