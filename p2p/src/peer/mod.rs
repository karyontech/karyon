mod connection;
mod info;
mod peer_id;
mod session;

use std::{sync::Arc, time::Duration};

use async_channel::{Receiver, Sender};
use log::{error, trace};

use karyon_core::{
    async_runtime::{Executor, lock::SemaphoreGuard},
    async_util::{TaskGroup, TaskGroupRef, TaskResult, timeout},
};

use crate::{
    Result,
    protocol::{Broadcaster, PeerConn, ResolvedProtocol},
    queued_conn::QueuedConn,
};

pub use peer_id::PeerID;

pub(crate) use connection::WireRef;
pub(crate) use info::{PeerInfo, PeerInfoRef};
pub(crate) use session::PeerSession;

/// A connected peer. Owned by its `run` task only; the PeerPool keeps a
/// `PeerSession` for it. Knows nothing about the pool.
pub(crate) struct Peer {
    info: PeerInfoRef,
    /// Hides the wire shape (single framed pipe vs. per-protocol streams).
    wire: WireRef,
    /// Constructors of the negotiated protocols, run once the wire is set up.
    protocols: Vec<ResolvedProtocol>,
    broadcaster: Broadcaster,
    /// Max time for `Wire::setup`.
    setup_timeout: Duration,
    /// Peer tasks send the reason the peer must stop. `run` waits on
    /// `stop_rx` and returns it.
    stop_tx: Sender<Result<()>>,
    stop_rx: Receiver<Result<()>>,
    task_group: TaskGroupRef,
}

impl Peer {
    /// Builds the peer and its wire. Does not wait for the remote, so the
    /// pool can call it while holding its sessions lock.
    pub(crate) fn new(
        info: PeerInfoRef,
        conn: QueuedConn,
        protocols: Vec<ResolvedProtocol>,
        broadcaster: Broadcaster,
        setup_timeout: Duration,
        executor: Executor,
    ) -> Result<Self> {
        let task_group = Arc::new(TaskGroup::with_executor(executor));
        let (stop_tx, stop_rx) = async_channel::bounded::<Result<()>>(1);

        let wire = connection::from_queued(conn, &info.negotiated, &task_group, stop_tx.clone())?;

        Ok(Self {
            info,
            wire,
            protocols,
            broadcaster,
            setup_timeout,
            stop_tx,
            stop_rx,
            task_group,
        })
    }

    /// The pool's handle to this peer. `slot` is freed once the session
    /// and all its clones are dropped.
    pub(crate) fn session(&self, conn_id: u64, slot: SemaphoreGuard) -> PeerSession {
        PeerSession::new(
            self.info.clone(),
            conn_id,
            self.wire.clone(),
            self.stop_tx.clone(),
            self.task_group.clone(),
            slot,
        )
    }

    /// Sets up the wire, starts the protocols and waits for the reason to
    /// stop. The pool stops the session once this returns.
    pub(crate) async fn run(self) -> Result<()> {
        // QUIC opens its protocol streams here. A peer that does not open
        // them in time is stopped, like on any other connection error.
        let setup = self.wire.setup(&self.task_group, self.stop_tx.clone());
        timeout(self.setup_timeout, setup).await??;

        self.start_protocols();

        self.stop_rx.recv().await?
    }

    fn start_protocols(&self) {
        for (proto_id, constructor) in self.protocols.iter() {
            trace!("peer {} run protocol {proto_id}", self.info.id);

            let peer_conn = PeerConn::new(
                self.info.clone(),
                self.wire.clone(),
                proto_id.clone(),
                self.broadcaster.clone(),
            );
            let protocol = constructor(peer_conn);

            let on_failure = {
                let stop_tx = self.stop_tx.clone();
                let proto_id = proto_id.clone();
                |result: TaskResult<Result<()>>| async move {
                    if let TaskResult::Completed(res) = result {
                        if res.is_err() {
                            error!("protocol {proto_id} stopped");
                        }
                        let _ = stop_tx.try_send(res);
                    }
                }
            };

            self.task_group.spawn_then(protocol.start(), on_failure);
        }
    }
}
