use std::{collections::VecDeque, sync::Arc};

use async_channel::Sender;

use karyon_core::{async_runtime::lock::Mutex, async_util::CondVar};

use karyon_net::{Endpoint, FramedConn, FramedReader, FramedWriter};

#[cfg(feature = "quic")]
use karyon_net::quic::QuicConn;

use crate::{codec::PeerNetMsgCodec, peer::ConnDirection, Error, PeerID, Result};

/// Framed peer-data-plane connection used by the queue.
type PeerConnRef = FramedConn<PeerNetMsgCodec>;

/// A connection queued for the PeerPool to handshake and turn into
/// a `Peer`. Carries the raw split halves plus metadata.
pub struct QueuedConn {
    pub reader: FramedReader<PeerNetMsgCodec>,
    pub writer: FramedWriter<PeerNetMsgCodec>,
    pub direction: ConnDirection,
    pub remote_endpoint: Endpoint,
    pub disconnect_signal: Sender<crate::Result<()>>,
    /// PeerID derived from the secure transport (TLS cert).
    /// `None` for unauthenticated transports (TCP, Unix).
    /// The application handshake asserts `vermsg.peer_id == this` when
    /// `Some`, so a peer can't claim a PeerID it can't prove.
    pub verified_peer_id: Option<PeerID>,
    #[cfg(feature = "quic")]
    pub quic_conn: Option<QuicConn>,
}

/// FIFO of pending connections awaiting handshake.
pub struct ConnQueue {
    queue: Mutex<VecDeque<QueuedConn>>,
    conn_available: CondVar,
}

impl ConnQueue {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            queue: Mutex::new(VecDeque::new()),
            conn_available: CondVar::new(),
        })
    }

    /// Handle a stream connection: split into halves, push, and
    /// block until disconnect.
    pub async fn handle(
        &self,
        conn: PeerConnRef,
        direction: ConnDirection,
        verified_peer_id: Option<PeerID>,
    ) -> Result<()> {
        let endpoint = conn
            .peer_endpoint()
            .ok_or(Error::InvalidEndpoint("missing peer endpoint".into()))?;
        let (reader, writer) = conn.split();

        let (disconnect_tx, disconnect_rx) = async_channel::bounded(1);
        let queued = QueuedConn {
            reader,
            writer,
            direction,
            remote_endpoint: endpoint,
            disconnect_signal: disconnect_tx,
            verified_peer_id,
            #[cfg(feature = "quic")]
            quic_conn: None,
        };

        self.queue.lock().await.push_back(queued);
        self.conn_available.signal();

        if let Ok(result) = disconnect_rx.recv().await {
            return result;
        }

        Ok(())
    }

    /// Handle a QUIC connection. The `conn` is the handshake stream;
    /// `quic_conn` is the full QUIC connection for protocol streams.
    #[cfg(feature = "quic")]
    pub async fn handle_quic(
        &self,
        conn: PeerConnRef,
        quic_conn: QuicConn,
        direction: ConnDirection,
        verified_peer_id: Option<PeerID>,
    ) -> Result<()> {
        let endpoint = conn
            .peer_endpoint()
            .ok_or(Error::InvalidEndpoint("missing peer endpoint".into()))?;
        let (reader, writer) = conn.split();

        let (disconnect_tx, disconnect_rx) = async_channel::bounded(1);
        let queued = QueuedConn {
            reader,
            writer,
            direction,
            remote_endpoint: endpoint,
            disconnect_signal: disconnect_tx,
            verified_peer_id,
            quic_conn: Some(quic_conn),
        };

        self.queue.lock().await.push_back(queued);
        self.conn_available.signal();

        if let Ok(result) = disconnect_rx.recv().await {
            return result;
        }

        Ok(())
    }

    /// Waits for the next connection in the queue.
    pub async fn next(&self) -> QueuedConn {
        let mut queue = self.queue.lock().await;
        while queue.is_empty() {
            queue = self.conn_available.wait(queue).await;
        }
        queue.pop_front().unwrap()
    }
}
