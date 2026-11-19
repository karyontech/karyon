use crate::{protocol::ProtocolID, sessions::SessionsRef};

/// Lets a protocol send to every connected peer. Holds only the session
/// table, not the pool. The table is drained on shutdown, so a broadcast
/// after that sends nothing.
#[derive(Clone)]
pub(crate) struct Broadcaster {
    sessions: SessionsRef,
}

impl Broadcaster {
    pub(crate) fn new(sessions: SessionsRef) -> Self {
        Self { sessions }
    }

    /// Sends `msg` on `proto_id` to every connected peer.
    pub(crate) async fn broadcast(&self, proto_id: &ProtocolID, msg: Vec<u8>) {
        self.sessions.broadcast(proto_id, msg).await;
    }
}
