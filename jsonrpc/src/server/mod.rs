pub mod channel;
pub mod pubsub_service;
pub mod service;

use std::{collections::HashMap, sync::Arc};

use log::{debug, error, warn};

#[cfg(feature = "smol")]
use futures_rustls::rustls;
#[cfg(feature = "tokio")]
use tokio_rustls::rustls;

use karyon_core::{
    async_runtime::Executor,
    async_util::{select, Either, TaskGroup, TaskResult},
};

use karyon_net::{Conn, Endpoint, Listener, ToEndpoint};

#[cfg(feature = "ws")]
use crate::codec::WsJsonCodec;

#[cfg(feature = "ws")]
use karyon_net::ws::ServerWsConfig;

use crate::{codec::JsonCodec, message, Error, PubSubRPCService, RPCService, Result, TcpConfig};

use channel::{ArcChannel, Channel};

const CHANNEL_CAP: usize = 10;

pub const INVALID_REQUEST_ERROR_MSG: &str = "Invalid request";
pub const FAILED_TO_PARSE_ERROR_MSG: &str = "Failed to parse";
pub const METHOD_NOT_FOUND_ERROR_MSG: &str = "Method not found";
pub const INTERNAL_ERROR_MSG: &str = "Internal error";

fn pack_err_res(code: i32, msg: &str, id: Option<serde_json::Value>) -> message::Response {
    let err = message::Error {
        code,
        message: msg.to_string(),
        data: None,
    };

    message::Response {
        jsonrpc: message::JSONRPC_VERSION.to_string(),
        error: Some(err),
        result: None,
        id,
        subscription: None,
    }
}

struct NewRequest {
    srvc_name: String,
    method_name: String,
    msg: message::Request,
}

enum SanityCheckResult {
    NewReq(NewRequest),
    ErrRes(message::Response),
}

/// Represents an RPC server
pub struct Server {
    listener: Listener<serde_json::Value>,
    task_group: TaskGroup,
    services: HashMap<String, Arc<dyn RPCService + 'static>>,
    pubsub_services: HashMap<String, Arc<dyn PubSubRPCService + 'static>>,
}

impl Server {
    /// Returns the local endpoint.
    pub fn local_endpoint(&self) -> Result<Endpoint> {
        self.listener.local_endpoint().map_err(Error::from)
    }

    /// Starts the RPC server
    pub async fn start(self: Arc<Self>) -> Result<()> {
        loop {
            match self.listener.accept().await {
                Ok(conn) => {
                    if let Err(err) = self.handle_conn(conn).await {
                        error!("Failed to handle a new conn: {err}")
                    }
                }
                Err(err) => {
                    error!("Failed to accept a new conn: {err}")
                }
            }
        }
    }

    /// Shuts down the RPC server
    pub async fn shutdown(&self) {
        self.task_group.cancel().await;
    }

    /// Handles a new connection
    async fn handle_conn(self: &Arc<Self>, conn: Conn<serde_json::Value>) -> Result<()> {
        let endpoint = conn.peer_endpoint().expect("get peer endpoint");
        debug!("Handle a new connection {endpoint}");

        let on_failure = |result: TaskResult<Result<()>>| async move {
            if let TaskResult::Completed(Err(err)) = result {
                error!("Connection {} dropped: {}", endpoint, err);
            } else {
                warn!("Connection {} dropped", endpoint);
            }
        };

        let selfc = self.clone();
        let (ch_tx, ch_rx) = async_channel::bounded(CHANNEL_CAP);
        let channel = Channel::new(ch_tx);
        self.task_group.spawn(
            async move {
                loop {
                    match select(conn.recv(), ch_rx.recv()).await {
                        Either::Left(msg) => {
                            // TODO spawn a task
                            let response = selfc.handle_request(channel.clone(), msg?).await;
                            debug!("--> {response}");
                            conn.send(serde_json::to_value(response)?).await?;
                        }
                        Either::Right(msg) => {
                            let (sub_id, result) = msg?;
                            let response = message::Notification {
                                jsonrpc: message::JSONRPC_VERSION.to_string(),
                                method: None,
                                params: Some(result),
                                subscription: Some(sub_id.into()),
                            };
                            debug!("--> {response}");
                            conn.send(serde_json::to_value(response)?).await?;
                        }
                    }
                }
            },
            on_failure,
        );

        Ok(())
    }

    fn sanity_check(&self, request: serde_json::Value) -> SanityCheckResult {
        let rpc_msg = match serde_json::from_value::<message::Request>(request) {
            Ok(m) => m,
            Err(_) => {
                return SanityCheckResult::ErrRes(pack_err_res(
                    message::PARSE_ERROR_CODE,
                    FAILED_TO_PARSE_ERROR_MSG,
                    None,
                ));
            }
        };
        debug!("<-- {rpc_msg}");

        let srvc_method_str = rpc_msg.method.clone();
        let srvc_method: Vec<&str> = srvc_method_str.split('.').collect();
        if srvc_method.len() < 2 {
            return SanityCheckResult::ErrRes(pack_err_res(
                message::INVALID_REQUEST_ERROR_CODE,
                INVALID_REQUEST_ERROR_MSG,
                Some(rpc_msg.id),
            ));
        }

        let srvc_name = srvc_method[0].to_string();
        let method_name = srvc_method[1].to_string();

        SanityCheckResult::NewReq(NewRequest {
            srvc_name,
            method_name,
            msg: rpc_msg,
        })
    }

    /// Handles a new request
    async fn handle_request(
        &self,
        channel: ArcChannel,
        msg: serde_json::Value,
    ) -> message::Response {
        let req = match self.sanity_check(msg) {
            SanityCheckResult::NewReq(req) => req,
            SanityCheckResult::ErrRes(res) => return res,
        };

        if req.msg.subscriber.is_some() {
            match self.pubsub_services.get(&req.srvc_name) {
                Some(s) => {
                    self.handle_pubsub_request(channel, s, &req.method_name, req.msg)
                        .await
                }
                None => pack_err_res(
                    message::METHOD_NOT_FOUND_ERROR_CODE,
                    METHOD_NOT_FOUND_ERROR_MSG,
                    Some(req.msg.id),
                ),
            }
        } else {
            match self.services.get(&req.srvc_name) {
                Some(s) => self.handle_call_request(s, &req.method_name, req.msg).await,
                None => pack_err_res(
                    message::METHOD_NOT_FOUND_ERROR_CODE,
                    METHOD_NOT_FOUND_ERROR_MSG,
                    Some(req.msg.id),
                ),
            }
        }
    }

    /// Handles a call request
    async fn handle_call_request(
        &self,
        service: &Arc<dyn RPCService + 'static>,
        method_name: &str,
        rpc_msg: message::Request,
    ) -> message::Response {
        let method = match service.get_method(method_name) {
            Some(m) => m,
            None => {
                return pack_err_res(
                    message::METHOD_NOT_FOUND_ERROR_CODE,
                    METHOD_NOT_FOUND_ERROR_MSG,
                    Some(rpc_msg.id),
                );
            }
        };

        let result = match method(rpc_msg.params.clone()).await {
            Ok(res) => res,
            Err(err) => return self.handle_error(err, rpc_msg.id),
        };

        message::Response {
            jsonrpc: message::JSONRPC_VERSION.to_string(),
            error: None,
            result: Some(result),
            id: Some(rpc_msg.id),
            subscription: None,
        }
    }

    /// Handles a pubsub request
    async fn handle_pubsub_request(
        &self,
        channel: ArcChannel,
        service: &Arc<dyn PubSubRPCService + 'static>,
        method_name: &str,
        rpc_msg: message::Request,
    ) -> message::Response {
        let method = match service.get_pubsub_method(method_name) {
            Some(m) => m,
            None => {
                return pack_err_res(
                    message::METHOD_NOT_FOUND_ERROR_CODE,
                    METHOD_NOT_FOUND_ERROR_MSG,
                    Some(rpc_msg.id),
                );
            }
        };

        let result = match method(channel, rpc_msg.params.clone()).await {
            Ok(res) => res,
            Err(err) => return self.handle_error(err, rpc_msg.id),
        };

        message::Response {
            jsonrpc: message::JSONRPC_VERSION.to_string(),
            error: None,
            result: None,
            id: Some(rpc_msg.id),
            subscription: Some(result),
        }
    }

    fn handle_error(&self, err: Error, msg_id: serde_json::Value) -> message::Response {
        match err {
            Error::ParseJSON(_) => pack_err_res(
                message::PARSE_ERROR_CODE,
                FAILED_TO_PARSE_ERROR_MSG,
                Some(msg_id),
            ),
            Error::InvalidParams(msg) => {
                pack_err_res(message::INVALID_PARAMS_ERROR_CODE, msg, Some(msg_id))
            }
            Error::InvalidRequest(msg) => {
                pack_err_res(message::INVALID_REQUEST_ERROR_CODE, msg, Some(msg_id))
            }
            Error::RPCMethodError(code, msg) => pack_err_res(code, msg, Some(msg_id)),
            _ => pack_err_res(
                message::INTERNAL_ERROR_CODE,
                INTERNAL_ERROR_MSG,
                Some(msg_id),
            ),
        }
    }
}

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
