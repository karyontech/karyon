use std::{collections::HashMap, sync::Arc, time::Duration};

use log::{debug, error, warn};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;

#[cfg(feature = "smol")]
use futures_rustls::rustls;
#[cfg(feature = "tokio")]
use tokio_rustls::rustls;

use karyon_core::{
    async_runtime::lock::Mutex,
    async_util::{timeout, TaskGroup, TaskResult},
    util::random_64,
};
use karyon_net::{tls::ClientTlsConfig, Conn, Endpoint, ToEndpoint};

#[cfg(feature = "ws")]
use karyon_net::ws::{ClientWsConfig, ClientWssConfig};

#[cfg(feature = "ws")]
use crate::codec::WsJsonCodec;

use crate::{codec::JsonCodec, message, Error, Result, SubscriptionID, TcpConfig};

const CHANNEL_CAP: usize = 10;

const DEFAULT_TIMEOUT: u64 = 1000; // 1s

/// Type alias for a subscription to receive notifications.
///
/// The receiver channel is returned by the `subscribe` method to receive
/// notifications from the server.
pub type Subscription = async_channel::Receiver<serde_json::Value>;

/// Represents an RPC client
pub struct Client {
    conn: Conn<serde_json::Value>,
    chan_tx: async_channel::Sender<message::Response>,
    chan_rx: async_channel::Receiver<message::Response>,
    timeout: Option<u64>,
    subscriptions: Mutex<HashMap<SubscriptionID, async_channel::Sender<serde_json::Value>>>,
    task_group: TaskGroup,
}

impl Client {
    /// Calls the provided method, waits for the response, and returns the result.
    pub async fn call<T: Serialize + DeserializeOwned, V: DeserializeOwned>(
        &self,
        method: &str,
        params: T,
    ) -> Result<V> {
        let request = self.send_request(method, params, None).await?;
        debug!("--> {request}");

        let response = match self.timeout {
            Some(t) => timeout(Duration::from_millis(t), self.chan_rx.recv()).await??,
            None => self.chan_rx.recv().await?,
        };
        debug!("<-- {response}");

        if let Some(error) = response.error {
            return Err(Error::CallError(error.code, error.message));
        }

        if response.id.is_none() || response.id.unwrap() != request.id {
            return Err(Error::InvalidMsg("Invalid response id"));
        }

        match response.result {
            Some(result) => Ok(serde_json::from_value::<V>(result)?),
            None => Err(Error::InvalidMsg("Invalid response result")),
        }
    }

    /// Subscribes to the provided method, waits for the response, and returns the result.
    ///
    /// This function sends a subscription request to the specified method
    /// with the given parameters. It waits for the response and returns a
    /// tuple containing a `SubscriptionID` and a `Subscription` (channel receiver).
    pub async fn subscribe<T: Serialize + DeserializeOwned>(
        &self,
        method: &str,
        params: T,
    ) -> Result<(SubscriptionID, Subscription)> {
        let request = self.send_request(method, params, Some(json!(true))).await?;
        debug!("--> {request}");

        let response = match self.timeout {
            Some(t) => timeout(Duration::from_millis(t), self.chan_rx.recv()).await??,
            None => self.chan_rx.recv().await?,
        };
        debug!("<-- {response}");

        if let Some(error) = response.error {
            return Err(Error::SubscribeError(error.code, error.message));
        }

        if response.id.is_none() || response.id.unwrap() != request.id {
            return Err(Error::InvalidMsg("Invalid response id"));
        }

        let sub_id = match response.result {
            Some(result) => serde_json::from_value::<SubscriptionID>(result)?,
            None => return Err(Error::InvalidMsg("Invalid subscription id")),
        };

        let (ch_tx, ch_rx) = async_channel::bounded(CHANNEL_CAP);
        self.subscriptions.lock().await.insert(sub_id, ch_tx);

        Ok((sub_id, ch_rx))
    }

    /// Unsubscribes from the provided method, waits for the response, and returns the result.
    ///
    /// This function sends an unsubscription request for the specified method
    /// and subscription ID. It waits for the response to confirm the unsubscription.
    pub async fn unsubscribe(&self, method: &str, sub_id: SubscriptionID) -> Result<()> {
        let request = self
            .send_request(method, json!(sub_id), Some(json!(true)))
            .await?;
        debug!("--> {request}");

        let response = match self.timeout {
            Some(t) => timeout(Duration::from_millis(t), self.chan_rx.recv()).await??,
            None => self.chan_rx.recv().await?,
        };
        debug!("<-- {response}");

        if let Some(error) = response.error {
            return Err(Error::SubscribeError(error.code, error.message));
        }

        if response.id.is_none() || response.id.unwrap() != request.id {
            return Err(Error::InvalidMsg("Invalid response id"));
        }

        self.subscriptions.lock().await.remove(&sub_id);
        Ok(())
    }

    async fn send_request<T: Serialize + DeserializeOwned>(
        &self,
        method: &str,
        params: T,
        subscriber: Option<serde_json::Value>,
    ) -> Result<message::Request> {
        let id = random_64();

        let request = message::Request {
            jsonrpc: message::JSONRPC_VERSION.to_string(),
            id: json!(id),
            method: method.to_string(),
            params: json!(params),
            subscriber,
        };

        let req_json = serde_json::to_value(&request)?;

        match self.timeout {
            Some(ms) => {
                let t = Duration::from_millis(ms);
                timeout(t, self.conn.send(req_json)).await??;
            }
            None => {
                self.conn.send(req_json).await?;
            }
        }

        Ok(request)
    }

    fn start_background_receiving(self: &Arc<Self>) {
        let selfc = self.clone();
        let on_failure = |result: TaskResult<Result<()>>| async move {
            if let TaskResult::Completed(Err(err)) = result {
                error!("background receiving stopped: {err}");
            }
            // drop all subscription channels
            selfc.subscriptions.lock().await.clear();
        };
        let selfc = self.clone();
        self.task_group.spawn(
            async move {
                loop {
                    let msg = selfc.conn.recv().await?;
                    if let Ok(res) = serde_json::from_value::<message::Response>(msg.clone()) {
                        selfc.chan_tx.send(res).await?;
                        continue;
                    }

                    if let Ok(nt) = serde_json::from_value::<message::Notification>(msg.clone()) {
                        let sub_result: message::NotificationResult = match nt.params {
                            Some(p) => serde_json::from_value(p)?,
                            None => {
                                return Err(Error::InvalidMsg(
                                    "Invalid notification msg: subscription id not found",
                                ))
                            }
                        };

                        match selfc
                            .subscriptions
                            .lock()
                            .await
                            .get(&sub_result.subscription)
                        {
                            Some(s) => {
                                s.send(sub_result.result.unwrap_or(json!(""))).await?;
                                continue;
                            }
                            None => {
                                warn!("Receive unknown notification {}", sub_result.subscription);
                                continue;
                            }
                        }
                    }

                    error!("Receive unexpected msg: {msg}");
                    return Err(Error::InvalidMsg("Unexpected msg"));
                }
            },
            on_failure,
        );
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
    /// Set timeout for requests, in milliseconds.
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

        let (tx, rx) = async_channel::bounded(CHANNEL_CAP);
        let client = Arc::new(Client {
            timeout: self.timeout,
            conn,
            chan_tx: tx,
            chan_rx: rx,
            subscriptions: Mutex::new(HashMap::new()),
            task_group: TaskGroup::new(),
        });
        client.start_background_receiving();
        Ok(client)
    }
}
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
