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

use log::{debug, error, info};

use karyon_core::{
    async_runtime::Executor,
    async_util::{select, timeout, AsyncQueue, Either, TaskGroup, TaskResult},
};

use karyon_net::{Endpoint, MessageRx, MessageTx};

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
        acceptor::{AsyncAcceptor, StreamAcceptor},
        channel::NewNotification,
    },
};

#[cfg(feature = "ws")]
use crate::{codec::JsonRpcWsCodec, server::acceptor::WsAcceptor};

pub use builder::ServerBuilder;
pub use channel::Channel;
pub use pubsub_service::{PubSubRPCMethod, PubSubRPCService};
pub use service::{RPCMethod, RPCService};

pub const INVALID_REQUEST_ERROR_MSG: &str = "Invalid request";
pub const FAILED_TO_PARSE_ERROR_MSG: &str = "Failed to parse";
pub const METHOD_NOT_FOUND_ERROR_MSG: &str = "Method not found";
pub const UNSUPPORTED_JSONRPC_VERSION: &str = "Unsupported jsonrpc version";

const CHANNEL_SUBSCRIPTION_BUFFER_SIZE: usize = 100;

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
    pub services: HashMap<String, Arc<dyn RPCService + 'static>>,
    pub pubsub_services: HashMap<String, Arc<dyn PubSubRPCService + 'static>>,
    /// `None` (the default) means never. Set via
    /// `ServerBuilder::read_timeout`.
    pub read_timeout: Option<Duration>,
    /// Set via `ServerBuilder::handshake_timeout`.
    #[cfg(any(feature = "tls", feature = "ws"))]
    pub handshake_timeout: Duration,
    /// User-customizable notification wire format.
    /// Set via `ServerBuilder::with_notification_encoder`.
    pub notification_encoder: fn(NewNotification) -> message::Notification,
}

/// One variant per backend family. Stream-based transports share
/// a single `AsyncAcceptor`; QUIC and HTTP have their own loops.
enum ServerBackend {
    StreamAcceptor(Arc<dyn AsyncAcceptor>),
    #[cfg(feature = "quic")]
    QuicEndpoint(karyon_net::quic::QuicEndpoint),
    #[cfg(feature = "http")]
    Http(http::HttpServer),
}

/// A JSON-RPC 2.0 server.
pub struct Server {
    backend: ServerBackend,
    pub(crate) task_group: Arc<TaskGroup>,
    pub(crate) config: ServerConfig,
}

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
        match &self.backend {
            // Only the kernel accept runs here; the handshake runs in a
            // separate task, so a slow client cannot stall the loop.
            ServerBackend::StreamAcceptor(acceptor) => loop {
                match acceptor.accept().await {
                    Ok(stream) => {
                        let acceptor = acceptor.clone();
                        let server = self.clone();
                        self.task_group.spawn(async move {
                            if let Err(err) = acceptor.handle(stream, &server).await {
                                error!("Handle connection: {err}");
                            }
                        });
                    }
                    Err(err) => error!("Accept connection: {err}"),
                }
            },
            // As above: the QUIC handshake runs in its own task.
            #[cfg(feature = "quic")]
            ServerBackend::QuicEndpoint(endpoint) => loop {
                match endpoint.accept_incoming().await {
                    Ok(incoming) => self.handle_quic_incoming(incoming),
                    Err(err) => {
                        error!("Accept QUIC conn: {err}")
                    }
                }
            },
            #[cfg(feature = "http")]
            ServerBackend::Http(http_server) => {
                http::accept_loop(self.clone(), http_server).await?;
                Ok(())
            }
        }
    }

    pub fn local_endpoint(&self) -> Result<Endpoint> {
        match &self.backend {
            ServerBackend::StreamAcceptor(acceptor) => acceptor.local_endpoint(),
            #[cfg(feature = "quic")]
            ServerBackend::QuicEndpoint(endpoint) => endpoint.local_endpoint().map_err(Error::from),
            #[cfg(feature = "http")]
            ServerBackend::Http(http_server) => http_server.local_endpoint(),
        }
    }

    pub async fn shutdown(&self) {
        self.task_group.cancel().await;
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
        let channel = Channel::new(ch_tx);
        let queue = AsyncQueue::new(RESPONSE_QUEUE_SIZE);

        let writer_chan = channel.clone();
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
            },
        );

        let reader_chan = channel.clone();
        let read_timeout = self.config.read_timeout;
        self.task_group.spawn_then(
            stream_reader_task(self.clone(), reader, queue, channel, read_timeout),
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

    async fn new_request(
        self: &Arc<Self>,
        queue: Arc<AsyncQueue<serde_json::Value>>,
        channel: Arc<Channel>,
        msg: serde_json::Value,
    ) {
        self.task_group.spawn_then(
            request_task(self.clone(), queue, channel, msg),
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
    ) -> Result<Arc<Self>>
    where
        B: JsonRpcCodec,
        W: WsCodec,
    {
        let task_group = Arc::new(match ex {
            Some(ex) => TaskGroup::with_executor(ex),
            None => TaskGroup::new(),
        });

        let backend = create_backend(&config, byte_codec, ws_codec).await?;
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

async fn create_backend<B, W>(
    config: &ServerConfig,
    byte_codec: B,
    ws_codec: W,
) -> Result<ServerBackend>
where
    B: JsonRpcCodec,
    W: WsCodec,
{
    let endpoint = config.endpoint.clone();
    match endpoint {
        #[cfg(feature = "http")]
        Endpoint::Http(..) => {
            #[cfg(feature = "http3")]
            let http_server = match config.quic_config.clone() {
                Some(quic_cfg) => http::HttpServer::new_h3(&endpoint, quic_cfg).await?,
                None => http::HttpServer::new(&endpoint).await?,
            };
            #[cfg(not(feature = "http3"))]
            let http_server = http::HttpServer::new(&endpoint).await?;
            Ok(ServerBackend::Http(http_server))
        }
        #[cfg(feature = "quic")]
        Endpoint::Quic(..) => match &config.quic_config {
            Some(conf) => {
                let quic_endpoint =
                    karyon_net::quic::QuicEndpoint::listen(&endpoint, conf.clone()).await?;
                Ok(ServerBackend::QuicEndpoint(quic_endpoint))
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
    queue: Arc<AsyncQueue<serde_json::Value>>,
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

async fn stream_reader_task<R>(
    server: Arc<Server>,
    mut reader: R,
    queue: Arc<AsyncQueue<serde_json::Value>>,
    channel: Arc<Channel>,
    read_timeout: Option<Duration>,
) -> Result<()>
where
    R: MessageRx<Message = serde_json::Value> + Send,
{
    loop {
        // Once set, an idle client is dropped instead of holding the
        // socket open forever.
        let msg = match read_timeout {
            Some(t) => timeout(t, reader.recv_msg()).await??,
            None => reader.recv_msg().await?,
        };
        server
            .new_request(queue.clone(), channel.clone(), msg)
            .await;
    }
}

async fn request_task(
    server: Arc<Server>,
    queue: Arc<AsyncQueue<serde_json::Value>>,
    channel: Arc<Channel>,
    msg: serde_json::Value,
) -> Result<()> {
    let response = server.handle_request(Some(channel), msg).await;
    debug!("--> {response}");
    queue.push(serde_json::json!(response)).await;
    Ok(())
}
