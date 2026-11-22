use std::{marker::PhantomData, sync::Arc, time::Duration};

use log::{error, warn};

use karyon_core::{
    async_runtime::{
        Executor,
        lock::{Semaphore, SemaphoreGuard},
    },
    async_util::{Backoff, TaskGroup},
    crypto::KeyPair,
};
use karyon_net::{ByteBuffer, ByteStream, Endpoint, FramedConn, codec::Codec, framed};

#[cfg(feature = "quic")]
use karyon_net::quic::QuicConn;

use crate::{
    Error, PeerID, Result,
    builtin_transports::BuiltinTransports,
    codec::PeerNetMsgCodec,
    config::Config,
    conn_direction::ConnDirection,
    discovery::DiscoveryContext,
    monitor::{ConnectionKind, MonitorRef},
    peer_cert::peer_id_from_certs,
    queued_conn::{ConnQueueRef, Incoming, QueuedConn},
    transport::TransportRef,
};

/// Internal dial result: the stream, not framed yet, and the peer id its
/// certificate proves, which the handshake then enforces.
pub(crate) enum DialResult {
    /// TCP, TLS or a user transport: one stream.
    Channel(Box<dyn ByteStream>, Option<PeerID>),
    /// QUIC: the handshake stream and the QUIC connection.
    #[cfg(feature = "quic")]
    Quic(Box<dyn ByteStream>, QuicConn, Option<PeerID>),
}

/// `Arc` reference to a `Connector`.
pub type ConnectorRef<C> = Arc<Connector<C>>;

/// Creates outbound connections with other peers. Generic over the
/// codec applied to the resulting framed stream so the same machinery
/// (TLS, retries, slots, monitor events) serves both the peer
/// data-plane (`PeerNetMsgCodec`) and a discovery's own messages.
pub struct Connector<C: Codec<ByteBuffer> + Default + Clone> {
    /// Dials the endpoints `transport` does not support.
    builtin: BuiltinTransports,
    task_group: TaskGroup,
    connection_slots: Semaphore,
    max_retries: usize,
    /// Max time for one dial: connect plus the TLS or QUIC handshake.
    dial_timeout: Duration,
    /// Hands dialed connections to the PeerPool. Only on the Node's own
    /// connector.
    conn_queue: Option<ConnQueueRef>,
    /// Dials the endpoints it supports instead of the built-in transports.
    transport: Option<TransportRef>,
    monitor: MonitorRef,
    _codec: PhantomData<C>,
}

impl<C> Connector<C>
where
    C: Codec<ByteBuffer, Error = karyon_net::Error> + Default + Clone + Send + Sync + 'static,
{
    /// Create a Connector for a discovery's own connections. The key
    /// pair, monitor, executor and the dial timeout (`handshake_timeout`),
    /// transport and QUIC runtime of the node's config come from `ctx`.
    pub fn new(ctx: &DiscoveryContext, max_retries: usize, slots: usize) -> ConnectorRef<C> {
        Self::build(
            &ctx.key_pair,
            &ctx.config,
            max_retries,
            slots,
            None,
            ctx.monitor.clone(),
            ctx.executor.clone(),
        )
    }

    /// Builds a Connector. `conn_queue` is set only on the Node's own.
    fn build(
        key_pair: &KeyPair,
        config: &Config,
        max_retries: usize,
        slots: usize,
        conn_queue: Option<ConnQueueRef>,
        monitor: MonitorRef,
        ex: Executor,
    ) -> ConnectorRef<C> {
        Arc::new(Self {
            builtin: BuiltinTransports::new(
                key_pair,
                #[cfg(feature = "quic")]
                config.quic_runtime.clone(),
            ),
            max_retries,
            dial_timeout: Duration::from_secs(config.handshake_timeout),
            task_group: TaskGroup::with_executor(ex),
            monitor,
            connection_slots: Semaphore::new(slots),
            conn_queue,
            transport: config.transport.clone(),
            _codec: PhantomData,
        })
    }

    pub async fn shutdown(&self) {
        self.task_group.cancel().await;
    }

    /// Connect and return the framed connection. The slot is held until
    /// the returned guard is dropped.
    pub async fn connect(
        &self,
        endpoint: &Endpoint,
        peer_id: &Option<PeerID>,
    ) -> Result<(FramedConn<C>, SemaphoreGuard)> {
        let slot = self.acquire_slot().await?;
        let stream = match self.connect_internal(endpoint, peer_id).await? {
            DialResult::Channel(stream, _) => stream,
            #[cfg(feature = "quic")]
            DialResult::Quic(stream, _, _) => stream,
        };
        Ok((framed(stream, C::default()), slot))
    }

    /// Wait for a free connection slot.
    pub(crate) async fn acquire_slot(&self) -> Result<SemaphoreGuard> {
        Ok(self.connection_slots.acquire().await?)
    }

    /// Dial with retries. Each try is bounded by `dial_timeout`, so an
    /// endpoint that accepts and then stays silent can not hang the dial.
    async fn connect_internal(
        &self,
        endpoint: &Endpoint,
        peer_id: &Option<PeerID>,
    ) -> Result<DialResult> {
        let mut retry = 0;
        let ex = self.task_group.executor();
        let backoff = Backoff::with_executor(500, 2000, ex.clone());
        while retry < self.max_retries {
            let dial = async {
                ex.timeout(self.dial_timeout, self.dial(endpoint, peer_id))
                    .await?
            };
            match dial.await {
                Ok(result) => {
                    self.monitor
                        .notify(ConnectionKind::Connected(endpoint.clone()))
                        .await;
                    return Ok(result);
                }
                Err(err) => {
                    error!("Failed to connect to {endpoint}: {err}");
                }
            }

            self.monitor
                .notify(ConnectionKind::ConnectRetried(endpoint.clone()))
                .await;

            backoff.sleep().await;
            warn!("try to reconnect {endpoint}");
            retry += 1;
        }

        self.monitor
            .notify(ConnectionKind::ConnectFailed(endpoint.clone()))
            .await;

        Err(Error::Timeout)
    }

    /// Dial with the user transport if it supports `endpoint`, else with
    /// the built-in ones.
    async fn dial(&self, endpoint: &Endpoint, peer_id: &Option<PeerID>) -> Result<DialResult> {
        if let Some(transport) = self.transport.as_ref().filter(|t| t.supports(endpoint)) {
            let stream = transport.dial(endpoint).await?;
            let vpid = stream
                .peer_certificates()
                .as_deref()
                .and_then(peer_id_from_certs);
            return Ok(DialResult::Channel(stream, vpid));
        }

        #[cfg(feature = "quic")]
        if endpoint.is_quic() {
            let ex = self.task_group.executor();
            return self.builtin.dial_quic(endpoint, peer_id, ex).await;
        }

        self.builtin.dial(endpoint, peer_id).await
    }
}

// `connect_and_queue` only lives on the peer-plane Connector since the
// PeerPool takes only peer-plane connections.
impl Connector<PeerNetMsgCodec> {
    /// Create a new Connector that hands its connections to the PeerPool.
    pub(crate) fn new_with_queue(
        key_pair: &KeyPair,
        config: &Config,
        conn_queue: ConnQueueRef,
        monitor: MonitorRef,
        ex: Executor,
    ) -> ConnectorRef<PeerNetMsgCodec> {
        Self::build(
            key_pair,
            config,
            config.max_connect_retries,
            config.outbound_slots,
            Some(conn_queue),
            monitor,
            ex,
        )
    }

    /// Connect and hand the connection to the PeerPool, which handshakes
    /// it. Handshake failure and disconnect are observable via
    /// `Node::register_peer_events`, not via this call. `slot` comes
    /// from `acquire_slot` and is held until the connection is closed.
    pub(crate) async fn connect_and_queue(
        &self,
        endpoint: &Endpoint,
        peer_id: &Option<PeerID>,
        slot: SemaphoreGuard,
    ) -> Result<()> {
        let conn_queue = self.conn_queue.as_ref().ok_or_else(|| {
            Error::Config("connect_and_queue called on Connector built without a PeerPool".into())
        })?;

        let conn = match self.connect_internal(endpoint, peer_id).await? {
            DialResult::Channel(stream, vpid) => QueuedConn::new(
                framed(stream, PeerNetMsgCodec::new()),
                ConnDirection::Outbound,
                vpid,
            ),
            #[cfg(feature = "quic")]
            DialResult::Quic(stream, quic_conn, vpid) => {
                let conn = framed(stream, PeerNetMsgCodec::new());
                QueuedConn::new(conn, ConnDirection::Outbound, vpid).map(|c| c.with_quic(quic_conn))
            }
        };

        match conn.map(|c| c.with_dialed(peer_id.clone())) {
            Ok(conn) => {
                conn_queue.push(Incoming { conn, slot }).await;
                Ok(())
            }
            Err(err) => {
                self.monitor
                    .notify(ConnectionKind::Disconnected(endpoint.clone()))
                    .await;
                Err(err)
            }
        }
    }
}
