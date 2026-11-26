mod acceptor;
pub mod builder;
pub mod channel;
mod dispatch;
pub mod pubsub_service;
pub mod service;

#[cfg(feature = "quic")]
mod quic;

#[cfg(feature = "http")]
mod http;

use std::{collections::HashMap, sync::Arc, time::Duration};

use serde_json::Value;

use log::{debug, error, info};

use karyon_core::{
    async_runtime::{
        Executor,
        lock::{Semaphore, SemaphoreGuard},
    },
    async_util::{
        AsyncQueue, AsyncQueueRef, Backoff, CondWait, CondWaitRef, Either, TaskGroup, TaskGroupRef,
        TaskResult, select,
    },
};

use karyon_net::{Endpoint, MessageRx, MessageTx, transport::TransportRef};

#[cfg(feature = "ws")]
use karyon_net::layers::ws::WsLayer;
#[cfg(feature = "tcp")]
use karyon_net::tcp::{TcpConfig, TcpListener};
#[cfg(feature = "tls")]
use karyon_net::tls::TlsLayer;
#[cfg(all(feature = "unix", target_family = "unix"))]
use karyon_net::unix::UnixListener;

use crate::{
    codec::JsonRpcCodec,
    error::{Error, Result},
    message,
    server::{
        acceptor::{AsyncAcceptorRef, StreamAcceptor},
        channel::NewNotification,
    },
};

#[cfg(feature = "http")]
use crate::server::http::{HttpAcceptorRef, HttpServer};
#[cfg(feature = "quic")]
use crate::server::quic::{QuicAcceptorRef, QuicServer};
#[cfg(feature = "ws")]
use crate::{codec::JsonRpcWsCodec, server::acceptor::WsAcceptor};

pub use builder::ServerBuilder;
pub use channel::{Channel, ChannelRef};
#[cfg(feature = "http")]
pub use http::HttpConfig;
pub use pubsub_service::{PubSubRPCMethod, PubSubRPCService, PubSubRPCServiceRef};
pub use service::{RPCMethod, RPCService, RPCServiceRef};

pub const INVALID_REQUEST_ERROR_MSG: &str = "Invalid request";
pub const FAILED_TO_PARSE_ERROR_MSG: &str = "Failed to parse";
pub const METHOD_NOT_FOUND_ERROR_MSG: &str = "Method not found";
pub const UNSUPPORTED_JSONRPC_VERSION: &str = "Unsupported jsonrpc version";

const CHANNEL_SUBSCRIPTION_BUFFER_SIZE: usize = 100;

/// Default max requests handled at once per message connection. The reader
/// waits for a free slot before reading the next request.
pub(super) const DEFAULT_MAX_CONCURRENT_REQUESTS: usize = 64;

/// Default initial retry delay after a failed accept, e.g. when out of file
/// descriptors.
pub(super) const DEFAULT_ACCEPT_BACKOFF_BASE: Duration = Duration::from_millis(10);
/// Default maximum retry delay after a failed accept.
pub(super) const DEFAULT_ACCEPT_BACKOFF_MAX: Duration = Duration::from_millis(1000);

/// Bound on the per-connection outbound response queue.
const RESPONSE_QUEUE_SIZE: usize = 256;

pub(crate) struct ServerConfig {
    pub endpoint: Endpoint,
    #[cfg(feature = "tcp")]
    pub tcp_config: TcpConfig,
    #[cfg(feature = "tls")]
    pub tls_config: Option<karyon_net::tls::ServerTlsConfig>,
    #[cfg(feature = "quic")]
    pub quic_config: Option<karyon_net::quic::ServerQuicConfig>,
    /// Listens on the endpoints it supports instead of the built-in
    /// transports.
    pub transport: Option<TransportRef>,
    pub services: HashMap<String, RPCServiceRef>,
    pub methods: HashMap<String, service::RegisteredMethod>,
    pub pubsub_services: HashMap<String, PubSubRPCServiceRef>,
    /// `None` (the default) means never. Set via
    /// `ServerBuilder::read_timeout`.
    pub read_timeout: Option<Duration>,
    /// Set via `ServerBuilder::handshake_timeout`.
    #[cfg(any(feature = "tls", feature = "ws"))]
    pub handshake_timeout: Duration,
    /// User-customizable notification wire format.
    /// Set via `ServerBuilder::with_notification_encoder`.
    pub notification_encoder: fn(NewNotification) -> message::Notification,
    /// Set via `ServerBuilder::http_config`.
    #[cfg(feature = "http")]
    pub http_config: HttpConfig,
    /// Set via `ServerBuilder::max_concurrent_requests`.
    pub max_concurrent_requests: usize,
    /// Set via `ServerBuilder::accept_backoff_base`.
    pub accept_backoff_base: Duration,
    /// Set via `ServerBuilder::accept_backoff_max`.
    pub accept_backoff_max: Duration,
}

impl ServerConfig {
    fn service_method(&self, service: &str, method: &str) -> Option<RPCMethod<'_>> {
        self.services.get(service)?.get_method(method)
    }
}

/// One variant per backend family. Stream-based transports share
/// a single `AsyncAcceptor`; QUIC and HTTP have their own loops.
enum ServerBackend {
    StreamAcceptor(AsyncAcceptorRef),
    #[cfg(feature = "quic")]
    Quic(QuicAcceptorRef),
    #[cfg(feature = "http")]
    Http(HttpAcceptorRef),
}

/// A JSON-RPC 2.0 server.
pub struct Server {
    backend: ServerBackend,
    pub(crate) task_group: TaskGroupRef,
    pub(crate) config: ServerConfig,
}

/// `Arc` reference to a `Server`.
pub type ServerRef = Arc<Server>;

impl Server {
    pub fn start(self: Arc<Self>) {
        self.task_group.spawn(self.clone().start_block());
    }

    pub async fn start_block(self: Arc<Self>) -> Result<()> {
        if let Err(err) = self.accept_loop().await {
            error!("Main accept loop stopped: {err}");
            self.shutdown().await;
        };
        Ok(())
    }

    async fn accept_loop(self: &Arc<Self>) -> Result<()> {
        // Back off on accept errors, so the loop does not spin.
        let backoff = self.accept_backoff();
        match &self.backend {
            ServerBackend::StreamAcceptor(acceptor) => loop {
                match acceptor.accept().await {
                    Ok(stream) => {
                        backoff.reset();
                        let acceptor = acceptor.clone();
                        let server = self.clone();
                        self.task_group.spawn(async move {
                            if let Err(err) = acceptor.handle(stream, &server).await {
                                error!("Handle connection: {err}");
                            }
                        });
                    }
                    Err(err) => {
                        error!("Accept connection: {err}");
                        backoff.sleep().await;
                    }
                }
            },
            #[cfg(feature = "quic")]
            ServerBackend::Quic(quic_server) => {
                quic_server.accept_loop(self.clone()).await?;
                Ok(())
            }
            #[cfg(feature = "http")]
            ServerBackend::Http(http_server) => {
                http_server.accept_loop(self.clone()).await?;
                Ok(())
            }
        }
    }

    pub fn local_endpoint(&self) -> Result<Endpoint> {
        match &self.backend {
            ServerBackend::StreamAcceptor(acceptor) => acceptor.local_endpoint(),
            #[cfg(feature = "quic")]
            ServerBackend::Quic(quic_server) => quic_server.local_endpoint(),
            #[cfg(feature = "http")]
            ServerBackend::Http(http_server) => http_server.local_endpoint(),
        }
    }

    pub async fn shutdown(&self) {
        self.task_group.cancel().await;
    }

    pub(super) fn accept_backoff(&self) -> Backoff {
        let base_ms = self
            .config
            .accept_backoff_base
            .as_millis()
            .min(u64::MAX as u128) as u64;
        let max_ms = self
            .config
            .accept_backoff_max
            .as_millis()
            .min(u64::MAX as u128) as u64;
        Backoff::with_executor(base_ms, max_ms, self.task_group.executor().clone())
    }

    /// Handle a split message connection (framed byte stream or
    /// WebSocket). Spawns a reader and writer task; the writer drains
    /// both responses and notifications.
    pub(crate) fn handle_message_conn<R, W>(
        self: &Arc<Self>,
        reader: R,
        writer: W,
        peer: Option<Endpoint>,
    ) where
        R: MessageRx<Message = serde_json::Value> + Send + 'static,
        W: MessageTx<Message = serde_json::Value> + Send + 'static,
    {
        debug!("Handle connection {peer:?}");

        let (ch_tx, ch_rx) = async_channel::bounded(CHANNEL_SUBSCRIPTION_BUFFER_SIZE);
        let channel = Channel::new(ch_tx, peer.clone());
        let queue = AsyncQueue::new(RESPONSE_QUEUE_SIZE);
        // Set when the writer stops. The reader and the request tasks
        // stop too, instead of waiting forever on a full queue.
        let writer_done = Arc::new(CondWait::new());

        let writer_chan = channel.clone();
        let writer_stopped = writer_done.clone();
        self.task_group.spawn_then(
            stream_writer_task(
                writer,
                queue.clone(),
                ch_rx,
                self.config.notification_encoder,
            ),
            |result: TaskResult<Result<()>>| async move {
                if let TaskResult::Completed(Err(err)) = result {
                    debug!("Writer stopped: {err}");
                }
                writer_chan.close();
                writer_stopped.broadcast().await;
            },
        );

        let reader_chan = channel.clone();
        let read_timeout = self.config.read_timeout;
        let conn = ConnState {
            queue,
            channel,
            writer_done,
        };
        self.task_group.spawn_then(
            stream_reader_task(self.clone(), reader, conn, read_timeout),
            |result: TaskResult<Result<()>>| async move {
                if let TaskResult::Completed(Err(err)) = result {
                    debug!("Connection {peer:?} dropped: {err}");
                } else {
                    debug!("Connection {peer:?} dropped");
                }
                reader_chan.close();
            },
        );
    }

    fn new_request(
        self: &Arc<Self>,
        conn: ConnState,
        permit: SemaphoreGuard,
        msg: serde_json::Value,
    ) {
        self.task_group.spawn_then(
            request_task(self.clone(), conn, permit, msg),
            |result: TaskResult<Result<()>>| async move {
                if let TaskResult::Completed(Err(err)) = result {
                    error!("Handle request: {err}");
                }
            },
        );
    }

    pub(super) async fn init<B, W>(
        config: ServerConfig,
        ex: Option<Executor>,
        byte_codec: B,
        ws_codec: W,
    ) -> Result<ServerRef>
    where
        B: JsonRpcCodec,
        W: WsCodec,
    {
        let task_group = Arc::new(match ex {
            Some(ex) => TaskGroup::with_executor(ex),
            None => TaskGroup::new(),
        });

        let backend = create_backend(&config, task_group.executor(), byte_codec, ws_codec).await?;
        info!("RPC server listens to the endpoint: {}", config.endpoint);

        Ok(Arc::new(Server {
            backend,
            task_group,
            config,
        }))
    }
}

/// Bound on the WebSocket codec generic. With the `ws` feature it
/// requires `JsonRpcWsCodec`; otherwise it accepts any clonable type
/// (the codec is unused) so callers can pass `JsonCodec` unchanged.
#[cfg(feature = "ws")]
pub trait WsCodec: JsonRpcWsCodec {}
#[cfg(feature = "ws")]
impl<T: JsonRpcWsCodec> WsCodec for T {}

#[cfg(not(feature = "ws"))]
pub trait WsCodec: Clone + Send + Sync + 'static {}
#[cfg(not(feature = "ws"))]
impl<T: Clone + Send + Sync + 'static> WsCodec for T {}

/// QUIC and HTTP/3 run on `ex`, the server's executor.
async fn create_backend<B, W>(
    config: &ServerConfig,
    ex: &Executor,
    byte_codec: B,
    ws_codec: W,
) -> Result<ServerBackend>
where
    B: JsonRpcCodec,
    W: WsCodec,
{
    let endpoint = config.endpoint.clone();
    // Only the WS backend uses the WS codec.
    #[cfg(not(feature = "ws"))]
    drop(ws_codec);
    // Only QUIC and HTTP/3 use the executor.
    #[cfg(not(feature = "quic"))]
    let _ = ex;

    if let Some(transport) = config.transport.as_ref().filter(|t| t.supports(&endpoint)) {
        return Ok(ServerBackend::StreamAcceptor(Arc::new(StreamAcceptor {
            listener: transport.listen(&endpoint).await?,
            codec: byte_codec,
            #[cfg(feature = "tls")]
            tls: None,
            #[cfg(feature = "tls")]
            handshake_timeout: config.handshake_timeout,
        })));
    }

    match endpoint {
        #[cfg(feature = "http")]
        Endpoint::Http(..) => {
            #[cfg(feature = "http3")]
            let http_server = match config.quic_config.clone() {
                Some(quic_cfg) => {
                    let quic_cfg = quic_cfg.with_executor(ex.clone());
                    HttpServer::new_h3(&endpoint, quic_cfg, byte_codec).await?
                }
                None => HttpServer::new(&endpoint, byte_codec).await?,
            };
            #[cfg(not(feature = "http3"))]
            let http_server = HttpServer::new(&endpoint, byte_codec).await?;
            Ok(ServerBackend::Http(Arc::new(http_server)))
        }
        #[cfg(feature = "quic")]
        Endpoint::Quic(..) => match &config.quic_config {
            Some(conf) => {
                let conf = conf.clone().with_executor(ex.clone());
                let quic_endpoint = karyon_net::quic::QuicEndpoint::listen(&endpoint, conf).await?;
                Ok(ServerBackend::Quic(Arc::new(QuicServer {
                    endpoint: quic_endpoint,
                    codec: byte_codec,
                })))
            }
            None => Err(Error::QUICConfigRequired),
        },
        #[cfg(feature = "tcp")]
        Endpoint::Tcp(..) => {
            let listener = TcpListener::bind(&endpoint, config.tcp_config.clone()).await?;
            Ok(ServerBackend::StreamAcceptor(Arc::new(StreamAcceptor {
                listener: Box::new(listener),
                codec: byte_codec,
                #[cfg(feature = "tls")]
                tls: None,
                #[cfg(feature = "tls")]
                handshake_timeout: config.handshake_timeout,
            })))
        }
        #[cfg(feature = "tls")]
        Endpoint::Tls(..) => {
            let tls_config = config.tls_config.as_ref().ok_or(Error::TLSConfigRequired)?;
            // Plain TCP listener; `StreamAcceptor::handle` runs the TLS
            // handshake.
            let listener = TcpListener::bind(&endpoint, config.tcp_config.clone()).await?;
            Ok(ServerBackend::StreamAcceptor(Arc::new(StreamAcceptor {
                listener: Box::new(listener),
                codec: byte_codec,
                tls: Some(TlsLayer::server(tls_config.clone())),
                handshake_timeout: config.handshake_timeout,
            })))
        }
        #[cfg(feature = "ws")]
        Endpoint::Ws(..) => {
            let listener = TcpListener::bind(&endpoint, config.tcp_config.clone()).await?;
            let layer = Arc::new(WsLayer::server(ws_codec));
            Ok(ServerBackend::StreamAcceptor(Arc::new(WsAcceptor {
                listener: Box::new(listener),
                layer,
                #[cfg(feature = "tls")]
                tls: None,
                handshake_timeout: config.handshake_timeout,
            })))
        }
        #[cfg(all(feature = "ws", feature = "tls"))]
        Endpoint::Wss(..) => {
            let tls_config = config.tls_config.as_ref().ok_or(Error::TLSConfigRequired)?;
            let listener = TcpListener::bind(&endpoint, config.tcp_config.clone()).await?;
            let layer = Arc::new(WsLayer::server(ws_codec));
            Ok(ServerBackend::StreamAcceptor(Arc::new(WsAcceptor {
                listener: Box::new(listener),
                layer,
                tls: Some(TlsLayer::server(tls_config.clone())),
                handshake_timeout: config.handshake_timeout,
            })))
        }
        #[cfg(all(feature = "unix", target_family = "unix"))]
        Endpoint::Unix(..) => {
            let listener = UnixListener::bind(&endpoint)?;
            Ok(ServerBackend::StreamAcceptor(Arc::new(StreamAcceptor {
                listener: Box::new(listener),
                codec: byte_codec,
                #[cfg(feature = "tls")]
                tls: None,
                #[cfg(feature = "tls")]
                handshake_timeout: config.handshake_timeout,
            })))
        }
        _ => Err(Error::UnsupportedProtocol(endpoint.to_string())),
    }
}

async fn stream_writer_task<W>(
    mut writer: W,
    queue: AsyncQueueRef<Value>,
    ch_rx: async_channel::Receiver<NewNotification>,
    notification_encoder: fn(NewNotification) -> message::Notification,
) -> Result<()>
where
    W: MessageTx<Message = serde_json::Value> + Send,
{
    loop {
        match select(queue.recv(), ch_rx.recv()).await {
            Either::Left(res) => {
                writer.send_msg(res).await?;
            }
            Either::Right(notification) => {
                let nt = notification?;
                let notification = notification_encoder(nt);
                debug!("--> {notification}");
                writer.send_msg(serde_json::json!(notification)).await?;
            }
        }
    }
}

/// Shared state of one message connection, cloned into each request.
#[derive(Clone)]
struct ConnState {
    queue: AsyncQueueRef<Value>,
    channel: ChannelRef,
    writer_done: CondWaitRef,
}

async fn stream_reader_task<R>(
    server: ServerRef,
    mut reader: R,
    conn: ConnState,
    read_timeout: Option<Duration>,
) -> Result<()>
where
    R: MessageRx<Message = serde_json::Value> + Send,
{
    let permits = Semaphore::new(server.config.max_concurrent_requests);
    let ex = server.task_group.executor().clone();
    loop {
        // Wait for a free slot, so one client cannot start an unbounded
        // number of request tasks.
        let permit = permits.acquire().await?;

        // Once set, an idle client is dropped instead of holding the
        // socket open forever.
        let recv = async {
            match read_timeout {
                Some(t) => ex.timeout(t, reader.recv_msg()).await?,
                None => reader.recv_msg().await,
            }
        };
        let msg = match select(conn.writer_done.wait(), recv).await {
            // Nobody sends the responses anymore.
            Either::Left(_) => return Ok(()),
            Either::Right(msg) => msg?,
        };
        server.new_request(conn.clone(), permit, msg);
    }
}

async fn request_task(
    server: ServerRef,
    conn: ConnState,
    _permit: SemaphoreGuard,
    msg: serde_json::Value,
) -> Result<()> {
    let response = server.handle_request(Some(conn.channel), msg).await;
    debug!("--> {response}");
    // Drop the response if the writer is gone.
    select(
        conn.queue.push(serde_json::json!(response)),
        conn.writer_done.wait(),
    )
    .await;
    Ok(())
}

#[cfg(all(test, not(target_family = "wasm")))]
mod tests {
    use std::future::pending;

    use async_trait::async_trait;
    use karyon_core::testing::run_test;
    use karyon_net::{
        ByteStream,
        transport::{Transport, TransportListener},
    };

    use super::*;

    struct DummyTransport;

    #[async_trait]
    impl Transport for DummyTransport {
        fn supports(&self, endpoint: &Endpoint) -> bool {
            matches!(endpoint, Endpoint::Custom { scheme, .. } if scheme == "mem")
        }

        async fn dial(&self, _endpoint: &Endpoint) -> karyon_net::Result<Box<dyn ByteStream>> {
            Err(karyon_net::Error::IO(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "dial unused in unit test",
            )))
        }

        async fn listen(
            &self,
            endpoint: &Endpoint,
        ) -> karyon_net::Result<Box<dyn TransportListener>> {
            Ok(Box::new(DummyListener(endpoint.clone())))
        }
    }

    struct DummyListener(Endpoint);

    #[async_trait]
    impl TransportListener for DummyListener {
        async fn accept(&self) -> karyon_net::Result<Box<dyn ByteStream>> {
            pending().await
        }

        fn local_endpoint(&self) -> karyon_net::Result<Endpoint> {
            Ok(self.0.clone())
        }
    }

    fn test_builder() -> ServerBuilder<crate::codec::JsonCodec, crate::codec::JsonCodec> {
        ServerBuilder::new("mem://test")
            .unwrap()
            .with_transport(Arc::new(DummyTransport))
    }

    #[test]
    fn server_limits_use_defaults_and_custom_values() {
        run_test(10, async {
            let default_srv = test_builder().build().await.unwrap();
            assert_eq!(default_srv.config.max_concurrent_requests, 64);
            assert_eq!(
                default_srv.config.accept_backoff_base,
                Duration::from_millis(10)
            );
            assert_eq!(
                default_srv.config.accept_backoff_max,
                Duration::from_millis(1000)
            );
            let default_backoff = default_srv.accept_backoff();
            assert_eq!(default_backoff.sleep().await, 10);
            default_srv.shutdown().await;

            let custom_srv = test_builder()
                .max_concurrent_requests(4)
                .accept_backoff_base(Duration::from_millis(2))
                .accept_backoff_max(Duration::from_millis(6))
                .build()
                .await
                .unwrap();
            assert_eq!(custom_srv.config.max_concurrent_requests, 4);
            let custom_backoff = custom_srv.accept_backoff();
            assert_eq!(custom_backoff.sleep().await, 2);
            assert_eq!(custom_backoff.sleep().await, 4);
            assert_eq!(custom_backoff.sleep().await, 6);
            assert_eq!(custom_backoff.sleep().await, 6);
            custom_srv.shutdown().await;
        });
    }

    #[test]
    fn build_rejects_invalid_server_limits() {
        run_test(10, async {
            assert!(matches!(
                test_builder().max_concurrent_requests(0).build().await,
                Err(Error::InvalidState(_))
            ));
            assert!(matches!(
                test_builder()
                    .accept_backoff_base(Duration::ZERO)
                    .build()
                    .await,
                Err(Error::InvalidState(_))
            ));
            assert!(matches!(
                test_builder()
                    .accept_backoff_base(Duration::from_millis(500))
                    .accept_backoff_max(Duration::from_millis(100))
                    .build()
                    .await,
                Err(Error::InvalidState(_))
            ));
            assert!(matches!(
                test_builder()
                    .accept_backoff_max(Duration::MAX)
                    .build()
                    .await,
                Err(Error::InvalidState(_))
            ));
        });
    }
}
