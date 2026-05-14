use std::{future::Future, marker::PhantomData, sync::Arc};

use log::{debug, error, info};

use karyon_core::{
    async_runtime::Executor,
    async_util::{TaskGroup, TaskResult},
    crypto::KeyPair,
};
use karyon_net::{
    codec::Codec,
    framed,
    tcp::TcpListener,
    tls::{ServerTlsConfig, TlsListener},
    ByteBuffer, ByteStream, Endpoint, FramedConn,
};

use crate::{
    codec::PeerNetMsgCodec,
    conn_queue::ConnQueue,
    monitor::{ConnectionKind, Monitor},
    peer::ConnDirection,
    slots::ConnectionSlots,
    tls_config::{peer_id_from_certs, tls_server_config},
    Error, Result,
};

/// Listener for byte-stream transports (TCP, TLS). Each `accept` yields
/// a single `Box<dyn ByteStream>`. QUIC uses a separate `StreamMux` path.
enum StreamListener {
    Tcp(TcpListener),
    Tls(Box<TlsListener>),
}

impl StreamListener {
    async fn accept(&self) -> Result<Box<dyn ByteStream>> {
        match self {
            Self::Tcp(l) => Ok(l.accept().await?),
            Self::Tls(l) => Ok(l.accept().await?),
        }
    }

    fn local_endpoint(&self) -> Result<Endpoint> {
        match self {
            Self::Tcp(l) => Ok(l.local_endpoint()?),
            Self::Tls(l) => Ok(l.local_endpoint()?),
        }
    }
}

#[cfg(feature = "quic")]
use karyon_net::{quic, StreamMux};

/// Creates inbound connections with other peers. Generic over the
/// codec applied to the framed accepted streams so the same accept
/// machinery serves the peer data-plane (`PeerNetMsgCodec`) and the
/// kademlia lookup-plane (`KadNetMsgCodec`).
pub struct Listener<C: Codec<ByteBuffer> + Default + Clone> {
    key_pair: KeyPair,
    task_group: TaskGroup,
    connection_slots: Arc<ConnectionSlots>,
    conn_queue: Option<Arc<ConnQueue>>,
    monitor: Arc<Monitor>,
    _codec: PhantomData<C>,
}

impl<C> Listener<C>
where
    C: Codec<ByteBuffer, Error = karyon_net::Error> + Default + Clone + Send + Sync + 'static,
{
    /// Create a new Listener (no auto-queue; use `start_with_callback`).
    pub fn new(
        key_pair: &KeyPair,
        connection_slots: Arc<ConnectionSlots>,
        monitor: Arc<Monitor>,
        ex: Executor,
    ) -> Arc<Self> {
        Arc::new(Self {
            key_pair: key_pair.clone(),
            connection_slots,
            conn_queue: None,
            task_group: TaskGroup::with_executor(ex),
            monitor,
            _codec: PhantomData,
        })
    }

    /// Start with a user-provided callback for each connection.
    pub async fn start_with_callback<Fut>(
        self: &Arc<Self>,
        endpoint: Endpoint,
        callback: impl FnOnce(FramedConn<C>) -> Fut + Clone + Send + 'static,
    ) -> Result<Endpoint>
    where
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let listener = match self.listen(&endpoint).await {
            Ok(l) => {
                self.monitor
                    .notify(ConnectionKind::Listening(endpoint.clone()))
                    .await;
                l
            }
            Err(err) => {
                error!("Failed to listen on {endpoint}: {err}");
                self.monitor
                    .notify(ConnectionKind::ListenFailed(endpoint))
                    .await;
                return Err(err);
            }
        };

        let resolved = listener.local_endpoint()?;
        info!("Start listening on {resolved}");

        self.task_group.spawn(
            {
                let this = self.clone();
                async move { this.listen_loop_callback(listener, callback).await }
            },
            |res: TaskResult<()>| async move {
                debug!("Listener callback loop ended: {res}");
            },
        );
        Ok(resolved)
    }

    pub async fn shutdown(&self) {
        self.task_group.cancel().await;
    }

    /// Accept loop (callback mode).
    async fn listen_loop_callback<Fut>(
        self: Arc<Self>,
        listener: StreamListener,
        callback: impl FnOnce(FramedConn<C>) -> Fut + Clone + Send + 'static,
    ) where
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        loop {
            self.connection_slots.wait_for_slot().await;
            let result = listener.accept().await;

            let conn: FramedConn<C> = match result {
                Ok(stream) => framed(stream, C::default()),
                Err(err) => {
                    error!("Failed to accept connection: {err}");
                    self.monitor.notify(ConnectionKind::AcceptFailed).await;
                    continue;
                }
            };

            let endpoint = match conn.peer_endpoint() {
                Some(ep) => ep,
                None => {
                    self.monitor.notify(ConnectionKind::AcceptFailed).await;
                    error!("Failed to get peer endpoint");
                    continue;
                }
            };

            self.monitor
                .notify(ConnectionKind::Accepted(endpoint.clone()))
                .await;
            self.connection_slots.add();

            let on_disconnect = {
                let this = self.clone();
                |res| async move {
                    if let TaskResult::Completed(Err(err)) = res {
                        debug!("Inbound connection dropped: {err}");
                    }
                    this.monitor
                        .notify(ConnectionKind::Disconnected(endpoint))
                        .await;
                    this.connection_slots.remove().await;
                }
            };

            let callback = callback.clone();
            self.task_group.spawn(callback(conn), on_disconnect);
        }
    }

    /// Create a listener for TCP/TLS.
    async fn listen(&self, endpoint: &Endpoint) -> Result<StreamListener> {
        match endpoint {
            Endpoint::Tcp(..) => {
                let listener = TcpListener::bind(endpoint, Default::default()).await?;
                Ok(StreamListener::Tcp(listener))
            }
            Endpoint::Tls(..) => {
                let tls_config = ServerTlsConfig {
                    server_config: tls_server_config(&self.key_pair)?,
                };
                let tcp_listener = TcpListener::bind(endpoint, Default::default()).await?;
                let listener = TlsListener::new(tcp_listener, tls_config);
                Ok(StreamListener::Tls(Box::new(listener)))
            }
            _ => Err(Error::UnsupportedEndpoint(endpoint.to_string())),
        }
    }
}

// Auto-queue paths only live on the peer-plane Listener. The kademlia
// lookup plane uses `start_with_callback` and handles each connection
// inline (no ConnQueue / handshake pipeline).
impl Listener<PeerNetMsgCodec> {
    /// Create a new Listener with a ConnQueue (auto-queue mode).
    pub fn new_with_queue(
        key_pair: &KeyPair,
        connection_slots: Arc<ConnectionSlots>,
        conn_queue: Arc<ConnQueue>,
        monitor: Arc<Monitor>,
        ex: Executor,
    ) -> Arc<Self> {
        Arc::new(Self {
            key_pair: key_pair.clone(),
            connection_slots,
            conn_queue: Some(conn_queue),
            task_group: TaskGroup::with_executor(ex),
            monitor,
            _codec: PhantomData,
        })
    }

    /// Start listening (auto-queue mode). Returns the resolved endpoint.
    pub async fn start(self: &Arc<Self>, endpoint: Endpoint) -> Result<Endpoint> {
        #[cfg(feature = "quic")]
        if endpoint.is_quic() {
            return self.start_quic(endpoint).await;
        }

        let listener = match self.listen(&endpoint).await {
            Ok(l) => {
                self.monitor
                    .notify(ConnectionKind::Listening(endpoint.clone()))
                    .await;
                l
            }
            Err(err) => {
                error!("Failed to listen on {endpoint}: {err}");
                self.monitor
                    .notify(ConnectionKind::ListenFailed(endpoint))
                    .await;
                return Err(err);
            }
        };

        let resolved = listener.local_endpoint()?;
        info!("Start listening on {resolved}");

        self.task_group.spawn(
            {
                let this = self.clone();
                async move { this.listen_loop(listener).await }
            },
            |_| async {},
        );
        Ok(resolved)
    }

    /// Accept loop (auto-queue mode).
    async fn listen_loop(self: Arc<Self>, listener: StreamListener) {
        let conn_queue = self
            .conn_queue
            .as_ref()
            .expect("listen_loop requires ConnQueue")
            .clone();

        loop {
            self.connection_slots.wait_for_slot().await;
            let result = listener.accept().await;

            let (conn, vpid) = match result {
                Ok(stream) => {
                    // Extract peer cert (TLS) before framing consumes the stream.
                    let vpid = stream
                        .peer_certificates()
                        .as_deref()
                        .and_then(peer_id_from_certs);
                    let conn: FramedConn<PeerNetMsgCodec> = framed(stream, PeerNetMsgCodec::new());
                    (conn, vpid)
                }
                Err(err) => {
                    error!("Failed to accept connection: {err}");
                    self.monitor.notify(ConnectionKind::AcceptFailed).await;
                    continue;
                }
            };

            let endpoint = match conn.peer_endpoint() {
                Some(ep) => ep,
                None => {
                    self.monitor.notify(ConnectionKind::AcceptFailed).await;
                    error!("Failed to get peer endpoint");
                    continue;
                }
            };

            self.monitor
                .notify(ConnectionKind::Accepted(endpoint.clone()))
                .await;
            self.connection_slots.add();

            let on_disconnect = {
                let this = self.clone();
                |res: TaskResult<Result<()>>| async move {
                    if let TaskResult::Completed(Err(err)) = res {
                        debug!("Inbound connection dropped: {err}");
                    }
                    this.monitor
                        .notify(ConnectionKind::Disconnected(endpoint))
                        .await;
                    this.connection_slots.remove().await;
                }
            };

            let cq = conn_queue.clone();
            self.task_group.spawn(
                async move {
                    cq.handle(conn, ConnDirection::Inbound, vpid).await?;
                    Ok(())
                },
                on_disconnect,
            );
        }
    }

    /// QUIC listener.
    #[cfg(feature = "quic")]
    async fn start_quic(self: &Arc<Self>, endpoint: Endpoint) -> Result<Endpoint> {
        let rustls_config = tls_server_config(&self.key_pair)?;
        let server_config = quic::ServerQuicConfig::from_rustls(rustls_config);

        let quic_endpoint = match quic::QuicEndpoint::listen(&endpoint, server_config).await {
            Ok(ep) => {
                self.monitor
                    .notify(ConnectionKind::Listening(endpoint.clone()))
                    .await;
                ep
            }
            Err(err) => {
                error!("Failed to listen on {endpoint}: {err}");
                self.monitor
                    .notify(ConnectionKind::ListenFailed(endpoint))
                    .await;
                return Err(err.into());
            }
        };

        let resolved: Endpoint = quic_endpoint.local_endpoint().map_err(Error::from)?;
        info!("Start listening on {resolved}");

        self.task_group.spawn(
            {
                let this = self.clone();
                async move { this.listen_loop_quic(quic_endpoint).await }
            },
            |res: TaskResult<()>| async move {
                debug!("QUIC listen loop ended: {res}");
            },
        );

        Ok(resolved)
    }

    /// QUIC accept loop.
    #[cfg(feature = "quic")]
    async fn listen_loop_quic(self: Arc<Self>, quic_endpoint: quic::QuicEndpoint) {
        loop {
            self.connection_slots.wait_for_slot().await;

            let quic_conn = match quic_endpoint.accept().await {
                Ok(c) => c,
                Err(err) => {
                    error!("Failed to accept QUIC conn: {err}");
                    self.monitor.notify(ConnectionKind::AcceptFailed).await;
                    continue;
                }
            };

            let peer_ep = match quic_conn.peer_endpoint() {
                Ok(ep) => ep,
                Err(err) => {
                    error!("Failed to get peer endpoint: {err}");
                    self.monitor.notify(ConnectionKind::AcceptFailed).await;
                    continue;
                }
            };

            self.monitor
                .notify(ConnectionKind::Accepted(peer_ep.clone()))
                .await;

            let vpid = quic_conn
                .peer_certificates()
                .as_deref()
                .and_then(peer_id_from_certs);

            let stream = match quic_conn.accept_stream().await {
                Ok(s) => s,
                Err(err) => {
                    error!("Failed to accept handshake stream: {err}");
                    self.monitor.notify(ConnectionKind::AcceptFailed).await;
                    continue;
                }
            };

            let conn: FramedConn<PeerNetMsgCodec> = framed(stream, PeerNetMsgCodec::new());

            self.connection_slots.add();

            let on_disconnect = {
                let this = self.clone();
                |res: TaskResult<Result<()>>| async move {
                    if let TaskResult::Completed(Err(err)) = res {
                        debug!("Inbound QUIC conn dropped: {err}");
                    }
                    this.monitor
                        .notify(ConnectionKind::Disconnected(peer_ep))
                        .await;
                    this.connection_slots.remove().await;
                }
            };

            let conn_queue = self
                .conn_queue
                .as_ref()
                .expect("QUIC listener requires ConnQueue")
                .clone();
            self.task_group.spawn(
                async move {
                    conn_queue
                        .handle_quic(conn, quic_conn, ConnDirection::Inbound, vpid)
                        .await?;
                    Ok(())
                },
                on_disconnect,
            );
        }
    }
}
