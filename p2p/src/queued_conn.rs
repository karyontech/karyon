use karyon_core::{async_runtime::lock::SemaphoreGuard, async_util::AsyncQueueRef};

use karyon_net::{Endpoint, FramedConn, FramedReader, FramedWriter};

#[cfg(feature = "quic")]
use karyon_net::quic::QuicConn;

use crate::{Error, PeerID, Result, codec::PeerNetMsgCodec, conn_direction::ConnDirection};

/// Queue that hands connections from the listener and the connector to
/// the PeerPool.
pub(crate) type ConnQueueRef = AsyncQueueRef<Incoming>;

/// Max connections waiting for the PeerPool. The pool takes each one
/// right away, so the queue is rarely full.
pub(crate) const CONN_QUEUE_SIZE: usize = 64;

/// A connection for the PeerPool to handshake and turn into a
/// `PeerSession`.
/// Carries the raw split halves plus metadata.
pub(crate) struct QueuedConn {
    pub reader: FramedReader<PeerNetMsgCodec>,
    pub writer: FramedWriter<PeerNetMsgCodec>,
    pub direction: ConnDirection,
    pub remote_endpoint: Endpoint,
    /// PeerID derived from the secure transport (TLS cert).
    /// `None` for unauthenticated transports (TCP, Unix).
    /// The application handshake asserts `vermsg.peer_id == this` when
    /// `Some`, so a peer can't claim a PeerID it can't prove.
    pub verified_peer_id: Option<PeerID>,
    #[cfg(feature = "quic")]
    pub quic_conn: Option<QuicConn>,
}

impl QueuedConn {
    /// Splits a stream connection. For QUIC, `conn` is the handshake
    /// stream; add the connection with `with_quic`.
    pub(crate) fn new(
        conn: FramedConn<PeerNetMsgCodec>,
        direction: ConnDirection,
        verified_peer_id: Option<PeerID>,
    ) -> Result<Self> {
        let remote_endpoint = conn
            .peer_endpoint()
            .ok_or(Error::InvalidEndpoint("missing peer endpoint".into()))?;
        let (reader, writer) = conn.split();
        Ok(Self {
            reader,
            writer,
            direction,
            remote_endpoint,
            verified_peer_id,
            #[cfg(feature = "quic")]
            quic_conn: None,
        })
    }

    /// Adds the QUIC connection that carries the protocol streams.
    #[cfg(feature = "quic")]
    pub(crate) fn with_quic(mut self, quic_conn: QuicConn) -> Self {
        self.quic_conn = Some(quic_conn);
        self
    }
}

/// A connection and the listener or connector slot it holds. The slot
/// is freed once the connection is closed.
pub(crate) struct Incoming {
    pub conn: QueuedConn,
    pub slot: SemaphoreGuard,
}
