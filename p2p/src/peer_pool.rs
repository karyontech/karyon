use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use log::{debug, error, info, warn};

use karyon_core::{
    async_runtime::{Executor, lock::RwLock},
    async_util::{TaskGroup, TaskResult},
};

use karyon_eventemitter::{EventEmitter, EventListener, EventTopic, EventValue};

use karyon_net::Endpoint;

use crate::{
    Error, PeerID, Result,
    access_control::{Action, PeerCandidate, Subject},
    config::Config,
    conn_direction::ConnDirection,
    conn_queue::{ConnQueue, QueuedConn},
    handshake::{HandshakeParams, handshake},
    monitor::{Monitor, PoolEvent},
    peer::Peer,
    protocol::{Protocol, ProtocolConstructor, ProtocolID, ProtocolMeta},
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

/// Listener returned by `register_peer_events`.
pub type PeerEventListener = EventListener<PeerEventTopic, PeerEvent>;

pub struct PeerPool {
    /// Peer's ID
    pub id: PeerID,

    /// Connection queue
    conn_queue: Arc<ConnQueue>,

    /// Holds the running peers.
    peers: RwLock<HashMap<PeerID, Arc<Peer>>>,

    /// Next connection id, see `Peer::conn_id`.
    next_conn_id: AtomicU64,

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
            next_conn_id: AtomicU64::new(0),
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
        self.task_group.spawn(self.clone().run());
        Ok(())
    }

    /// Shuts down
    pub async fn shutdown(&self) {
        // Cancel the task group first. This waits for the run loop to
        // actually stop, so no new peer can be admitted while we close
        // the ones already in the map.
        self.task_group.cancel().await;

        for peer in self.peers.read().await.values() {
            let _ = peer.shutdown().await;
        }
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
                flags: P::flags(),
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
            let queued = self.conn_queue.next().await;

            // One task per connection, so a slow or silent peer does not
            // hold back the others. Every queued connection holds a
            // listener or connector slot, which bounds these tasks.
            self.task_group.spawn(self.clone().admit(queued));
        }
    }

    /// Handshake a queued connection and add it to the pool.
    async fn admit(self: Arc<Self>, mut queued: QueuedConn) {
        // Snapshot the protocol metadata so we don't hold a lock
        // across the handshake. Drives both version negotiation
        // and the mandatory-subset check.
        let meta = self.protocol_meta.read().await.clone();

        let params = HandshakeParams {
            own_id: &self.id,
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
                let _ = queued.conn_closed_tx.send(Err(err)).await;
                return;
            }
        };

        // Admission gate. Runs before the Peer is built, so a denied
        // peer never gets tasks spawned or QUIC streams opened.
        let candidate = PeerCandidate {
            peer_id: &pid,
            remote_endpoint: &queued.remote_endpoint,
            negotiated_protocols: &negotiated,
        };
        let allowed = self.config.access_control.allow(
            &Subject::Peer(&candidate),
            Action::Connect(queued.direction.clone()),
        );

        if !allowed {
            warn!("Access denied for peer {pid}");
            self.monitor
                .notify(PoolEvent::HandshakeFailed(Some(pid.clone())))
                .await;
            let _ = self
                .peer_emitter
                .emit(&PeerEvent::HandshakeFailed(Some(pid)))
                .await;
            let _ = queued.conn_closed_tx.send(Err(Error::AccessDenied)).await;
            return;
        }

        // Wake the task waiting in `ConnQueue::handle`, or it keeps
        // its connection slot forever.
        let conn_closed_tx = queued.conn_closed_tx.clone();
        if let Err(err) = self.new_peer(queued, pid, negotiated).await {
            error!("new_peer failed: {err}");
            let _ = conn_closed_tx.send(Err(err)).await;
        }
    }

    /// Build a Peer from a post-handshake `QueuedConn` and run it.
    async fn new_peer(
        self: &Arc<Self>,
        queued: QueuedConn,
        pid: PeerID,
        negotiated: Vec<ProtocolID>,
    ) -> Result<()> {
        let negotiated: HashSet<ProtocolID> = negotiated.into_iter().collect();

        let res = self.insert_peer(queued, &pid, negotiated).await;
        if matches!(res, Err(Error::PeerAlreadyConnected)) {
            self.monitor
                .notify(PoolEvent::PeerAlreadyConnected(pid.clone()))
                .await;
        }
        let (peer, replaced) = res?;

        let on_disconnect = {
            let this = self.clone();
            let peer = peer.clone();
            |result| async move {
                if let TaskResult::Completed(_) = result
                    && let Err(err) = this.remove_peer(&peer).await
                {
                    error!("Failed to remove peer {}: {err}", peer.id());
                }
            }
        };

        self.task_group
            .spawn_then(peer.clone().run(), on_disconnect);

        // Same peer, now on the connection both nodes keep. It stays in
        // the pool, so no `Removed`/`Added` events.
        if let Some(old) = replaced {
            debug!("Replace the connection of peer {pid}");
            let _ = old.shutdown().await;
            return Ok(());
        }

        info!("Add new peer {pid}");
        self.monitor.notify(PoolEvent::NewPeer(pid.clone())).await;
        let _ = self.peer_emitter.emit(&PeerEvent::Added(pid)).await;

        Ok(())
    }

    /// Builds the peer and adds it. If a peer with this id is already in
    /// the pool, the new connection either replaces it (returned as the
    /// second value, for the caller to shut down) or is rejected; see
    /// `new_conn_wins`. Connections are admitted in parallel, so the check
    /// and the insert run under one write lock. `Peer::new` does not wait
    /// for the remote, so the lock is held only briefly.
    async fn insert_peer(
        self: &Arc<Self>,
        queued: QueuedConn,
        pid: &PeerID,
        negotiated: HashSet<ProtocolID>,
    ) -> Result<(Arc<Peer>, Option<Arc<Peer>>)> {
        let mut peers = self.peers.write().await;
        if let Some(old) = peers.get(pid)
            && !new_conn_wins(&self.id, pid, old.direction(), &queued.direction)
        {
            return Err(Error::PeerAlreadyConnected);
        }
        let conn_id = self.next_conn_id.fetch_add(1, Ordering::Relaxed);
        let peer = Peer::new(self.clone(), queued, pid.clone(), conn_id, negotiated)?;
        let replaced = peers.insert(pid.clone(), peer.clone());
        Ok((peer, replaced))
    }

    /// Shuts down the peer and removes it from the peer list. Does nothing
    /// if the pool holds another peer under this id (the connection was
    /// replaced) or none.
    async fn remove_peer(&self, peer: &Arc<Peer>) -> Result<()> {
        let pid = peer.id();
        {
            let mut peers = self.peers.write().await;
            match peers.get(pid) {
                Some(p) if p.conn_id() == peer.conn_id() => peers.remove(pid),
                _ => return Ok(()),
            };
        }

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

/// Decides whether a new connection to a peer replaces the one already in
/// the pool. When two nodes dial each other at the same time, each sees
/// both connections in opposite directions. Both keep the one dialed by
/// the node with the smaller PeerID, so they keep the same connection.
/// Two connections in the same direction keep the first.
fn new_conn_wins(own: &PeerID, remote: &PeerID, old: &ConnDirection, new: &ConnDirection) -> bool {
    match (old, new) {
        // The new one was dialed by us.
        (ConnDirection::Inbound, ConnDirection::Outbound) => own.0 < remote.0,
        // The new one was dialed by the remote.
        (ConnDirection::Outbound, ConnDirection::Inbound) => remote.0 < own.0,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_nodes_keep_the_same_connection() {
        let a = PeerID([1; 32]);
        let b = PeerID([2; 32]);
        let (inb, outb) = (ConnDirection::Inbound, ConnDirection::Outbound);

        // A has the smaller id, so both keep the connection A dialed:
        // outbound on A, inbound on B. The other one never wins.
        assert!(new_conn_wins(&a, &b, &inb, &outb));
        assert!(!new_conn_wins(&a, &b, &outb, &inb));
        assert!(new_conn_wins(&b, &a, &outb, &inb));
        assert!(!new_conn_wins(&b, &a, &inb, &outb));

        // Same direction: the first one stays.
        assert!(!new_conn_wins(&a, &b, &outb, &outb));
        assert!(!new_conn_wins(&a, &b, &inb, &inb));
    }
}
