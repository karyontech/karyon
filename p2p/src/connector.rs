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
use karyon_net::{ByteBuffer, Endpoint, FramedConn, codec::Codec, framed};

#[cfg(not(target_family = "wasm"))]
use karyon_net::{
    ClientLayer, tcp,
    tls::{ClientTlsConfig, TlsLayer},
};

use crate::{
    Error, PeerID, Result,
    codec::PeerNetMsgCodec,
    config::Config,
    conn_direction::ConnDirection,
    discovery::DiscoveryContext,
    monitor::{ConnectionKind, MonitorRef},
    peer_cert::peer_id_from_certs,
    queued_conn::{ConnQueueRef, Incoming, QueuedConn},
    transport::TransportRef,
};

#[cfg(feature = "quic")]
use karyon_net::{
    StreamMux,
    quic::{self, QuicRuntimeRef},
};

#[cfg(feature = "quic")]
use crate::tls_config::quic_client_config;

#[cfg(not(target_family = "wasm"))]
use crate::tls_config::tls_client_config;

#[cfg(not(target_family = "wasm"))]
static DNS_NAME: &str = "karyontech.net";

/// Internal dial result. Generic over the codec used to frame the
/// resulting connection. Carries the optional cert-derived PeerID so
/// the caller can stamp it on the QueuedConn for the application
/// handshake to enforce.
enum DialResult<C: Codec<ByteBuffer> + Default + Clone> {
    /// TCP or TLS: a framed connection.
    Channel(FramedConn<C>, Option<PeerID>),
    /// QUIC: handshake stream + full QUIC connection.
    #[cfg(feature = "quic")]
    Quic(FramedConn<C>, quic::QuicConn, Option<PeerID>),
}

/// `Arc` reference to a `Connector`.
pub type ConnectorRef<C> = Arc<Connector<C>>;

/// Creates outbound connections with other peers. Generic over the
/// codec applied to the resulting framed stream so the same machinery
/// (TLS, retries, slots, monitor events) serves both the peer
/// data-plane (`PeerNetMsgCodec`) and a discovery's own messages.
pub struct Connector<C: Codec<ByteBuffer> + Default + Clone> {
    /// For the built-in TLS and QUIC configs; none on wasm.
    #[cfg(not(target_family = "wasm"))]
    key_pair: KeyPair,
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
    /// Runs the built-in QUIC (see `Config::quic_runtime`).
    #[cfg(feature = "quic")]
    quic_runtime: Option<QuicRuntimeRef>,
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
        // No built-in TLS or QUIC on wasm, so the key pair is not kept.
        #[cfg(target_family = "wasm")]
        let _ = key_pair;
        Arc::new(Self {
            #[cfg(not(target_family = "wasm"))]
            key_pair: key_pair.clone(),
            max_retries,
            dial_timeout: Duration::from_secs(config.handshake_timeout),
            task_group: TaskGroup::with_executor(ex),
            monitor,
            connection_slots: Semaphore::new(slots),
            conn_queue,
            transport: config.transport.clone(),
            #[cfg(feature = "quic")]
            quic_runtime: config.quic_runtime.clone(),
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
        let result = self.connect_internal(endpoint, peer_id).await?;
        match result {
            DialResult::Channel(conn, _) => Ok((conn, slot)),
            #[cfg(feature = "quic")]
            DialResult::Quic(conn, _, _) => Ok((conn, slot)),
        }
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
    ) -> Result<DialResult<C>> {
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

    /// Dial, selecting transport based on endpoint type.
    async fn dial(&self, endpoint: &Endpoint, peer_id: &Option<PeerID>) -> Result<DialResult<C>> {
        if let Some(transport) = self.transport.as_ref().filter(|t| t.supports(endpoint)) {
            let stream = transport.dial(endpoint).await?;
            let vpid = stream
                .peer_certificates()
                .as_deref()
                .and_then(peer_id_from_certs);
            return Ok(DialResult::Channel(framed(stream, C::default()), vpid));
        }

        self.dial_builtin(endpoint, peer_id).await
    }

    /// Dial with the built-in TCP, TLS or QUIC.
    #[cfg(not(target_family = "wasm"))]
    async fn dial_builtin(
        &self,
        endpoint: &Endpoint,
        peer_id: &Option<PeerID>,
    ) -> Result<DialResult<C>> {
        match endpoint {
            Endpoint::Tcp(..) => {
                let stream = tcp::connect(endpoint, Default::default()).await?;
                let conn = framed(stream, C::default());
                Ok(DialResult::Channel(conn, None))
            }
            Endpoint::Tls(..) => {
                let tls_config = ClientTlsConfig {
                    client_config: tls_client_config(&self.key_pair, peer_id.clone())?,
                    dns_name: DNS_NAME.to_string(),
                };
                let stream = tcp::connect(endpoint, Default::default()).await?;
                let tls_layer = TlsLayer::client(tls_config);
                let tls_stream = ClientLayer::handshake(&tls_layer, stream).await?;
                // Extract the peer cert before framing consumes the stream.
                let vpid = tls_stream
                    .peer_certificates()
                    .as_deref()
                    .and_then(peer_id_from_certs);
                let conn = framed(tls_stream, C::default());
                Ok(DialResult::Channel(conn, vpid))
            }
            #[cfg(feature = "quic")]
            Endpoint::Quic(..) => {
                let rustls_config = quic_client_config(&self.key_pair, peer_id.clone())?;
                let mut client_config =
                    quic::ClientQuicConfig::from_rustls(rustls_config, DNS_NAME)
                        .with_executor(self.task_group.executor().clone());
                if let Some(runtime) = &self.quic_runtime {
                    client_config = client_config.with_runtime(runtime.clone());
                }
                let quic_conn = quic::QuicEndpoint::dial(endpoint, client_config).await?;

                let vpid = quic_conn
                    .peer_certificates()
                    .as_deref()
                    .and_then(peer_id_from_certs);

                // First stream for handshake via StreamMux.
                let stream = quic_conn.open_stream().await?;
                let conn = framed(stream, C::default());

                Ok(DialResult::Quic(conn, quic_conn, vpid))
            }
            _ => Err(Error::UnsupportedEndpoint(endpoint.to_string())),
        }
    }

    /// wasm has no built-in transports: only `Config::transport` dials.
    #[cfg(target_family = "wasm")]
    async fn dial_builtin(
        &self,
        endpoint: &Endpoint,
        _peer_id: &Option<PeerID>,
    ) -> Result<DialResult<C>> {
        Err(Error::UnsupportedEndpoint(endpoint.to_string()))
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
            DialResult::Channel(conn, vpid) => QueuedConn::new(conn, ConnDirection::Outbound, vpid),
            #[cfg(feature = "quic")]
            DialResult::Quic(conn, quic_conn, vpid) => {
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
