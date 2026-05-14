use std::sync::Arc;

use karyon_core::async_runtime::Executor;
use karyon_net::ToEndpoint;

#[cfg(any(feature = "tcp", feature = "tls", feature = "quic"))]
use karyon_net::Endpoint;

#[cfg(feature = "quic")]
use karyon_net::quic::ClientQuicConfig;
#[cfg(feature = "tcp")]
use karyon_net::tcp::TcpConfig;

use crate::{
    client::WsCodec,
    codec::{JsonCodec, JsonRpcCodec},
    error::Result,
};

#[cfg(any(feature = "tcp", feature = "tls", feature = "quic"))]
use crate::error::Error;

use super::{Client, ClientConfig};

const DEFAULT_TIMEOUT: u64 = 3000;
const DEFAULT_MAX_SUBSCRIPTION_BUFFER_SIZE: usize = 20000;

/// Builder for constructing an RPC [`Client`].
///
/// # Example
///
/// ```no_run
/// use karyon_jsonrpc::client::ClientBuilder;
///
/// async {
///     let client = ClientBuilder::new("tcp://127.0.0.1:60000")
///         .expect("create builder")
///         .set_timeout(5000)
///         .build().await
///         .expect("build client");
///
///     let result: i32 = client
///         .call("Calc.add", (1, 2))
///         .await
///         .expect("call method");
/// };
/// ```
pub struct ClientBuilder<B, W = JsonCodec> {
    inner: ClientConfig,
    byte_codec: B,
    ws_codec: W,
    executor: Option<Executor>,
}

impl ClientBuilder<JsonCodec, JsonCodec> {
    /// Create a builder with the default JSON codec.
    pub fn new(endpoint: impl ToEndpoint) -> Result<ClientBuilder<JsonCodec, JsonCodec>> {
        ClientBuilder::new_with_codec(endpoint, JsonCodec::default())
    }
}

impl<B: JsonRpcCodec> ClientBuilder<B, JsonCodec> {
    /// Create a builder with a custom byte-stream codec. The default
    /// `JsonCodec` is used for `ws://` / `wss://` endpoints; override
    /// with `with_ws_codec`.
    pub fn new_with_codec(
        endpoint: impl ToEndpoint,
        codec: B,
    ) -> Result<ClientBuilder<B, JsonCodec>> {
        let endpoint = endpoint.to_endpoint()?;
        Ok(ClientBuilder {
            inner: ClientConfig {
                endpoint,
                timeout: Some(DEFAULT_TIMEOUT),
                #[cfg(feature = "tcp")]
                tcp_config: Default::default(),
                #[cfg(feature = "tls")]
                tls_config: None,
                #[cfg(feature = "quic")]
                quic_config: None,
                subscription_buffer_size: DEFAULT_MAX_SUBSCRIPTION_BUFFER_SIZE,
            },
            byte_codec: codec,
            ws_codec: JsonCodec::default(),
            executor: None,
        })
    }
}

impl<B, W> ClientBuilder<B, W>
where
    B: JsonRpcCodec,
    W: WsCodec,
{
    /// Set request timeout in milliseconds.
    pub fn set_timeout(mut self, timeout: u64) -> Self {
        self.inner.timeout = Some(timeout);
        self
    }

    /// Set max subscription buffer size.
    pub fn set_max_subscription_buffer_size(mut self, size: usize) -> Self {
        self.inner.subscription_buffer_size = size;
        self
    }

    /// Set TCP config.
    #[cfg(feature = "tcp")]
    pub fn tcp_config(mut self, config: TcpConfig) -> Result<Self> {
        match self.inner.endpoint {
            Endpoint::Tcp(..) | Endpoint::Tls(..) | Endpoint::Ws(..) | Endpoint::Wss(..) => {
                self.inner.tcp_config = config;
                Ok(self)
            }
            _ => Err(Error::UnsupportedProtocol(self.inner.endpoint.to_string())),
        }
    }

    /// Set TLS config.
    #[cfg(feature = "tls")]
    pub fn tls_config(mut self, config: karyon_net::tls::ClientTlsConfig) -> Result<Self> {
        match self.inner.endpoint {
            Endpoint::Tls(..) | Endpoint::Wss(..) => {
                self.inner.tls_config = Some(config);
                Ok(self)
            }
            _ => Err(Error::UnsupportedProtocol(format!(
                "Invalid tls config for endpoint: {}",
                self.inner.endpoint
            ))),
        }
    }

    /// Set QUIC config.
    #[cfg(feature = "quic")]
    pub fn quic_config(mut self, config: ClientQuicConfig) -> Result<Self> {
        match self.inner.endpoint {
            Endpoint::Quic(..) => {
                self.inner.quic_config = Some(config);
                Ok(self)
            }
            #[cfg(feature = "http3")]
            Endpoint::Http(..) => {
                self.inner.quic_config = Some(config);
                Ok(self)
            }
            _ => Err(Error::UnsupportedProtocol(format!(
                "Invalid quic config for endpoint: {}",
                self.inner.endpoint
            ))),
        }
    }

    /// Override the WebSocket codec. Only meaningful for `ws://` /
    /// `wss://` endpoints.
    #[cfg(feature = "ws")]
    pub fn with_ws_codec<W2: WsCodec>(self, ws_codec: W2) -> ClientBuilder<B, W2> {
        ClientBuilder {
            inner: self.inner,
            byte_codec: self.byte_codec,
            ws_codec,
            executor: self.executor,
        }
    }

    /// Set an executor. Used for the client's task group.
    pub fn with_executor(mut self, ex: Executor) -> Self {
        self.executor = Some(ex);
        self
    }

    /// Build the client.
    pub async fn build(self) -> Result<Arc<Client<B, W>>> {
        Client::init(self.inner, self.byte_codec, self.ws_codec, self.executor).await
    }
}
