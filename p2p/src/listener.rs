use std::{future::Future, marker::PhantomData, sync::Arc, time::Duration};

use log::{debug, error, info};

use karyon_core::{
    async_runtime::Executor,
    async_util::{timeout, TaskGroup, TaskResult},
    crypto::KeyPair,
};
use karyon_net::{
    codec::Codec,
    framed,
    tcp::TcpListener,
    tls::{ServerTlsConfig, TlsLayer},
    ByteBuffer, ByteStream, Endpoint, FramedConn, ServerLayer,
};

use crate::{
    access_control::{AccessControl, Action, Subject},
    codec::PeerNetMsgCodec,
    conn_queue::ConnQueue,
    monitor::{ConnectionKind, Monitor},
    peer::ConnDirection,
    slots::ConnectionSlots,
    tls_config::{peer_id_from_certs, tls_server_config},
    Error, PeerID, Result,
};

/// Listener for byte-stream transports (TCP, TLS). Accepting is split in
/// two phases: `accept` does only the cheap kernel accept, `handshake`
/// runs the TLS upgrade. QUIC uses a separate `StreamMux` path.
enum StreamListener {
    Tcp(TcpListener),
    Tls(TcpListener, Box<TlsLayer>),
}

impl StreamListener {
    /// Kernel accept only. Returns the raw stream before any handshake.
    async fn accept(&self) -> Result<Box<dyn ByteStream>> {
        match self {
            Self::Tcp(l) | Self::Tls(l, _) => Ok(l.accept().await?),
        }
    }

    /// TLS upgrade, if this is a TLS listener. Runs off the accept loop.
    async fn handshake(
        &self,
        stream: Box<dyn ByteStream>,
        handshake_timeout: Duration,
    ) -> Result<Box<dyn ByteStream>> {
        match self {
            Self::Tcp(_) => Ok(stream),
            Self::Tls(_, layer) => Ok(timeout(
                handshake_timeout,
                ServerLayer::handshake(layer.as_ref(), stream),
            )
            .await??),
        }
    }

    fn local_endpoint(&self) -> Result<Endpoint> {
        match self {
            Self::Tcp(l) => Ok(l.local_endpoint()?),
            // The listener is plain TCP; report the TLS scheme so peers
            // dialling this endpoint run the handshake.
            Self::Tls(l, _) => {
                let ep = l.local_endpoint()?;
                Ok(Endpoint::Tls(ep.addr()?, ep.port()?))
            }
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
    handshake_timeout: Duration,
    access_control: Arc<dyn AccessControl>,
    _codec: PhantomData<C>,
}

impl<C> Listener<C>
where
    C: Codec<ByteBuffer, Error = karyon_net::Error> + Default + Clone + Send + Sync + 'static,
{
    /// Create a new Listener (no auto-queue; use `start_with_callback`).
    /// `handshake_timeout` is in seconds.
    pub fn new(
        key_pair: &KeyPair,
        connection_slots: Arc<ConnectionSlots>,
        monitor: Arc<Monitor>,
        handshake_timeout: u64,
        access_control: Arc<dyn AccessControl>,
        ex: Executor,
    ) -> Arc<Self> {
        Arc::new(Self {
            key_pair: key_pair.clone(),
            connection_slots,
            conn_queue: None,
            task_group: TaskGroup::with_executor(ex),
            monitor,
            handshake_timeout: Duration::from_secs(handshake_timeout),
            access_control,
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

        self.task_group.spawn_then(
            {
                let this = self.clone();
                async move {
                    this.listen_loop_callback(Arc::new(listener), callback)
                        .await
                }
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

    /// Runs the handshake and frames the stream. Called off the accept
    /// loop. Also returns the peer id proved by the TLS client cert.
    async fn upgrade(
        &self,
        listener: &StreamListener,
        stream: Box<dyn ByteStream>,
    ) -> Result<(FramedConn<C>, Option<PeerID>)> {
        let stream = listener.handshake(stream, self.handshake_timeout).await?;
        // Extract the peer cert before framing consumes the stream.
        let vpid = stream
            .peer_certificates()
            .as_deref()
            .and_then(peer_id_from_certs);
        Ok((framed(stream, C::default()), vpid))
    }

    /// Accept loop (callback mode).
    async fn listen_loop_callback<Fut>(
        self: Arc<Self>,
        listener: Arc<StreamListener>,
        callback: impl FnOnce(FramedConn<C>) -> Fut + Clone + Send + 'static,
    ) where
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        loop {
            self.connection_slots.wait_for_slot().await;

            let stream = match listener.accept().await {
                Ok(s) => s,
                Err(err) => {
                    error!("Failed to accept connection: {err}");
                    self.monitor.notify(ConnectionKind::AcceptFailed).await;
                    continue;
                }
            };

            let endpoint = match stream.peer_endpoint() {
                Some(ep) => ep,
                None => {
                    self.monitor.notify(ConnectionKind::AcceptFailed).await;
                    error!("Failed to get peer endpoint");
                    continue;
                }
            };

            // Callback mode is the kademlia lookup plane.
            if !self.access_control.allow(
                &Subject::Endpoint(&endpoint),
                Action::Lookup(ConnDirection::Inbound),
            ) {
                debug!("Rejected inbound lookup connection from {endpoint}");
                continue;
            }

            self.monitor
                .notify(ConnectionKind::Accepted(endpoint.clone()))
                .await;
            // Counted here so `wait_for_slot` keeps throttling. The task
            // releases it again, including when the handshake fails.
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

            let this = self.clone();
            let listener = listener.clone();
            let callback = callback.clone();
            self.task_group.spawn_then(
                async move {
                    let (conn, _) = this.upgrade(&listener, stream).await?;
                    callback(conn).await
                },
                on_disconnect,
            );
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
                Ok(StreamListener::Tls(
                    tcp_listener,
                    Box::new(TlsLayer::server(tls_config)),
                ))
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
    /// `handshake_timeout` is in seconds.
    pub fn new_with_queue(
        key_pair: &KeyPair,
        connection_slots: Arc<ConnectionSlots>,
        conn_queue: Arc<ConnQueue>,
        monitor: Arc<Monitor>,
        handshake_timeout: u64,
        access_control: Arc<dyn AccessControl>,
        ex: Executor,
    ) -> Arc<Self> {
        Arc::new(Self {
            key_pair: key_pair.clone(),
            connection_slots,
            conn_queue: Some(conn_queue),
            task_group: TaskGroup::with_executor(ex),
            monitor,
            handshake_timeout: Duration::from_secs(handshake_timeout),
            access_control,
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

        self.task_group.spawn({
            let this = self.clone();
            async move { this.listen_loop(Arc::new(listener)).await }
        });
        Ok(resolved)
    }

    /// Accept loop (auto-queue mode).
    async fn listen_loop(self: Arc<Self>, listener: Arc<StreamListener>) {
        let conn_queue = self
            .conn_queue
            .as_ref()
            .expect("listen_loop requires ConnQueue")
            .clone();

        loop {
            self.connection_slots.wait_for_slot().await;

            let stream = match listener.accept().await {
                Ok(s) => s,
                Err(err) => {
                    error!("Failed to accept connection: {err}");
                    self.monitor.notify(ConnectionKind::AcceptFailed).await;
                    continue;
                }
            };

            let endpoint = match stream.peer_endpoint() {
                Some(ep) => ep,
                None => {
                    self.monitor.notify(ConnectionKind::AcceptFailed).await;
                    error!("Failed to get peer endpoint");
                    continue;
                }
            };

            if !self.access_control.allow(
                &Subject::Endpoint(&endpoint),
                Action::Connect(ConnDirection::Inbound),
            ) {
                debug!("Rejected inbound connection from {endpoint}");
                continue;
            }

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
            let this = self.clone();
            let listener = listener.clone();
            self.task_group.spawn_then(
                async move {
                    let (conn, vpid) = this.upgrade(&listener, stream).await?;
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

        self.task_group.spawn_then(
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

            // Only the accept runs here. The handshake and the wait for
            // the peer's first stream both happen in the spawned task,
            // so a slow peer cannot stall the loop.
            let incoming = match quic_endpoint.accept_incoming().await {
                Ok(c) => c,
                Err(err) => {
                    error!("Failed to accept QUIC conn: {err}");
                    self.monitor.notify(ConnectionKind::AcceptFailed).await;
                    continue;
                }
            };

            let peer_ep = incoming.peer_endpoint();

            if !self.access_control.allow(
                &Subject::Endpoint(&peer_ep),
                Action::Connect(ConnDirection::Inbound),
            ) {
                debug!("Rejected inbound QUIC connection from {peer_ep}");
                continue;
            }

            self.monitor
                .notify(ConnectionKind::Accepted(peer_ep.clone()))
                .await;

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
            let handshake_timeout = self.handshake_timeout;
            self.task_group.spawn_then(
                async move {
                    // Without this the only bound is the QUIC idle
                    // timeout, which holds the slot far longer.
                    let quic_conn = timeout(handshake_timeout, incoming.handshake()).await??;
                    let vpid = quic_conn
                        .peer_certificates()
                        .as_deref()
                        .and_then(peer_id_from_certs);
                    // A peer that handshakes and then never opens a
                    // stream only wastes its own slot.
                    let stream = timeout(handshake_timeout, quic_conn.accept_stream()).await??;
                    let conn: FramedConn<PeerNetMsgCodec> = framed(stream, PeerNetMsgCodec::new());
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
