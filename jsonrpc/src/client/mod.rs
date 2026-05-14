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
    sync::{atomic::AtomicBool, Arc},
};

use async_channel::{Receiver, Sender};
use log::info;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;

use karyon_core::{async_runtime::Executor, async_util::TaskGroup, util::random_32};

use karyon_net::Endpoint;

#[cfg(feature = "tcp")]
use karyon_net::tcp::TcpConfig;

use crate::{
    codec::{JsonCodec, JsonRpcCodec},
    error::{Error, Result},
    message::{self, SubscriptionID},
};

#[cfg(feature = "quic")]
use karyon_net::quic::ClientQuicConfig;
#[cfg(feature = "quic")]
use parking_lot::Mutex;
#[cfg(feature = "quic")]
use std::collections::HashMap;

#[cfg(feature = "ws")]
use crate::codec::JsonRpcWsCodec;

pub use builder::ClientBuilder;
pub use subscriptions::Subscription;

use message_dispatcher::MessageDispatcher;
use subscriptions::Subscriptions;

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
    pub timeout: Option<u64>,
    pub subscription_buffer_size: usize,
}

pub(crate) enum ClientBackend {
    /// One persistent message connection used by many concurrent calls
    /// and subscriptions. Each request is tagged with a unique id; the
    /// reader task multiplexes responses back to the right caller.
    /// Used for TCP, TLS, WS, WSS, and Unix.
    Multiplexed {
        message_dispatcher: MessageDispatcher,
        subscriptions: Arc<Subscriptions>,
        // TODO what is the reason for this  ?
        // AND why not using Async queue ?
        send_chan: (Sender<serde_json::Value>, Receiver<serde_json::Value>),
    },
    #[cfg(feature = "quic")]
    QuicStream {
        quic_conn: Arc<karyon_net::quic::QuicConn>,
        subscriptions: Arc<Subscriptions>,
        /// Per-subscription unsubscribe signal: send the unsubscribe
        /// JSON; the reader task forwards it and exits.
        // TODO why not using async queue here ?
        unsub_chans: Mutex<HashMap<SubscriptionID, Sender<serde_json::Value>>>,
    },
    #[cfg(feature = "http")]
    Http {
        // Boxed because the HTTP backend (with hyper Client) is much
        // larger than the other ClientBackend variants.
        http_backend: Box<http::HttpClientBackend>,
        #[cfg(feature = "http3")]
        subscriptions: Arc<Subscriptions>,
    },
}

/// An RPC client that connects to a JSON-RPC 2.0 server.
///
/// `B` is the byte-stream codec used for TCP/TLS/Unix/QUIC/HTTP.
/// `W` is the WebSocket codec used for `ws://` / `wss://`.
/// Both default to `JsonCodec`.
pub struct Client<B = JsonCodec, W = JsonCodec> {
    pub(crate) disconnect: AtomicBool,
    pub(crate) task_group: Arc<TaskGroup>,
    pub(crate) config: ClientConfig,
    pub(crate) backend: ClientBackend,
    _codecs: PhantomData<(B, W)>,
}

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
    ) -> Result<Arc<Subscription>> {
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
                unsub_chans,
            } => {
                quic_stream::subscribe(self, quic_conn, subscriptions, unsub_chans, method, params)
                    .await
            }
            ClientBackend::Multiplexed { subscriptions, .. } => {
                let response = multiplexed::send_request(self, method, params).await?;
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
            ClientBackend::Http { http_backend, .. } => {
                self.h3_unsubscribe(http_backend, method, sub_id).await
            }
            #[cfg(feature = "quic")]
            ClientBackend::QuicStream {
                subscriptions,
                unsub_chans,
                ..
            } => {
                // The QUIC subscription stream's reader and writer halves
                // are owned by the spawned task. We can't write directly
                // from here, so we hand the unsubscribe message to that
                // task via a channel; the task forwards it on the wire
                // and exits.
                // Bind the removed sender in its own `let` so the parking_lot
                // MutexGuard temporary doesn't live across the `.await`.
                let ch = unsub_chans.lock().remove(&sub_id);
                if let Some(ch) = ch {
                    let request = message::Request {
                        jsonrpc: message::JSONRPC_VERSION.to_string(),
                        id: json!(random_32()),
                        method: method.to_string(),
                        params: Some(json!(sub_id)),
                    };
                    let _ = ch.send(serde_json::to_value(request)?).await;
                }
                subscriptions.unsubscribe(&sub_id).await;
                Ok(())
            }
            ClientBackend::Multiplexed { subscriptions, .. } => {
                let _ = multiplexed::send_request(self, method, sub_id).await?;
                subscriptions.unsubscribe(&sub_id).await;
                Ok(())
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
    ) -> Result<Arc<Self>> {
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
            return Ok(Self::with_backend(config, task_group, backend));
        }

        #[cfg(feature = "quic")]
        if let Endpoint::Quic(..) = &config.endpoint {
            let backend = quic_stream::build_backend(&config).await?;
            return Ok(Self::with_backend(config, task_group, backend));
        }

        #[cfg(feature = "ws")]
        if matches!(&config.endpoint, Endpoint::Ws(..) | Endpoint::Wss(..)) {
            let (backend, conn) = multiplexed::build_ws_backend(&config, ws_codec).await?;
            let client = Self::with_backend(config, task_group, backend);
            let (reader, writer) = conn.split();
            multiplexed::start_io_loop(&client, reader, writer);
            return Ok(client);
        }

        let (backend, conn) = multiplexed::build_byte_backend(&config, byte_codec).await?;
        let client = Self::with_backend(config, task_group, backend);
        let (reader, writer) = conn.split();
        multiplexed::start_io_loop(&client, reader, writer);
        Ok(client)
    }

    fn with_backend(
        config: ClientConfig,
        task_group: Arc<TaskGroup>,
        backend: ClientBackend,
    ) -> Arc<Self> {
        Arc::new(Client {
            disconnect: AtomicBool::new(false),
            task_group,
            config,
            backend,
            _codecs: PhantomData,
        })
    }
}
