use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use log::{debug, info, warn};

use karyon_core::{
    async_runtime::{Executor, lock::SemaphoreGuard},
    async_util::{TaskGroup, TaskResult},
};

use karyon_net::Endpoint;

use crate::{
    Error, PeerID, Result,
    admission::{AdmitConfig, Admitted, admit},
    peer::{Peer, PeerSession},
    peer_events::PeerEventsRef,
    protocol::{Broadcaster, ProtocolID, ProtocolRegistryRef},
    queued_conn::{ConnQueueRef, Incoming},
    sessions::{Sessions, SessionsRef},
};

/// `Arc` reference to the `PeerPool`.
pub type PeerPoolRef = Arc<PeerPool>;

/// Takes the connections from the listener and the connector, admits
/// them and runs one `Peer` per admitted connection.
pub struct PeerPool {
    /// Peer's ID
    pub id: PeerID,

    /// Connections handed over by the listener and the connector.
    conn_queue: ConnQueueRef,

    /// One session per running peer.
    sessions: SessionsRef,

    /// Next connection id, see `PeerSession::conn_id`.
    next_conn_id: AtomicU64,

    /// Attached protocols. Source of truth for the handshake's mandatory
    /// check and version negotiation.
    registry: ProtocolRegistryRef,

    /// Where peer events are reported.
    events: PeerEventsRef,

    /// Managing spawned tasks.
    task_group: TaskGroup,

    /// A global Executor
    executor: Executor,

    config: AdmitConfig,
}

impl PeerPool {
    /// Creates a new PeerPool
    pub(crate) fn new(
        id: &PeerID,
        conn_queue: ConnQueueRef,
        registry: ProtocolRegistryRef,
        config: AdmitConfig,
        events: PeerEventsRef,
        executor: Executor,
    ) -> PeerPoolRef {
        Arc::new(Self {
            id: id.clone(),
            conn_queue,
            sessions: Sessions::new(),
            next_conn_id: AtomicU64::new(0),
            registry,
            events,
            task_group: TaskGroup::with_executor(executor.clone()),
            executor,
            config,
        })
    }

    /// Starts the [`PeerPool`]
    pub async fn start(self: &PeerPoolRef) -> Result<()> {
        self.task_group.spawn(self.clone().run());
        Ok(())
    }

    /// Shuts down
    pub async fn shutdown(&self) {
        // Cancel the task group first. This waits for the run loop to
        // actually stop, so nothing takes from the queue or adds a peer
        // while we clean up.
        self.task_group.cancel().await;

        // Drop the connections still waiting in the queue.
        while !self.conn_queue.is_empty().await {
            let incoming = self.conn_queue.recv().await;
            self.events
                .disconnected(&incoming.conn.remote_endpoint)
                .await;
        }

        for session in self.sessions.drain().await {
            session.stop().await;
            self.events.disconnected(&session.info.endpoint).await;
        }
    }

    /// Broadcast a message to all connected peers.
    pub async fn broadcast(&self, proto_id: &ProtocolID, msg: Vec<u8>) {
        self.sessions.broadcast(proto_id, msg).await
    }

    /// Broadcast a message to a specific set of peers.
    pub async fn broadcast_to(
        &self,
        proto_id: &ProtocolID,
        msg: Vec<u8>,
        targets: &HashSet<PeerID>,
    ) {
        self.sessions.broadcast_to(proto_id, msg, targets).await
    }

    /// Send a message to a specific peer on the given protocol. Returns
    /// `PeerNotFound` if the peer is not currently in the pool.
    pub async fn send_to(
        &self,
        peer_id: &PeerID,
        proto_id: &ProtocolID,
        msg: Vec<u8>,
    ) -> Result<()> {
        self.sessions.send_to(peer_id, proto_id, msg).await
    }

    /// Returns the negotiated protocol set for a peer.
    pub async fn peer_protocol_set(&self, pid: &PeerID) -> Option<HashSet<ProtocolID>> {
        self.sessions.negotiated(pid).await
    }

    /// Checks if the peer list contains a peer with the given peer id
    pub async fn contains_peer(&self, pid: &PeerID) -> bool {
        self.sessions.contains(pid).await
    }

    /// Returns the number of currently connected peers.
    pub async fn peers_len(&self) -> usize {
        self.sessions.len().await
    }

    /// Returns a map of inbound peers with their endpoints.
    pub async fn inbound_peers(&self) -> HashMap<PeerID, Endpoint> {
        self.sessions.peers_by(|info| info.is_inbound()).await
    }

    /// Returns a map of outbound peers with their endpoints.
    pub async fn outbound_peers(&self) -> HashMap<PeerID, Endpoint> {
        self.sessions.peers_by(|info| !info.is_inbound()).await
    }

    async fn run(self: PeerPoolRef) {
        loop {
            let incoming = self.conn_queue.recv().await;
            // One task per connection, so a slow or silent peer does not
            // hold back the others. Every connection holds a listener or
            // connector slot, which bounds these tasks.
            self.task_group.spawn(self.clone().handle(incoming));
        }
    }

    /// Admits a connection and adds its peer. On failure the connection
    /// is dropped and its slot freed.
    async fn handle(self: PeerPoolRef, incoming: Incoming) {
        let Incoming { conn, slot } = incoming;
        let endpoint = conn.remote_endpoint.clone();

        let res = match admit(&self.id, conn, &self.registry, &self.config).await {
            Ok(admitted) => self.add_peer(admitted, slot).await,
            Err(rejected) => {
                self.events.handshake_failed(rejected.peer_id).await;
                Err(rejected.error)
            }
        };

        if let Err(err) = res {
            debug!("Connection from {endpoint} not admitted: {err}");
            self.events.disconnected(&endpoint).await;
        }
    }

    /// Builds the peer, adds its session and runs it.
    async fn add_peer(self: &PeerPoolRef, admitted: Admitted, slot: SemaphoreGuard) -> Result<()> {
        let Admitted {
            info,
            conn,
            protocols,
        } = admitted;
        let pid = info.id.clone();

        let make = || {
            let peer = Peer::new(
                info.clone(),
                conn,
                protocols,
                Broadcaster::new(self.sessions.clone()),
                Duration::from_secs(self.config.handshake_timeout),
                self.executor.clone(),
            )?;
            let conn_id = self.next_conn_id.fetch_add(1, Ordering::Relaxed);
            let session = peer.session(conn_id, slot);
            Ok((peer, session))
        };
        let res = self.sessions.insert(&self.id, &info, make).await;
        if matches!(res, Err(Error::PeerAlreadyConnected)) {
            self.events.already_connected(&pid).await;
        }
        let (peer, session, replaced) = res?;

        let on_end = {
            let this = self.clone();
            |result: TaskResult<Result<()>>| async move {
                if let TaskResult::Completed(_) = result {
                    this.remove_peer(session).await;
                }
            }
        };
        self.task_group.spawn_then(peer.run(), on_end);

        // Same peer, now on the connection both nodes keep. It stays in
        // the pool, so no `Removed`/`Added` events.
        if let Some(old) = replaced {
            debug!("Replace the connection of peer {pid}");
            old.stop().await;
            return Ok(());
        }

        info!("Add new peer {pid}");
        self.events.added(&pid).await;
        Ok(())
    }

    /// Called once the peer's `run` returns. Stops the session and removes
    /// it, unless the pool holds another connection under this id (this
    /// one was replaced and already stopped).
    async fn remove_peer(&self, session: PeerSession) {
        if self.sessions.remove(&session).await {
            let pid = &session.info.id;
            session.stop().await;
            self.events.removed(pid).await;
            warn!("Peer {pid} removed");
        }
        self.events.disconnected(&session.info.endpoint).await;
    }
}
