use crate::{
    Error, Result,
    peer::{PeerInfoRef, WireRef},
    protocol::{Broadcaster, ProtocolEvent, ProtocolID},
};

/// Per-protocol view of a peer with the protocol id baked in. Built
/// by karyon and handed to the protocol constructor. A protocol can
/// only send and receive on its own id.
pub struct PeerConn {
    info: PeerInfoRef,
    wire: WireRef,
    proto_id: ProtocolID,
    broadcaster: Broadcaster,
}

impl PeerConn {
    /// karyon-internal constructor. User code receives a fully-built
    /// `PeerConn` from the protocol constructor closure.
    pub(crate) fn new(
        info: PeerInfoRef,
        wire: WireRef,
        proto_id: ProtocolID,
        broadcaster: Broadcaster,
    ) -> Self {
        Self {
            info,
            wire,
            proto_id,
            broadcaster,
        }
    }

    /// Send pre-encoded payload bytes to the peer on this protocol.
    pub async fn send(&self, payload: Vec<u8>) -> Result<()> {
        self.wire.send(&self.proto_id, payload).await
    }

    /// Receive the next message bytes. Returns `Err(PeerShutdown)`
    /// when the peer is closing -- callers can treat that as an
    /// orderly end of stream.
    pub async fn recv(&self) -> Result<Vec<u8>> {
        match self.wire.recv(&self.proto_id).await? {
            ProtocolEvent::Message(bytes) => Ok(bytes),
            ProtocolEvent::Shutdown => Err(Error::PeerShutdown),
        }
    }

    /// Broadcast pre-encoded bytes to every connected peer on this
    /// protocol, this one included. Does nothing once the node is shut
    /// down.
    pub async fn broadcast(&self, payload: Vec<u8>) {
        self.broadcaster.broadcast(&self.proto_id, payload).await
    }

    /// Everything known about the remote peer. Cheap to clone.
    pub fn info(&self) -> &PeerInfoRef {
        &self.info
    }
}
