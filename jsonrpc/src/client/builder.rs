use std::sync::{atomic::AtomicBool, Arc};

use karyon_core::async_util::TaskGroup;
use karyon_net::{Conn, Endpoint, ToEndpoint};

#[cfg(feature = "tls")]
use karyon_net::{async_rustls::rustls, tls::ClientTlsConfig};

#[cfg(feature = "ws")]
use karyon_net::ws::ClientWsConfig;

#[cfg(all(feature = "ws", feature = "tls"))]
use karyon_net::ws::ClientWssConfig;

#[cfg(feature = "ws")]
use crate::codec::WsJsonCodec;

#[cfg(feature = "tcp")]
use crate::TcpConfig;

use crate::{codec::JsonCodec, Error, Result};

use super::{Client, MessageDispatcher, Subscriptions};

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
    ///     let builder = Client::builder("ws://127.0.0.1:3000").unwrap();
    ///     let client = builder.build().await.unwrap();
    /// };
    /// ```
    pub fn builder(endpoint: impl ToEndpoint) -> Result<ClientBuilder> {
        let endpoint = endpoint.to_endpoint()?;
        Ok(ClientBuilder {
            endpoint,
            timeout: Some(DEFAULT_TIMEOUT),
            #[cfg(feature = "tcp")]
            tcp_config: Default::default(),
            #[cfg(feature = "tls")]
            tls_config: None,
            subscription_buffer_size: DEFAULT_MAX_SUBSCRIPTION_BUFFER_SIZE,
        })
    }
}

/// Builder for constructing an RPC [`Client`].
pub struct ClientBuilder {
    endpoint: Endpoint,
    #[cfg(feature = "tcp")]
    tcp_config: TcpConfig,
    #[cfg(feature = "tls")]
    tls_config: Option<(rustls::ClientConfig, String)>,
    timeout: Option<u64>,
    subscription_buffer_size: usize,
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
    ///     let client = Client::builder("ws://127.0.0.1:3000").unwrap()
    ///         .set_timeout(5000)
    ///         .build().await.unwrap();
    /// };
    /// ```
    pub fn set_timeout(mut self, timeout: u64) -> Self {
        self.timeout = Some(timeout);
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
    ///     let client = Client::builder("ws://127.0.0.1:3000").unwrap()
    ///         .set_max_subscription_buffer_size(10000)
    ///         .build().await.unwrap();
    /// };
    /// ```
    pub fn set_max_subscription_buffer_size(mut self, size: usize) -> Self {
        self.subscription_buffer_size = size;
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
    ///     let client = Client::builder("ws://127.0.0.1:3000").unwrap()
    ///         .tcp_config(tcp_config).unwrap().build().await.unwrap();
    /// };
    /// ```
    ///
    /// This function will return an error if the endpoint does not support TCP protocols.
    #[cfg(feature = "tcp")]
    pub fn tcp_config(mut self, config: TcpConfig) -> Result<Self> {
        match self.endpoint {
            Endpoint::Tcp(..) | Endpoint::Tls(..) | Endpoint::Ws(..) | Endpoint::Wss(..) => {
                self.tcp_config = config;
                Ok(self)
            }
            _ => Err(Error::UnsupportedProtocol(self.endpoint.to_string())),
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
    ///     let client_builder = Client::builder("ws://127.0.0.1:3000").unwrap()
    ///         .tls_config(tls_config, "example.com").unwrap()
    ///         .build().await.unwrap();
    /// };
    /// ```
    ///
    /// This function will return an error if the endpoint does not support TLS protocols.
    #[cfg(feature = "tls")]
    pub fn tls_config(mut self, config: rustls::ClientConfig, dns_name: &str) -> Result<Self> {
        match self.endpoint {
            Endpoint::Tls(..) | Endpoint::Wss(..) => {
                self.tls_config = Some((config, dns_name.to_string()));
                Ok(self)
            }
            _ => Err(Error::UnsupportedProtocol(format!(
                "Invalid tls config for endpoint: {}",
                self.endpoint
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
    ///     let client = Client::builder("ws://127.0.0.1:3000").unwrap()
    ///         .tcp_config(tcp_config).unwrap()
    ///         .set_timeout(5000)
    ///         .build().await.unwrap();
    /// };
    ///
    /// ```
    pub async fn build(self) -> Result<Arc<Client>> {
        let conn: Conn<serde_json::Value> = match self.endpoint {
            #[cfg(feature = "tcp")]
            Endpoint::Tcp(..) => Box::new(
                karyon_net::tcp::dial(&self.endpoint, self.tcp_config, JsonCodec {}).await?,
            ),
            #[cfg(feature = "tls")]
            Endpoint::Tls(..) => match self.tls_config {
                Some((conf, dns_name)) => Box::new(
                    karyon_net::tls::dial(
                        &self.endpoint,
                        ClientTlsConfig {
                            dns_name,
                            client_config: conf,
                            tcp_config: self.tcp_config,
                        },
                        JsonCodec {},
                    )
                    .await?,
                ),
                None => return Err(Error::TLSConfigRequired),
            },
            #[cfg(feature = "ws")]
            Endpoint::Ws(..) => {
                let config = ClientWsConfig {
                    tcp_config: self.tcp_config,
                    wss_config: None,
                };
                Box::new(karyon_net::ws::dial(&self.endpoint, config, WsJsonCodec {}).await?)
            }
            #[cfg(all(feature = "ws", feature = "tls"))]
            Endpoint::Wss(..) => match self.tls_config {
                Some((conf, dns_name)) => Box::new(
                    karyon_net::ws::dial(
                        &self.endpoint,
                        ClientWsConfig {
                            tcp_config: self.tcp_config,
                            wss_config: Some(ClientWssConfig {
                                dns_name,
                                client_config: conf,
                            }),
                        },
                        WsJsonCodec {},
                    )
                    .await?,
                ),
                None => return Err(Error::TLSConfigRequired),
            },
            #[cfg(all(feature = "unix", target_family = "unix"))]
            Endpoint::Unix(..) => Box::new(
                karyon_net::unix::dial(&self.endpoint, Default::default(), JsonCodec {}).await?,
            ),
            _ => return Err(Error::UnsupportedProtocol(self.endpoint.to_string())),
        };

        let send_chan = async_channel::bounded(10);

        let client = Arc::new(Client {
            timeout: self.timeout,
            disconnect: AtomicBool::new(false),
            send_chan,
            message_dispatcher: MessageDispatcher::new(),
            subscriptions: Subscriptions::new(self.subscription_buffer_size),
            task_group: TaskGroup::new(),
        });
        client.start_background_loop(conn);
        Ok(client)
    }
}
