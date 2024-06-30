pub mod builder;
mod message_dispatcher;
mod subscriptions;

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use async_channel::{Receiver, Sender};
use log::{debug, error, info};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::json;

#[cfg(feature = "tcp")]
use karyon_net::tcp::TcpConfig;
#[cfg(feature = "ws")]
use karyon_net::ws::ClientWsConfig;
#[cfg(all(feature = "ws", feature = "tls"))]
use karyon_net::ws::ClientWssConfig;
#[cfg(feature = "tls")]
use karyon_net::{async_rustls::rustls, tls::ClientTlsConfig};
use karyon_net::{Conn, Endpoint};

use karyon_core::{
    async_util::{select, timeout, Either, TaskGroup, TaskResult},
    util::random_32,
};

#[cfg(feature = "ws")]
use crate::codec::WsJsonCodec;

use crate::{
    codec::JsonCodec,
    message::{self, SubscriptionID},
    Error, Result,
};

use message_dispatcher::MessageDispatcher;
pub use subscriptions::Subscription;
use subscriptions::Subscriptions;

type RequestID = u32;

struct ClientConfig {
    endpoint: Endpoint,
    #[cfg(feature = "tcp")]
    tcp_config: TcpConfig,
    #[cfg(feature = "tls")]
    tls_config: Option<(rustls::ClientConfig, String)>,
    timeout: Option<u64>,
    subscription_buffer_size: usize,
}

/// Represents an RPC client
pub struct Client {
    disconnect: AtomicBool,
    message_dispatcher: MessageDispatcher,
    subscriptions: Arc<Subscriptions>,
    send_chan: (Sender<serde_json::Value>, Receiver<serde_json::Value>),
    task_group: TaskGroup,
    config: ClientConfig,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum NewMsg {
    Notification(message::Notification),
    Response(message::Response),
}

impl Client {
    /// Calls the provided method, waits for the response, and returns the result.
    pub async fn call<T: Serialize + DeserializeOwned, V: DeserializeOwned>(
        &self,
        method: &str,
        params: T,
    ) -> Result<V> {
        let response = self.send_request(method, params).await?;

        match response.result {
            Some(result) => Ok(serde_json::from_value::<V>(result)?),
            None => Err(Error::InvalidMsg("Invalid response result")),
        }
    }

    /// Subscribes to the provided method, waits for the response, and returns the result.
    ///
    /// This function sends a subscription request to the specified method
    /// with the given parameters. It waits for the response and returns a
    /// `Subscription`.
    pub async fn subscribe<T: Serialize + DeserializeOwned>(
        &self,
        method: &str,
        params: T,
    ) -> Result<Arc<Subscription>> {
        let response = self.send_request(method, params).await?;

        let sub_id = match response.result {
            Some(result) => serde_json::from_value::<SubscriptionID>(result)?,
            None => return Err(Error::InvalidMsg("Invalid subscription id")),
        };

        let sub = self.subscriptions.subscribe(sub_id).await;

        Ok(sub)
    }

    /// Unsubscribes from the provided method, waits for the response, and returns the result.
    ///
    /// This function sends an unsubscription request for the specified method
    /// and subscription ID. It waits for the response to confirm the unsubscription.
    pub async fn unsubscribe(&self, method: &str, sub_id: SubscriptionID) -> Result<()> {
        let _ = self.send_request(method, sub_id).await?;
        self.subscriptions.unsubscribe(&sub_id).await;
        Ok(())
    }

    /// Disconnect the client
    pub async fn stop(&self) {
        self.task_group.cancel().await;
    }

    async fn send_request<T: Serialize + DeserializeOwned>(
        &self,
        method: &str,
        params: T,
    ) -> Result<message::Response> {
        let id: RequestID = random_32();
        let request = message::Request {
            jsonrpc: message::JSONRPC_VERSION.to_string(),
            id: json!(id),
            method: method.to_string(),
            params: Some(json!(params)),
        };

        // Send the request
        self.send(request).await?;

        // Register a new request
        let rx = self.message_dispatcher.register(id).await;

        // Wait for the message dispatcher to send the response
        let result = match self.config.timeout {
            Some(t) => timeout(Duration::from_millis(t), rx.recv()).await?,
            None => rx.recv().await,
        };

        let response = match result {
            Ok(r) => r,
            Err(err) => {
                // Unregister the request if an error occurs
                self.message_dispatcher.unregister(&id).await;
                return Err(err.into());
            }
        };

        if let Some(error) = response.error {
            return Err(Error::SubscribeError(error.code, error.message));
        }

        // It should be OK to unwrap here, as the message dispatcher checks
        // for the response id.
        if *response.id.as_ref().expect("Get response id") != id {
            return Err(Error::InvalidMsg("Invalid response id"));
        }

        Ok(response)
    }

    async fn send(&self, req: message::Request) -> Result<()> {
        if self.disconnect.load(Ordering::Relaxed) {
            return Err(Error::ClientDisconnected);
        }
        let req = serde_json::to_value(req)?;
        self.send_chan.0.send(req).await?;
        Ok(())
    }

    /// Initializes a new [`Client`] from the provided [`ClientConfig`].
    async fn init(config: ClientConfig) -> Result<Arc<Self>> {
        let client = Arc::new(Client {
            disconnect: AtomicBool::new(false),
            subscriptions: Subscriptions::new(config.subscription_buffer_size),
            send_chan: async_channel::bounded(10),
            message_dispatcher: MessageDispatcher::new(),
            task_group: TaskGroup::new(),
            config,
        });

        let conn = client.connect().await?;
        info!(
            "Successfully connected to the RPC server: {}",
            conn.peer_endpoint()?
        );
        client.start_background_loop(conn);
        Ok(client)
    }

    async fn connect(self: &Arc<Self>) -> Result<Conn<serde_json::Value>> {
        let endpoint = self.config.endpoint.clone();
        let conn: Conn<serde_json::Value> = match endpoint {
            #[cfg(feature = "tcp")]
            Endpoint::Tcp(..) => Box::new(
                karyon_net::tcp::dial(&endpoint, self.config.tcp_config.clone(), JsonCodec {})
                    .await?,
            ),
            #[cfg(feature = "tls")]
            Endpoint::Tls(..) => match &self.config.tls_config {
                Some((conf, dns_name)) => Box::new(
                    karyon_net::tls::dial(
                        &self.config.endpoint,
                        ClientTlsConfig {
                            dns_name: dns_name.to_string(),
                            client_config: conf.clone(),
                            tcp_config: self.config.tcp_config.clone(),
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
                    tcp_config: self.config.tcp_config.clone(),
                    wss_config: None,
                };
                Box::new(karyon_net::ws::dial(&endpoint, config, WsJsonCodec {}).await?)
            }
            #[cfg(all(feature = "ws", feature = "tls"))]
            Endpoint::Wss(..) => match &self.config.tls_config {
                Some((conf, dns_name)) => Box::new(
                    karyon_net::ws::dial(
                        &endpoint,
                        ClientWsConfig {
                            tcp_config: self.config.tcp_config.clone(),
                            wss_config: Some(ClientWssConfig {
                                dns_name: dns_name.clone(),
                                client_config: conf.clone(),
                            }),
                        },
                        WsJsonCodec {},
                    )
                    .await?,
                ),
                None => return Err(Error::TLSConfigRequired),
            },
            #[cfg(all(feature = "unix", target_family = "unix"))]
            Endpoint::Unix(..) => {
                Box::new(karyon_net::unix::dial(&endpoint, Default::default(), JsonCodec {}).await?)
            }
            _ => return Err(Error::UnsupportedProtocol(endpoint.to_string())),
        };

        Ok(conn)
    }

    fn start_background_loop(self: &Arc<Self>, conn: Conn<serde_json::Value>) {
        let on_complete = {
            let this = self.clone();
            |result: TaskResult<Result<()>>| async move {
                if let TaskResult::Completed(Err(err)) = result {
                    error!("Client stopped: {err}");
                }
                this.disconnect.store(true, Ordering::Relaxed);
                this.subscriptions.clear().await;
                this.message_dispatcher.clear().await;
            }
        };

        // Spawn a new task
        self.task_group.spawn(
            {
                let this = self.clone();
                async move { this.background_loop(conn).await }
            },
            on_complete,
        );
    }

    async fn background_loop(self: Arc<Self>, conn: Conn<serde_json::Value>) -> Result<()> {
        loop {
            match select(self.send_chan.1.recv(), conn.recv()).await {
                Either::Left(req) => {
                    conn.send(req?).await?;
                }
                Either::Right(msg) => match self.handle_msg(msg?).await {
                    Err(Error::SubscriptionBufferFull) => {
                        return Err(Error::SubscriptionBufferFull);
                    }
                    Err(err) => {
                        let endpoint = conn.peer_endpoint()?;
                        error!("Handle a new msg from the endpoint {endpoint} : {err}",);
                    }
                    Ok(_) => {}
                },
            }
        }
    }

    async fn handle_msg(&self, msg: serde_json::Value) -> Result<()> {
        match serde_json::from_value::<NewMsg>(msg.clone()) {
            Ok(msg) => match msg {
                NewMsg::Response(res) => {
                    debug!("<-- {res}");
                    self.message_dispatcher.dispatch(res).await
                }
                NewMsg::Notification(nt) => {
                    debug!("<-- {nt}");
                    self.subscriptions.notify(nt).await
                }
            },
            Err(err) => {
                error!("Receive unexpected msg {msg}: {err}");
                Err(Error::InvalidMsg("Unexpected msg"))
            }
        }
    }
}
