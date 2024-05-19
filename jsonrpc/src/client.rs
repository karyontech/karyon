use std::time::Duration;

use log::debug;
use serde::{de::DeserializeOwned, Serialize};

#[cfg(feature = "smol")]
use futures_rustls::rustls;
#[cfg(feature = "tokio")]
use tokio_rustls::rustls;

use karyon_core::{async_util::timeout, util::random_32};
use karyon_net::{tls::ClientTlsConfig, Conn, Endpoint, ToEndpoint};

#[cfg(feature = "ws")]
use karyon_net::ws::{ClientWsConfig, ClientWssConfig};

#[cfg(feature = "ws")]
use crate::codec::WsJsonCodec;

use crate::{codec::JsonCodec, message, Error, Result};

/// Represents an RPC client
pub struct Client {
    conn: Conn<serde_json::Value>,
    timeout: Option<u64>,
}

impl Client {
    /// Calls the provided method, waits for the response, and returns the result.
    pub async fn call<T: Serialize + DeserializeOwned, V: DeserializeOwned>(
        &self,
        method: &str,
        params: T,
    ) -> Result<V> {
        let id = serde_json::json!(random_32());

        let request = message::Request {
            jsonrpc: message::JSONRPC_VERSION.to_string(),
            id,
            method: method.to_string(),
            params: serde_json::json!(params),
        };

        let req_json = serde_json::to_value(&request)?;
        match self.timeout {
            Some(s) => {
                let dur = Duration::from_secs(s);
                timeout(dur, self.conn.send(req_json)).await??;
            }
            None => {
                self.conn.send(req_json).await?;
            }
        }
        debug!("--> {request}");

        let msg = self.conn.recv().await?;
        let response = serde_json::from_value::<message::Response>(msg)?;
        debug!("<-- {response}");

        if response.id.is_none() || response.id.unwrap() != request.id {
            return Err(Error::InvalidMsg("Invalid response id"));
        }

        if let Some(error) = response.error {
            return Err(Error::CallError(error.code, error.message));
        }

        match response.result {
            Some(result) => Ok(serde_json::from_value::<V>(result)?),
            None => Err(Error::InvalidMsg("Invalid response result")),
        }
    }
}

pub struct ClientBuilder {
    endpoint: Endpoint,
    tls_config: Option<(rustls::ClientConfig, String)>,
    timeout: Option<u64>,
}

impl ClientBuilder {
    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn tls_config(mut self, config: rustls::ClientConfig, dns_name: &str) -> Result<Self> {
        match self.endpoint {
            Endpoint::Tcp(..) | Endpoint::Tls(..) | Endpoint::Ws(..) | Endpoint::Wss(..) => {
                self.tls_config = Some((config, dns_name.to_string()));
                Ok(self)
            }
            _ => Err(Error::UnsupportedProtocol(self.endpoint.to_string())),
        }
    }

    pub async fn build(self) -> Result<Client> {
        let conn: Conn<serde_json::Value> = match self.endpoint {
            Endpoint::Tcp(..) | Endpoint::Tls(..) => match self.tls_config {
                Some((conf, dns_name)) => Box::new(
                    karyon_net::tls::dial(
                        &self.endpoint,
                        ClientTlsConfig {
                            dns_name,
                            client_config: conf,
                            tcp_config: Default::default(),
                        },
                        JsonCodec {},
                    )
                    .await?,
                ),
                None => Box::new(
                    karyon_net::tcp::dial(&self.endpoint, Default::default(), JsonCodec {}).await?,
                ),
            },
            #[cfg(feature = "ws")]
            Endpoint::Ws(..) | Endpoint::Wss(..) => match self.tls_config {
                Some((conf, dns_name)) => Box::new(
                    karyon_net::ws::dial(
                        &self.endpoint,
                        ClientWsConfig {
                            tcp_config: Default::default(),
                            wss_config: Some(ClientWssConfig {
                                dns_name,
                                client_config: conf,
                            }),
                        },
                        WsJsonCodec {},
                    )
                    .await?,
                ),
                None => Box::new(
                    karyon_net::ws::dial(&self.endpoint, Default::default(), WsJsonCodec {})
                        .await?,
                ),
            },
            #[cfg(all(feature = "unix", target_family = "unix"))]
            Endpoint::Unix(..) => Box::new(
                karyon_net::unix::dial(&self.endpoint, Default::default(), JsonCodec {}).await?,
            ),
            _ => return Err(Error::UnsupportedProtocol(self.endpoint.to_string())),
        };
        Ok(Client {
            timeout: self.timeout,
            conn,
        })
    }
}
impl Client {
    pub fn builder(endpoint: impl ToEndpoint) -> Result<ClientBuilder> {
        let endpoint = endpoint.to_endpoint()?;
        Ok(ClientBuilder {
            endpoint,
            timeout: None,
            tls_config: None,
        })
    }
}
