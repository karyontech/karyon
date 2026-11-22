use std::{future::Future, marker::PhantomData, sync::Arc, time::Duration};

use log::{debug, error, info};

use karyon_core::{
    async_runtime::{
        Executor,
        lock::{Semaphore, SemaphoreGuard},
    },
    async_util::{TaskGroup, TaskResult},
    crypto::KeyPair,
};

use karyon_net::{
    ByteBuffer, ByteStream, Endpoint, FramedConn, ServerLayer, codec::Codec, framed, tls::TlsLayer,
};

#[cfg(feature = "quic")]
use karyon_net::{
    StreamMux,
    quic::{QuicEndpoint, QuicIncoming},
};

use crate::{
    PeerID, Result,
    access_control::{AccessControlRef, Action, Subject},
    builtin_transports::BuiltinTransports,
    codec::PeerNetMsgCodec,
    config::Config,
    conn_direction::ConnDirection,
    discovery::DiscoveryContext,
    monitor::{ConnectionKind, MonitorRef},
    peer_cert::peer_id_from_certs,
    queued_conn::{ConnQueueRef, Incoming, QueuedConn},
    transport::{TransportListener, TransportRef},
};

/// `Arc` reference to a `StreamListener`.
type StreamListenerRef = Arc<StreamListener>;

/// Listener for byte-stream transports (TCP, TLS, a user `Transport`).
/// Accepting is split in two phases: `accept` does only the cheap accept,
/// `handshake` runs the TLS upgrade. QUIC uses a separate `StreamMux` path.
pub(crate) struct StreamListener {
    pub(crate) inner: Box<dyn TransportListener>,
    /// Upgrades the accepted streams of a TLS endpoint.
    pub(crate) tls: Option<Box<TlsLayer>>,
}

impl StreamListener {
    /// A listener whose streams are used as accepted.
    pub(crate) fn plain(inner: Box<dyn TransportListener>) -> Self {
        Self { inner, tls: None }
    }

    /// Kernel accept only. Returns the raw stream before any handshake.
    async fn accept(&self) -> Result<Box<dyn ByteStream>> {
        Ok(self.inner.accept().await?)
    }

    /// TLS upgrade, if this is a TLS listener. Runs off the accept loop.
    async fn handshake(
        &self,
        stream: Box<dyn ByteStream>,
        handshake_timeout: Duration,
        executor: &Executor,
    ) -> Result<Box<dyn ByteStream>> {
        match &self.tls {
            None => Ok(stream),
            Some(layer) => Ok(executor
                .timeout(
                    handshake_timeout,
                    ServerLayer::handshake(layer.as_ref(), stream),
                )
                .await??),
        }
    }

    fn local_endpoint(&self) -> Result<Endpoint> {
        let ep = self.inner.local_endpoint()?;
        // A TLS listener is plain TCP; report the TLS scheme so peers
        // dialling this endpoint run the handshake.
        if self.tls.is_some() {
            return Ok(Endpoint::Tls(ep.addr()?, ep.port()?));
        }
        Ok(ep)
    }
}

/// `Arc` reference to a `Listener`.
pub type ListenerRef<C> = Arc<Listener<C>>;

/// Creates inbound connections with other peers. Generic over the
/// codec applied to the framed accepted streams so the same accept
/// machinery serves the peer data-plane (`PeerNetMsgCodec`) and a
/// discovery's own messages.
pub struct Listener<C: Codec<ByteBuffer> + Default + Clone> {
    /// Listens on the endpoints `transport` does not support.
    builtin: BuiltinTransports,
    task_group: TaskGroup,
    connection_slots: Semaphore,
    /// Hands accepted connections to the PeerPool. Only on the Node's
    /// own listener.
    conn_queue: Option<ConnQueueRef>,
    monitor: MonitorRef,
    handshake_timeout: Duration,
    access_control: AccessControlRef,
    /// Listens on the endpoints it supports instead of the built-in
    /// transports.
    transport: Option<TransportRef>,
    _codec: PhantomData<C>,
}

impl<C> Listener<C>
where
    C: Codec<ByteBuffer, Error = karyon_net::Error> + Default + Clone + Send + Sync + 'static,
{
    /// Create a Listener for a discovery's own connections (no
    /// auto-queue; use `start_with_callback`). The key pair, monitor,
    /// executor and the handshake timeout, access control, transport and
    /// QUIC runtime of the node's config come from `ctx`.
    pub fn new(ctx: &DiscoveryContext, slots: usize) -> ListenerRef<C> {
        Self::build(
            &ctx.key_pair,
            &ctx.config,
            slots,
            None,
            ctx.monitor.clone(),
            ctx.executor.clone(),
        )
    }

    /// Builds a Listener. `conn_queue` is set only on the Node's own.
    fn build(
        key_pair: &KeyPair,
        config: &Config,
        slots: usize,
        conn_queue: Option<ConnQueueRef>,
        monitor: MonitorRef,
        ex: Executor,
    ) -> ListenerRef<C> {
        Arc::new(Self {
            builtin: BuiltinTransports::new(key_pair, config),
            connection_slots: Semaphore::new(slots),
            conn_queue,
            task_group: TaskGroup::with_executor(ex),
            monitor,
            handshake_timeout: Duration::from_secs(config.handshake_timeout),
            access_control: config.access_control.clone(),
            transport: config.transport.clone(),
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
        let stream = listener
            .handshake(stream, self.handshake_timeout, self.task_group.executor())
            .await?;
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
        listener: StreamListenerRef,
        callback: impl FnOnce(FramedConn<C>) -> Fut + Clone + Send + 'static,
    ) where
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        loop {
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

            // Callback mode serves discovery queries.
            if !self.access_control.allow(
                &Subject::Endpoint(&endpoint),
                Action::Lookup(ConnDirection::Inbound),
            ) {
                debug!("Rejected inbound lookup connection from {endpoint}");
                continue;
            }

            // Taken only once a peer is accepted: a slot waiting in an
            // idle endpoint's accept would block the other endpoints,
            // which share the slots. Released once the connection ends.
            let slot = match self.connection_slots.acquire().await {
                Ok(s) => s,
                Err(err) => {
                    error!("Failed to acquire a connection slot: {err}");
                    return;
                }
            };

            self.monitor
                .notify(ConnectionKind::Accepted(endpoint.clone()))
                .await;

            let on_disconnect = {
                let this = self.clone();
                |res| async move {
                    if let TaskResult::Completed(Err(err)) = res {
                        debug!("Inbound connection dropped: {err}");
                    }
                    this.monitor
                        .notify(ConnectionKind::Disconnected(endpoint))
                        .await;
                    drop(slot);
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

    /// Listen with the user transport if it supports `endpoint`, else
    /// with the built-in TCP or TLS.
    async fn listen(&self, endpoint: &Endpoint) -> Result<StreamListener> {
        if let Some(transport) = self.transport.as_ref().filter(|t| t.supports(endpoint)) {
            return Ok(StreamListener::plain(transport.listen(endpoint).await?));
        }
        self.builtin.listen(endpoint).await
    }
}

// Auto-queue paths only live on the peer-plane Listener. A discovery
// uses `start_with_callback` and handles each connection inline (no
// PeerPool / handshake pipeline).
impl Listener<PeerNetMsgCodec> {
    /// Create a new Listener that hands its connections to the PeerPool.
    pub(crate) fn new_with_queue(
        key_pair: &KeyPair,
        config: &Config,
        conn_queue: ConnQueueRef,
        monitor: MonitorRef,
        ex: Executor,
    ) -> ListenerRef<PeerNetMsgCodec> {
        Self::build(
            key_pair,
            config,
            config.inbound_slots,
            Some(conn_queue),
            monitor,
            ex,
        )
    }

    /// Start listening (auto-queue mode). Returns the resolved endpoint.
    pub(crate) async fn start(
        self: &ListenerRef<PeerNetMsgCodec>,
        endpoint: Endpoint,
    ) -> Result<Endpoint> {
        // A user transport takes the endpoints it supports, QUIC included.
        #[cfg(feature = "quic")]
        if endpoint.is_quic()
            && !self
                .transport
                .as_ref()
                .is_some_and(|t| t.supports(&endpoint))
        {
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

    /// The queue to the PeerPool. Set on every listener that runs the
    /// auto-queue paths.
    fn conn_queue(&self) -> ConnQueueRef {
        self.conn_queue
            .clone()
            .expect("auto-queue listener requires a PeerPool queue")
    }

    /// Accept loop (auto-queue mode).
    async fn listen_loop(self: ListenerRef<PeerNetMsgCodec>, listener: StreamListenerRef) {
        let conn_queue = self.conn_queue();

        loop {
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

            // Taken only once a peer is accepted: a slot waiting in an
            // idle endpoint's accept would block the other endpoints,
            // which share the slots. Released once the connection ends.
            let slot = match self.connection_slots.acquire().await {
                Ok(s) => s,
                Err(err) => {
                    error!("Failed to acquire a connection slot: {err}");
                    return;
                }
            };

            self.monitor
                .notify(ConnectionKind::Accepted(endpoint.clone()))
                .await;

            // The task ends once the connection is handed to the PeerPool.
            let this = self.clone();
            let listener = listener.clone();
            let conn_queue = conn_queue.clone();
            self.task_group.spawn(async move {
                let res = this
                    .queue_inbound(&listener, stream, slot, &conn_queue)
                    .await;
                if let Err(err) = res {
                    debug!("Inbound connection dropped: {err}");
                    this.monitor
                        .notify(ConnectionKind::Disconnected(endpoint))
                        .await;
                }
            });
        }
    }

    /// Upgrades an accepted stream and hands it to the PeerPool.
    async fn queue_inbound(
        &self,
        listener: &StreamListener,
        stream: Box<dyn ByteStream>,
        slot: SemaphoreGuard,
        conn_queue: &ConnQueueRef,
    ) -> Result<()> {
        let (conn, vpid) = self.upgrade(listener, stream).await?;
        let conn = QueuedConn::new(conn, ConnDirection::Inbound, vpid)?;
        conn_queue.push(Incoming { conn, slot }).await;
        Ok(())
    }

    /// QUIC listener.
    #[cfg(feature = "quic")]
    async fn start_quic(
        self: &ListenerRef<PeerNetMsgCodec>,
        endpoint: Endpoint,
    ) -> Result<Endpoint> {
        let server_config = self
            .builtin
            .quic_server_config(self.task_group.executor())?;
        let quic_endpoint = match QuicEndpoint::listen(&endpoint, server_config).await {
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

        let resolved = quic_endpoint.local_endpoint()?;
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
    async fn listen_loop_quic(self: ListenerRef<PeerNetMsgCodec>, quic_endpoint: QuicEndpoint) {
        let conn_queue = self.conn_queue();

        loop {
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

            // Taken only once a peer is accepted: a slot waiting in an
            // idle endpoint's accept would block the other endpoints,
            // which share the slots. Released once the connection ends.
            let slot = match self.connection_slots.acquire().await {
                Ok(s) => s,
                Err(err) => {
                    error!("Failed to acquire a connection slot: {err}");
                    return;
                }
            };

            self.monitor
                .notify(ConnectionKind::Accepted(peer_ep.clone()))
                .await;

            // The task ends once the connection is handed to the PeerPool.
            let this = self.clone();
            let conn_queue = conn_queue.clone();
            self.task_group.spawn(async move {
                let res = this.queue_inbound_quic(incoming, slot, &conn_queue).await;
                if let Err(err) = res {
                    debug!("Inbound QUIC conn dropped: {err}");
                    this.monitor
                        .notify(ConnectionKind::Disconnected(peer_ep))
                        .await;
                }
            });
        }
    }

    /// Runs the QUIC handshake, waits for the peer's first stream and
    /// hands the connection to the PeerPool.
    #[cfg(feature = "quic")]
    async fn queue_inbound_quic(
        &self,
        incoming: QuicIncoming,
        slot: SemaphoreGuard,
        conn_queue: &ConnQueueRef,
    ) -> Result<()> {
        // Without this the only bound is the QUIC idle timeout, which
        // holds the slot far longer.
        let ex = self.task_group.executor();
        let quic_conn = ex
            .timeout(self.handshake_timeout, incoming.handshake())
            .await??;
        let vpid = quic_conn
            .peer_certificates()
            .as_deref()
            .and_then(peer_id_from_certs);
        // A peer that handshakes and then never opens a stream only
        // wastes its own slot.
        let stream = ex
            .timeout(self.handshake_timeout, quic_conn.accept_stream())
            .await??;
        let conn: FramedConn<PeerNetMsgCodec> = framed(stream, PeerNetMsgCodec::new());
        let conn = QueuedConn::new(conn, ConnDirection::Inbound, vpid)?.with_quic(quic_conn);
        conn_queue.push(Incoming { conn, slot }).await;
        Ok(())
    }
}
