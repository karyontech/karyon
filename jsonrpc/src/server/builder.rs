use std::{collections::HashMap, sync::Arc};

#[cfg(feature = "smol")]
use futures_rustls::rustls;
#[cfg(feature = "tokio")]
use tokio_rustls::rustls;

use karyon_core::{async_runtime::Executor, async_util::TaskGroup};
use karyon_net::{Endpoint, Listener, ToEndpoint};

#[cfg(feature = "ws")]
use crate::codec::WsJsonCodec;

#[cfg(feature = "ws")]
use karyon_net::ws::ServerWsConfig;

use crate::{codec::JsonCodec, Error, PubSubRPCService, RPCService, Result, TcpConfig};

use super::Server;

/// Builder for constructing an RPC [`Server`].
pub struct ServerBuilder {
    endpoint: Endpoint,
    tcp_config: TcpConfig,
    tls_config: Option<rustls::ServerConfig>,
    services: HashMap<String, Arc<dyn RPCService + 'static>>,
    pubsub_services: HashMap<String, Arc<dyn PubSubRPCService + 'static>>,
}

impl ServerBuilder {
    /// Adds a new RPC service to the server.
    pub fn service(mut self, service: Arc<dyn RPCService>) -> Self {
        self.services.insert(service.name(), service);
        self
    }

    /// Adds a new PubSub RPC service to the server.
    pub fn pubsub_service(mut self, service: Arc<dyn PubSubRPCService>) -> Self {
        self.pubsub_services.insert(service.name(), service);
        self
    }

    /// Configure TCP settings for the server.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let tcp_config = TcpConfig::default();
    /// let server = Server::builder()?.tcp_config(tcp_config)?.build()?;
    /// ```
    ///
    /// This function will return an error if the endpoint does not support TCP protocols.
    pub fn tcp_config(mut self, config: TcpConfig) -> Result<ServerBuilder> {
        match self.endpoint {
            Endpoint::Tcp(..) | Endpoint::Tls(..) | Endpoint::Ws(..) | Endpoint::Wss(..) => {
                self.tcp_config = config;
                Ok(self)
            }
            _ => Err(Error::UnsupportedProtocol(self.endpoint.to_string())),
        }
    }

    /// Configure TLS settings for the server.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let tls_config = rustls::ServerConfig::new(...);
    /// let server = Server::builder()?.tls_config(tls_config)?.build()?;
    /// ```
    ///
    /// This function will return an error if the endpoint does not support TLS protocols.
    pub fn tls_config(mut self, config: rustls::ServerConfig) -> Result<ServerBuilder> {
        match self.endpoint {
            Endpoint::Tcp(..) | Endpoint::Tls(..) | Endpoint::Ws(..) | Endpoint::Wss(..) => {
                self.tls_config = Some(config);
                Ok(self)
            }
            _ => Err(Error::UnsupportedProtocol(self.endpoint.to_string())),
        }
    }

    /// Builds the server with the configured options.
    pub async fn build(self) -> Result<Arc<Server>> {
        self._build(TaskGroup::new()).await
    }

    /// Builds the server with the configured options and an executor.
    pub async fn build_with_executor(self, ex: Executor) -> Result<Arc<Server>> {
        self._build(TaskGroup::with_executor(ex)).await
    }

    async fn _build(self, task_group: TaskGroup) -> Result<Arc<Server>> {
        let listener: Listener<serde_json::Value> = match self.endpoint {
            Endpoint::Tcp(..) | Endpoint::Tls(..) => match &self.tls_config {
                Some(conf) => Box::new(
                    karyon_net::tls::listen(
                        &self.endpoint,
                        karyon_net::tls::ServerTlsConfig {
                            server_config: conf.clone(),
                            tcp_config: self.tcp_config,
                        },
                        JsonCodec {},
                    )
                    .await?,
                ),
                None => Box::new(
                    karyon_net::tcp::listen(&self.endpoint, self.tcp_config, JsonCodec {}).await?,
                ),
            },
            #[cfg(feature = "ws")]
            Endpoint::Ws(..) | Endpoint::Wss(..) => match &self.tls_config {
                Some(conf) => Box::new(
                    karyon_net::ws::listen(
                        &self.endpoint,
                        ServerWsConfig {
                            tcp_config: self.tcp_config,
                            wss_config: Some(karyon_net::ws::ServerWssConfig {
                                server_config: conf.clone(),
                            }),
                        },
                        WsJsonCodec {},
                    )
                    .await?,
                ),
                None => {
                    let config = ServerWsConfig {
                        tcp_config: self.tcp_config,
                        wss_config: None,
                    };
                    Box::new(karyon_net::ws::listen(&self.endpoint, config, WsJsonCodec {}).await?)
                }
            },
            #[cfg(all(feature = "unix", target_family = "unix"))]
            Endpoint::Unix(..) => Box::new(karyon_net::unix::listen(
                &self.endpoint,
                Default::default(),
                JsonCodec {},
            )?),

            _ => return Err(Error::UnsupportedProtocol(self.endpoint.to_string())),
        };

        Ok(Arc::new(Server {
            listener,
            task_group,
            services: self.services,
            pubsub_services: self.pubsub_services,
        }))
    }
}

impl Server {
    /// Creates a new [`ServerBuilder`]
    ///
    /// This function initializes a `ServerBuilder` with the specified endpoint.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let builder = Server::builder("ws://127.0.0.1:3000")?.build()?;
    /// ```
    pub fn builder(endpoint: impl ToEndpoint) -> Result<ServerBuilder> {
        let endpoint = endpoint.to_endpoint()?;
        Ok(ServerBuilder {
            endpoint,
            services: HashMap::new(),
            pubsub_services: HashMap::new(),
            tcp_config: Default::default(),
            tls_config: None,
        })
    }
}
