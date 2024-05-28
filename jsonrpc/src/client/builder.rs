use std::{collections::HashMap, sync::Arc};

#[cfg(feature = "smol")]
use futures_rustls::rustls;
#[cfg(feature = "tokio")]
use tokio_rustls::rustls;

use karyon_core::{async_runtime::lock::Mutex, async_util::TaskGroup};
use karyon_net::{tls::ClientTlsConfig, Conn, Endpoint, ToEndpoint};

#[cfg(feature = "ws")]
use karyon_net::ws::{ClientWsConfig, ClientWssConfig};

#[cfg(feature = "ws")]
use crate::codec::WsJsonCodec;

use crate::{codec::JsonCodec, Error, Result, TcpConfig};

use super::Client;

const DEFAULT_TIMEOUT: u64 = 3000; // 3s

impl Client {
    /// Creates a new [`ClientBuilder`]
    ///
    /// This function initializes a `ClientBuilder` with the specified endpoint.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let builder = Client::builder("ws://127.0.0.1:3000")?.build()?;
    /// ```
    pub fn builder(endpoint: impl ToEndpoint) -> Result<ClientBuilder> {
        let endpoint = endpoint.to_endpoint()?;
        Ok(ClientBuilder {
            endpoint,
            timeout: Some(DEFAULT_TIMEOUT),
            tls_config: None,
            tcp_config: Default::default(),
        })
    }
}

/// Builder for constructing an RPC [`Client`].
pub struct ClientBuilder {
    endpoint: Endpoint,
    tls_config: Option<(rustls::ClientConfig, String)>,
    tcp_config: TcpConfig,
    timeout: Option<u64>,
}

impl ClientBuilder {
    /// Set timeout for receiving messages, in milliseconds. Requests will
    /// fail if it takes longer.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let client = Client::builder()?.set_timeout(5000).build()?;
    /// ```
    pub fn set_timeout(mut self, timeout: u64) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Configure TCP settings for the client.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let tcp_config = TcpConfig::default();
    /// let client = Client::builder()?.tcp_config(tcp_config)?.build()?;
    /// ```
    ///
    /// This function will return an error if the endpoint does not support TCP protocols.
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
    /// let tls_config = rustls::ClientConfig::new(...);
    /// let client = Client::builder()?.tls_config(tls_config, "example.com")?.build()?;
    /// ```
    ///
    /// This function will return an error if the endpoint does not support TLS protocols.
    pub fn tls_config(mut self, config: rustls::ClientConfig, dns_name: &str) -> Result<Self> {
        match self.endpoint {
            Endpoint::Tcp(..) | Endpoint::Tls(..) | Endpoint::Ws(..) | Endpoint::Wss(..) => {
                self.tls_config = Some((config, dns_name.to_string()));
                Ok(self)
            }
            _ => Err(Error::UnsupportedProtocol(self.endpoint.to_string())),
        }
    }

    /// Build RPC client from [`ClientBuilder`].
    ///
    /// This function creates a new RPC client using the configurations
    /// specified in the `ClientBuilder`. It returns a `Arc<Client>` on success.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let client = Client::builder(endpoint)?
    ///     .set_timeout(5000)
    ///     .tcp_config(tcp_config)?
    ///     .tls_config(tls_config, "example.com")?
    ///     .build()
    ///     .await?;
    /// ```
    pub async fn build(self) -> Result<Arc<Client>> {
        let conn: Conn<serde_json::Value> = match self.endpoint {
            Endpoint::Tcp(..) | Endpoint::Tls(..) => match self.tls_config {
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
                None => Box::new(
                    karyon_net::tcp::dial(&self.endpoint, self.tcp_config, JsonCodec {}).await?,
                ),
            },
            #[cfg(feature = "ws")]
            Endpoint::Ws(..) | Endpoint::Wss(..) => match self.tls_config {
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
                None => {
                    let config = ClientWsConfig {
                        tcp_config: self.tcp_config,
                        wss_config: None,
                    };
                    Box::new(karyon_net::ws::dial(&self.endpoint, config, WsJsonCodec {}).await?)
                }
            },
            #[cfg(all(feature = "unix", target_family = "unix"))]
            Endpoint::Unix(..) => Box::new(
                karyon_net::unix::dial(&self.endpoint, Default::default(), JsonCodec {}).await?,
            ),
            _ => return Err(Error::UnsupportedProtocol(self.endpoint.to_string())),
        };

        let client = Arc::new(Client {
            timeout: self.timeout,
            conn,
            chans: Mutex::new(HashMap::new()),
            subscriptions: Mutex::new(HashMap::new()),
            task_group: TaskGroup::new(),
        });
        client.start_background_receiving();
        Ok(client)
    }
}
