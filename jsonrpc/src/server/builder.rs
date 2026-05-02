use std::{collections::HashMap, sync::Arc};

use karyon_core::async_runtime::Executor;

use karyon_net::ToEndpoint;

#[cfg(any(feature = "tcp", feature = "tls", feature = "quic"))]
use karyon_net::Endpoint;

#[cfg(feature = "tcp")]
use karyon_net::tcp::TcpConfig;

#[cfg(feature = "quic")]
use karyon_net::quic::ServerQuicConfig;

use crate::{
    codec::{JsonCodec, JsonRpcCodec},
    error::Result,
    message::{Notification, NotificationResult, JSONRPC_VERSION},
    server::channel::NewNotification,
    server::PubSubRPCService,
    server::RPCService,
    server::WsCodec,
};

#[cfg(any(feature = "tcp", feature = "tls", feature = "quic"))]
use crate::error::Error;

use super::{Server, ServerConfig};

/// Builder for constructing an RPC [`Server`].
///
/// # Example
///
/// ```no_run
/// use std::sync::Arc;
/// use serde_json::Value;
/// use karyon_jsonrpc::{error::RPCError, rpc_impl, server::ServerBuilder};
///
/// struct Ping {}
///
/// #[rpc_impl]
/// impl Ping {
///     async fn ping(&self, _params: Value) -> Result<Value, RPCError> {
///         Ok(serde_json::json!("pong"))
///     }
/// }
///
/// async {
///     let server = ServerBuilder::new("tcp://127.0.0.1:60000")
///         .expect("create builder")
///         .service(Arc::new(Ping {}))
///         .build().await
///         .expect("build server");
///
///     server.start_block().await.expect("run server");
/// };
/// ```
pub struct ServerBuilder<B, W = JsonCodec> {
    config: ServerConfig,
    byte_codec: B,
    ws_codec: W,
    executor: Option<Executor>,
}

impl<B, W> ServerBuilder<B, W>
where
    B: JsonRpcCodec,
    W: WsCodec,
{
    /// Add an RPC service.
    pub fn service(mut self, service: Arc<dyn RPCService>) -> Self {
        self.config.services.insert(service.name(), service);
        self
    }

    /// Add a PubSub RPC service.
    pub fn pubsub_service(mut self, service: Arc<dyn PubSubRPCService>) -> Self {
        self.config.pubsub_services.insert(service.name(), service);
        self
    }

    /// Set TCP config.
    #[cfg(feature = "tcp")]
    pub fn tcp_config(mut self, config: TcpConfig) -> Result<Self> {
        match self.config.endpoint {
            Endpoint::Tcp(..) | Endpoint::Tls(..) | Endpoint::Ws(..) | Endpoint::Wss(..) => {
                self.config.tcp_config = config;
                Ok(self)
            }
            _ => Err(Error::UnsupportedProtocol(self.config.endpoint.to_string())),
        }
    }

    /// Set TLS config.
    #[cfg(feature = "tls")]
    pub fn tls_config(mut self, config: karyon_net::tls::ServerTlsConfig) -> Result<Self> {
        match self.config.endpoint {
            Endpoint::Tls(..) | Endpoint::Wss(..) => {
                self.config.tls_config = Some(config);
                Ok(self)
            }
            _ => Err(Error::UnsupportedProtocol(format!(
                "Invalid tls config for endpoint: {}",
                self.config.endpoint
            ))),
        }
    }

    /// Set QUIC config.
    #[cfg(feature = "quic")]
    pub fn quic_config(mut self, config: ServerQuicConfig) -> Result<Self> {
        match self.config.endpoint {
            Endpoint::Quic(..) => {
                self.config.quic_config = Some(config);
                Ok(self)
            }
            #[cfg(feature = "http3")]
            Endpoint::Http(..) => {
                self.config.quic_config = Some(config);
                Ok(self)
            }
            _ => Err(Error::UnsupportedProtocol(format!(
                "Invalid quic config for endpoint: {}",
                self.config.endpoint
            ))),
        }
    }

    /// Set an executor.
    pub async fn with_executor(mut self, ex: Executor) -> Self {
        self.executor = Some(ex);
        self
    }

    /// Set a custom notification encoder.
    pub fn with_notification_encoder(
        mut self,
        encoder: fn(NewNotification) -> Notification,
    ) -> Self {
        self.config.notification_encoder = encoder;
        self
    }

    /// Override the WebSocket codec. Only meaningful for `ws://` /
    /// `wss://` endpoints.
    #[cfg(feature = "ws")]
    pub fn with_ws_codec<W2: WsCodec>(self, ws_codec: W2) -> ServerBuilder<B, W2> {
        ServerBuilder {
            config: self.config,
            byte_codec: self.byte_codec,
            ws_codec,
            executor: self.executor,
        }
    }

    /// Build the server.
    pub async fn build(self) -> Result<Arc<Server>> {
        Server::init(self.config, self.executor, self.byte_codec, self.ws_codec).await
    }
}

impl<B: JsonRpcCodec> ServerBuilder<B, JsonCodec> {
    /// Create a builder with a custom byte-stream codec. The default
    /// `JsonCodec` is used for `ws://` / `wss://` endpoints; override
    /// with `with_ws_codec`.
    pub fn new_with_codec(
        endpoint: impl ToEndpoint,
        codec: B,
    ) -> Result<ServerBuilder<B, JsonCodec>> {
        let endpoint = endpoint.to_endpoint()?;
        Ok(ServerBuilder {
            config: ServerConfig {
                endpoint,
                services: HashMap::new(),
                pubsub_services: HashMap::new(),
                #[cfg(feature = "tcp")]
                tcp_config: Default::default(),
                #[cfg(feature = "tls")]
                tls_config: None,
                #[cfg(feature = "quic")]
                quic_config: None,
                notification_encoder: default_notification_encoder,
            },
            byte_codec: codec,
            ws_codec: JsonCodec::default(),
            executor: None,
        })
    }
}

impl ServerBuilder<JsonCodec, JsonCodec> {
    /// Create a builder with the default JSON codec.
    pub fn new(endpoint: impl ToEndpoint) -> Result<ServerBuilder<JsonCodec, JsonCodec>> {
        Self::new_with_codec(endpoint, JsonCodec::default())
    }
}

fn default_notification_encoder(nt: NewNotification) -> Notification {
    let params = Some(serde_json::json!(NotificationResult {
        subscription: nt.sub_id,
        result: Some(nt.result),
    }));

    Notification {
        jsonrpc: JSONRPC_VERSION.to_string(),
        method: nt.method,
        params,
    }
}
