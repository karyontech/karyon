use std::{collections::HashMap, sync::Arc};

use karyon_core::async_runtime::Executor;

#[cfg(feature = "tcp")]
use crate::net::Endpoint;

#[cfg(feature = "tls")]
use karyon_net::async_rustls::rustls;

#[cfg(feature = "tcp")]
use crate::{error::Error, net::TcpConfig};

use crate::{
    codec::{ClonableJsonCodec, JsonCodec},
    error::Result,
    message::Notification,
    net::ToEndpoint,
    server::channel::NewNotification,
    server::default_notification_encoder,
    server::PubSubRPCService,
    server::RPCService,
};

use super::{Server, ServerConfig};

/// Builder for constructing an RPC [`Server`].
pub struct ServerBuilder<C> {
    config: ServerConfig,
    codec: C,
    executor: Option<Executor>,
}

impl<C> ServerBuilder<C>
where
    C: ClonableJsonCodec + 'static,
{
    /// Creates a new [`ServerBuilder`] With a custom codec.
    ///
    /// This function initializes a `ServerBuilder` with the specified endpoint
    /// and custom codec.
    ///
    /// # Example
    ///
    /// ```
    ///
    /// #[cfg(feature = "ws")]
    /// use async_tungstenite::tungstenite::Message;
    /// use serde_json::Value;
    /// #[cfg(feature = "ws")]
    /// use karyon_jsonrpc::codec::{WebSocketCodec, WebSocketDecoder, WebSocketEncoder};
    /// use karyon_jsonrpc::{server::ServerBuilder, codec::{Codec, Decoder, Encoder, ByteBuffer}, error::{Error, Result}};
    ///
    ///
    /// #[derive(Clone)]
    /// pub struct CustomJsonCodec {}
    ///
    /// impl Codec for CustomJsonCodec {
    ///     type Message = serde_json::Value;
    ///     type Error = Error;
    /// }
    ///
    /// #[cfg(feature = "ws")]
    /// impl WebSocketCodec for CustomJsonCodec {
    ///     type Message = serde_json::Value;
    ///     type Error = Error;
    /// }
    ///
    /// impl Encoder for CustomJsonCodec {
    ///     type EnMessage = serde_json::Value;
    ///     type EnError = Error;
    ///     fn encode(&self, src: &Self::EnMessage, dst: &mut ByteBuffer) -> Result<usize> {
    ///         let msg = match serde_json::to_string(src) {
    ///             Ok(m) => m,
    ///             Err(err) => return Err(Error::Encode(err.to_string())),
    ///         };
    ///         let buf = msg.as_bytes();
    ///         dst.extend_from_slice(buf);
    ///         Ok(buf.len())
    ///     }
    /// }
    ///
    /// impl Decoder for CustomJsonCodec {
    ///     type DeMessage = serde_json::Value;
    ///     type DeError = Error;
    ///     fn decode(&self, src: &mut ByteBuffer) -> Result<Option<(usize, Self::DeMessage)>> {
    ///         let de = serde_json::Deserializer::from_slice(src.as_ref());
    ///         let mut iter = de.into_iter::<serde_json::Value>();
    ///
    ///         let item = match iter.next() {
    ///             Some(Ok(item)) => item,
    ///             Some(Err(ref e)) if e.is_eof() => return Ok(None),
    ///             Some(Err(e)) => return Err(Error::Decode(e.to_string())),
    ///             None => return Ok(None),
    ///         };
    ///
    ///         Ok(Some((iter.byte_offset(), item)))
    ///     }
    /// }
    ///
    ///
    /// #[cfg(feature = "ws")]
    /// impl WebSocketEncoder for CustomJsonCodec {
    ///     type EnMessage = serde_json::Value;
    ///     type EnError = Error;
    ///
    ///     fn encode(&self, src: &Self::EnMessage) -> Result<Message> {
    ///         let msg = match serde_json::to_string(src) {
    ///             Ok(m) => m,
    ///             Err(err) => return Err(Error::Encode(err.to_string())),
    ///         };
    ///         Ok(Message::Text(msg))
    ///     }
    /// }
    ///
    /// #[cfg(feature = "ws")]
    /// impl WebSocketDecoder for CustomJsonCodec {
    ///     type DeMessage = serde_json::Value;
    ///     type DeError = Error;
    ///     fn decode(&self, src: &Message) -> Result<Option<Self::DeMessage>> {
    ///          match src {
    ///              Message::Text(s) => match serde_json::from_str(s) {
    ///                  Ok(m) => Ok(Some(m)),
    ///                  Err(err) => Err(Error::Decode(err.to_string())),
    ///              },
    ///              Message::Binary(s) => match serde_json::from_slice(s) {
    ///                  Ok(m) => Ok(m),
    ///                  Err(err) => Err(Error::Decode(err.to_string())),
    ///              },
    ///              Message::Close(_) => Err(Error::IO(std::io::ErrorKind::ConnectionAborted.into())),
    ///              m => Err(Error::Decode(format!(
    ///                  "Receive unexpected message: {:?}",
    ///                  m
    ///              ))),
    ///          }
    ///      }
    /// }
    ///
    /// async {
    ///     let server = ServerBuilder::new_with_codec("tcp://127.0.0.1:3000", CustomJsonCodec{})
    ///         .expect("Create a new server builder")
    ///         .build().await
    ///         .expect("Build the server");
    /// };
    /// ```
    ///
    pub fn new_with_codec(endpoint: impl ToEndpoint, codec: C) -> Result<ServerBuilder<C>> {
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
                notification_encoder: default_notification_encoder,
            },
            codec,
            executor: None,
        })
    }

    /// Adds a new RPC service to the server.
    ///
    /// # Example
    /// ```
    /// use std::sync::Arc;
    ///
    /// use serde_json::Value;
    ///
    /// use karyon_jsonrpc::{rpc_impl, error::RPCError, server::ServerBuilder};
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
    ///     let server = ServerBuilder::new("ws://127.0.0.1:3000")
    ///         .expect("Create a new server builder")
    ///         .service(Arc::new(Ping{}))
    ///         .build().await
    ///         .expect("Build the server");
    /// };
    ///
    /// ```
    pub fn service(mut self, service: Arc<dyn RPCService>) -> Self {
        self.config.services.insert(service.name(), service);
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
    ///     rpc_impl, rpc_pubsub_impl, error::RPCError, message::SubscriptionID,
    ///     server::{ServerBuilder, Channel},
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
    ///         let sub = chan.new_subscription(&method, None).await.expect("Failed to subscribe");
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
    ///     let server = ServerBuilder::new("ws://127.0.0.1:3000")
    ///         .expect("Create a new server builder")
    ///         .service(ping_service.clone())
    ///         .pubsub_service(ping_service)
    ///         .build().await
    ///         .expect("Build the server");
    /// };
    ///
    /// ```
    pub fn pubsub_service(mut self, service: Arc<dyn PubSubRPCService>) -> Self {
        self.config.pubsub_services.insert(service.name(), service);
        self
    }

    /// Configure TCP settings for the server.
    ///
    /// # Example
    ///
    /// ```
    /// use karyon_jsonrpc::{server::ServerBuilder, net::TcpConfig};
    ///
    /// async {
    ///     let tcp_config = TcpConfig::default();
    ///     let server = ServerBuilder::new("ws://127.0.0.1:3000")
    ///         .expect("Create a new server builder")
    ///         .tcp_config(tcp_config)
    ///         .expect("Add tcp config")
    ///         .build().await
    ///         .expect("Build the server");
    /// };
    /// ```
    ///
    /// This function will return an error if the endpoint does not support TCP protocols.
    #[cfg(feature = "tcp")]
    pub fn tcp_config(mut self, config: TcpConfig) -> Result<ServerBuilder<C>> {
        match self.config.endpoint {
            Endpoint::Tcp(..) | Endpoint::Tls(..) | Endpoint::Ws(..) | Endpoint::Wss(..) => {
                self.config.tcp_config = config;
                Ok(self)
            }
            _ => Err(Error::UnsupportedProtocol(self.config.endpoint.to_string())),
        }
    }

    /// Configure TLS settings for the server.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use karon_jsonrpc::ServerBuilder;
    /// use futures_rustls::rustls;
    ///
    /// async {
    ///     let tls_config = rustls::ServerConfig::new(...);
    ///     let server = ServerBuilder::new("ws://127.0.0.1:3000")
    ///         .expect("Create a new server builder")
    ///         .tls_config(tls_config)
    ///         .expect("Add tls config")
    ///         .build().await
    ///         .expect("Build the server");
    /// };
    /// ```
    ///
    /// This function will return an error if the endpoint does not support TLS protocols.
    #[cfg(feature = "tls")]
    pub fn tls_config(mut self, config: rustls::ServerConfig) -> Result<ServerBuilder<C>> {
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

    /// With an executor.
    pub async fn with_executor(mut self, ex: Executor) -> Self {
        self.executor = Some(ex);
        self
    }

    /// With a custom notification encoder
    pub fn with_notification_encoder(
        mut self,
        notification_encoder: fn(NewNotification) -> Notification,
    ) -> Self {
        self.config.notification_encoder = notification_encoder;
        self
    }

    /// Builds the server with the configured options.
    pub async fn build(self) -> Result<Arc<Server>> {
        Server::init(self.config, self.executor, self.codec).await
    }
}

impl ServerBuilder<JsonCodec> {
    /// Creates a new [`ServerBuilder`]
    ///
    /// This function initializes a `ServerBuilder` with the specified endpoint.
    ///
    /// # Example
    ///
    /// ```
    /// use karyon_jsonrpc::server::ServerBuilder;
    ///
    /// async {
    ///     let server = ServerBuilder::new("ws://127.0.0.1:3000")
    ///         .expect("Create a new server builder")
    ///         .build().await
    ///         .expect("Build the server");
    /// };
    /// ```
    pub fn new(endpoint: impl ToEndpoint) -> Result<ServerBuilder<JsonCodec>> {
        Self::new_with_codec(endpoint, JsonCodec::default())
    }
}
