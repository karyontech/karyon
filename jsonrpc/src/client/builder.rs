use std::sync::Arc;

#[cfg(feature = "tcp")]
use karyon_net::Endpoint;
use karyon_net::ToEndpoint;

#[cfg(feature = "tls")]
use karyon_net::async_rustls::rustls;

use crate::Result;
#[cfg(feature = "tcp")]
use crate::{Error, TcpConfig};

use super::{Client, ClientConfig};

const DEFAULT_TIMEOUT: u64 = 3000; // 3s

const DEFAULT_MAX_SUBSCRIPTION_BUFFER_SIZE: usize = 20000;

impl Client {
    /// Creates a new [`ClientBuilder`]
    ///
    /// This function initializes a `ClientBuilder` with the specified endpoint.
    ///
    /// # Example
    ///
    /// ```
    /// use karyon_jsonrpc::Client;
    ///  
    /// async {
    ///     let builder = Client::builder("ws://127.0.0.1:3000")
    ///         .expect("Create a new client builder");
    ///     let client = builder.build().await
    ///         .expect("Build a new client");
    /// };
    /// ```
    pub fn builder(endpoint: impl ToEndpoint) -> Result<ClientBuilder> {
        let endpoint = endpoint.to_endpoint()?;
        Ok(ClientBuilder {
            inner: ClientConfig {
                endpoint,
                timeout: Some(DEFAULT_TIMEOUT),
                #[cfg(feature = "tcp")]
                tcp_config: Default::default(),
                #[cfg(feature = "tls")]
                tls_config: None,
                subscription_buffer_size: DEFAULT_MAX_SUBSCRIPTION_BUFFER_SIZE,
            },
        })
    }
}

/// Builder for constructing an RPC [`Client`].
pub struct ClientBuilder {
    inner: ClientConfig,
}

impl ClientBuilder {
    /// Set timeout for receiving messages, in milliseconds. Requests will
    /// fail if it takes longer.
    ///
    /// # Example
    ///
    /// ```
    /// use karyon_jsonrpc::Client;
    ///  
    /// async {
    ///     let client = Client::builder("ws://127.0.0.1:3000")
    ///         .expect("Create a new client builder")
    ///         .set_timeout(5000)
    ///         .build().await
    ///         .expect("Build a new client");
    /// };
    /// ```
    pub fn set_timeout(mut self, timeout: u64) -> Self {
        self.inner.timeout = Some(timeout);
        self
    }

    /// Set max size for the subscription buffer.
    ///
    /// The client will stop when the subscriber cannot keep up.
    /// When subscribing to a method, a new channel with the provided buffer
    /// size is initialized. Once the buffer is full and the subscriber doesn't
    /// process the messages in the buffer, the client will disconnect and
    /// raise an error.
    ///
    /// # Example
    ///
    /// ```
    /// use karyon_jsonrpc::Client;
    ///  
    /// async {
    ///     let client = Client::builder("ws://127.0.0.1:3000")
    ///         .expect("Create a new client builder")
    ///         .set_max_subscription_buffer_size(10000)
    ///         .build().await
    ///         .expect("Build a new client");
    /// };
    /// ```
    pub fn set_max_subscription_buffer_size(mut self, size: usize) -> Self {
        self.inner.subscription_buffer_size = size;
        self
    }

    /// Configure TCP settings for the client.
    ///
    /// # Example
    ///
    /// ```
    /// use karyon_jsonrpc::{Client, TcpConfig};
    ///  
    /// async {
    ///     let tcp_config = TcpConfig::default();
    ///
    ///     let client = Client::builder("ws://127.0.0.1:3000")
    ///         .expect("Create a new client builder")
    ///         .tcp_config(tcp_config)
    ///         .expect("Add tcp config")
    ///         .build().await
    ///         .expect("Build a new client");
    /// };
    /// ```
    ///
    /// This function will return an error if the endpoint does not support TCP protocols.
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

    /// Configure TLS settings for the client.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use karyon_jsonrpc::Client;
    /// use futures_rustls::rustls;
    ///  
    /// async {
    ///     let tls_config = rustls::ClientConfig::new(...);
    ///
    ///     let client_builder = Client::builder("ws://127.0.0.1:3000")
    ///         .expect("Create a new client builder")
    ///         .tls_config(tls_config, "example.com")
    ///         .expect("Add tls config")
    ///         .build().await
    ///         .expect("Build a new client");
    /// };
    /// ```
    ///
    /// This function will return an error if the endpoint does not support TLS protocols.
    #[cfg(feature = "tls")]
    pub fn tls_config(mut self, config: rustls::ClientConfig, dns_name: &str) -> Result<Self> {
        match self.inner.endpoint {
            Endpoint::Tls(..) | Endpoint::Wss(..) => {
                self.inner.tls_config = Some((config, dns_name.to_string()));
                Ok(self)
            }
            _ => Err(Error::UnsupportedProtocol(format!(
                "Invalid tls config for endpoint: {}",
                self.inner.endpoint
            ))),
        }
    }

    /// Build RPC client from [`ClientBuilder`].
    ///
    /// This function creates a new RPC client using the configurations
    /// specified in the `ClientBuilder`. It returns a `Arc<Client>` on success.
    ///
    /// # Example
    ///
    /// ```
    /// use karyon_jsonrpc::{Client, TcpConfig};
    ///  
    /// async {
    ///     let tcp_config = TcpConfig::default();
    ///     let client = Client::builder("ws://127.0.0.1:3000")
    ///         .expect("Create a new client builder")
    ///         .tcp_config(tcp_config)
    ///         .expect("Add tcp config")
    ///         .set_timeout(5000)
    ///         .build().await
    ///         .expect("Build a new client");
    /// };
    ///
    /// ```
    pub async fn build(self) -> Result<Arc<Client>> {
        let client = Client::init(self.inner).await?;
        Ok(client)
    }
}
