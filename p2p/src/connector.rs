use std::{marker::PhantomData, sync::Arc, time::Duration};

use log::{error, warn};

use karyon_core::{
    async_runtime::{
        Executor,
        lock::{Semaphore, SemaphoreGuard},
    },
    async_util::{Backoff, TaskGroup, timeout},
    crypto::KeyPair,
};
use karyon_net::{ByteBuffer, ClientLayer, Endpoint, FramedConn, codec::Codec, framed, tcp};

use crate::{
    Error, PeerID, Result,
    codec::PeerNetMsgCodec,
    config::Config,
    conn_direction::ConnDirection,
    monitor::{ConnectionKind, MonitorRef},
    queued_conn::{ConnQueueRef, Incoming, QueuedConn},
    tls_config::{peer_id_from_certs, tls_client_config},
    transport::TransportRef,
};

#[cfg(feature = "quic")]
use karyon_net::{
    StreamMux,
    quic::{self, QuicRuntimeRef},
};

#[cfg(feature = "quic")]
use crate::tls_config::quic_client_config;

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
    /// Create a new Connector not tied to the PeerPool, for plain `connect`
    /// calls such as a discovery's queries. `dial_timeout` is in seconds.
    pub fn new(
        key_pair: &KeyPair,
        max_retries: usize,
        slots: usize,
        dial_timeout: u64,
        monitor: MonitorRef,
        ex: Executor,
    ) -> ConnectorRef<C> {
        Self::new_with_transport(
            key_pair,
            max_retries,
            slots,
            dial_timeout,
            None,
            monitor,
            ex,
        )
    }

    /// Like `new`, but dials the endpoints `transport` supports through it
    /// (see `Config::transport`).
    pub fn new_with_transport(
        key_pair: &KeyPair,
        max_retries: usize,
        slots: usize,
        dial_timeout: u64,
        transport: Option<TransportRef>,
        monitor: MonitorRef,
        ex: Executor,
    ) -> ConnectorRef<C> {
        let config = Config {
            handshake_timeout: dial_timeout,
            transport,
            ..Config::default()
        };
        Self::new_with_config(key_pair, &config, max_retries, slots, monitor, ex)
    }

    /// Like `new`, but takes the dial timeout (`handshake_timeout`), the
    /// transport and the QUIC runtime from `config`.
    pub fn new_with_config(
        key_pair: &KeyPair,
        config: &Config,
        max_retries: usize,
        slots: usize,
        monitor: MonitorRef,
        ex: Executor,
    ) -> ConnectorRef<C> {
        Arc::new(Self {
            key_pair: key_pair.clone(),
            max_retries,
            dial_timeout: Duration::from_secs(config.handshake_timeout),
            task_group: TaskGroup::with_executor(ex),
            monitor,
            connection_slots: Semaphore::new(slots),
            conn_queue: None,
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
        let backoff = Backoff::new(500, 2000);
        while retry < self.max_retries {
            let dial = async { timeout(self.dial_timeout, self.dial(endpoint, peer_id)).await? };
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

        match endpoint {
            Endpoint::Tcp(..) => {
                let stream = tcp::connect(endpoint, Default::default()).await?;
                let conn = framed(stream, C::default());
                Ok(DialResult::Channel(conn, None))
            }
            Endpoint::Tls(..) => {
                let tls_config = karyon_net::tls::ClientTlsConfig {
                    client_config: tls_client_config(&self.key_pair, peer_id.clone())?,
                    dns_name: DNS_NAME.to_string(),
                };
                let stream = tcp::connect(endpoint, Default::default()).await?;
                let tls_layer = karyon_net::tls::TlsLayer::client(tls_config);
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
        Arc::new(Self {
            key_pair: key_pair.clone(),
            max_retries: config.max_connect_retries,
            dial_timeout: Duration::from_secs(config.handshake_timeout),
            task_group: TaskGroup::with_executor(ex),
            monitor,
            connection_slots: Semaphore::new(config.outbound_slots),
            conn_queue: Some(conn_queue),
            transport: config.transport.clone(),
            #[cfg(feature = "quic")]
            quic_runtime: config.quic_runtime.clone(),
            _codec: PhantomData,
        })
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
