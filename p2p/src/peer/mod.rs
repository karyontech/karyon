mod connection;
mod peer_id;

use std::{
    collections::HashSet,
    fmt,
    sync::{Arc, Weak},
};

use async_channel::{Receiver, Sender};
use log::{error, trace};

use karyon_core::{
    async_runtime::Executor,
    async_util::{TaskGroup, TaskResult},
};

use crate::{
    conn_queue::QueuedConn,
    endpoint::Endpoint,
    peer_pool::PeerPool,
    protocol::{PeerConn, ProtocolEvent, ProtocolID},
    Config, Result,
};

pub use peer_id::PeerID;

use connection::PeerConnection;

#[derive(Clone, Debug)]
pub enum ConnDirection {
    Inbound,
    Outbound,
}

impl fmt::Display for ConnDirection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ConnDirection::Inbound => write!(f, "Inbound"),
            ConnDirection::Outbound => write!(f, "Outbound"),
        }
    }
}

/// A connected peer. Holds a `PeerConnection` that hides the wire
/// shape (single framed pipe vs. per-protocol streams).
pub struct Peer {
    own_id: PeerID,
    id: PeerID,
    peer_pool: Weak<PeerPool>,

    direction: ConnDirection,
    remote_endpoint: Endpoint,

    connection: Arc<dyn PeerConnection>,
    disconnect_signal: Sender<Result<()>>,

    negotiated_protocols: HashSet<ProtocolID>,
    stop_chan: (Sender<Result<()>>, Receiver<Result<()>>),
    config: Arc<Config>,
    executor: Executor,
    task_group: TaskGroup,
}

impl Peer {
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

    pub fn own_id(&self) -> &PeerID {
        &self.own_id
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
        self.run_connect_protocols().await;
        let stop_signal = self.stop_chan.1.recv().await?;
        stop_signal
    }

    pub(crate) async fn shutdown(self: &Arc<Self>) -> Result<()> {
        trace!("peer {} shutting down", self.id);

        let _ = self.connection.shutdown().await;
        let _ = self.stop_chan.0.try_send(Ok(()));

        let _ = self.disconnect_signal.send(Ok(())).await;
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
            let protocol = match constructor(peer_conn) {
                Ok(p) => p,
                Err(err) => {
                    error!("Failed to build protocol {proto_id}: {err}");
                    continue;
                }
            };

            let on_failure = {
                let this = self.clone();
                let proto_id = proto_id.clone();
                |result: TaskResult<Result<()>>| async move {
                    if let TaskResult::Completed(res) = result {
                        if res.is_err() {
                            error!("protocol {proto_id} stopped");
                        }
                        let _ = this.stop_chan.0.try_send(res);
                    }
                }
            };

            self.task_group.spawn(protocol.start(), on_failure);
        }
    }

    fn peer_pool(&self) -> Arc<PeerPool> {
        self.peer_pool.upgrade().unwrap()
    }
}

impl Peer {
    pub(crate) async fn new(
        peer_pool: Arc<PeerPool>,
        queued: QueuedConn,
        id: PeerID,
        negotiated_protocols: HashSet<ProtocolID>,
        protocol_ids: impl IntoIterator<Item = ProtocolID> + Clone,
    ) -> Result<Arc<Self>> {
        let own_id = peer_pool.id.clone();
        let config = peer_pool.config.clone();
        let executor = peer_pool.executor.clone();
        let task_group = TaskGroup::with_executor(executor.clone());
        let stop_chan = async_channel::bounded::<Result<()>>(1);

        let remote_endpoint = queued.remote_endpoint.clone();
        let direction = queued.direction.clone();
        let disconnect_signal = queued.disconnect_signal.clone();

        let connection = connection::from_queued(
            queued,
            &negotiated_protocols,
            protocol_ids,
            &task_group,
            stop_chan.0.clone(),
        )
        .await?;

        let peer_pool_weak = Arc::downgrade(&peer_pool);
        Ok(Arc::new(Peer {
            own_id,
            id,
            peer_pool: peer_pool_weak,
            direction,
            remote_endpoint,
            connection,
            disconnect_signal,
            negotiated_protocols,
            stop_chan,
            config,
            executor,
            task_group,
        }))
    }
}
