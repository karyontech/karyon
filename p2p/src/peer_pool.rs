use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use log::{error, info, warn};

use karyon_core::{
    async_runtime::{lock::RwLock, Executor},
    async_util::{TaskGroup, TaskResult},
};

use karyon_eventemitter::{EventEmitter, EventListener, EventTopic, EventValue};

use karyon_net::Endpoint;

use crate::{
    config::Config,
    conn_queue::{ConnQueue, QueuedConn},
    handshake::{handshake, HandshakeParams},
    monitor::{Monitor, PoolEvent},
    peer::Peer,
    protocol::{Protocol, ProtocolConstructor, ProtocolID, ProtocolMeta},
    Error, PeerID, Result,
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

pub struct PeerPool {
    /// Peer's ID
    pub id: PeerID,

    /// Connection queue
    conn_queue: Arc<ConnQueue>,

    /// Holds the running peers.
    peers: RwLock<HashMap<PeerID, Arc<Peer>>>,

    /// Hashmap contains protocol constructors.
    pub(crate) protocols: RwLock<HashMap<ProtocolID, Box<ProtocolConstructor>>>,

    /// Per-protocol metadata (version + kind, extensible). Keyed by
    /// protocol id. Source of truth for the handshake's mandatory check
    /// and version negotiation.
    pub(crate) protocol_meta: RwLock<HashMap<ProtocolID, ProtocolMeta>>,

    /// Peer-lifecycle event emitter. Each registered listener gets its
    /// own copy of every event.
    peer_emitter: Arc<EventEmitter<PeerEventTopic>>,

    /// Managing spawned tasks.
    task_group: TaskGroup,

    /// A global Executor
    pub(crate) executor: Executor,

    /// The Configuration for the P2P network.
    pub(crate) config: Arc<Config>,

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
            protocol_meta: RwLock::new(HashMap::new()),
            peer_emitter: EventEmitter::new(),
            task_group: TaskGroup::with_executor(executor.clone()),
            executor,
            monitor,
            config,
        })
    }

    /// Register a listener for the peer-lifecycle events.
    pub fn register_peer_events(&self) -> EventListener<PeerEventTopic, PeerEvent> {
        self.peer_emitter.register(&PeerEventTopic::Lifecycle)
    }

    /// Starts the [`PeerPool`]
    pub async fn start(self: &Arc<Self>) -> Result<()> {
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

    /// Register a protocol's user-supplied constructor and metadata.
    /// Bloom advertising is handled by `Node::attach_protocol`.
    pub async fn attach_protocol<P: Protocol>(&self, c: Box<ProtocolConstructor>) -> Result<()> {
        let id = P::id();
        self.protocols.write().await.insert(id.clone(), c);
        self.protocol_meta.write().await.insert(
            id,
            ProtocolMeta {
                version: P::version()?,
                kind: P::kind(),
            },
        );
        Ok(())
    }

    /// Broadcast a message to all connected peers.
    pub async fn broadcast(&self, proto_id: &ProtocolID, msg: Vec<u8>) {
        for (pid, peer) in self.peers.read().await.iter() {
            if let Err(err) = peer.send(proto_id.to_string(), msg.clone()).await {
                error!("failed to send msg to {pid}: {err}");
                continue;
            }
        }
    }

    /// Broadcast a message to a specific set of peers.
    pub async fn broadcast_to(
        &self,
        proto_id: &ProtocolID,
        msg: Vec<u8>,
        targets: &HashSet<PeerID>,
    ) {
        for (pid, peer) in self.peers.read().await.iter() {
            if !targets.contains(pid) {
                continue;
            }
            if let Err(err) = peer.send(proto_id.to_string(), msg.clone()).await {
                error!("failed to send msg to {pid}: {err}");
            }
        }
    }

    /// Send a message to a specific peer on the given protocol. Returns
    /// `PeerNotFound` if the peer is not currently in the pool.
    pub async fn send_to(
        &self,
        peer_id: &PeerID,
        proto_id: &ProtocolID,
        msg: Vec<u8>,
    ) -> Result<()> {
        let peers = self.peers.read().await;
        let peer = peers
            .get(peer_id)
            .ok_or_else(|| Error::PeerNotFound(peer_id.to_string()))?;
        peer.send(proto_id.to_string(), msg).await
    }

    /// Returns the negotiated protocol set for a peer.
    pub async fn peer_protocol_set(&self, pid: &PeerID) -> Option<HashSet<ProtocolID>> {
        self.peers
            .read()
            .await
            .get(pid)
            .map(|p| p.negotiated_protocols().clone())
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
            let mut queued = self.conn_queue.next().await;

            // Snapshot the protocol metadata so we don't hold a lock
            // across the handshake. Drives both version negotiation
            // and the mandatory-subset check.
            let meta = self.protocol_meta.read().await.clone();

            let params = HandshakeParams {
                own_id: &self.id,
                is_inbound: matches!(queued.direction, crate::peer::ConnDirection::Inbound),
                config_version: &self.config.version,
                protocols: &meta,
                timeout_secs: self.config.handshake_timeout,
                verified_peer_id: queued.verified_peer_id.as_ref(),
            };
            let handshake = handshake(&mut queued.reader, &mut queued.writer, &params).await;

            let (pid, negotiated) = match handshake {
                Ok(v) => v,
                Err(err) => {
                    let pid = queued.verified_peer_id.clone();
                    self.monitor
                        .notify(PoolEvent::HandshakeFailed(pid.clone()))
                        .await;
                    let _ = self
                        .peer_emitter
                        .emit(&PeerEvent::HandshakeFailed(pid))
                        .await;
                    let _ = queued.disconnect_signal.send(Err(err)).await;
                    continue;
                }
            };

            if let Err(err) = self.new_peer(queued, pid, negotiated).await {
                error!("new_peer failed: {err}");
            }
        }
    }

    /// Build a Peer from a post-handshake `QueuedConn` and run it.
    async fn new_peer(
        self: &Arc<Self>,
        queued: QueuedConn,
        pid: PeerID,
        negotiated: Vec<ProtocolID>,
    ) -> Result<()> {
        if self.contains_peer(&pid).await {
            self.monitor
                .notify(PoolEvent::PeerAlreadyConnected(pid.clone()))
                .await;
            let _ = queued
                .disconnect_signal
                .send(Err(Error::PeerAlreadyConnected))
                .await;
            return Err(Error::PeerAlreadyConnected);
        }

        let protocol_ids: Vec<ProtocolID> = self.protocols.read().await.keys().cloned().collect();
        let negotiated: HashSet<ProtocolID> = negotiated.into_iter().collect();

        let peer = Peer::new(self.clone(), queued, pid.clone(), negotiated, protocol_ids).await?;

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

        self.task_group.spawn(peer.clone().run(), on_disconnect);

        info!("Add new peer {pid}");
        self.monitor.notify(PoolEvent::NewPeer(pid.clone())).await;
        let _ = self.peer_emitter.emit(&PeerEvent::Added(pid)).await;

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

        self.monitor
            .notify(PoolEvent::RemovePeer(pid.clone()))
            .await;
        let _ = self
            .peer_emitter
            .emit(&PeerEvent::Removed(pid.clone()))
            .await;

        warn!("Peer {pid} removed",);
        Ok(())
    }
}
