use std::sync::Arc;

#[cfg(feature = "tls")]
use karyon_net::async_rustls::rustls;

use crate::{
    codec::{ClonableJsonCodec, JsonCodec},
    error::Result,
    net::ToEndpoint,
};
#[cfg(feature = "tcp")]
use crate::{error::Error, net::Endpoint, net::TcpConfig};

use super::{Client, ClientConfig};

const DEFAULT_TIMEOUT: u64 = 3000; // 3s

const DEFAULT_MAX_SUBSCRIPTION_BUFFER_SIZE: usize = 20000;

/// Builder for constructing an RPC [`Client`].
pub struct ClientBuilder<C> {
    inner: ClientConfig,
    codec: C,
}

impl ClientBuilder<JsonCodec> {
    /// Creates a new [`ClientBuilder`]
    ///
    /// This function initializes a `ClientBuilder` with the specified endpoint.
    ///
    /// # Example
    ///
    /// ```
    /// use karyon_jsonrpc::client::ClientBuilder;
    ///  
    /// async {
    ///     let builder = ClientBuilder::new("ws://127.0.0.1:3000")
    ///         .expect("Create a new client builder");
    ///     let client = builder.build().await
    ///         .expect("Build a new client");
    /// };
    /// ```
    pub fn new(endpoint: impl ToEndpoint) -> Result<ClientBuilder<JsonCodec>> {
        ClientBuilder::new_with_codec(endpoint, JsonCodec {})
    }
}

impl<C> ClientBuilder<C>
where
    C: ClonableJsonCodec + 'static,
{
    /// Creates a new [`ClientBuilder`]
    ///
    /// This function initializes a `ClientBuilder` with the specified endpoint
    /// and the given json codec.
    /// # Example
    ///
    /// ```
    ///
    /// #[cfg(feature = "ws")]
    /// use karyon_jsonrpc::codec::{WebSocketCodec, WebSocketDecoder, WebSocketEncoder};
    /// #[cfg(feature = "ws")]
    /// use async_tungstenite::tungstenite::Message;
    /// use serde_json::Value;
    ///
    /// use karyon_jsonrpc::{
    ///     client::ClientBuilder, codec::{Codec, Decoder, Encoder, ByteBuffer},
    ///     error::{Error, Result}
    /// };
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
    ///     let builder = ClientBuilder::new_with_codec("tcp://127.0.0.1:3000", CustomJsonCodec {})
    ///         .expect("Create a new client builder with a custom json codec");
    ///     let client = builder.build().await
    ///         .expect("Build a new client");
    /// };
    /// ```
    pub fn new_with_codec(endpoint: impl ToEndpoint, codec: C) -> Result<ClientBuilder<C>> {
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
            codec,
        })
    }

    /// Set timeout for receiving messages, in milliseconds. Requests will
    /// fail if it takes longer.
    ///
    /// # Example
    ///
    /// ```
    /// use karyon_jsonrpc::client::ClientBuilder;
    ///  
    /// async {
    ///     let client = ClientBuilder::new("ws://127.0.0.1:3000")
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
    /// use karyon_jsonrpc::client::ClientBuilder;
    ///  
    /// async {
    ///     let client = ClientBuilder::new("ws://127.0.0.1:3000")
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
    /// use karyon_jsonrpc::{client::ClientBuilder, net::TcpConfig};
    ///  
    /// async {
    ///     let tcp_config = TcpConfig::default();
    ///
    ///     let client = ClientBuilder::new("ws://127.0.0.1:3000")
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
    /// use karyon_jsonrpc::client::ClientBuilder;
    /// use futures_rustls::rustls;
    ///  
    /// async {
    ///     let tls_config = rustls::ClientConfig::new(...);
    ///
    ///     let client_builder = ClientBuilder::new("ws://127.0.0.1:3000")
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
    /// use karyon_jsonrpc::{client::ClientBuilder, net::TcpConfig};
    ///  
    /// async {
    ///     let tcp_config = TcpConfig::default();
    ///     let client = ClientBuilder::new("ws://127.0.0.1:3000")
    ///         .expect("Create a new client builder")
    ///         .tcp_config(tcp_config)
    ///         .expect("Add tcp config")
    ///         .set_timeout(5000)
    ///         .build().await
    ///         .expect("Build a new client");
    /// };
    ///
    /// ```
    pub async fn build(self) -> Result<Arc<Client<C>>> {
        let client = Client::init(self.inner, self.codec).await?;
        Ok(client)
    }
}
