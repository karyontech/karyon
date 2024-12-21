pub mod builder;
pub mod channel;
pub mod pubsub_service;
mod response_queue;
pub mod service;

use std::{collections::HashMap, sync::Arc};

use log::{debug, error, info, trace, warn};

use karyon_core::{
    async_runtime::Executor,
    async_util::{select, Either, TaskGroup, TaskResult},
};

#[cfg(feature = "tls")]
use karyon_net::async_rustls::rustls;
#[cfg(feature = "ws")]
use karyon_net::ws::ServerWsConfig;
use karyon_net::{Conn, Listener};

#[cfg(feature = "tcp")]
use crate::net::TcpConfig;

use crate::{
    codec::ClonableJsonCodec,
    error::{Error, Result},
    message,
    net::Endpoint,
};

pub use builder::ServerBuilder;
pub use channel::Channel;
pub use pubsub_service::{PubSubRPCMethod, PubSubRPCService};
pub use service::{RPCMethod, RPCService};

use response_queue::ResponseQueue;

pub const INVALID_REQUEST_ERROR_MSG: &str = "Invalid request";
pub const FAILED_TO_PARSE_ERROR_MSG: &str = "Failed to parse";
pub const METHOD_NOT_FOUND_ERROR_MSG: &str = "Method not found";
pub const UNSUPPORTED_JSONRPC_VERSION: &str = "Unsupported jsonrpc version";

const CHANNEL_SUBSCRIPTION_BUFFER_SIZE: usize = 100;

struct NewRequest {
    srvc_name: String,
    method_name: String,
    msg: message::Request,
}

enum SanityCheckResult {
    NewReq(NewRequest),
    ErrRes(message::Response),
}

struct ServerConfig {
    endpoint: Endpoint,
    #[cfg(feature = "tcp")]
    tcp_config: TcpConfig,
    #[cfg(feature = "tls")]
    tls_config: Option<rustls::ServerConfig>,
    services: HashMap<String, Arc<dyn RPCService + 'static>>,
    pubsub_services: HashMap<String, Arc<dyn PubSubRPCService + 'static>>,
}

/// Represents an RPC server
pub struct Server {
    listener: Listener<serde_json::Value, Error>,
    task_group: TaskGroup,
    config: ServerConfig,
}

impl Server {
    /// Starts the RPC server. This will spawn a new task for the main accept loop,
    /// which listens for incoming connections.
    pub fn start(self: &Arc<Self>) {
        // Handle the completion of the accept loop task
        let on_complete = {
            let this = self.clone();
            |result: TaskResult<Result<()>>| async move {
                if let TaskResult::Completed(Err(err)) = result {
                    error!("Main accept loop stopped: {err}");
                    this.shutdown().await;
                }
            }
        };

        // Spawns a new task for the main accept loop
        self.task_group.spawn(
            {
                let this = self.clone();
                async move {
                    loop {
                        match this.listener.accept().await {
                            Ok(conn) => {
                                if let Err(err) = this.handle_conn(conn).await {
                                    error!("Handle a new connection: {err}")
                                }
                            }
                            Err(err) => {
                                error!("Accept a new connection: {err}")
                            }
                        }
                    }
                }
            },
            on_complete,
        );
    }

    /// Returns the local endpoint.
    pub fn local_endpoint(&self) -> Result<Endpoint> {
        self.listener.local_endpoint().map_err(Error::from)
    }

    /// Shuts down the RPC server
    pub async fn shutdown(&self) {
        self.task_group.cancel().await;
    }

    /// Handles a new connection
    async fn handle_conn(self: &Arc<Self>, conn: Conn<serde_json::Value, Error>) -> Result<()> {
        let endpoint: Option<Endpoint> = conn.peer_endpoint().ok();
        debug!("Handle a new connection {:?}", endpoint);

        let conn = Arc::new(conn);

        let (ch_tx, ch_rx) = async_channel::bounded(CHANNEL_SUBSCRIPTION_BUFFER_SIZE);
        // Create a new connection channel for managing subscriptions
        let channel = Channel::new(ch_tx);

        // Create a response queue
        let queue = ResponseQueue::new();

        let chan = channel.clone();
        let on_complete = |result: TaskResult<Result<()>>| async move {
            if let TaskResult::Completed(Err(err)) = result {
                debug!("Notification loop stopped: {err}");
            }
            // Close the connection channel
            chan.close();
        };

        // Start listening for new responses in the queue or new notifications
        self.task_group.spawn(
            {
                let conn = conn.clone();
                let queue = queue.clone();
                async move {
                    loop {
                        // The select function will prioritize the first future if both futures are ready.
                        // This gives priority to the responses in the response queue.
                        match select(queue.recv(), ch_rx.recv()).await {
                            Either::Left(res) => {
                                conn.send(res).await?;
                            }
                            Either::Right(notification) => {
                                let nt = notification?;
                                let params = Some(serde_json::json!(message::NotificationResult {
                                    subscription: nt.sub_id,
                                    result: Some(nt.result),
                                }));
                                let notification = message::Notification {
                                    jsonrpc: message::JSONRPC_VERSION.to_string(),
                                    method: nt.method,
                                    params,
                                };
                                debug!("--> {notification}");
                                conn.send(serde_json::json!(notification)).await?;
                            }
                        }
                    }
                }
            },
            on_complete,
        );

        let chan = channel.clone();
        let on_complete = |result: TaskResult<Result<()>>| async move {
            if let TaskResult::Completed(Err(err)) = result {
                error!("Connection {:?} dropped: {err}", endpoint);
            } else {
                warn!("Connection {:?} dropped", endpoint);
            }
            // Close the connection channel when the connection dropped
            chan.close();
        };

        // Spawn a new task and wait for new requests.
        self.task_group.spawn(
            {
                let this = self.clone();
                async move {
                    loop {
                        let msg = conn.recv().await?;
                        this.new_request(queue.clone(), channel.clone(), msg).await;
                    }
                }
            },
            on_complete,
        );

        Ok(())
    }

    fn sanity_check(&self, request: serde_json::Value) -> SanityCheckResult {
        let rpc_msg = match serde_json::from_value::<message::Request>(request) {
            Ok(m) => m,
            Err(_) => {
                let response = message::Response {
                    error: Some(message::Error {
                        code: message::PARSE_ERROR_CODE,
                        message: FAILED_TO_PARSE_ERROR_MSG.to_string(),
                        data: None,
                    }),
                    ..Default::default()
                };
                return SanityCheckResult::ErrRes(response);
            }
        };

        if rpc_msg.jsonrpc != message::JSONRPC_VERSION {
            let response = message::Response {
                error: Some(message::Error {
                    code: message::INVALID_REQUEST_ERROR_CODE,
                    message: UNSUPPORTED_JSONRPC_VERSION.to_string(),
                    data: None,
                }),
                id: Some(rpc_msg.id),
                ..Default::default()
            };
            return SanityCheckResult::ErrRes(response);
        }

        debug!("<-- {rpc_msg}");

        // Parse the service name and its method
        let srvc_method_str = rpc_msg.method.clone();
        let srvc_method: Vec<&str> = srvc_method_str.split('.').collect();
        if srvc_method.len() < 2 {
            let response = message::Response {
                error: Some(message::Error {
                    code: message::INVALID_REQUEST_ERROR_CODE,
                    message: INVALID_REQUEST_ERROR_MSG.to_string(),
                    data: None,
                }),
                id: Some(rpc_msg.id),
                ..Default::default()
            };
            return SanityCheckResult::ErrRes(response);
        }

        let srvc_name = srvc_method[0].to_string();
        // Method name is allowed to contain dots
        let method_name = srvc_method[1..].join(".");

        SanityCheckResult::NewReq(NewRequest {
            srvc_name,
            method_name,
            msg: rpc_msg,
        })
    }

    /// Spawns a new task for handling the new request
    async fn new_request(
        self: &Arc<Self>,
        queue: Arc<ResponseQueue<serde_json::Value>>,
        channel: Arc<Channel>,
        msg: serde_json::Value,
    ) {
        trace!("--> new request {msg}");
        let on_complete = |result: TaskResult<Result<()>>| async move {
            if let TaskResult::Completed(Err(err)) = result {
                error!("Handle a new request: {err}");
            }
        };

        // Spawns a new task for handling the new request, and push the
        // response to the response queue.
        self.task_group.spawn(
            {
                let this = self.clone();
                async move {
                    let response = this.handle_request(channel, msg).await;
                    debug!("--> {response}");
                    queue.push(serde_json::json!(response)).await;
                    Ok(())
                }
            },
            on_complete,
        );
    }

    /// Handles the new request, and returns an RPC Response that has either
    /// an error or result
    async fn handle_request(
        &self,
        channel: Arc<Channel>,
        msg: serde_json::Value,
    ) -> message::Response {
        let req = match self.sanity_check(msg) {
            SanityCheckResult::NewReq(req) => req,
            SanityCheckResult::ErrRes(res) => return res,
        };

        let mut response = message::Response {
            error: None,
            result: None,
            id: Some(req.msg.id.clone()),
            ..Default::default()
        };

        // Check if the service exists in pubsub services list
        if let Some(service) = self.config.pubsub_services.get(&req.srvc_name) {
            // Check if the method exists within the service
            if let Some(method) = service.get_pubsub_method(&req.method_name) {
                let params = req.msg.params.unwrap_or(serde_json::json!(()));
                let result = method(channel, req.msg.method, params);
                response.result = match result.await {
                    Ok(res) => Some(res),
                    Err(err) => return err.to_response(Some(req.msg.id), None),
                };

                return response;
            }
        }

        // Check if the service exists in services list
        if let Some(service) = self.config.services.get(&req.srvc_name) {
            // Check if the method exists within the service
            if let Some(method) = service.get_method(&req.method_name) {
                let params = req.msg.params.unwrap_or(serde_json::json!(()));
                let result = method(params);
                response.result = match result.await {
                    Ok(res) => Some(res),
                    Err(err) => return err.to_response(Some(req.msg.id), None),
                };

                return response;
            }
        }

        response.error = Some(message::Error {
            code: message::METHOD_NOT_FOUND_ERROR_CODE,
            message: METHOD_NOT_FOUND_ERROR_MSG.to_string(),
            data: None,
        });

        response
    }

    /// Initializes a new [`Server`] from the provided [`ServerConfig`]
    async fn init(
        config: ServerConfig,
        ex: Option<Executor>,
        codec: impl ClonableJsonCodec + 'static,
    ) -> Result<Arc<Self>> {
        let task_group = match ex {
            Some(ex) => TaskGroup::with_executor(ex),
            None => TaskGroup::new(),
        };
        let listener = Self::listen(&config, codec).await?;
        info!("RPC server listens to the endpoint: {}", config.endpoint);

        let server = Arc::new(Server {
            listener,
            task_group,
            config,
        });

        Ok(server)
    }

    async fn listen(
        config: &ServerConfig,
        codec: impl ClonableJsonCodec + 'static,
    ) -> Result<Listener<serde_json::Value, Error>> {
        let endpoint = config.endpoint.clone();
        let listener: Listener<serde_json::Value, Error> = match endpoint {
            #[cfg(feature = "tcp")]
            Endpoint::Tcp(..) => Box::new(
                karyon_net::tcp::listen(&endpoint, config.tcp_config.clone(), codec).await?,
            ),
            #[cfg(feature = "tls")]
            Endpoint::Tls(..) => match &config.tls_config {
                Some(conf) => Box::new(
                    karyon_net::tls::listen(
                        &endpoint,
                        karyon_net::tls::ServerTlsConfig {
                            server_config: conf.clone(),
                            tcp_config: config.tcp_config.clone(),
                        },
                        codec,
                    )
                    .await?,
                ),
                None => return Err(Error::TLSConfigRequired),
            },
            #[cfg(feature = "ws")]
            Endpoint::Ws(..) => {
                let config = ServerWsConfig {
                    tcp_config: config.tcp_config.clone(),
                    wss_config: None,
                };
                Box::new(karyon_net::ws::listen(&endpoint, config, codec).await?)
            }
            #[cfg(all(feature = "ws", feature = "tls"))]
            Endpoint::Wss(..) => match &config.tls_config {
                Some(conf) => Box::new(
                    karyon_net::ws::listen(
                        &endpoint,
                        ServerWsConfig {
                            tcp_config: config.tcp_config.clone(),
                            wss_config: Some(karyon_net::ws::ServerWssConfig {
                                server_config: conf.clone(),
                            }),
                        },
                        codec,
                    )
                    .await?,
                ),
                None => return Err(Error::TLSConfigRequired),
            },
            #[cfg(all(feature = "unix", target_family = "unix"))]
            Endpoint::Unix(..) => Box::new(karyon_net::unix::listen(
                &endpoint,
                Default::default(),
                codec,
            )?),

            _ => return Err(Error::UnsupportedProtocol(endpoint.to_string())),
        };

        Ok(listener)
    }
}
