pub mod builder;
mod message_dispatcher;
mod multiplexed;
mod subscriptions;

#[cfg(feature = "http")]
mod http;
#[cfg(feature = "quic")]
mod quic_stream;

use std::{
    marker::PhantomData,
    sync::{Arc, atomic::AtomicBool},
};

use async_channel::{Receiver, Sender};
use log::info;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::json;

use karyon_core::{
    async_runtime::Executor,
    async_util::{TaskGroup, TaskGroupRef},
    util::random_32,
};

use karyon_net::Endpoint;

#[cfg(feature = "tcp")]
use karyon_net::tcp::TcpConfig;

use crate::{
    codec::{JsonCodec, JsonRpcCodec},
    error::{Error, Result},
    message::{self, SubscriptionID},
};

#[cfg(feature = "quic")]
use karyon_net::quic::{ClientQuicConfig, QuicConnRef};

#[cfg(feature = "ws")]
use crate::codec::JsonRpcWsCodec;

pub use builder::ClientBuilder;
#[cfg(feature = "http")]
pub use http::HttpConfig;
pub use subscriptions::{NotificationDecoder, Subscription, SubscriptionRef};

use message_dispatcher::MessageDispatcher;
#[cfg(feature = "quic")]
use subscriptions::Subscriptions;
use subscriptions::SubscriptionsRef;

type RequestID = u32;

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

pub(crate) struct ClientConfig {
    pub endpoint: Endpoint,
    #[cfg(feature = "tcp")]
    pub tcp_config: TcpConfig,
    #[cfg(feature = "tls")]
    pub tls_config: Option<karyon_net::tls::ClientTlsConfig>,
    #[cfg(feature = "quic")]
    pub quic_config: Option<ClientQuicConfig>,
    #[cfg(feature = "http")]
    pub http_config: HttpConfig,
    pub timeout: Option<u64>,
    pub subscription_buffer_size: usize,
    /// Set via `ClientBuilder::with_notification_decoder`.
    pub notification_decoder: NotificationDecoder,
}

pub(crate) enum ClientBackend {
    /// One persistent message connection used by many concurrent calls
    /// and subscriptions. Each request is tagged with a unique id; the
    /// reader task multiplexes responses back to the right caller.
    /// Used for TCP, TLS, WS, WSS, and Unix.
    Multiplexed {
        message_dispatcher: MessageDispatcher,
        subscriptions: SubscriptionsRef,
        // TODO what is the reason for this  ?
        // AND why not using Async queue ?
        send_chan: (Sender<serde_json::Value>, Receiver<serde_json::Value>),
    },
    #[cfg(feature = "quic")]
    QuicStream {
        quic_conn: QuicConnRef,
        subscriptions: SubscriptionsRef,
    },
    #[cfg(feature = "http")]
    Http {
        // Boxed because the HTTP backend (with hyper Client) is much
        // larger than the other ClientBackend variants.
        http_backend: Box<http::HttpClientBackend>,
        #[cfg(feature = "http3")]
        subscriptions: SubscriptionsRef,
    },
}

/// An RPC client that connects to a JSON-RPC 2.0 server.
///
/// `B` is the byte-stream codec used for TCP/TLS/Unix/QUIC/HTTP.
/// `W` is the WebSocket codec used for `ws://` / `wss://`.
/// Both default to `JsonCodec`.
pub struct Client<B = JsonCodec, W = JsonCodec> {
    pub(crate) disconnect: AtomicBool,
    pub(crate) task_group: TaskGroupRef,
    pub(crate) config: ClientConfig,
    pub(crate) backend: ClientBackend,
    /// Encodes and decodes HTTP bodies, HTTP/3 and QUIC streams. The
    /// other backends own their codec inside the connection.
    #[cfg(any(feature = "http", feature = "quic"))]
    pub(crate) codec: B,
    _codecs: PhantomData<(B, W)>,
}

/// `Arc` reference to a `Client`.
pub type ClientRef<B = JsonCodec, W = JsonCodec> = Arc<Client<B, W>>;

impl<B, W> Client<B, W>
where
    B: JsonRpcCodec,
    W: WsCodec,
{
    /// Call a method, wait for response.
    pub async fn call<T: Serialize + DeserializeOwned, V: DeserializeOwned>(
        &self,
        method: &str,
        params: T,
    ) -> Result<V> {
        let response = match &self.backend {
            #[cfg(feature = "http")]
            ClientBackend::Http { http_backend, .. } => {
                self.http_call(http_backend, method, params).await?
            }
            #[cfg(feature = "quic")]
            ClientBackend::QuicStream { quic_conn, .. } => {
                quic_stream::call(self, quic_conn, method, params).await?
            }
            _ => multiplexed::send_request(self, method, params).await?,
        };

        if let Some(error) = response.error {
            return Err(Error::CallError(error.code, error.message));
        }

        match response.result {
            Some(result) => Ok(serde_json::from_value::<V>(result)?),
            None => Err(Error::InvalidMsg("Invalid response result".to_string())),
        }
    }

    /// Subscribe to a method.
    pub async fn subscribe<T: Serialize + DeserializeOwned>(
        self: &Arc<Self>,
        method: &str,
        params: T,
    ) -> Result<SubscriptionRef> {
        match &self.backend {
            #[cfg(all(feature = "http", not(feature = "http3")))]
            ClientBackend::Http { .. } => Err(Error::UnsupportedProtocol(
                "Subscriptions not supported over HTTP/1-2".to_string(),
            )),
            #[cfg(feature = "http3")]
            ClientBackend::Http { http_backend, .. } => {
                self.h3_subscribe(http_backend, method, params).await
            }
            #[cfg(feature = "quic")]
            ClientBackend::QuicStream {
                quic_conn,
                subscriptions,
            } => quic_stream::subscribe(self, quic_conn, subscriptions, method, params).await,
            ClientBackend::Multiplexed { subscriptions, .. } => {
                let response = multiplexed::send_request(self, method, params).await?;
                if let Some(error) = response.error {
                    return Err(Error::SubscribeError(error.code, error.message));
                }
                let sub_id = match response.result {
                    Some(result) => serde_json::from_value::<SubscriptionID>(result)?,
                    None => return Err(Error::InvalidMsg("Invalid subscription id".to_string())),
                };
                let sub = subscriptions.subscribe(sub_id).await;
                Ok(sub)
            }
        }
    }

    /// Unsubscribe.
    pub async fn unsubscribe(&self, method: &str, sub_id: SubscriptionID) -> Result<()> {
        match &self.backend {
            #[cfg(all(feature = "http", not(feature = "http3")))]
            ClientBackend::Http { .. } => Err(Error::UnsupportedProtocol(
                "Subscriptions not supported over HTTP/1-2".to_string(),
            )),
            #[cfg(feature = "http3")]
            ClientBackend::Http { subscriptions, .. } => {
                stream_unsubscribe(subscriptions, method, sub_id).await
            }
            #[cfg(feature = "quic")]
            ClientBackend::QuicStream { subscriptions, .. } => {
                stream_unsubscribe(subscriptions, method, sub_id).await
            }
            ClientBackend::Multiplexed { subscriptions, .. } => {
                let response = multiplexed::send_request(self, method, sub_id).await?;
                subscriptions.unsubscribe(&sub_id).await;
                match response.error {
                    Some(error) => Err(Error::SubscribeError(error.code, error.message)),
                    None => Ok(()),
                }
            }
        }
    }

    /// Disconnect the client.
    pub async fn stop(&self) {
        self.task_group.cancel().await;
    }

    /// Pick the right backend based on endpoint kind, then build the
    /// `Client`. Each backend module returns just the `ClientBackend`;
    /// assembly happens here. Multiplexed mode also needs an extra
    /// step to start its background reader/writer loop.
    pub(super) async fn init(
        config: ClientConfig,
        byte_codec: B,
        ws_codec: W,
        executor: Option<Executor>,
    ) -> Result<ClientRef<B, W>> {
        info!("Connecting to RPC endpoint: {}", config.endpoint);

        // Build task_group early so backends (HTTP driver tasks) can
        // spawn into the same group that Client::stop cancels.
        let task_group = Arc::new(match executor {
            Some(ex) => TaskGroup::with_executor(ex),
            None => TaskGroup::new(),
        });

        #[cfg(feature = "http")]
        if let Endpoint::Http(..) = &config.endpoint {
            let backend = http::build_backend(&config, task_group.clone()).await?;
            return Ok(Self::with_backend(config, task_group, backend, byte_codec));
        }

        #[cfg(feature = "quic")]
        if let Endpoint::Quic(..) = &config.endpoint {
            let backend = quic_stream::build_backend(&config, task_group.executor()).await?;
            return Ok(Self::with_backend(config, task_group, backend, byte_codec));
        }

        #[cfg(feature = "ws")]
        if matches!(&config.endpoint, Endpoint::Ws(..) | Endpoint::Wss(..)) {
            let (backend, conn) = multiplexed::build_ws_backend(&config, ws_codec).await?;
            let client = Self::with_backend(config, task_group, backend, byte_codec);
            let (reader, writer) = conn.split();
            multiplexed::start_io_loop(&client, reader, writer);
            return Ok(client);
        }

        let (backend, conn) = multiplexed::build_byte_backend(&config, byte_codec.clone()).await?;
        let client = Self::with_backend(config, task_group, backend, byte_codec);
        let (reader, writer) = conn.split();
        multiplexed::start_io_loop(&client, reader, writer);
        Ok(client)
    }

    fn with_backend(
        config: ClientConfig,
        task_group: TaskGroupRef,
        backend: ClientBackend,
        codec: B,
    ) -> ClientRef<B, W> {
        // Only the HTTP and QUIC backends read the codec from the client.
        #[cfg(not(any(feature = "http", feature = "quic")))]
        drop(codec);
        Arc::new(Client {
            disconnect: AtomicBool::new(false),
            task_group,
            config,
            backend,
            #[cfg(any(feature = "http", feature = "quic"))]
            codec,
            _codecs: PhantomData,
        })
    }
}

/// Unsubscribe on a stream backend (QUIC, HTTP/3): remove the
/// subscription and let its stream task send the request.
#[cfg(feature = "quic")]
async fn stream_unsubscribe(
    subscriptions: &Subscriptions,
    method: &str,
    sub_id: SubscriptionID,
) -> Result<()> {
    let request = message::Request {
        jsonrpc: message::JSONRPC_VERSION.to_string(),
        id: json!(random_32()),
        method: method.to_string(),
        params: Some(json!(sub_id)),
    };
    let msg = serde_json::to_value(request)?;
    subscriptions.unsubscribe_stream(&sub_id, msg).await;
    Ok(())
}
