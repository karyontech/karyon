use std::sync::Arc;

use crate::{
    peer::Peer,
    protocol::{ProtocolEvent, ProtocolID},
    Error, Result,
};

/// Per-protocol view of a Peer with the protocol id baked in. Built
/// by karyon and handed to the protocol constructor. Hides the
/// underlying `proto_id` routing so user code never types it.
pub struct PeerConn {
    peer: Arc<Peer>,
    proto_id: ProtocolID,
}

impl PeerConn {
    /// karyon-internal constructor. User code receives a fully-built
    /// `PeerConn` from the protocol constructor closure.
    pub(crate) fn new(peer: Arc<Peer>, proto_id: ProtocolID) -> Self {
        Self { peer, proto_id }
    }

    /// Send pre-encoded payload bytes to the peer on this protocol.
    pub async fn send(&self, payload: Vec<u8>) -> Result<()> {
        self.peer.send(self.proto_id.clone(), payload).await
    }

    /// Receive the next message bytes. Returns `Err(PeerShutdown)`
    /// when the peer is closing -- callers can treat that as an
    /// orderly end of stream.
    pub async fn recv(&self) -> Result<Vec<u8>> {
        match self.peer.recv(&self.proto_id).await? {
            ProtocolEvent::Message(bytes) => Ok(bytes),
            ProtocolEvent::Shutdown => Err(Error::PeerShutdown),
        }
    }

    /// Broadcast pre-encoded bytes to every connected peer on this
    /// protocol.
    pub async fn broadcast(&self, payload: Vec<u8>) {
        self.peer.broadcast(&self.proto_id, payload).await
    }

    /// Underlying peer handle. Escape hatch for code that needs the
    /// raw `Arc<Peer>` (peer id, endpoint, lifecycle hooks).
    pub fn inner(&self) -> &Arc<Peer> {
        &self.peer
    }
}
