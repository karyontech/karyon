mod connection;
mod info;

use std::{sync::Arc, time::Duration};

use async_channel::{Receiver, Sender};
use log::{error, trace};

use karyon_core::{
    async_runtime::{Executor, lock::SemaphoreGuard},
    async_util::{TaskGroup, TaskGroupRef, TaskResult, timeout},
};

use crate::{
    Result,
    protocol::{Broadcaster, PeerConn, ProtocolID, ResolvedProtocol},
    queued_conn::QueuedConn,
};

pub use info::{SessionInfo, SessionInfoRef};

pub(crate) use connection::WireRef;

/// One admitted connection to a peer, from the handshake until it stops:
/// the wire, its tasks and the slot. The PeerPool runs it, `Sessions`
/// keeps a clone of it.
#[derive(Clone)]
pub(crate) struct PeerSession {
    pub(crate) info: SessionInfoRef,
    /// Unique per connection in the pool. A peer that reconnects or whose
    /// connection is replaced gets a new one, while the id stays the same.
    pub(crate) conn_id: u64,
    /// From the handshake, the same on both nodes. Picks which of two
    /// connections to the same peer stays (see `Sessions::insert`).
    pub(crate) conn_key: u64,
    /// Hides the wire shape (single framed pipe vs. per-protocol streams).
    wire: WireRef,
    /// Session tasks send the reason the session must stop. `run` waits on
    /// `stop_rx` and returns it.
    stop_tx: Sender<Result<()>>,
    stop_rx: Receiver<Result<()>>,
    /// The wire and protocol tasks of this session only.
    task_group: TaskGroupRef,
    /// The listener or connector slot, freed once the last clone of the
    /// session is dropped.
    _slot: Arc<SemaphoreGuard>,
}

impl PeerSession {
    /// Builds the session and its wire. Does not wait for the remote.
    pub(crate) fn new(
        info: SessionInfoRef,
        conn: QueuedConn,
        conn_id: u64,
        conn_key: u64,
        slot: SemaphoreGuard,
        executor: Executor,
    ) -> Result<Self> {
        let task_group = Arc::new(TaskGroup::with_executor(executor));
        let (stop_tx, stop_rx) = async_channel::bounded::<Result<()>>(1);
        let wire = connection::from_queued(conn, &info.negotiated, &task_group, stop_tx.clone())?;

        Ok(Self {
            info,
            conn_id,
            conn_key,
            wire,
            stop_tx,
            stop_rx,
            task_group,
            _slot: Arc::new(slot),
        })
    }

    /// Sets up the wire, starts the protocols and waits for the reason to
    /// stop. The pool stops the session once this returns.
    pub(crate) async fn run(
        &self,
        protocols: Vec<ResolvedProtocol>,
        broadcaster: Broadcaster,
        setup_timeout: Duration,
    ) -> Result<()> {
        // QUIC opens its protocol streams here. A peer that does not open
        // them in time is stopped, like on any other connection error.
        let setup = self.wire.setup(&self.task_group, self.stop_tx.clone());
        timeout(setup_timeout, setup).await??;

        for (proto_id, constructor) in protocols {
            trace!("peer {} run protocol {proto_id}", self.info.id);
            let peer_conn = PeerConn::new(
                self.info.clone(),
                self.wire.clone(),
                proto_id.clone(),
                broadcaster.clone(),
            );
            let protocol = constructor(peer_conn);

            let on_end = {
                let stop_tx = self.stop_tx.clone();
                |result: TaskResult<Result<()>>| async move {
                    if let TaskResult::Completed(res) = result {
                        if res.is_err() {
                            error!("protocol {proto_id} stopped");
                        }
                        let _ = stop_tx.try_send(res);
                    }
                }
            };
            self.task_group.spawn_then(protocol.start(), on_end);
        }

        self.stop_rx.recv().await?
    }

    pub(crate) async fn send(&self, proto_id: &ProtocolID, msg: Vec<u8>) -> Result<()> {
        self.wire.send(proto_id, msg).await
    }

    /// Closes the wire, wakes `run` and cancels the session's tasks.
    /// Call it once per session.
    pub(crate) async fn stop(&self) {
        trace!("peer {} shutting down", self.info.id);
        let _ = self.wire.shutdown().await;
        let _ = self.stop_tx.try_send(Ok(()));
        self.task_group.cancel().await;
    }
}
