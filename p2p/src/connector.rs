use std::{marker::PhantomData, sync::Arc};

use log::{error, trace, warn};

use karyon_core::{
    async_runtime::Executor,
    async_util::{Backoff, TaskGroup, TaskResult},
    crypto::KeyPair,
};
use karyon_net::{codec::Codec, framed, tcp, ByteBuffer, ClientLayer, Endpoint, FramedConn};

use crate::{
    codec::PeerNetMsgCodec,
    conn_queue::ConnQueue,
    monitor::{ConnectionKind, Monitor},
    peer::ConnDirection,
    slots::ConnectionSlots,
    tls_config::{peer_id_from_certs, tls_client_config},
    Error, PeerID, Result,
};

#[cfg(feature = "quic")]
use karyon_net::{quic, StreamMux};

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

/// Creates outbound connections with other peers. Generic over the
/// codec applied to the resulting framed stream so the same machinery
/// (TLS, retries, slots, monitor events) serves both the peer
/// data-plane (`PeerNetMsgCodec`) and the kademlia lookup-plane
/// (`KadNetMsgCodec`).
pub struct Connector<C: Codec<ByteBuffer> + Default + Clone> {
    key_pair: KeyPair,
    task_group: TaskGroup,
    connection_slots: Arc<ConnectionSlots>,
    max_retries: usize,
    conn_queue: Option<Arc<ConnQueue>>,
    monitor: Arc<Monitor>,
    _codec: PhantomData<C>,
}

impl<C> Connector<C>
where
    C: Codec<ByteBuffer, Error = karyon_net::Error> + Default + Clone + Send + Sync + 'static,
{
    /// Create a new Connector without a ConnQueue (used for plain
    /// `connect` paths like Kademlia lookup).
    pub fn new(
        key_pair: &KeyPair,
        max_retries: usize,
        connection_slots: Arc<ConnectionSlots>,
        monitor: Arc<Monitor>,
        ex: Executor,
    ) -> Arc<Self> {
        Arc::new(Self {
            key_pair: key_pair.clone(),
            max_retries,
            task_group: TaskGroup::with_executor(ex),
            monitor,
            connection_slots,
            conn_queue: None,
            _codec: PhantomData,
        })
    }

    pub async fn shutdown(&self) {
        self.task_group.cancel().await;
    }

    /// Connect and return the framed connection.
    pub async fn connect(
        &self,
        endpoint: &Endpoint,
        peer_id: &Option<PeerID>,
    ) -> Result<FramedConn<C>> {
        let result = self.connect_internal(endpoint, peer_id).await?;
        match result {
            DialResult::Channel(conn, _) => Ok(conn),
            #[cfg(feature = "quic")]
            DialResult::Quic(conn, _, _) => Ok(conn),
        }
    }

    /// Dial with retries.
    async fn connect_internal(
        &self,
        endpoint: &Endpoint,
        peer_id: &Option<PeerID>,
    ) -> Result<DialResult<C>> {
        self.connection_slots.wait_for_slot().await;
        self.connection_slots.add();

        let mut retry = 0;
        let backoff = Backoff::new(500, 2000);
        while retry < self.max_retries {
            match self.dial(endpoint, peer_id).await {
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

        self.connection_slots.remove().await;
        Err(Error::Timeout)
    }

    /// Dial, selecting transport based on endpoint type.
    async fn dial(&self, endpoint: &Endpoint, peer_id: &Option<PeerID>) -> Result<DialResult<C>> {
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
                let rustls_config = tls_client_config(&self.key_pair, peer_id.clone())?;
                let client_config = quic::ClientQuicConfig::from_rustls(rustls_config, DNS_NAME);
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
// ConnQueue is part of the data-plane handshake pipeline.
impl Connector<PeerNetMsgCodec> {
    /// Create a new Connector with a ConnQueue.
    pub fn new_with_queue(
        key_pair: &KeyPair,
        max_retries: usize,
        connection_slots: Arc<ConnectionSlots>,
        conn_queue: Arc<ConnQueue>,
        monitor: Arc<Monitor>,
        ex: Executor,
    ) -> Arc<Self> {
        Arc::new(Self {
            key_pair: key_pair.clone(),
            max_retries,
            task_group: TaskGroup::with_executor(ex),
            monitor,
            connection_slots,
            conn_queue: Some(conn_queue),
            _codec: PhantomData,
        })
    }

    /// Connect, queue for handshake, and run until the peer
    /// disconnects. Disconnect/handshake-failure is observable via
    /// `PeerPool::register_peer_events`, not via this call.
    pub async fn connect_and_queue(
        self: &Arc<Self>,
        endpoint: &Endpoint,
        peer_id: &Option<PeerID>,
    ) -> Result<()> {
        let dial_result = self.connect_internal(endpoint, peer_id).await?;

        let endpoint = endpoint.clone();
        let on_disconnect = {
            let this = self.clone();
            |res: TaskResult<Result<()>>| async move {
                if let TaskResult::Completed(Err(err)) = res {
                    trace!("Outbound connection dropped: {err}");
                }
                this.monitor
                    .notify(ConnectionKind::Disconnected(endpoint.clone()))
                    .await;
                this.connection_slots.remove().await;
            }
        };

        let conn_queue = self
            .conn_queue
            .as_ref()
            .ok_or_else(|| {
                Error::Config(
                    "connect_and_queue called on Connector built without ConnQueue".into(),
                )
            })?
            .clone();
        self.task_group.spawn(
            async move {
                match dial_result {
                    DialResult::Channel(conn, vpid) => {
                        conn_queue.handle(conn, ConnDirection::Outbound, vpid).await
                    }
                    #[cfg(feature = "quic")]
                    DialResult::Quic(conn, quic_conn, vpid) => {
                        conn_queue
                            .handle_quic(conn, quic_conn, ConnDirection::Outbound, vpid)
                            .await
                    }
                }
            },
            on_disconnect,
        );

        Ok(())
    }
}
