use std::sync::Arc;

use async_channel::Sender;
use log::trace;

use karyon_core::{async_runtime::lock::SemaphoreGuard, async_util::TaskGroupRef};

use crate::{
    Result,
    peer::{PeerInfoRef, WireRef},
    protocol::ProtocolID,
};

/// The PeerPool's handle to a running peer: send to it and stop it.
/// The peer itself is owned by its `run` task.
#[derive(Clone)]
pub(crate) struct PeerSession {
    pub(crate) info: PeerInfoRef,
    /// Unique per connection in the pool. A peer that reconnects or whose
    /// connection is replaced gets a new one, while the id stays the same.
    pub(crate) conn_id: u64,
    wire: WireRef,
    stop_tx: Sender<Result<()>>,
    task_group: TaskGroupRef,
    /// The listener or connector slot, freed once the last clone of the
    /// session is dropped.
    _slot: Arc<SemaphoreGuard>,
}

impl PeerSession {
    pub(crate) fn new(
        info: PeerInfoRef,
        conn_id: u64,
        wire: WireRef,
        stop_tx: Sender<Result<()>>,
        task_group: TaskGroupRef,
        slot: SemaphoreGuard,
    ) -> Self {
        Self {
            info,
            conn_id,
            wire,
            stop_tx,
            task_group,
            _slot: Arc::new(slot),
        }
    }

    pub(crate) async fn send(&self, proto_id: &ProtocolID, msg: Vec<u8>) -> Result<()> {
        self.wire.send(proto_id, msg).await
    }

    /// Closes the wire, wakes the peer's `run` and cancels its tasks.
    /// Call it once per session.
    pub(crate) async fn stop(&self) {
        trace!("peer {} shutting down", self.info.id);
        let _ = self.wire.shutdown().await;
        let _ = self.stop_tx.try_send(Ok(()));
        self.task_group.cancel().await;
    }
}
