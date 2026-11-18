mod connection;
mod peer_id;

use std::{
    collections::HashSet,
    sync::{Arc, Weak},
    time::Duration,
};

use async_channel::{Receiver, Sender};
use log::{error, trace};

use karyon_core::{
    async_runtime::Executor,
    async_util::{TaskGroup, TaskResult, timeout},
};

use crate::{
    Config, Result,
    conn_direction::ConnDirection,
    conn_queue::QueuedConn,
    endpoint::Endpoint,
    peer_pool::PeerPool,
    protocol::{PeerConn, ProtocolEvent, ProtocolID},
};

pub use peer_id::PeerID;

use connection::Wire;

/// A connected peer. Holds a `Wire` that hides the wire shape
/// (single framed pipe vs. per-protocol streams).
pub struct Peer {
    id: PeerID,
    /// Unique per connection in the pool. A peer that reconnects or whose
    /// connection is replaced gets a new one, while `id` stays the same.
    conn_id: u64,
    peer_pool: Weak<PeerPool>,

    direction: ConnDirection,
    remote_endpoint: Endpoint,

    connection: Arc<dyn Wire>,
    /// Wakes the `ConnQueue::handle` call that queued this connection, so
    /// the listener/connector task waiting on it can end.
    conn_closed_tx: Sender<Result<()>>,

    negotiated_protocols: HashSet<ProtocolID>,
    /// Peer tasks send the reason the peer must stop. `run` waits on
    /// `stop_reason_rx` and returns it.
    stop_reason_tx: Sender<Result<()>>,
    stop_reason_rx: Receiver<Result<()>>,
    config: Arc<Config>,
    executor: Executor,
    task_group: TaskGroup,
}

impl Peer {
    /// Builds the peer and its wire. Does not wait for the remote, so the
    /// pool can call it while holding its peers lock.
    pub(crate) fn new(
        peer_pool: Arc<PeerPool>,
        queued: QueuedConn,
        id: PeerID,
        conn_id: u64,
        negotiated_protocols: HashSet<ProtocolID>,
    ) -> Result<Arc<Self>> {
        let config = peer_pool.config.clone();
        let executor = peer_pool.executor.clone();
        let task_group = TaskGroup::with_executor(executor.clone());
        let (stop_reason_tx, stop_reason_rx) = async_channel::bounded::<Result<()>>(1);

        let remote_endpoint = queued.remote_endpoint.clone();
        let direction = queued.direction.clone();
        let conn_closed_tx = queued.conn_closed_tx.clone();

        let connection = connection::from_queued(
            queued,
            &negotiated_protocols,
            &task_group,
            stop_reason_tx.clone(),
        )?;

        let peer_pool_weak = Arc::downgrade(&peer_pool);
        Ok(Arc::new(Peer {
            id,
            conn_id,
            peer_pool: peer_pool_weak,
            direction,
            remote_endpoint,
            connection,
            conn_closed_tx,
            negotiated_protocols,
            stop_reason_tx,
            stop_reason_rx,
            config,
            executor,
            task_group,
        }))
    }

    pub async fn send(&self, proto_id: ProtocolID, msg: Vec<u8>) -> Result<()> {
        self.connection.send(&proto_id, msg).await
    }

    pub async fn recv(&self, proto_id: &ProtocolID) -> Result<ProtocolEvent> {
        self.connection.recv(proto_id).await
    }

    pub async fn broadcast(&self, proto_id: &ProtocolID, msg: Vec<u8>) {
        self.peer_pool().broadcast(proto_id, msg).await;
    }

    pub fn id(&self) -> &PeerID {
        &self.id
    }

    /// Identifies this connection to the peer, see the field.
    pub(crate) fn conn_id(&self) -> u64 {
        self.conn_id
    }

    pub fn config(&self) -> Arc<Config> {
        self.config.clone()
    }

    pub fn executor(&self) -> Executor {
        self.executor.clone()
    }

    pub fn remote_endpoint(&self) -> &Endpoint {
        &self.remote_endpoint
    }

    pub fn is_inbound(&self) -> bool {
        matches!(self.direction, ConnDirection::Inbound)
    }

    pub fn direction(&self) -> &ConnDirection {
        &self.direction
    }

    pub fn negotiated_protocols(&self) -> &HashSet<ProtocolID> {
        &self.negotiated_protocols
    }

    pub(crate) async fn run(self: Arc<Self>) -> Result<()> {
        // QUIC opens its protocol streams here. A peer that does not open
        // them in time is stopped, like on any other connection error.
        let setup_timeout = Duration::from_secs(self.config.handshake_timeout);
        let setup = self
            .connection
            .setup(&self.task_group, self.stop_reason_tx.clone());
        timeout(setup_timeout, setup).await??;

        self.run_connect_protocols().await;

        self.stop_reason_rx.recv().await?
    }

    pub(crate) async fn shutdown(self: &Arc<Self>) -> Result<()> {
        trace!("peer {} shutting down", self.id);

        let _ = self.connection.shutdown().await;
        let _ = self.stop_reason_tx.try_send(Ok(()));

        let _ = self.conn_closed_tx.send(Ok(())).await;
        self.task_group.cancel().await;
        Ok(())
    }

    async fn run_connect_protocols(self: &Arc<Self>) {
        for (proto_id, constructor) in self.peer_pool().protocols.read().await.iter() {
            if !self.negotiated_protocols.contains(proto_id) {
                trace!("peer {} skip protocol {proto_id} (not negotiated)", self.id);
                continue;
            }
            trace!("peer {} run protocol {proto_id}", self.id);

            let peer_conn = PeerConn::new(self.clone(), proto_id.clone());
            let protocol = constructor(peer_conn);

            let on_failure = {
                let this = self.clone();
                let proto_id = proto_id.clone();
                |result: TaskResult<Result<()>>| async move {
                    if let TaskResult::Completed(res) = result {
                        if res.is_err() {
                            error!("protocol {proto_id} stopped");
                        }
                        let _ = this.stop_reason_tx.try_send(res);
                    }
                }
            };

            self.task_group.spawn_then(protocol.start(), on_failure);
        }
    }

    fn peer_pool(&self) -> Arc<PeerPool> {
        self.peer_pool.upgrade().unwrap()
    }
}
