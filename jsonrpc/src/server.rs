use std::{collections::HashMap, sync::Arc};

use log::{debug, error, warn};

#[cfg(feature = "smol")]
use futures_rustls::rustls;
#[cfg(feature = "tokio")]
use tokio_rustls::rustls;

use karyon_core::async_runtime::Executor;
use karyon_core::async_util::{TaskGroup, TaskResult};

use karyon_net::{Conn, Endpoint, Listener, ToEndpoint};

#[cfg(feature = "ws")]
use crate::codec::WsJsonCodec;

use crate::{codec::JsonCodec, message, Error, RPCService, Result};

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
    }
}

/// Represents an RPC server
pub struct Server {
    listener: Listener<serde_json::Value>,
    task_group: TaskGroup,
    services: HashMap<String, Box<dyn RPCService + 'static>>,
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
        self.task_group.spawn(
            async move {
                loop {
                    let msg = conn.recv().await?;
                    let response = selfc.handle_request(msg).await;
                    let response = serde_json::to_value(response)?;
                    debug!("--> {response}");
                    conn.send(response).await?;
                }
            },
            on_failure,
        );

        Ok(())
    }

    /// Handles a request
    async fn handle_request(&self, msg: serde_json::Value) -> message::Response {
        let rpc_msg = match serde_json::from_value::<message::Request>(msg) {
            Ok(m) => m,
            Err(_) => {
                return pack_err_res(message::PARSE_ERROR_CODE, FAILED_TO_PARSE_ERROR_MSG, None);
            }
        };
        debug!("<-- {rpc_msg}");

        let srvc_method: Vec<&str> = rpc_msg.method.split('.').collect();
        if srvc_method.len() != 2 {
            return pack_err_res(
                message::INVALID_REQUEST_ERROR_CODE,
                INVALID_REQUEST_ERROR_MSG,
                Some(rpc_msg.id),
            );
        }

        let srvc_name = srvc_method[0];
        let method_name = srvc_method[1];

        let service = match self.services.get(srvc_name) {
            Some(s) => s,
            None => {
                return pack_err_res(
                    message::METHOD_NOT_FOUND_ERROR_CODE,
                    METHOD_NOT_FOUND_ERROR_MSG,
                    Some(rpc_msg.id),
                );
            }
        };

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
            Err(Error::ParseJSON(_)) => {
                return pack_err_res(
                    message::PARSE_ERROR_CODE,
                    FAILED_TO_PARSE_ERROR_MSG,
                    Some(rpc_msg.id),
                );
            }
            Err(Error::InvalidParams(msg)) => {
                return pack_err_res(message::INVALID_PARAMS_ERROR_CODE, msg, Some(rpc_msg.id));
            }
            Err(Error::InvalidRequest(msg)) => {
                return pack_err_res(message::INVALID_REQUEST_ERROR_CODE, msg, Some(rpc_msg.id));
            }
            Err(Error::RPCMethodError(code, msg)) => {
                return pack_err_res(code, msg, Some(rpc_msg.id));
            }
            Err(_) => {
                return pack_err_res(
                    message::INTERNAL_ERROR_CODE,
                    INTERNAL_ERROR_MSG,
                    Some(rpc_msg.id),
                );
            }
        };

        message::Response {
            jsonrpc: message::JSONRPC_VERSION.to_string(),
            error: None,
            result: Some(result),
            id: Some(rpc_msg.id),
        }
    }
}

pub struct ServerBuilder {
    endpoint: Endpoint,
    tls_config: Option<rustls::ServerConfig>,
    services: HashMap<String, Box<dyn RPCService + 'static>>,
}

impl ServerBuilder {
    pub fn service(mut self, service: impl RPCService + 'static) -> Self {
        self.services.insert(service.name(), Box::new(service));
        self
    }

    pub fn tls_config(mut self, config: rustls::ServerConfig) -> Result<ServerBuilder> {
        match self.endpoint {
            Endpoint::Tcp(..) | Endpoint::Tls(..) | Endpoint::Ws(..) | Endpoint::Wss(..) => {
                self.tls_config = Some(config);
                Ok(self)
            }
            _ => Err(Error::UnsupportedProtocol(self.endpoint.to_string())),
        }
    }

    pub async fn build(self) -> Result<Arc<Server>> {
        self._build(TaskGroup::new()).await
    }

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
                            tcp_config: Default::default(),
                        },
                        JsonCodec {},
                    )
                    .await?,
                ),
                None => Box::new(
                    karyon_net::tcp::listen(&self.endpoint, Default::default(), JsonCodec {})
                        .await?,
                ),
            },
            #[cfg(feature = "ws")]
            Endpoint::Ws(..) | Endpoint::Wss(..) => match &self.tls_config {
                Some(conf) => Box::new(
                    karyon_net::ws::listen(
                        &self.endpoint,
                        karyon_net::ws::ServerWsConfig {
                            tcp_config: Default::default(),
                            wss_config: Some(karyon_net::ws::ServerWssConfig {
                                server_config: conf.clone(),
                            }),
                        },
                        WsJsonCodec {},
                    )
                    .await?,
                ),
                None => Box::new(
                    karyon_net::ws::listen(&self.endpoint, Default::default(), WsJsonCodec {})
                        .await?,
                ),
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
        }))
    }
}

impl ServerBuilder {}

impl Server {
    pub fn builder(endpoint: impl ToEndpoint) -> Result<ServerBuilder> {
        let endpoint = endpoint.to_endpoint()?;
        Ok(ServerBuilder {
            endpoint,
            services: HashMap::new(),
            tls_config: None,
        })
    }
}
