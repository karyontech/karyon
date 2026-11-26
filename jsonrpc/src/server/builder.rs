use std::{collections::HashMap, future::Future, time::Duration};

use karyon_core::async_runtime::Executor;
use serde_json::Value;

use karyon_net::{ToEndpoint, transport::TransportRef};

#[cfg(any(feature = "tcp", feature = "tls", feature = "quic"))]
use karyon_net::Endpoint;

#[cfg(feature = "tcp")]
use karyon_net::tcp::TcpConfig;

#[cfg(feature = "quic")]
use karyon_net::quic::ServerQuicConfig;

use crate::{
    codec::{JsonCodec, JsonRpcCodec},
    error::{Error, RPCResult, Result},
    message::{JSONRPC_VERSION, Notification, NotificationResult},
    server::PubSubRPCServiceRef,
    server::RPCServiceRef,
    server::WsCodec,
    server::channel::NewNotification,
};

#[cfg(feature = "http")]
use super::HttpConfig;
use super::{
    DEFAULT_ACCEPT_BACKOFF_BASE, DEFAULT_ACCEPT_BACKOFF_MAX, DEFAULT_MAX_CONCURRENT_REQUESTS,
    Server, ServerConfig, ServerRef,
};

#[cfg(any(feature = "tls", feature = "ws"))]
const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

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
    pub fn service(mut self, service: RPCServiceRef) -> Self {
        self.config.services.insert(service.name(), service);
        self
    }

    /// Add an async RPC method under its exact wire name.
    ///
    /// The name can be unqualified or contain dots. Registering the same standalone
    /// name again replaces its handler. If the name matches a regular RPC service
    /// method, [`Self::build`] returns [`Error::InvalidState`].
    /// PubSub handlers still take priority.
    pub fn method<F, Fut>(mut self, name: impl Into<String>, handler: F) -> Self
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = RPCResult<Value>> + Send + Sync + 'static,
    {
        self.config.methods.insert(
            name.into(),
            Box::new(move |params| Box::pin(handler(params))),
        );
        self
    }

    /// Add a PubSub RPC service.
    pub fn pubsub_service(mut self, service: PubSubRPCServiceRef) -> Self {
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

    /// Set HTTP config for an `http://` endpoint.
    #[cfg(feature = "http")]
    pub fn http_config(mut self, config: HttpConfig) -> Result<Self> {
        match self.config.endpoint {
            Endpoint::Http(..) => {
                self.config.http_config = config;
                Ok(self)
            }
            _ => Err(Error::UnsupportedProtocol(format!(
                "Invalid http config for endpoint: {}",
                self.config.endpoint
            ))),
        }
    }

    /// Listen with `transport` for the endpoints it supports (see
    /// `karyon_net::transport::Transport`), e.g. on WASI.
    pub fn with_transport(mut self, transport: TransportRef) -> Self {
        self.config.transport = Some(transport);
        self
    }

    /// Run the server's tasks and timers on `ex`. A user runtime goes
    /// here as `Executor::from(runtime)`.
    pub fn with_executor(mut self, ex: Executor) -> Self {
        self.executor = Some(ex);
        self
    }

    /// Drop a connection that sends nothing for this long. Off by
    /// default, because pubsub subscribers idle by design.
    pub fn read_timeout(mut self, duration: Duration) -> Self {
        self.config.read_timeout = Some(duration);
        self
    }

    /// Drop a client that does not finish the TLS and/or WebSocket
    /// handshake within this duration. Defaults to 10 seconds.
    #[cfg(any(feature = "tls", feature = "ws"))]
    pub fn handshake_timeout(mut self, duration: Duration) -> Self {
        self.config.handshake_timeout = duration;
        self
    }

    /// Set the maximum number of concurrent in-flight requests per message
    /// connection. The reader waits for a free slot before reading the next
    /// request. Defaults to 64.
    ///
    /// If set to `0`, [`Self::build`] returns [`Error::InvalidState`].
    pub fn max_concurrent_requests(mut self, max: usize) -> Self {
        self.config.max_concurrent_requests = max;
        self
    }

    /// Set the initial retry delay after a failed accept. Defaults to 10 ms.
    ///
    /// [`Self::build`] returns [`Error::InvalidState`] if the delay is less
    /// than 1 ms, exceeds `u64::MAX` ms, or exceeds `accept_backoff_max`.
    pub fn accept_backoff_base(mut self, delay: Duration) -> Self {
        self.config.accept_backoff_base = delay;
        self
    }

    /// Set the maximum retry delay after a failed accept. Defaults to 1000 ms.
    ///
    /// [`Self::build`] returns [`Error::InvalidState`] if the delay exceeds
    /// `u64::MAX` ms or is smaller than `accept_backoff_base`.
    pub fn accept_backoff_max(mut self, delay: Duration) -> Self {
        self.config.accept_backoff_max = delay;
        self
    }

    /// Set a custom notification encoder. A karyon client then needs the
    /// matching `ClientBuilder::with_notification_decoder`.
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
    pub async fn build(self) -> Result<ServerRef> {
        if self.config.max_concurrent_requests == 0 {
            return Err(Error::InvalidState(
                "max_concurrent_requests must be greater than 0".into(),
            ));
        }

        let base_ms =
            duration_to_backoff_ms("accept_backoff_base", self.config.accept_backoff_base)?;
        if base_ms == 0 {
            return Err(Error::InvalidState(
                "accept_backoff_base must be at least 1 ms".into(),
            ));
        }
        let max_ms = duration_to_backoff_ms("accept_backoff_max", self.config.accept_backoff_max)?;
        if base_ms > max_ms {
            return Err(Error::InvalidState(format!(
                "accept_backoff_base ({base_ms} ms) cannot exceed accept_backoff_max ({max_ms} ms)"
            )));
        }

        for name in self.config.methods.keys() {
            if let Some((service, method)) = name.split_once('.')
                && self.config.service_method(service, method).is_some()
            {
                return Err(Error::InvalidState(format!(
                    "standalone RPC method '{name}' conflicts with a registered service method"
                )));
            }
        }

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
                transport: None,
                services: HashMap::new(),
                methods: HashMap::new(),
                pubsub_services: HashMap::new(),
                #[cfg(feature = "tcp")]
                tcp_config: Default::default(),
                #[cfg(feature = "tls")]
                tls_config: None,
                #[cfg(feature = "quic")]
                quic_config: None,
                notification_encoder: default_notification_encoder,
                #[cfg(feature = "http")]
                http_config: HttpConfig::default(),
                read_timeout: None,
                #[cfg(any(feature = "tls", feature = "ws"))]
                handshake_timeout: DEFAULT_HANDSHAKE_TIMEOUT,
                max_concurrent_requests: DEFAULT_MAX_CONCURRENT_REQUESTS,
                accept_backoff_base: DEFAULT_ACCEPT_BACKOFF_BASE,
                accept_backoff_max: DEFAULT_ACCEPT_BACKOFF_MAX,
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

fn duration_to_backoff_ms(name: &str, duration: Duration) -> Result<u64> {
    u64::try_from(duration.as_millis())
        .map_err(|_| Error::InvalidState(format!("{name} exceeds maximum duration")))
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
