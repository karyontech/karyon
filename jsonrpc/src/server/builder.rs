use std::{collections::HashMap, sync::Arc};

use karyon_core::async_runtime::Executor;

#[cfg(feature = "tcp")]
use karyon_net::Endpoint;
use karyon_net::ToEndpoint;

#[cfg(feature = "tls")]
use karyon_net::async_rustls::rustls;

#[cfg(feature = "tcp")]
use crate::{Error, TcpConfig};
use crate::{PubSubRPCService, RPCService, Result};

use super::{Server, ServerConfig};

/// Builder for constructing an RPC [`Server`].
pub struct ServerBuilder {
    inner: ServerConfig,
}

impl ServerBuilder {
    /// Adds a new RPC service to the server.
    ///
    /// # Example
    /// ```
    /// use std::sync::Arc;
    ///
    /// use serde_json::Value;
    ///
    /// use karyon_jsonrpc::{Server, rpc_impl, RPCError};
    ///
    /// struct Ping {}
    ///
    /// #[rpc_impl]
    /// impl Ping {
    ///     async fn ping(&self, _params: Value) -> Result<Value, RPCError> {
    ///         Ok(serde_json::json!("Pong"))
    ///     }
    /// }
    ///
    /// async {
    ///     let server = Server::builder("ws://127.0.0.1:3000").unwrap()
    ///         .service(Arc::new(Ping{}))
    ///         .build().await.unwrap();
    /// };
    ///
    /// ```
    pub fn service(mut self, service: Arc<dyn RPCService>) -> Self {
        self.inner.services.insert(service.name(), service);
        self
    }

    /// Adds a new PubSub RPC service to the server.
    ///
    /// # Example
    /// ```
    /// use std::sync::Arc;
    ///
    /// use serde_json::Value;
    ///
    /// use karyon_jsonrpc::{
    ///     Server, rpc_impl, rpc_pubsub_impl, RPCError, Channel, SubscriptionID,
    /// };
    ///
    /// struct Ping {}
    ///
    /// #[rpc_impl]
    /// impl Ping {
    ///     async fn ping(&self, _params: Value) -> Result<Value, RPCError> {
    ///         Ok(serde_json::json!("Pong"))
    ///     }
    /// }
    ///
    /// #[rpc_pubsub_impl]
    /// impl Ping {
    ///    async fn log_subscribe(
    ///         &self,
    ///         chan: Arc<Channel>,
    ///         method: String,
    ///         _params: Value,
    ///     ) -> Result<Value, RPCError> {
    ///         let sub = chan.new_subscription(&method).await;
    ///         let sub_id = sub.id.clone();
    ///         Ok(serde_json::json!(sub_id))
    ///     }
    ///
    ///     async fn log_unsubscribe(
    ///         &self,
    ///         chan: Arc<Channel>,
    ///         _method: String,
    ///         params: Value,
    ///     ) -> Result<Value, RPCError> {
    ///         let sub_id: SubscriptionID = serde_json::from_value(params)?;
    ///         chan.remove_subscription(&sub_id).await;
    ///         Ok(serde_json::json!(true))
    ///     }
    /// }
    ///
    /// async {
    ///     let ping_service = Arc::new(Ping{});
    ///     let server = Server::builder("ws://127.0.0.1:3000").unwrap()
    ///         .service(ping_service.clone())
    ///         .pubsub_service(ping_service)
    ///         .build().await.unwrap();
    /// };
    ///
    /// ```
    pub fn pubsub_service(mut self, service: Arc<dyn PubSubRPCService>) -> Self {
        self.inner.pubsub_services.insert(service.name(), service);
        self
    }

    /// Configure TCP settings for the server.
    ///
    /// # Example
    ///
    /// ```
    /// use karyon_jsonrpc::{Server, TcpConfig};
    ///
    /// async {
    ///     let tcp_config = TcpConfig::default();
    ///     let server = Server::builder("ws://127.0.0.1:3000").unwrap()
    ///         .tcp_config(tcp_config).unwrap()
    ///         .build().await.unwrap();
    /// };
    /// ```
    ///
    /// This function will return an error if the endpoint does not support TCP protocols.
    #[cfg(feature = "tcp")]
    pub fn tcp_config(mut self, config: TcpConfig) -> Result<ServerBuilder> {
        match self.inner.endpoint {
            Endpoint::Tcp(..) | Endpoint::Tls(..) | Endpoint::Ws(..) | Endpoint::Wss(..) => {
                self.inner.tcp_config = config;
                Ok(self)
            }
            _ => Err(Error::UnsupportedProtocol(self.inner.endpoint.to_string())),
        }
    }

    /// Configure TLS settings for the server.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use karon_jsonrpc::Server;
    /// use futures_rustls::rustls;
    ///
    /// async {
    ///     let tls_config = rustls::ServerConfig::new(...);
    ///     let server = Server::builder("ws://127.0.0.1:3000").unwrap()
    ///         .tls_config(tls_config).unwrap()
    ///         .build().await.unwrap();
    /// };
    /// ```
    ///
    /// This function will return an error if the endpoint does not support TLS protocols.
    #[cfg(feature = "tls")]
    pub fn tls_config(mut self, config: rustls::ServerConfig) -> Result<ServerBuilder> {
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

    /// Builds the server with the configured options.
    pub async fn build(self) -> Result<Arc<Server>> {
        Server::init(self.inner, None).await
    }

    /// Builds the server with the configured options and an executor.
    pub async fn build_with_executor(self, ex: Executor) -> Result<Arc<Server>> {
        Server::init(self.inner, Some(ex)).await
    }
}

impl Server {
    /// Creates a new [`ServerBuilder`]
    ///
    /// This function initializes a `ServerBuilder` with the specified endpoint.
    ///
    /// # Example
    ///
    /// ```
    /// use karyon_jsonrpc::Server;
    /// async {
    ///     let server = Server::builder("ws://127.0.0.1:3000").unwrap()
    ///         .build().await.unwrap();
    /// };
    /// ```
    pub fn builder(endpoint: impl ToEndpoint) -> Result<ServerBuilder> {
        let endpoint = endpoint.to_endpoint()?;
        Ok(ServerBuilder {
            inner: ServerConfig {
                endpoint,
                services: HashMap::new(),
                pubsub_services: HashMap::new(),
                #[cfg(feature = "tcp")]
                tcp_config: Default::default(),
                #[cfg(feature = "tls")]
                tls_config: None,
            },
        })
    }
}
